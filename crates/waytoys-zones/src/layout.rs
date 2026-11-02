// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license. See LICENSE-MIT-PowerToys in the repository root.
// Adapted from FancyZonesLib/LayoutConfigurator.cpp at PowerToys 8827f848.

//! Layout calculation in work-area-local logical units. Platform DPI conversion
//! and global screen placement belong to the eventual backend adapter.

use serde::{Deserialize, Serialize};

use crate::geometry::{Rect, Size};
use crate::model::{
    CanvasLayout, CustomLayout, CustomLayoutData, GRID_MULTIPLIER, GridLayout, LayoutType,
    MAX_ZONES, ZoneId,
};
use crate::{Result, ZonesError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub id: ZoneId,
    pub rect: Rect,
}

fn rect_from_edges(left: i64, top: i64, right: i64, bottom: i64) -> Result<Rect> {
    let int = |n| {
        i32::try_from(n).map_err(|_| ZonesError::InvalidData("layout coordinate overflow".into()))
    };
    let rect = Rect::new(
        int(left)?,
        int(top)?,
        int(right - left)?,
        int(bottom - top)?,
    );
    rect.validate()?;
    Ok(rect)
}

pub fn calculate_custom(layout: &CustomLayout, work_area: Size) -> Result<Vec<Zone>> {
    match &layout.data {
        CustomLayoutData::Canvas(canvas) => calculate_canvas(canvas, work_area),
        CustomLayoutData::Grid(grid) => calculate_grid(grid, work_area),
    }
}

pub fn calculate_canvas(layout: &CanvasLayout, work_area: Size) -> Result<Vec<Zone>> {
    layout.validate()?;
    work_area.validate()?;
    layout
        .zones
        .iter()
        .enumerate()
        .map(|(id, zone)| {
            let x =
                |value| i64::from(value) * i64::from(work_area.width) / i64::from(layout.ref_width);
            let y = |value| {
                i64::from(value) * i64::from(work_area.height) / i64::from(layout.ref_height)
            };
            let right = zone
                .x
                .checked_add(zone.width)
                .ok_or_else(|| ZonesError::InvalidData("canvas right edge overflow".into()))?;
            let bottom = zone
                .y
                .checked_add(zone.height)
                .ok_or_else(|| ZonesError::InvalidData("canvas bottom edge overflow".into()))?;
            Ok(Zone {
                id,
                rect: rect_from_edges(x(zone.x), y(zone.y), x(right), y(bottom))?,
            })
        })
        .collect()
}

pub fn calculate_grid(layout: &GridLayout, work_area: Size) -> Result<Vec<Zone>> {
    layout.validate()?;
    work_area.validate()?;
    let boundaries = |percentages: &[i32], extent: i32| {
        let mut sum = 0i64;
        let mut result = vec![0i64];
        for percentage in percentages {
            sum += i64::from(*percentage);
            result.push(sum * i64::from(extent) / i64::from(GRID_MULTIPLIER));
        }
        result
    };
    let xs = boundaries(&layout.columns_percentage, work_area.width);
    let ys = boundaries(&layout.rows_percentage, work_area.height);
    let spacing = if layout.show_spacing {
        i64::from(layout.spacing)
    } else {
        0
    };
    let mut extents = vec![None; layout.zone_count()];
    for (row, cells) in layout.cell_child_map.iter().enumerate() {
        for (column, id) in cells.iter().copied().enumerate() {
            let bounds = extents[id].get_or_insert((row, column, row, column));
            bounds.0 = bounds.0.min(row);
            bounds.1 = bounds.1.min(column);
            bounds.2 = bounds.2.max(row);
            bounds.3 = bounds.3.max(column);
        }
    }
    extents
        .into_iter()
        .enumerate()
        .map(|(id, bounds)| {
            let (top, left, bottom, right) =
                bounds.ok_or_else(|| ZonesError::InvalidData("missing grid zone".into()))?;
            let x0 = xs[left] + if left == 0 { spacing } else { spacing / 2 };
            let y0 = ys[top] + if top == 0 { spacing } else { spacing / 2 };
            let x1 = xs[right + 1]
                - if right + 1 == layout.columns {
                    spacing
                } else {
                    spacing / 2
                };
            let y1 = ys[bottom + 1]
                - if bottom + 1 == layout.rows {
                    spacing
                } else {
                    spacing / 2
                };
            Ok(Zone {
                id,
                rect: rect_from_edges(x0, y0, x1, y1)?,
            })
        })
        .collect()
}

fn equal_percentages(count: usize) -> Vec<i32> {
    (0..count)
        .map(|i| {
            ((i + 1) * GRID_MULTIPLIER as usize / count - i * GRID_MULTIPLIER as usize / count)
                as i32
        })
        .collect()
}

/// Editable Grid template using the original row-major zone assignment.
pub fn grid_template(zone_count: usize, priority: bool) -> Result<GridLayout> {
    if zone_count == 0 || zone_count > MAX_ZONES {
        return Err(ZonesError::InvalidData(
            "grid zone count must be between 1 and 128".into(),
        ));
    }
    // The native source's actual branch uses presets for 1..=10, not 11.
    if priority && zone_count <= 10 {
        let (rows, columns, map) = match zone_count {
            1 => (vec![10_000], vec![10_000], vec![vec![0]]),
            2 => (vec![10_000], vec![6667, 3333], vec![vec![0, 1]]),
            3 => (vec![10_000], vec![2500, 5000, 2500], vec![vec![0, 1, 2]]),
            4 => (
                vec![5000, 5000],
                vec![2500, 5000, 2500],
                vec![vec![0, 1, 2], vec![0, 1, 3]],
            ),
            5 => (
                vec![5000, 5000],
                vec![2500, 5000, 2500],
                vec![vec![0, 1, 2], vec![3, 1, 4]],
            ),
            6 => (
                vec![3333, 3334, 3333],
                vec![2500, 5000, 2500],
                vec![vec![0, 1, 2], vec![0, 1, 3], vec![4, 1, 5]],
            ),
            7 => (
                vec![3333, 3334, 3333],
                vec![2500, 5000, 2500],
                vec![vec![0, 1, 2], vec![3, 1, 4], vec![5, 1, 6]],
            ),
            8 => (
                vec![3333, 3334, 3333],
                vec![2500; 4],
                vec![vec![0, 1, 2, 3], vec![4, 1, 2, 5], vec![6, 1, 2, 7]],
            ),
            9 => (
                vec![3333, 3334, 3333],
                vec![2500; 4],
                vec![vec![0, 1, 2, 3], vec![4, 1, 2, 5], vec![6, 1, 7, 8]],
            ),
            10 => (
                vec![3333, 3334, 3333],
                vec![2500; 4],
                vec![vec![0, 1, 2, 3], vec![4, 1, 5, 6], vec![7, 1, 8, 9]],
            ),
            _ => unreachable!(),
        };
        return GridLayout::new(rows.len(), columns.len(), rows, columns, map);
    }
    let mut rows = 1usize;
    while (rows + 1) * (rows + 1) <= zone_count {
        rows += 1;
    }
    let columns = zone_count.div_ceil(rows);
    let map = (0..rows)
        .map(|r| {
            (0..columns)
                .map(|c| (r * columns + c).min(zone_count - 1))
                .collect()
        })
        .collect();
    GridLayout::new(
        rows,
        columns,
        equal_percentages(rows),
        equal_percentages(columns),
        map,
    )
}

pub fn calculate_template(
    kind: LayoutType,
    work_area: Size,
    zone_count: usize,
    spacing: i32,
) -> Result<Vec<Zone>> {
    work_area.validate()?;
    crate::model::validate_spacing(spacing)?;
    if zone_count > MAX_ZONES {
        return Err(ZonesError::InvalidData("too many template zones".into()));
    }
    match kind {
        LayoutType::Blank => Ok(Vec::new()),
        LayoutType::Custom => Err(ZonesError::InvalidData(
            "custom layout needs a definition".into(),
        )),
        LayoutType::Focus => (0..zone_count)
            .map(|id| {
                let start = 100 + id as i64 * 50;
                let width = (f64::from(work_area.width) * 0.4) as i64;
                let height = (f64::from(work_area.height) * 0.4) as i64;
                Ok(Zone {
                    id,
                    rect: rect_from_edges(start, start, start + width, start + height)?,
                })
            })
            .collect(),
        LayoutType::Grid | LayoutType::PriorityGrid => {
            let mut grid = grid_template(zone_count, kind == LayoutType::PriorityGrid)?;
            grid.spacing = spacing;
            grid.show_spacing = true;
            calculate_grid(&grid, work_area)
        }
        LayoutType::Rows | LayoutType::Columns => {
            if zone_count == 0 {
                return Err(ZonesError::InvalidData(
                    "template zone count must be positive".into(),
                ));
            }
            let (w, h, gap, n) = (
                i64::from(work_area.width),
                i64::from(work_area.height),
                i64::from(spacing),
                zone_count as i64,
            );
            let horizontal = kind == LayoutType::Columns;
            let available = (if horizontal { w } else { h }) - gap * (n + 1);
            (0..zone_count)
                .map(|id| {
                    let i = id as i64;
                    let start = gap * (i + 1) + available * i / n;
                    let end = gap * (i + 1) + available * (i + 1) / n;
                    let rect = if horizontal {
                        rect_from_edges(start, gap, end, h - gap)?
                    } else {
                        rect_from_edges(gap, start, w - gap, end)?
                    };
                    Ok(Zone { id, rect })
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CanvasZone;

    #[test]
    fn columns_distribute_remainders_and_gaps_without_drift() {
        let zones = calculate_template(LayoutType::Columns, Size::new(101, 80), 3, 2).unwrap();
        assert_eq!(
            zones
                .iter()
                .map(|z| (z.rect.x, z.rect.width))
                .collect::<Vec<_>>(),
            vec![(2, 31), (35, 31), (68, 31)]
        );
        assert_eq!(zones[2].rect.right(), 99);
    }

    #[test]
    fn merged_grid_has_only_external_edges_spaced() {
        let grid = GridLayout::new(
            2,
            2,
            vec![5000, 5000],
            vec![5000, 5000],
            vec![vec![0, 1], vec![0, 2]],
        )
        .unwrap();
        let zones = calculate_grid(&grid, Size::new(1000, 800)).unwrap();
        assert_eq!(zones[0].rect, Rect::new(16, 16, 476, 768));
        assert_eq!(zones[1].rect, Rect::new(508, 16, 476, 376));
    }

    #[test]
    fn every_template_count_retains_rectangular_maps_and_zone_count() {
        for count in 1..=MAX_ZONES {
            for priority in [false, true] {
                let grid = grid_template(count, priority).unwrap();
                assert_eq!(grid.zone_count(), count);
                let zones = calculate_grid(&grid, Size::new(2560, 1440)).unwrap();
                assert_eq!(zones.len(), count);
            }
        }
        assert_eq!(
            grid_template(11, true).unwrap(),
            grid_template(11, false).unwrap()
        );
        assert_eq!(
            grid_template(3, true).unwrap().columns_percentage,
            vec![2500, 5000, 2500]
        );
    }

    #[test]
    fn canvas_is_scaled_locally_without_applying_global_output_offsets() {
        let mut canvas = CanvasLayout::new(1000, 500).unwrap();
        canvas.zones.push(CanvasZone::new(100, 50, 300, 200));
        let zones = calculate_canvas(&canvas, Size::new(2000, 1000)).unwrap();
        assert_eq!(zones[0].rect, Rect::new(200, 100, 600, 400));
        assert!(calculate_template(LayoutType::Rows, Size::new(20, 20), 3, 16).is_err());
    }
}
