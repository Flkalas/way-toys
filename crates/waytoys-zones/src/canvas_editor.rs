// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license. See LICENSE-MIT-PowerToys in the repository root.
// Rust adaptation of PowerToys 8827f848fbbac4afeedcae2907c71cd05fd5aa86:
// src/modules/fancyzones/editor/FancyZonesEditor/Models/CanvasLayoutModel.cs,
// CanvasZone.xaml.cs, EditorWindow.cs, and LayoutBackup.cs.

//! Portable canvas editing in integer work-area-local logical units.
//!
//! Preserved source contracts include the 40% default zone, 100/50 placement
//! offsets, 64x72 resize minimum, eight handles, sequential centered keyboard
//! resizing, and the canvas-specific magnetic transfer curve. There is one
//! rectangular work area; monitor enumeration, DPI conversion, focus, z-order,
//! persistence, and WPF event routing belong to a frontend.
//!
//! A source model can contain off-canvas or smaller-than-minimum zones. Loading,
//! adding and scaling retain those states; drag helpers constrain the affected
//! edges as in the source. Impossible helper bounds return an error rather than
//! create an invalid rectangle. Scale truncation can lose units and is not
//! generally reversible. Session commit/cancel follows the source backup model;
//! grouped drag cancellation and per-operation undo are explicit portable
//! extensions, not a claim that PowerToys implements an undo stack.

use crate::geometry::Size;
use crate::magnetic_snap::attract;
use crate::model::{CanvasLayout, CanvasZone, MAX_ZONES};
use crate::{Result, ZonesError};

pub const MIN_ZONE_WIDTH: i32 = 64;
pub const MIN_ZONE_HEIGHT: i32 = 72;
const DEFAULT_OFFSET: i32 = 100;
const OFFSET_SHIFT: i32 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeHandle {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    Move,
    Resize(ResizeHandle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasKey {
    Left,
    Right,
    Up,
    Down,
    Delete,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub shift: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    layout: CanvasLayout,
    next_offset: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AxisMode {
    LowEdge,
    HighEdge,
    Move,
}

#[derive(Clone, Debug)]
struct AxisSnap {
    mode: AxisMode,
    position: i64,
    low: i64,
    high: i64,
    magnets: Vec<(i64, i64)>,
}

impl AxisSnap {
    fn new(
        layout: &CanvasLayout,
        index: usize,
        is_x: bool,
        mode: AxisMode,
        magnetic: bool,
    ) -> Result<Self> {
        let zone = get_zone(layout, index)?;
        let (position, size, screen, minimum) = if is_x {
            (zone.x, zone.width, layout.ref_width, MIN_ZONE_WIDTH)
        } else {
            (zone.y, zone.height, layout.ref_height, MIN_ZONE_HEIGHT)
        };
        let (position, size, screen, minimum) = (
            i64::from(position),
            i64::from(size),
            i64::from(screen),
            i64::from(minimum),
        );
        let (low, high, position) = match mode {
            AxisMode::LowEdge => (0, position + size - minimum, position),
            AxisMode::HighEdge => (position + minimum, screen, position + size),
            AxisMode::Move => (0, screen - size, position),
        };
        if low > high {
            return Err(ZonesError::InvalidOperation(
                "zone cannot satisfy this drag's work-area bounds and minimum size".into(),
            ));
        }
        let mut magnets = Vec::new();
        if magnetic {
            let mut points = Vec::new();
            for (other_index, other) in layout.zones.iter().enumerate() {
                if other_index == index {
                    continue;
                }
                let (origin, extent) = if is_x {
                    (other.x, other.width)
                } else {
                    (other.y, other.height)
                };
                let (origin, extent) = (i64::from(origin), i64::from(extent));
                points.extend([origin, origin + extent]);
                if mode == AxisMode::Move {
                    points.extend([origin - size, origin + extent - size]);
                }
            }
            points.extend([0, screen]);
            if mode == AxisMode::Move {
                points.extend([-size, screen - size]);
            }
            points.sort_unstable();
            points.dedup();
            // Source: (int)(0.08 * ScreenW), then halve the minimum gap.
            let maximum = (0.08 * screen as f64) as i64;
            for (index, &point) in points.iter().enumerate() {
                let previous = if index == 0 { 0 } else { points[index - 1] };
                let next = points.get(index + 1).copied().unwrap_or(screen);
                let radius = (point - previous).min(next - point).min(maximum) / 2;
                magnets.push((point, radius));
            }
        }
        Ok(Self {
            mode,
            position,
            low,
            high,
            magnets,
        })
    }

    fn move_by(&mut self, delta: i32) {
        // The source starts each event from its previous snapped Position. It
        // does not accumulate an independent unsnapped mouse position.
        let free_position = self.position + i64::from(delta);
        let snapped = self
            .magnets
            .iter()
            .find(|&&(point, radius)| (free_position - point).abs() <= radius)
            .map(|&(point, radius)| attract(free_position, point, radius))
            .unwrap_or(free_position);
        self.position = snapped.clamp(self.low, self.high);
    }
}

#[derive(Clone, Debug)]
struct DragState {
    before: Snapshot,
    index: usize,
    x: Option<AxisSnap>,
    y: Option<AxisSnap>,
}

#[derive(Clone, Debug)]
pub struct CanvasEditor {
    state: Snapshot,
    committed: Snapshot,
    history: Vec<Snapshot>,
    drag: Option<DragState>,
}

impl CanvasEditor {
    pub fn new(layout: CanvasLayout) -> Result<Self> {
        layout.validate()?;
        let state = Snapshot {
            layout,
            next_offset: DEFAULT_OFFSET,
        };
        Ok(Self {
            committed: state.clone(),
            state,
            history: Vec::new(),
            drag: None,
        })
    }

    pub fn layout(&self) -> &CanvasLayout {
        &self.state.layout
    }

    /// Consume the editor and return its current preview, including an active
    /// drag. Call `cancel_drag` or `cancel` first if that preview is unwanted.
    pub fn into_layout(self) -> CanvasLayout {
        self.state.layout
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn is_dirty(&self) -> bool {
        self.state != self.committed
    }

    /// Add at the source's 40%-of-work-area size and advancing diagonal offset.
    /// If the zone touches/exceeds the far edge, reset both origins to 100.
    /// As in the source, an exceptionally small work area can still overflow
    /// after that reset; no undocumented fit-to-screen adjustment is applied.
    pub fn add_default_zone(&mut self) -> Result<usize> {
        self.require_idle()?;
        self.require_zone_capacity()?;
        let mut next = self.state.clone();
        if next.layout.zones.is_empty() {
            next.next_offset = DEFAULT_OFFSET;
        } else if next
            .layout
            .zones
            .last()
            .is_some_and(|zone| zone.x == next.next_offset)
        {
            next.next_offset = increment_offset(next.next_offset)?;
        }
        let width = (f64::from(next.layout.ref_width) * 0.4) as i32;
        let height = (f64::from(next.layout.ref_height) * 0.4) as i32;
        if i64::from(next.next_offset) + i64::from(width) >= i64::from(next.layout.ref_width)
            || i64::from(next.next_offset) + i64::from(height) >= i64::from(next.layout.ref_height)
        {
            next.next_offset = DEFAULT_OFFSET;
        }
        let index = next.layout.zones.len();
        next.layout.zones.push(CanvasZone::new(
            next.next_offset,
            next.next_offset,
            width,
            height,
        ));
        next.next_offset = increment_offset(next.next_offset)?;
        self.apply(next)?;
        Ok(index)
    }

    pub fn add_zone(&mut self, zone: CanvasZone) -> Result<usize> {
        self.require_idle()?;
        self.require_zone_capacity()?;
        let mut next = self.state.clone();
        let index = next.layout.zones.len();
        next.layout.zones.push(zone);
        self.apply(next)?;
        Ok(index)
    }

    pub fn remove_zone(&mut self, index: usize) -> Result<CanvasZone> {
        self.require_idle()?;
        let zone = get_zone(self.layout(), index)?;
        let mut next = self.state.clone();
        next.layout.zones.remove(index);
        self.apply(next)?;
        Ok(zone)
    }

    /// Begin a gesture, freezing its magnetic neighbors and opposite edges.
    /// Deltas supplied to `drag_by` are incremental, not relative to its start.
    pub fn begin_drag(&mut self, index: usize, kind: DragKind, magnetic: bool) -> Result<()> {
        self.require_idle()?;
        get_zone(self.layout(), index)?;
        let (x_mode, y_mode) = axis_modes(kind);
        let x = x_mode
            .map(|mode| AxisSnap::new(self.layout(), index, true, mode, magnetic))
            .transpose()?;
        let y = y_mode
            .map(|mode| AxisSnap::new(self.layout(), index, false, mode, magnetic))
            .transpose()?;
        self.drag = Some(DragState {
            before: self.state.clone(),
            index,
            x,
            y,
        });
        Ok(())
    }

    /// Apply an incremental gesture update atomically. Integer extremes clamp
    /// safely to valid helper bounds rather than overflowing intermediate sums.
    pub fn drag_by(&mut self, dx: i32, dy: i32) -> Result<()> {
        let mut drag = self.drag.clone().ok_or_else(no_drag)?;
        if let Some(snap) = drag.x.as_mut() {
            snap.move_by(dx);
        }
        if let Some(snap) = drag.y.as_mut() {
            snap.move_by(dy);
        }
        let mut next = self.state.clone();
        next.layout.zones[drag.index] = apply_axes(
            get_zone(&next.layout, drag.index)?,
            drag.x.as_ref(),
            drag.y.as_ref(),
        )?;
        next.layout.validate()?;
        self.state = next;
        self.drag = Some(drag);
        Ok(())
    }

    /// Finish one gesture as one undoable operation.
    pub fn commit_drag(&mut self) -> Result<()> {
        let drag = self.drag.take().ok_or_else(no_drag)?;
        if drag.before != self.state {
            self.history.push(drag.before);
        }
        Ok(())
    }

    pub fn cancel_drag(&mut self) -> Result<()> {
        let drag = self.drag.take().ok_or_else(no_drag)?;
        self.state = drag.before;
        Ok(())
    }

    pub fn move_zone(&mut self, index: usize, dx: i32, dy: i32, magnetic: bool) -> Result<()> {
        self.one_drag(index, DragKind::Move, dx, dy, magnetic)
    }

    pub fn resize_zone(
        &mut self,
        index: usize,
        handle: ResizeHandle,
        dx: i32,
        dy: i32,
        magnetic: bool,
    ) -> Result<()> {
        self.one_drag(index, DragKind::Resize(handle), dx, dy, magnetic)
    }

    /// Source keyboard behavior: arrows move 10 units (Ctrl: 1), without
    /// magnets. Shift moves both edges sequentially by half that amount, at
    /// least 1: Right/Up grow and Left/Down shrink. Delete removes the zone.
    /// Tab, focus switching, and Escape routing belong to the frontend; Escape
    /// at editor scope can call `cancel`.
    pub fn keyboard(
        &mut self,
        index: usize,
        key: CanvasKey,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        self.require_idle()?;
        get_zone(self.layout(), index)?;
        if key == CanvasKey::Delete {
            return self.remove_zone(index).map(|_| ());
        }
        let mut amount = if modifiers.ctrl { 1 } else { 10 };
        let is_x = matches!(key, CanvasKey::Left | CanvasKey::Right);
        let positive = matches!(key, CanvasKey::Right | CanvasKey::Down);
        let mut next = self.state.clone();
        if modifiers.shift {
            amount = (amount / 2).max(1);
            let grows = matches!(key, CanvasKey::Right | CanvasKey::Up);
            let high_delta = if grows { amount } else { -amount };
            edit_one_axis(
                &mut next.layout,
                index,
                is_x,
                AxisMode::HighEdge,
                high_delta,
            )?;
            edit_one_axis(
                &mut next.layout,
                index,
                is_x,
                AxisMode::LowEdge,
                -high_delta,
            )?;
        } else {
            edit_one_axis(
                &mut next.layout,
                index,
                is_x,
                AxisMode::Move,
                if positive { amount } else { -amount },
            )?;
        }
        self.apply(next)
    }

    /// Resize the reference canvas and independently scale x/y/width/height,
    /// truncating toward zero exactly as `CanvasLayoutModel.ScaleLayout` does.
    /// A scale that collapses a positive extent to zero is rejected atomically.
    /// Integer scaling back need not restore lost fractions; `undo` does.
    pub fn scale_reference(&mut self, size: Size) -> Result<()> {
        self.require_idle()?;
        size.validate()?;
        let mut next = self.state.clone();
        for zone in &mut next.layout.zones {
            zone.x = scaled(zone.x, size.width, self.state.layout.ref_width)?;
            zone.y = scaled(zone.y, size.height, self.state.layout.ref_height)?;
            zone.width = scaled(zone.width, size.width, self.state.layout.ref_width)?;
            zone.height = scaled(zone.height, size.height, self.state.layout.ref_height)?;
        }
        next.layout.ref_width = size.width;
        next.layout.ref_height = size.height;
        self.apply(next)
    }

    /// Adopt current edits as the new session backup. This does not serialize
    /// data or perform platform operations, and requires a finished gesture.
    pub fn commit(&mut self) -> Result<()> {
        self.require_idle()?;
        self.committed = self.state.clone();
        self.history.clear();
        Ok(())
    }

    /// Restore the last committed backup, including any active preview.
    pub fn cancel(&mut self) {
        self.state = self.committed.clone();
        self.drag = None;
        self.history.clear();
    }

    /// Undo a completed operation. This portable extension is scoped to the
    /// current session; committing or cancelling clears its history.
    pub fn undo(&mut self) -> Result<bool> {
        self.require_idle()?;
        if let Some(previous) = self.history.pop() {
            self.state = previous;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn require_idle(&self) -> Result<()> {
        if self.drag.is_some() {
            Err(ZonesError::InvalidOperation(
                "finish or cancel the active canvas drag first".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn require_zone_capacity(&self) -> Result<()> {
        if self.layout().zones.len() >= MAX_ZONES {
            Err(ZonesError::InvalidOperation(
                "canvas already has the maximum of 128 zones".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn apply(&mut self, next: Snapshot) -> Result<()> {
        next.layout.validate()?;
        if next != self.state {
            self.history.push(self.state.clone());
            self.state = next;
        }
        Ok(())
    }

    fn one_drag(
        &mut self,
        index: usize,
        kind: DragKind,
        dx: i32,
        dy: i32,
        magnetic: bool,
    ) -> Result<()> {
        self.begin_drag(index, kind, magnetic)?;
        if let Err(error) = self.drag_by(dx, dy) {
            // A failed convenience operation must also restore interaction state.
            self.cancel_drag()?;
            return Err(error);
        }
        self.commit_drag()
    }
}

fn get_zone(layout: &CanvasLayout, index: usize) -> Result<CanvasZone> {
    layout.zones.get(index).copied().ok_or_else(|| {
        ZonesError::InvalidOperation(format!("canvas zone index {index} does not exist"))
    })
}

fn no_drag() -> ZonesError {
    ZonesError::InvalidOperation("there is no active canvas drag".into())
}

fn increment_offset(offset: i32) -> Result<i32> {
    offset
        .checked_add(OFFSET_SHIFT)
        .ok_or_else(|| ZonesError::InvalidData("default zone placement offset overflow".into()))
}

fn scaled(value: i32, new_extent: i32, old_extent: i32) -> Result<i32> {
    // Retain the source's double multiplication/division order, including its
    // rounding at large coordinates, followed by a truncating Int32 conversion.
    let scaled = (f64::from(value) * f64::from(new_extent) / f64::from(old_extent)).trunc();
    if scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
        return Err(ZonesError::InvalidData(
            "scaled canvas coordinate exceeds the integer range".into(),
        ));
    }
    Ok(scaled as i32)
}

fn axis_modes(kind: DragKind) -> (Option<AxisMode>, Option<AxisMode>) {
    use AxisMode::{HighEdge, LowEdge, Move};
    use ResizeHandle::*;
    match kind {
        DragKind::Move => (Some(Move), Some(Move)),
        DragKind::Resize(handle) => match handle {
            North => (None, Some(LowEdge)),
            NorthEast => (Some(HighEdge), Some(LowEdge)),
            East => (Some(HighEdge), None),
            SouthEast => (Some(HighEdge), Some(HighEdge)),
            South => (None, Some(HighEdge)),
            SouthWest => (Some(LowEdge), Some(HighEdge)),
            West => (Some(LowEdge), None),
            NorthWest => (Some(LowEdge), Some(LowEdge)),
        },
    }
}

fn apply_axes(
    mut zone: CanvasZone,
    x: Option<&AxisSnap>,
    y: Option<&AxisSnap>,
) -> Result<CanvasZone> {
    for (is_x, snap) in [(true, x), (false, y)] {
        if let Some(snap) = snap {
            let (origin, extent) = if is_x {
                (&mut zone.x, &mut zone.width)
            } else {
                (&mut zone.y, &mut zone.height)
            };
            let (next_origin, next_extent) = match snap.mode {
                AxisMode::LowEdge => (
                    snap.position,
                    i64::from(*origin) + i64::from(*extent) - snap.position,
                ),
                AxisMode::HighEdge => (i64::from(*origin), snap.position - i64::from(*origin)),
                AxisMode::Move => (snap.position, i64::from(*extent)),
            };
            *origin = i32::try_from(next_origin)
                .map_err(|_| ZonesError::InvalidData("canvas origin overflow".into()))?;
            *extent = i32::try_from(next_extent)
                .map_err(|_| ZonesError::InvalidData("canvas extent overflow".into()))?;
        }
    }
    Ok(zone)
}

fn edit_one_axis(
    layout: &mut CanvasLayout,
    index: usize,
    is_x: bool,
    mode: AxisMode,
    delta: i32,
) -> Result<()> {
    let mut snap = AxisSnap::new(layout, index, is_x, mode, false)?;
    snap.move_by(delta);
    let zone = get_zone(layout, index)?;
    layout.zones[index] = if is_x {
        apply_axes(zone, Some(&snap), None)?
    } else {
        apply_axes(zone, None, Some(&snap))?
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(zones: Vec<CanvasZone>) -> CanvasEditor {
        CanvasEditor::new(CanvasLayout {
            ref_width: 1000,
            ref_height: 800,
            zones,
            sensitivity_radius: 20,
        })
        .unwrap()
    }

    fn zone(x: i32, y: i32, width: i32, height: i32) -> CanvasZone {
        CanvasZone::new(x, y, width, height)
    }

    #[test]
    fn default_placement_wraps_at_touching_edge_and_avoids_last_origin() {
        let mut edit = editor(vec![]);
        edit.add_default_zone().unwrap();
        edit.add_default_zone().unwrap();
        assert_eq!(
            edit.layout().zones,
            vec![zone(100, 100, 400, 320), zone(150, 150, 400, 320)]
        );
        // Moving the last zone onto the pending offset adds an extra shift.
        edit.move_zone(1, 50, 0, false).unwrap();
        edit.add_default_zone().unwrap();
        assert_eq!(edit.layout().zones[2].x, 250);
        let mut edit = CanvasEditor::new(CanvasLayout::new(500, 500).unwrap()).unwrap();
        for _ in 0..5 {
            edit.add_default_zone().unwrap();
        }
        assert_eq!(
            edit.layout()
                .zones
                .iter()
                .map(|zone| zone.x)
                .collect::<Vec<_>>(),
            vec![100, 150, 200, 250, 100]
        );
        let mut tiny = CanvasEditor::new(CanvasLayout::new(100, 100).unwrap()).unwrap();
        tiny.add_default_zone().unwrap();
        assert_eq!(tiny.layout().zones[0], zone(100, 100, 40, 40));
    }

    #[test]
    fn magnets_align_both_moving_edges_to_neighbors_and_work_area() {
        let initial = vec![zone(100, 100, 200, 200), zone(500, 400, 200, 200)];
        let mut edit = editor(initial.clone());
        edit.move_zone(0, 190, 0, true).unwrap();
        assert_eq!(edit.layout().zones[0].x, 300); // Right edge meets neighbor's left.
        let mut edit = editor(initial);
        edit.move_zone(0, 390, 0, true).unwrap();
        assert_eq!(edit.layout().zones[0].x, 500); // Left edges align.
        let mut edit = editor(vec![zone(100, 100, 200, 200)]);
        edit.move_zone(0, 690, -95, true).unwrap();
        assert_eq!(edit.layout().zones[0], zone(800, 0, 200, 200));
    }

    #[test]
    fn magnetic_resize_uses_source_curve_and_incremental_snapped_position() {
        let mut edit = editor(vec![zone(100, 100, 200, 200), zone(500, 400, 200, 200)]);
        edit.begin_drag(0, DragKind::Resize(ResizeHandle::East), true)
            .unwrap();
        edit.drag_by(170, 0).unwrap(); // Free edge470; radius40 -> attracted to480.
        assert_eq!(edit.layout().zones[0].width, 380);
        edit.drag_by(5, 0).unwrap(); // Previous snapped480 +5 lies in the dead zone.
        assert_eq!(edit.layout().zones[0].width, 400);
        edit.commit_drag().unwrap();
        assert!(edit.undo().unwrap());
        assert_eq!(edit.layout().zones[0], zone(100, 100, 200, 200));
    }

    #[test]
    fn nonmagnetic_moves_and_all_resize_handles_obey_bounds_and_minimums() {
        let original = zone(100, 100, 200, 200);
        for (handle, expected) in [
            (ResizeHandle::North, zone(100, 110, 200, 190)),
            (ResizeHandle::NorthEast, zone(100, 110, 220, 190)),
            (ResizeHandle::East, zone(100, 100, 220, 200)),
            (ResizeHandle::SouthEast, zone(100, 100, 220, 210)),
            (ResizeHandle::South, zone(100, 100, 200, 210)),
            (ResizeHandle::SouthWest, zone(120, 100, 180, 210)),
            (ResizeHandle::West, zone(120, 100, 180, 200)),
            (ResizeHandle::NorthWest, zone(120, 110, 180, 190)),
        ] {
            let mut edit = editor(vec![original]);
            edit.resize_zone(0, handle, 20, 10, false).unwrap();
            assert_eq!(edit.layout().zones[0], expected, "{handle:?}");
            edit.resize_zone(0, handle, -20, -10, false).unwrap();
            assert_eq!(edit.layout().zones[0], original);
        }
        let mut edit = editor(vec![original]);
        edit.resize_zone(0, ResizeHandle::NorthWest, i32::MAX, i32::MAX, false)
            .unwrap();
        assert_eq!(edit.layout().zones[0], zone(236, 228, 64, 72));
        edit.resize_zone(0, ResizeHandle::SouthEast, i32::MIN, i32::MIN, false)
            .unwrap();
        assert_eq!(edit.layout().zones[0], zone(236, 228, 64, 72));
        edit.move_zone(0, i32::MAX, i32::MIN, false).unwrap();
        assert_eq!(edit.layout().zones[0], zone(936, 0, 64, 72));
        edit.resize_zone(0, ResizeHandle::SouthEast, i32::MAX, i32::MAX, false)
            .unwrap();
        assert_eq!(edit.layout().zones[0], zone(936, 0, 64, 800));
    }

    #[test]
    fn keyboard_matches_ctrl_shift_and_asymmetric_boundary_semantics() {
        let mut edit = editor(vec![zone(100, 100, 200, 200)]);
        edit.keyboard(0, CanvasKey::Right, KeyModifiers::default())
            .unwrap();
        edit.keyboard(
            0,
            CanvasKey::Down,
            KeyModifiers {
                ctrl: true,
                shift: false,
            },
        )
        .unwrap();
        assert_eq!(edit.layout().zones[0], zone(110, 101, 200, 200));
        let shift = KeyModifiers {
            shift: true,
            ctrl: false,
        };
        edit.keyboard(0, CanvasKey::Up, shift).unwrap();
        edit.keyboard(0, CanvasKey::Right, shift).unwrap();
        assert_eq!(edit.layout().zones[0], zone(105, 96, 210, 210));
        edit.keyboard(0, CanvasKey::Down, shift).unwrap();
        edit.keyboard(0, CanvasKey::Left, shift).unwrap();
        assert_eq!(edit.layout().zones[0], zone(110, 101, 200, 200));
        edit.keyboard(
            0,
            CanvasKey::Right,
            KeyModifiers {
                ctrl: true,
                shift: true,
            },
        )
        .unwrap();
        assert_eq!(edit.layout().zones[0], zone(109, 101, 202, 200));
        let mut boundary = editor(vec![zone(800, 100, 200, 200)]);
        boundary.keyboard(0, CanvasKey::Right, shift).unwrap();
        assert_eq!(boundary.layout().zones[0], zone(795, 100, 205, 200));
        boundary.keyboard(0, CanvasKey::Delete, shift).unwrap();
        assert!(boundary.layout().zones.is_empty());
    }

    #[test]
    fn drag_cancellation_and_session_snapshots_restore_geometry_and_placement() {
        let mut edit = editor(vec![]);
        edit.add_default_zone().unwrap();
        edit.commit().unwrap();
        let committed = edit.layout().clone();
        edit.begin_drag(0, DragKind::Move, false).unwrap();
        edit.drag_by(100, 100).unwrap();
        edit.drag_by(50, 20).unwrap();
        assert!(edit.is_dirty());
        assert!(edit.undo().is_err());
        edit.cancel_drag().unwrap();
        assert_eq!(edit.layout(), &committed);
        assert!(!edit.undo().unwrap());
        edit.add_default_zone().unwrap();
        edit.begin_drag(0, DragKind::Move, false).unwrap();
        edit.drag_by(50, 20).unwrap();
        edit.cancel();
        assert_eq!(edit.layout(), &committed);
        assert!(!edit.is_dragging());
        assert!(!edit.is_dirty());
        edit.add_default_zone().unwrap();
        assert_eq!(edit.layout().zones[1].x, 150);
        edit.undo().unwrap();
        edit.add_default_zone().unwrap();
        assert_eq!(edit.layout().zones[1].x, 150);
    }

    #[test]
    fn invalid_operations_preserve_data_history_and_drag_state() {
        let mut edit = editor(vec![zone(100, 100, 200, 200)]);
        let before = edit.layout().clone();
        assert!(edit.add_zone(zone(0, 0, 0, 100)).is_err());
        assert!(edit.remove_zone(4).is_err());
        assert!(edit.move_zone(4, 10, 10, false).is_err());
        assert!(edit.scale_reference(Size::new(0, 800)).is_err());
        assert!(edit.scale_reference(Size::new(1, 1)).is_err());
        assert_eq!(edit.layout(), &before);
        assert!(!edit.is_dragging());
        assert!(!edit.undo().unwrap());
        edit.begin_drag(0, DragKind::Move, true).unwrap();
        assert!(edit.remove_zone(0).is_err());
        assert!(edit.begin_drag(0, DragKind::Move, true).is_err());
        assert!(edit.commit().is_err());
        assert_eq!(edit.layout(), &before);
        edit.cancel_drag().unwrap();
        let mut oversize = editor(vec![zone(0, 0, 1200, 200)]);
        assert!(oversize.move_zone(0, 10, 10, false).is_err());
        assert!(!oversize.is_dragging());
        assert_eq!(oversize.layout().zones[0], zone(0, 0, 1200, 200));
    }

    #[test]
    fn scaling_roundtrips_exact_ratios_and_preserves_source_truncation() {
        let mut edit = editor(vec![zone(100, 100, 200, 200)]);
        let original = edit.layout().clone();
        edit.scale_reference(Size::new(2000, 1600)).unwrap();
        assert_eq!(edit.layout().zones[0], zone(200, 200, 400, 400));
        edit.scale_reference(Size::new(1000, 800)).unwrap();
        assert_eq!(edit.layout(), &original);
        let mut edit = editor(vec![zone(-101, 101, 201, 201)]);
        edit.scale_reference(Size::new(500, 400)).unwrap();
        assert_eq!(edit.layout().zones[0], zone(-50, 50, 100, 100));
        edit.scale_reference(Size::new(1000, 800)).unwrap();
        assert_eq!(edit.layout().zones[0], zone(-100, 100, 200, 200));
        edit.undo().unwrap();
        edit.undo().unwrap();
        assert_eq!(edit.layout().zones[0], zone(-101, 101, 201, 201));
    }
}
