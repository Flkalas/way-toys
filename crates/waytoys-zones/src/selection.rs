// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license. See LICENSE-MIT-PowerToys in the repository root.
// Adapted from FancyZonesLib/{Layout,HighlightedZones,DraggingState}.cpp,
// PowerToys 8827f848. This module performs no platform input capture.

use std::collections::BTreeSet;

use crate::geometry::{Point, Rect};
use crate::layout::Zone;
use crate::model::ZoneId;
use crate::settings::{OverlappingZonesAlgorithm, ZonesSettings};
use crate::{Result, ZonesError};

fn validate(zones: &[Zone]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for zone in zones {
        zone.rect.validate()?;
        if !ids.insert(zone.id) {
            return Err(ZonesError::InvalidData("duplicate zone ID".into()));
        }
    }
    Ok(())
}

pub fn zones_from_point(
    zones: &[Zone],
    point: Point,
    radius: i32,
    algorithm: OverlappingZonesAlgorithm,
) -> Result<Vec<ZoneId>> {
    validate(zones)?;
    if !point.is_finite() || radius < 0 {
        return Err(ZonesError::InvalidData(
            "invalid pointer or sensitivity radius".into(),
        ));
    }
    let r = f64::from(radius);
    let mut candidates: Vec<&Zone> = zones
        .iter()
        .filter(|z| {
            point.x >= f64::from(z.rect.x) - r
                && point.x <= z.rect.right() as f64 + r
                && point.y >= f64::from(z.rect.y) - r
                && point.y <= z.rect.bottom() as f64 + r
        })
        .collect();
    candidates.sort_by_key(|z| z.id);
    if candidates.len() == 1 && !candidates[0].rect.contains(point) {
        return Ok(Vec::new());
    }
    let overlap = candidates.iter().enumerate().any(|(i, a)| {
        candidates.iter().skip(i + 1).any(|b| {
            i64::from(a.rect.x.max(b.rect.x)) + i64::from(radius)
                < a.rect.right().min(b.rect.right())
                && i64::from(a.rect.y.max(b.rect.y)) + i64::from(radius)
                    < a.rect.bottom().min(b.rect.bottom())
        })
    });
    if !overlap {
        return Ok(candidates.iter().map(|z| z.id).collect());
    }
    let center = |z: &Zone| {
        Point::new(
            ((i64::from(z.rect.x) + z.rect.right()) / 2) as f64,
            ((i64::from(z.rect.y) + z.rect.bottom()) / 2) as f64,
        )
    };
    let distance = |a: Point, b: Point| (a.x - b.x).powi(2) + (a.y - b.y).powi(2);
    let chosen = if algorithm == OverlappingZonesAlgorithm::Positional {
        let half = f64::from(radius / 2);
        let (mut left, mut top, mut right, mut bottom) = (
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        );
        for z in &candidates {
            left = left.max(f64::from(z.rect.x) - half);
            top = top.max(f64::from(z.rect.y) - half);
            right = right.min(z.rect.right() as f64 + half);
            bottom = bottom.min(z.rect.bottom() as f64 + half);
        }
        let (width, height) = ((right - left).max(1.0), (bottom - top).max(1.0));
        let fraction = if height > width {
            (point.y - top) / height
        } else {
            (point.x - left) / width
        };
        let index =
            (fraction * candidates.len() as f64).clamp(0.0, (candidates.len() - 1) as f64) as usize;
        candidates[index]
    } else {
        let mut chosen = candidates[0];
        for candidate in candidates.iter().skip(1).copied() {
            let replace = match algorithm {
                OverlappingZonesAlgorithm::Smallest => candidate.rect.area() < chosen.rect.area(),
                OverlappingZonesAlgorithm::Largest => candidate.rect.area() > chosen.rect.area(),
                OverlappingZonesAlgorithm::ClosestCenter => {
                    if distance(center(candidate), center(chosen)) > 75.0 {
                        distance(center(candidate), point) < distance(center(chosen), point)
                    } else {
                        candidate.rect.area() < chosen.rect.area()
                    }
                }
                OverlappingZonesAlgorithm::Positional => unreachable!(),
            };
            if replace {
                chosen = candidate;
            }
        }
        chosen
    };
    Ok(vec![chosen.id])
}

pub fn combined_zones_rect(zones: &[Zone], ids: &[ZoneId]) -> Result<Option<Rect>> {
    validate(zones)?;
    let mut result: Option<Rect> = None;
    for id in ids {
        let rect = zones
            .iter()
            .find(|z| z.id == *id)
            .ok_or_else(|| ZonesError::InvalidOperation(format!("unknown zone {id}")))?
            .rect;
        result = Some(match result {
            Some(current) => current.union(rect)?,
            None => rect,
        });
    }
    Ok(result)
}

pub fn combined_zone_range(
    zones: &[Zone],
    initial: &[ZoneId],
    current: &[ZoneId],
) -> Result<Vec<ZoneId>> {
    let ids: Vec<_> = initial
        .iter()
        .chain(current)
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let Some(bounds) = combined_zones_rect(zones, &ids)? else {
        return Ok(Vec::new());
    };
    let mut result: Vec<_> = zones
        .iter()
        .filter(|z| {
            z.rect.x >= bounds.x
                && z.rect.y >= bounds.y
                && z.rect.right() <= bounds.right()
                && z.rect.bottom() <= bounds.bottom()
        })
        .map(|z| z.id)
        .collect();
    result.sort_unstable();
    Ok(result)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HighlightedZones {
    initial: Vec<ZoneId>,
    current: Vec<ZoneId>,
}

impl HighlightedZones {
    pub fn zones(&self) -> &[ZoneId] {
        &self.current
    }
    pub fn reset(&mut self) {
        self.initial.clear();
        self.current.clear();
    }
    pub fn update(
        &mut self,
        zones: &[Zone],
        point: Point,
        radius: i32,
        algorithm: OverlappingZonesAlgorithm,
        select_many: bool,
    ) -> Result<bool> {
        let mut selected = zones_from_point(zones, point, radius, algorithm)?;
        if select_many {
            if self.initial.is_empty() {
                self.initial.clone_from(&selected);
            } else {
                selected = combined_zone_range(zones, &self.initial, &selected)?;
            }
        } else {
            self.initial.clear();
        }
        let changed = selected != self.current;
        self.current = selected;
        Ok(changed)
    }
}

/// Pure input policy; the platform adapter supplies events and re-evaluates the
/// highlight even when a modifier changes without pointer motion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InteractionState {
    active: bool,
    shift: bool,
    ctrl: bool,
    secondary_toggle: bool,
    middle_toggle: bool,
}

impl InteractionState {
    pub fn begin(&mut self, shift: bool, ctrl: bool) {
        *self = Self {
            active: true,
            shift,
            ctrl,
            ..Self::default()
        };
    }
    pub fn end(&mut self) {
        *self = Self::default();
    }
    pub fn set_modifiers(&mut self, shift: bool, ctrl: bool) {
        self.shift = shift;
        self.ctrl = ctrl;
    }
    pub fn secondary_click(&mut self) {
        if self.active {
            self.secondary_toggle = !self.secondary_toggle;
        }
    }
    pub fn middle_click(&mut self, settings: &ZonesSettings) {
        if self.active {
            if settings.mouse_middle_click_spanning_multiple_zones {
                self.middle_toggle = !self.middle_toggle;
            } else {
                self.secondary_toggle = !self.secondary_toggle;
            }
        }
    }
    pub fn snapping(&self, settings: &ZonesSettings) -> bool {
        self.active && (self.shift ^ self.secondary_toggle) == settings.shift_drag
    }
    pub fn select_many(&self) -> bool {
        self.active && (self.ctrl || self.middle_toggle)
    }
    pub fn wheel_switch_enabled(&self, settings: &ZonesSettings) -> bool {
        self.snapping(settings) && settings.mouse_wheel_layout_switch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gutter_selects_both_adjacent_zones_but_far_outside_selects_none() {
        let zones = [
            Zone {
                id: 0,
                rect: Rect::new(0, 0, 90, 100),
            },
            Zone {
                id: 1,
                rect: Rect::new(110, 0, 90, 100),
            },
        ];
        assert_eq!(
            zones_from_point(
                &zones,
                Point::new(100.0, 50.0),
                20,
                OverlappingZonesAlgorithm::Smallest
            )
            .unwrap(),
            vec![0, 1]
        );
        assert!(
            zones_from_point(
                &zones,
                Point::new(-10.0, 50.0),
                20,
                OverlappingZonesAlgorithm::Smallest
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn overlapping_zone_algorithms_and_anchor_range_are_distinct() {
        let zones = [
            Zone {
                id: 0,
                rect: Rect::new(0, 0, 100, 100),
            },
            Zone {
                id: 1,
                rect: Rect::new(20, 20, 30, 30),
            },
        ];
        let p = Point::new(30.0, 30.0);
        assert_eq!(
            zones_from_point(&zones, p, 0, OverlappingZonesAlgorithm::Smallest).unwrap(),
            vec![1]
        );
        assert_eq!(
            zones_from_point(&zones, p, 0, OverlappingZonesAlgorithm::Largest).unwrap(),
            vec![0]
        );
        let row: Vec<_> = (0..3)
            .map(|id| Zone {
                id,
                rect: Rect::new(id as i32 * 100, 0, 100, 100),
            })
            .collect();
        assert_eq!(
            combined_zone_range(&row, &[0], &[2]).unwrap(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn stationary_modifier_changes_toggle_snap_and_end_clears_toggles() {
        let settings = ZonesSettings::default();
        let mut state = InteractionState::default();
        state.begin(false, false);
        assert!(!state.snapping(&settings));
        state.set_modifiers(true, true);
        assert!(state.snapping(&settings));
        assert!(state.select_many());
        state.secondary_click();
        assert!(!state.snapping(&settings));
        state.end();
        state.begin(false, false);
        assert!(!state.snapping(&settings));
    }
}
