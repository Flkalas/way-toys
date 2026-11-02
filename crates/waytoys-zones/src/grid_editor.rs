// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license. See LICENSE-MIT-PowerToys in the repository root.
// Rust adaptation of PowerToys 8827f848fbbac4afeedcae2907c71cd05fd5aa86:
// src/modules/fancyzones/editor/FancyZonesEditor/GridData.cs,
// GridEditor.xaml.cs, and Models/GridLayoutModel.cs.

//! UI-independent grid editing in integer ratios of [`GRID_MULTIPLIER`].
//!
//! The rectangle/model conversion, rectangular merge closure, split ordering,
//! and shared-boundary discovery follow upstream `GridData`. Every edit first
//! builds and validates a replacement, so an error leaves the editor unchanged.
//! Splits and merges renumber zones contiguously; resizer indices must be fetched
//! again after an edit because boundaries can join, separate, or disappear.
//!
//! This module does not implement WPF pointer/keyboard handling, pixel-to-ratio
//! conversion, split snapping, selection UI, or monitor/backend integration.
//! Unlike upstream's silent no-ops, invalid edits return errors. Merge insertion
//! uses the smallest ID in the *closure*, so a closure that adds an earlier zone
//! cannot insert outside the remaining zone list. Snapshots provide explicit
//! cancellation without a GUI or an implicit undo history.

use crate::model::{GRID_MULTIPLIER, GridLayout, MAX_ZONES};
use crate::{Result, ZonesError};

/// Coordinate changed by an operation, rather than the divider's orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// X coordinate: a vertical divider; left/right zone widths change.
    X,
    /// Y coordinate: a horizontal divider; top/bottom zone heights change.
    Y,
}

/// A zone's half-open rectangle in the `0..=10000` coordinate space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridZone {
    pub index: usize,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl GridZone {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    fn contains(&self, other: &Self) -> bool {
        self.left <= other.left
            && other.right <= self.right
            && self.top <= other.top
            && other.bottom <= self.bottom
    }

    fn intersects(&self, other: &Self) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }

    fn extend(&mut self, other: &Self) {
        self.left = self.left.min(other.left);
        self.top = self.top.min(other.top);
        self.right = self.right.max(other.right);
        self.bottom = self.bottom.max(other.bottom);
    }
}

/// One uninterrupted shared boundary. A spanning zone can separate two resizers
/// at the same coordinate; moving one of them does not move the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridResizer {
    pub axis: Axis,
    pub position: i32,
    /// Inclusive start along the other axis.
    pub start: i32,
    /// Exclusive end along the other axis.
    pub end: i32,
    /// Left (X) or upper (Y) zones, in geometric order, without repetitions.
    pub negative_side_indices: Vec<usize>,
    /// Right (X) or lower (Y) zones, in geometric order, without repetitions.
    pub positive_side_indices: Vec<usize>,
}

/// A validated editor checkpoint, including the minimum-size policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridSnapshot {
    layout: GridLayout,
    minimum_width: i32,
    minimum_height: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridEditor {
    layout: GridLayout,
    zones: Vec<GridZone>,
    resizers: Vec<GridResizer>,
    minimum_width: i32,
    minimum_height: i32,
}

impl GridEditor {
    pub fn new(layout: GridLayout) -> Result<Self> {
        layout.validate()?;
        let zones = model_to_zones(&layout);
        let resizers = model_to_resizers(&layout);
        Ok(Self {
            layout,
            zones,
            resizers,
            minimum_width: 1,
            minimum_height: 1,
        })
    }

    pub fn layout(&self) -> &GridLayout {
        &self.layout
    }

    pub fn into_layout(self) -> GridLayout {
        self.layout
    }

    pub fn zones(&self) -> &[GridZone] {
        &self.zones
    }

    /// Follows upstream ordering: horizontal dividers (Y) first, then vertical
    /// dividers (X), each in increasing coordinate and span order.
    pub fn resizers(&self) -> &[GridResizer] {
        &self.resizers
    }

    /// Minimum width and height in ratio units, **not pixels**. Values must be
    /// between 1 and 10000. This constrains subsequent split/drag operations and
    /// does not resize existing zones, matching upstream's adjustable policy.
    pub fn set_minimum_zone_size(&mut self, width: i32, height: i32) -> Result<()> {
        if !(1..=GRID_MULTIPLIER).contains(&width) || !(1..=GRID_MULTIPLIER).contains(&height) {
            return Err(invalid_operation(
                "minimum zone dimensions must be between 1 and 10000 ratio units",
            ));
        }
        self.minimum_width = width;
        self.minimum_height = height;
        Ok(())
    }

    pub fn minimum_zone_size(&self) -> (i32, i32) {
        (self.minimum_width, self.minimum_height)
    }

    pub fn snapshot(&self) -> GridSnapshot {
        GridSnapshot {
            layout: self.layout.clone(),
            minimum_width: self.minimum_width,
            minimum_height: self.minimum_height,
        }
    }

    /// Restore a checkpoint atomically. Snapshots are reusable by cloning them.
    pub fn restore(&mut self, snapshot: GridSnapshot) -> Result<()> {
        let mut restored = Self::new(snapshot.layout)?;
        restored.set_minimum_zone_size(snapshot.minimum_width, snapshot.minimum_height)?;
        *self = restored;
        Ok(())
    }

    /// `position` is a global coordinate, not a fraction of the selected zone.
    /// A missing zone is an error; a position blocked by the minimum size or
    /// zone-count limit returns `false`.
    pub fn can_split(&self, zone: usize, axis: Axis, position: i32) -> Result<bool> {
        let zone = self.zone(zone)?;
        let (start, end, minimum) = match axis {
            Axis::X => (zone.left, zone.right, self.minimum_width),
            Axis::Y => (zone.top, zone.bottom, self.minimum_height),
        };
        Ok(
            self.zones.len() < MAX_ZONES
                && start + minimum <= position
                && position <= end - minimum,
        )
    }

    /// Replace one zone with the negative-side zone followed by the positive-side
    /// zone, returning their new IDs. Later zone IDs increase by one.
    pub fn split(&mut self, zone: usize, axis: Axis, position: i32) -> Result<(usize, usize)> {
        if !self.can_split(zone, axis, position)? {
            return Err(invalid_operation(
                "split exceeds the zone limit or violates the zone's minimum size",
            ));
        }
        let mut zones = self.zones.clone();
        let mut second = zones[zone];
        match axis {
            Axis::X => {
                zones[zone].right = position;
                second.left = position;
            }
            Axis::Y => {
                zones[zone].bottom = position;
                second.top = position;
            }
        }
        zones.insert(zone + 1, second);
        self.replace_zones(zones)?;
        Ok((zone, zone + 1))
    }

    /// Expand a nonempty selection to a rectangle containing whole zones.
    /// Positive-area intersections recursively enlarge the rectangle; merely
    /// touching its edge or corner does not pull an extra zone into the closure.
    /// Duplicate IDs are accepted, but missing IDs and empty selections fail.
    pub fn merge_closure(&self, indices: &[usize]) -> Result<Vec<usize>> {
        self.compute_closure(indices).map(|(indices, _)| indices)
    }

    /// Merge the rectangular closure, return its new ID, and compact later IDs.
    /// A single-zone selection is valid. Removing a grid zone is represented by
    /// merging it with neighbors; a standalone deletion would leave a hole.
    pub fn merge(&mut self, indices: &[usize]) -> Result<usize> {
        let (closure, mut merged) = self.compute_closure(indices)?;
        let insertion = closure[0];
        merged.index = insertion;
        let mut removed = vec![false; self.zones.len()];
        for index in closure {
            removed[index] = true;
        }
        let mut zones: Vec<_> = self
            .zones
            .iter()
            .copied()
            .filter(|zone| !removed[zone.index])
            .collect();
        // Every earlier ID survives because insertion is the lowest closure ID.
        zones.insert(insertion, merged);
        self.replace_zones(zones)?;
        Ok(insertion)
    }

    /// Check every zone adjoining this shared boundary. Arithmetic uses i64 so
    /// even extreme untrusted deltas are rejected without integer overflow.
    pub fn can_drag_resizer(&self, resizer: usize, delta: i32) -> Result<bool> {
        let resizer = self.resizer(resizer)?;
        let (minimum, size): (i32, fn(&GridZone) -> i32) = match resizer.axis {
            Axis::X => (self.minimum_width, GridZone::width),
            Axis::Y => (self.minimum_height, GridZone::height),
        };
        let delta = i64::from(delta);
        let minimum = i64::from(minimum);
        Ok(resizer
            .negative_side_indices
            .iter()
            .all(|&index| i64::from(size(&self.zones[index])) + delta >= minimum)
            && resizer
                .positive_side_indices
                .iter()
                .all(|&index| i64::from(size(&self.zones[index])) - delta >= minimum))
    }

    /// Move one complete shared boundary by `delta` ratio units. Positive deltas
    /// move right/down. This applies a delta relative to the current layout;
    /// callers using cumulative gesture deltas should restore their snapshot
    /// before each update. Fetch resizers again after a successful mutation.
    pub fn drag_resizer(&mut self, resizer: usize, delta: i32) -> Result<()> {
        if !self.can_drag_resizer(resizer, delta)? {
            return Err(invalid_operation(
                "divider movement violates a minimum zone size",
            ));
        }
        let resizer = &self.resizers[resizer];
        let mut zones = self.zones.clone();
        for &index in &resizer.negative_side_indices {
            let edge = match resizer.axis {
                Axis::X => &mut zones[index].right,
                Axis::Y => &mut zones[index].bottom,
            };
            *edge = edge
                .checked_add(delta)
                .ok_or_else(|| invalid_operation("divider coordinate overflow"))?;
        }
        for &index in &resizer.positive_side_indices {
            let edge = match resizer.axis {
                Axis::X => &mut zones[index].left,
                Axis::Y => &mut zones[index].top,
            };
            *edge = edge
                .checked_add(delta)
                .ok_or_else(|| invalid_operation("divider coordinate overflow"))?;
        }
        self.replace_zones(zones)
    }

    fn zone(&self, index: usize) -> Result<&GridZone> {
        self.zones
            .get(index)
            .ok_or_else(|| invalid_operation(format!("zone {index} does not exist")))
    }

    fn resizer(&self, index: usize) -> Result<&GridResizer> {
        self.resizers
            .get(index)
            .ok_or_else(|| invalid_operation(format!("resizer {index} does not exist")))
    }

    fn compute_closure(&self, indices: &[usize]) -> Result<(Vec<usize>, GridZone)> {
        let first = *indices
            .first()
            .ok_or_else(|| invalid_operation("merge requires a nonempty zone selection"))?;
        let mut bounds = *self.zone(first)?;
        for &index in &indices[1..] {
            bounds.extend(self.zone(index)?);
        }

        loop {
            let mut changed = false;
            for zone in &self.zones {
                if bounds.intersects(zone) && !bounds.contains(zone) {
                    bounds.extend(zone);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let closure = self
            .zones
            .iter()
            .filter(|zone| bounds.contains(zone))
            .map(|zone| zone.index)
            .collect();
        Ok((closure, bounds))
    }

    fn replace_zones(&mut self, zones: Vec<GridZone>) -> Result<()> {
        let layout = zones_to_model(&zones, &self.layout)?;
        let mut replacement = Self::new(layout)?;
        replacement.minimum_width = self.minimum_width;
        replacement.minimum_height = self.minimum_height;
        *self = replacement;
        Ok(())
    }
}

fn invalid_operation(message: impl Into<String>) -> ZonesError {
    ZonesError::InvalidOperation(message.into())
}

fn prefix_sum(percentages: &[i32]) -> Vec<i32> {
    let mut result = Vec::with_capacity(percentages.len() + 1);
    result.push(0);
    let mut sum = 0;
    for &percentage in percentages {
        sum += percentage;
        result.push(sum);
    }
    result
}

/// Requires a validated layout, including contiguous IDs and full rectangles.
fn model_to_zones(layout: &GridLayout) -> Vec<GridZone> {
    let x = prefix_sum(&layout.columns_percentage);
    let y = prefix_sum(&layout.rows_percentage);
    let mut zones: Vec<_> = (0..layout.zone_count())
        .map(|index| GridZone {
            index,
            left: GRID_MULTIPLIER,
            top: GRID_MULTIPLIER,
            right: 0,
            bottom: 0,
        })
        .collect();
    for (row, cells) in layout.cell_child_map.iter().enumerate() {
        for (column, &index) in cells.iter().enumerate() {
            let zone = &mut zones[index];
            zone.left = zone.left.min(x[column]);
            zone.top = zone.top.min(y[row]);
            zone.right = zone.right.max(x[column + 1]);
            zone.bottom = zone.bottom.max(y[row + 1]);
        }
    }
    zones
}

fn push_unique(indices: &mut Vec<usize>, index: usize) {
    if indices.last() != Some(&index) {
        indices.push(index);
    }
}

fn model_to_resizers(layout: &GridLayout) -> Vec<GridResizer> {
    let mut result = Vec::new();
    let x = prefix_sum(&layout.columns_percentage);
    let y = prefix_sum(&layout.rows_percentage);
    let grid = &layout.cell_child_map;

    for row in 1..layout.rows {
        let mut column = 0;
        while column < layout.columns {
            if grid[row - 1][column] == grid[row][column] {
                column += 1;
                continue;
            }
            let start = column;
            let mut negative = Vec::new();
            let mut positive = Vec::new();
            while column < layout.columns && grid[row - 1][column] != grid[row][column] {
                push_unique(&mut negative, grid[row - 1][column]);
                push_unique(&mut positive, grid[row][column]);
                column += 1;
            }
            result.push(GridResizer {
                axis: Axis::Y,
                position: y[row],
                start: x[start],
                end: x[column],
                negative_side_indices: negative,
                positive_side_indices: positive,
            });
        }
    }

    for column in 1..layout.columns {
        let mut row = 0;
        while row < layout.rows {
            if grid[row][column - 1] == grid[row][column] {
                row += 1;
                continue;
            }
            let start = row;
            let mut negative = Vec::new();
            let mut positive = Vec::new();
            while row < layout.rows && grid[row][column - 1] != grid[row][column] {
                push_unique(&mut negative, grid[row][column - 1]);
                push_unique(&mut positive, grid[row][column]);
                row += 1;
            }
            result.push(GridResizer {
                axis: Axis::X,
                position: x[column],
                start: y[start],
                end: y[row],
                negative_side_indices: negative,
                positive_side_indices: positive,
            });
        }
    }
    result
}

/// Canonicalize coordinates and renumber by vector order, as ZonesToModel does.
/// Extra coverage/overlap checks prevent silently producing invalid cell maps.
fn zones_to_model(zones: &[GridZone], original: &GridLayout) -> Result<GridLayout> {
    if zones.is_empty() || zones.len() > MAX_ZONES {
        return Err(invalid_operation(
            "a grid must contain between 1 and 128 zones",
        ));
    }
    let mut x = vec![0, GRID_MULTIPLIER];
    let mut y = vec![0, GRID_MULTIPLIER];
    for zone in zones {
        if zone.left < 0
            || zone.top < 0
            || zone.right > GRID_MULTIPLIER
            || zone.bottom > GRID_MULTIPLIER
            || zone.left >= zone.right
            || zone.top >= zone.bottom
        {
            return Err(invalid_operation(
                "zone rectangle is empty or outside the grid",
            ));
        }
        x.extend([zone.left, zone.right]);
        y.extend([zone.top, zone.bottom]);
    }
    x.sort_unstable();
    x.dedup();
    y.sort_unstable();
    y.dedup();

    let mut cells = vec![vec![usize::MAX; x.len() - 1]; y.len() - 1];
    for (index, zone) in zones.iter().enumerate() {
        // All four coordinates were inserted above.
        let left = x.binary_search(&zone.left).unwrap();
        let right = x.binary_search(&zone.right).unwrap();
        let top = y.binary_search(&zone.top).unwrap();
        let bottom = y.binary_search(&zone.bottom).unwrap();
        for row in &mut cells[top..bottom] {
            for cell in &mut row[left..right] {
                if *cell != usize::MAX {
                    return Err(invalid_operation("edited zone rectangles overlap"));
                }
                *cell = index;
            }
        }
    }
    if cells.iter().flatten().any(|&cell| cell == usize::MAX) {
        return Err(invalid_operation(
            "edited zone rectangles leave an uncovered grid cell",
        ));
    }

    let mut result = original.clone();
    result.columns = x.len() - 1;
    result.rows = y.len() - 1;
    result.columns_percentage = x.windows(2).map(|pair| pair[1] - pair[0]).collect();
    result.rows_percentage = y.windows(2).map(|pair| pair[1] - pair[0]).collect();
    result.cell_child_map = cells;
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[i32], columns: &[i32], cells: &[&[usize]]) -> GridLayout {
        GridLayout::new(
            rows.len(),
            columns.len(),
            rows.to_vec(),
            columns.to_vec(),
            cells.iter().map(|row| row.to_vec()).collect(),
        )
        .unwrap()
    }

    fn resizer_at(editor: &GridEditor, axis: Axis, position: i32, start: i32) -> usize {
        editor
            .resizers()
            .iter()
            .position(|r| r.axis == axis && r.position == position && r.start == start)
            .unwrap()
    }

    fn assert_invariants(editor: &GridEditor) {
        editor.layout().validate().unwrap();
        for percentages in [
            &editor.layout().rows_percentage,
            &editor.layout().columns_percentage,
        ] {
            assert!(percentages.iter().all(|&value| value > 0));
            assert_eq!(percentages.iter().sum::<i32>(), GRID_MULTIPLIER);
        }
        assert_eq!(editor.zones().len(), editor.layout().zone_count());
        for (index, zone) in editor.zones().iter().enumerate() {
            assert_eq!(zone.index, index);
        }
        assert_eq!(
            editor
                .zones()
                .iter()
                .map(|zone| i64::from(zone.width()) * i64::from(zone.height()))
                .sum::<i64>(),
            i64::from(GRID_MULTIPLIER).pow(2)
        );
    }

    #[test]
    fn split_and_merge_restore_one_zone_and_preserve_metadata() {
        let layout = GridLayout {
            show_spacing: false,
            spacing: -10,
            sensitivity_radius: 321,
            ..GridLayout::default()
        };
        let mut editor = GridEditor::new(layout.clone()).unwrap();
        assert_eq!(editor.split(0, Axis::X, 3333).unwrap(), (0, 1));
        assert_eq!(editor.layout().columns_percentage, [3333, 6667]);
        assert_eq!(editor.split(1, Axis::Y, 6000).unwrap(), (1, 2));
        assert_eq!(editor.layout().cell_child_map, [vec![0, 1], vec![0, 2]]);
        assert_eq!(editor.zones()[0].bottom, 10000);
        assert_invariants(&editor);
        assert_eq!(editor.merge(&[1, 2]).unwrap(), 1);
        assert_eq!(editor.merge(&[0, 1]).unwrap(), 0);
        assert_eq!(editor.into_layout(), layout);
    }

    #[test]
    fn splitting_uses_global_coordinates_and_compacts_zone_order() {
        let mut editor = GridEditor::new(grid(&[10000], &[3000, 7000], &[&[0, 1]])).unwrap();
        assert!(!editor.can_split(1, Axis::X, 2000).unwrap());
        editor.split(0, Axis::Y, 1234).unwrap();
        assert_eq!(editor.layout().cell_child_map, [vec![0, 2], vec![1, 2]]);
        editor.split(2, Axis::X, 7000).unwrap();
        assert_eq!(editor.zones()[2].left, 3000);
        assert_eq!(editor.zones()[2].right, 7000);
        assert_eq!(editor.zones()[3].left, 7000);
        assert_invariants(&editor);
    }

    #[test]
    fn closure_recursively_includes_intersections_and_earlier_ids() {
        // Selecting the top-left and lower-right interior zones first cuts
        // zone 0, which expands downward into zone 2; it then expands right.
        let layout = grid(
            &[2500, 2500, 2500, 2500],
            &[2500, 2500, 2500, 2500],
            &[&[1, 0, 4, 4], &[5, 0, 3, 6], &[5, 0, 2, 2], &[7, 7, 2, 2]],
        );
        let mut editor = GridEditor::new(layout).unwrap();
        assert_eq!(
            editor.merge_closure(&[1, 3]).unwrap(),
            (0..8).collect::<Vec<_>>()
        );
        assert_eq!(editor.merge(&[1, 3]).unwrap(), 0);
        assert_eq!(editor.layout(), &GridLayout::default());
        assert_invariants(&editor);
    }

    #[test]
    fn closure_ignores_edge_contacts_and_fills_corner_selection() {
        let layout = grid(&[5000, 5000], &[5000, 5000], &[&[0, 1], &[2, 3]]);
        let mut editor = GridEditor::new(layout).unwrap();
        assert_eq!(editor.merge_closure(&[0, 0]).unwrap(), [0]);
        assert_eq!(editor.merge_closure(&[0, 1]).unwrap(), [0, 1]);
        // Diagonally selected zones cannot leave a disconnected merged zone.
        assert_eq!(editor.merge_closure(&[0, 3]).unwrap(), [0, 1, 2, 3]);
        editor.merge(&[1, 3]).unwrap();
        assert_eq!(editor.layout().cell_child_map, [vec![0, 1], vec![2, 1]]);
        assert_invariants(&editor);
    }

    #[test]
    fn shared_divider_updates_all_adjoining_zones_and_honors_minima() {
        let mut editor =
            GridEditor::new(grid(&[5000, 5000], &[5000, 5000], &[&[0, 1], &[0, 2]])).unwrap();
        editor.set_minimum_zone_size(4000, 2500).unwrap();
        let divider = resizer_at(&editor, Axis::X, 5000, 0);
        assert_eq!(editor.resizers()[divider].negative_side_indices, [0]);
        assert_eq!(editor.resizers()[divider].positive_side_indices, [1, 2]);
        let before = editor.clone();
        assert!(!editor.can_drag_resizer(divider, 1001).unwrap());
        assert!(editor.drag_resizer(divider, 1001).is_err());
        assert_eq!(editor, before);
        for delta in [i32::MIN, i32::MAX] {
            assert!(!editor.can_drag_resizer(divider, delta).unwrap());
            assert!(editor.drag_resizer(divider, delta).is_err());
            assert_eq!(editor, before);
        }
        editor.drag_resizer(divider, 1000).unwrap();
        assert_eq!(editor.zones()[0].right, 6000);
        assert_eq!(editor.zones()[1].left, 6000);
        assert_eq!(editor.zones()[2].left, 6000);
        let horizontal = resizer_at(&editor, Axis::Y, 5000, 6000);
        editor.drag_resizer(horizontal, -2500).unwrap();
        assert_eq!(editor.zones()[1].bottom, 2500);
        assert_eq!(editor.zones()[2].top, 2500);
        assert_eq!(editor.zones()[0].height(), 10000);
        assert_invariants(&editor);
    }

    #[test]
    fn separated_dividers_move_independently_and_coordinates_rejoin() {
        let layout = grid(
            &[3000, 4000, 3000],
            &[5000, 5000],
            &[&[0, 1], &[2, 2], &[3, 4]],
        );
        let mut editor = GridEditor::new(layout.clone()).unwrap();
        let top = resizer_at(&editor, Axis::X, 5000, 0);
        editor.drag_resizer(top, 1000).unwrap();
        assert_eq!(editor.layout().columns_percentage, [5000, 1000, 4000]);
        assert_eq!(editor.zones()[0].right, 6000);
        assert_eq!(editor.zones()[3].right, 5000);
        assert_invariants(&editor);
        let top = resizer_at(&editor, Axis::X, 6000, 0);
        editor.drag_resizer(top, -1000).unwrap();
        assert_eq!(editor.layout(), &layout);
        assert_invariants(&editor);
    }

    #[test]
    fn snapshot_cancels_a_sequence_and_restores_minimum_policy() {
        let mut editor = GridEditor::new(GridLayout::default()).unwrap();
        editor.set_minimum_zone_size(1000, 1500).unwrap();
        let before = editor.clone();
        let checkpoint = editor.snapshot();
        editor.split(0, Axis::X, 4567).unwrap();
        editor.split(1, Axis::Y, 7654).unwrap();
        editor.set_minimum_zone_size(1, 1).unwrap();
        editor.restore(checkpoint.clone()).unwrap();
        assert_eq!(editor, before);
        editor.restore(checkpoint).unwrap();
        assert_eq!(editor, before);
    }

    #[test]
    fn malformed_operations_are_atomic_and_do_not_panic() {
        let mut editor = GridEditor::new(GridLayout::default()).unwrap();
        let before = editor.clone();
        for position in [i32::MIN, -1, 0, 10000, 10001, i32::MAX] {
            assert!(editor.split(0, Axis::X, position).is_err());
            assert_eq!(editor, before);
        }
        assert!(editor.split(usize::MAX, Axis::Y, 5000).is_err());
        assert!(editor.merge(&[]).is_err());
        assert!(editor.merge(&[0, usize::MAX]).is_err());
        assert!(editor.merge_closure(&[]).is_err());
        assert!(editor.drag_resizer(0, 1).is_err());
        assert!(editor.set_minimum_zone_size(0, 1).is_err());
        assert!(editor.set_minimum_zone_size(1, i32::MAX).is_err());
        assert_eq!(editor, before);
    }

    #[test]
    fn split_obeys_exact_minimum_and_zone_count_limit() {
        let mut editor = GridEditor::new(GridLayout::default()).unwrap();
        editor.set_minimum_zone_size(2000, 2000).unwrap();
        assert!(!editor.can_split(0, Axis::X, 1999).unwrap());
        editor.split(0, Axis::X, 2000).unwrap();
        assert!(!editor.can_split(0, Axis::X, 1000).unwrap());

        let columns: Vec<_> = (0..MAX_ZONES)
            .map(|i| {
                (GRID_MULTIPLIER * (i as i32 + 1) / MAX_ZONES as i32)
                    - (GRID_MULTIPLIER * i as i32 / MAX_ZONES as i32)
            })
            .collect();
        let cells: Vec<_> = (0..MAX_ZONES).collect();
        let mut editor = GridEditor::new(grid(&[10000], &columns, &[&cells])).unwrap();
        let before = editor.clone();
        assert!(!editor.can_split(0, Axis::Y, 5000).unwrap());
        assert!(editor.split(0, Axis::Y, 5000).is_err());
        assert_eq!(editor, before);
    }

    #[test]
    fn invalid_shapes_and_coverage_are_rejected() {
        let mut layout = GridLayout {
            rows: 2,
            columns: 2,
            rows_percentage: vec![5000, 5000],
            columns_percentage: vec![5000, 5000],
            cell_child_map: vec![vec![0, 1], vec![1, 0]],
            ..GridLayout::default()
        };
        assert!(GridEditor::new(layout.clone()).is_err());
        layout.cell_child_map = vec![vec![0, 0], vec![0, 1]];
        assert!(GridEditor::new(layout.clone()).is_err());
        layout.cell_child_map = vec![vec![0, 2], vec![0, 2]];
        assert!(GridEditor::new(layout).is_err());

        let editor = GridEditor::new(GridLayout::default()).unwrap();
        let mut half = editor.zones()[0];
        half.right = 5000;
        assert!(zones_to_model(&[half], editor.layout()).is_err());
        assert!(zones_to_model(&[half, editor.zones()[0]], editor.layout()).is_err());
    }
}
