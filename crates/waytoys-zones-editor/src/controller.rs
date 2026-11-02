// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license. See LICENSE-MIT-PowerToys in the repository root.
// Interaction behavior adapted from PowerToys GridZone.xaml.cs and GridEditor.xaml.cs.

use waytoys_zones::geometry::Size;
use waytoys_zones::grid_editor::{Axis, GridEditor, GridSnapshot};
use waytoys_zones::layout::calculate_grid;
use waytoys_zones::magnetic_snap::MagneticSnap;
use waytoys_zones::model::{GRID_MULTIPLIER, GridLayout};
use waytoys_zones::{Result, ZonesError};

use crate::ui::{Action, Button, DividerVisual, Label, RectF, Scene, ZoneVisual};

const TOOLBAR_HEIGHT: f64 = 96.0;

#[derive(Clone, Copy, Debug)]
pub enum EditorKey {
    Left,
    Right,
    Up,
    Down,
    Split,
    Delete,
    Tab,
}

#[derive(Clone, Debug)]
enum Gesture {
    Button(Action),
    Resize {
        snapshot: GridSnapshot,
        index: usize,
        axis: Axis,
        negative_zone: usize,
        start: f64,
    },
    Body {
        zone: usize,
        x: f64,
        y: f64,
        dragging: bool,
    },
}

/// Pointer coordinates, geometry, and minimum sizes are all logical pixels.
/// Buffer scaling and output placement belong to the Wayland frontend.
pub struct Controller {
    editor: GridEditor,
    saved: GridLayout,
    size: Size,
    output_name: String,
    pointer: Option<(f64, f64)>,
    shift: bool,
    gesture: Option<Gesture>,
    merge: Vec<usize>,
    switch_prompt: bool,
    focus: Option<usize>,
    message: String,
}

impl Controller {
    pub fn new(layout: GridLayout, size: Size, output_name: String) -> Result<Self> {
        size.validate()?;
        calculate_grid(&layout, size)?;
        let mut result = Self {
            editor: GridEditor::new(layout.clone())?,
            saved: layout,
            size,
            output_name,
            pointer: None,
            shift: false,
            gesture: None,
            merge: Vec::new(),
            switch_prompt: false,
            focus: None,
            message: String::new(),
        };
        result.update_minimum()?;
        Ok(result)
    }

    pub fn editor(&self) -> &GridEditor {
        &self.editor
    }
    pub fn is_dirty(&self) -> bool {
        self.editor.layout() != &self.saved
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.editor.layout().clone();
    }
    pub fn set_message(&mut self, message: String) {
        self.message = message;
    }

    pub fn set_switch_prompt(&mut self, value: bool) {
        if value {
            self.cancel_gesture();
            self.merge.clear();
        }
        self.switch_prompt = value;
    }

    pub fn resize(&mut self, size: Size) -> Result<()> {
        size.validate()?;
        self.cancel_gesture();
        calculate_grid(self.editor.layout(), size)?;
        self.size = size;
        self.merge.clear();
        self.pointer = None;
        self.update_minimum()
    }

    fn update_minimum(&mut self) -> Result<()> {
        let pixels = 100.0 + 2.0 * self.spacing();
        let ratio = |extent| {
            (pixels / f64::from(extent) * f64::from(GRID_MULTIPLIER))
                .round_ties_even()
                .clamp(1.0, f64::from(GRID_MULTIPLIER)) as i32
        };
        self.editor
            .set_minimum_zone_size(ratio(self.size.width), ratio(self.size.height))
    }

    fn spacing(&self) -> f64 {
        if self.editor.layout().show_spacing {
            f64::from(self.editor.layout().spacing)
        } else {
            0.0
        }
    }

    fn check_pointer(x: f64, y: f64) -> Result<()> {
        if !x.is_finite() || !y.is_finite() {
            return Err(ZonesError::InvalidData(
                "pointer coordinates must be finite".into(),
            ));
        }
        Ok(())
    }

    pub fn modifiers(&mut self, shift: bool) -> Result<()> {
        self.shift = shift;
        // Hover is derived from pointer and modifiers on every scene, including
        // stationary Shift events. No fake pointer motion is necessary.
        Ok(())
    }

    /// Losing the pointer ends its gesture; a later enter/motion must not
    /// resume editing without a new press. Released merge confirmations are
    /// independent of pointer capture and remain available after a leave.
    pub fn pointer_lost(&mut self) {
        let selecting = matches!(self.gesture, Some(Gesture::Body { .. }));
        self.cancel_gesture();
        if selecting {
            self.merge.clear();
        }
        self.pointer = None;
    }

    pub fn pointer_move(&mut self, x: f64, y: f64) -> Result<()> {
        Self::check_pointer(x, y)?;
        self.pointer = Some((x, y));
        match self.gesture.clone() {
            Some(Gesture::Resize {
                snapshot,
                index,
                axis,
                start,
                ..
            }) => {
                // Always apply the cumulative delta to the initial checkpoint.
                // Work on a temporary editor so rejected motion keeps the last
                // valid layout and moving back can recover without index drift.
                let coordinate = if axis == Axis::X { x } else { y };
                let extent = if axis == Axis::X {
                    self.size.width
                } else {
                    self.size.height
                };
                let delta = ((coordinate - start) / f64::from(extent) * f64::from(GRID_MULTIPLIER))
                    .round_ties_even();
                if delta >= f64::from(i32::MIN) && delta <= f64::from(i32::MAX) {
                    let mut candidate = self.editor.clone();
                    candidate.restore(snapshot)?;
                    if candidate.can_drag_resizer(index, delta as i32)? {
                        candidate.drag_resizer(index, delta as i32)?;
                        if calculate_grid(candidate.layout(), self.size).is_ok() {
                            self.editor = candidate;
                        }
                    }
                }
            }
            Some(Gesture::Body {
                zone,
                x: start_x,
                y: start_y,
                dragging,
            }) => {
                let dragging = dragging
                    || (x - start_x).abs() > self.drag_threshold()
                    || (y - start_y).abs() > self.drag_threshold();
                self.gesture = Some(Gesture::Body {
                    zone,
                    x: start_x,
                    y: start_y,
                    dragging,
                });
                if dragging {
                    self.merge = self.selection_closure(zone, start_x, start_y, x, y)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn press(&mut self, x: f64, y: f64) -> Result<()> {
        Self::check_pointer(x, y)?;
        if self.gesture.is_some() {
            return Ok(());
        }
        self.pointer = Some((x, y));
        if let Some(action) = self.button_at(x, y) {
            self.gesture = Some(Gesture::Button(action));
        } else if !self.switch_prompt && self.merge.is_empty() && y >= TOOLBAR_HEIGHT {
            if let Some(index) = self.divider_at(x, y) {
                let axis = self.editor.resizers()[index].axis;
                self.gesture = Some(Gesture::Resize {
                    snapshot: self.editor.snapshot(),
                    index,
                    axis,
                    negative_zone: self.editor.resizers()[index].negative_side_indices[0],
                    start: if axis == Axis::X { x } else { y },
                });
            } else if let Some(zone) = self.zone_at(x, y) {
                self.focus = Some(zone);
                self.gesture = Some(Gesture::Body {
                    zone,
                    x,
                    y,
                    dragging: false,
                });
            }
        }
        Ok(())
    }

    pub fn release(&mut self, x: f64, y: f64) -> Result<Option<Action>> {
        self.pointer_move(x, y)?;
        match self.gesture.take() {
            Some(Gesture::Button(action)) if self.button_at(x, y) == Some(action) => match action {
                Action::Merge => {
                    if self.merge.len() > 1 {
                        self.editor.merge(&self.merge)?;
                    }
                    self.merge.clear();
                    self.focus = None;
                }
                Action::CancelMerge => self.merge.clear(),
                Action::SwitchStay => {
                    self.switch_prompt = false;
                    return Ok(Some(action));
                }
                _ => return Ok(Some(action)),
            },
            Some(Gesture::Body {
                zone,
                dragging: false,
                ..
            }) => {
                if y >= TOOLBAR_HEIGHT
                    && self.zone_at(x, y) == Some(zone)
                    && self.divider_at(x, y).is_none()
                    && let Some((axis, position, enabled)) = self.split_at(zone, x, y)?
                        && enabled {
                            self.editor.split(zone, axis, position)?;
                            self.focus = None;
                        }
            }
            Some(Gesture::Body { dragging: true, .. })
                // A merge drag never falls through into a split, even when its
                // closure contains only one zone or the pointer returns home.
                if self.merge.len() < 2 => {
                    self.merge.clear();
                }
            _ => {}
        }
        Ok(None)
    }

    pub fn escape(&mut self) -> bool {
        if self.gesture.is_some() {
            self.cancel_gesture();
            self.merge.clear();
            return false;
        }
        if self.switch_prompt {
            self.switch_prompt = false;
            return false;
        }
        if !self.merge.is_empty() {
            self.merge.clear();
            return false;
        }
        true
    }

    fn cancel_gesture(&mut self) {
        if let Some(Gesture::Resize { snapshot, .. }) = self.gesture.take() {
            // Checkpoints are generated by this validated editor.
            self.editor
                .restore(snapshot)
                .expect("controller checkpoint must remain valid");
        }
    }

    fn drag_threshold(&self) -> f64 {
        self.spacing().max(1.0) / 2.0
    }

    fn zones(&self) -> Vec<ZoneVisual> {
        calculate_grid(self.editor.layout(), self.size)
            .expect("controller geometry is validated")
            .into_iter()
            .map(|zone| ZoneVisual {
                id: zone.id,
                rect: RectF::new(
                    f64::from(zone.rect.x),
                    f64::from(zone.rect.y),
                    f64::from(zone.rect.width),
                    f64::from(zone.rect.height),
                ),
                selected: self.merge.contains(&zone.id) || self.focus == Some(zone.id),
            })
            .collect()
    }

    fn zone_at(&self, x: f64, y: f64) -> Option<usize> {
        self.zones()
            .iter()
            .find(|zone| zone.rect.contains(x, y))
            .map(|zone| zone.id)
    }

    fn divider_rect(&self, index: usize) -> RectF {
        let divider = &self.editor.resizers()[index];
        let thickness = self.spacing().max(8.0);
        let width = f64::from(self.size.width) / f64::from(GRID_MULTIPLIER);
        let height = f64::from(self.size.height) / f64::from(GRID_MULTIPLIER);
        match divider.axis {
            Axis::X => RectF::new(
                f64::from(divider.position) * width - thickness / 2.0,
                f64::from(divider.start) * height,
                thickness,
                f64::from(divider.end - divider.start) * height,
            ),
            Axis::Y => RectF::new(
                f64::from(divider.start) * width,
                f64::from(divider.position) * height - thickness / 2.0,
                f64::from(divider.end - divider.start) * width,
                thickness,
            ),
        }
    }

    fn divider_at(&self, x: f64, y: f64) -> Option<usize> {
        (0..self.editor.resizers().len()).find(|&i| self.divider_rect(i).contains(x, y))
    }

    fn split_at(&self, id: usize, x: f64, y: f64) -> Result<Option<(Axis, i32, bool)>> {
        let visuals = self.zones();
        let Some(visual) = visuals.iter().find(|zone| zone.id == id) else {
            return Ok(None);
        };
        let zone = self.editor.zones()[id];
        let vertical = (visual.rect.width > visual.rect.height) ^ self.shift;
        let (axis, percentages, extent, pixel, low, high) = if vertical {
            (
                Axis::X,
                &self.editor.layout().columns_percentage,
                self.size.width,
                x,
                zone.left,
                zone.right,
            )
        } else {
            (
                Axis::Y,
                &self.editor.layout().rows_percentage,
                self.size.height,
                y,
                zone.top,
                zone.bottom,
            )
        };
        let mut points = vec![0];
        let mut sum = 0;
        for value in percentages {
            sum += value;
            points.push(sum);
        }
        let snap = MagneticSnap::new(points, f64::from(extent))?;
        let position = snap.pixel_to_data_with_snapping(pixel, low, high)?;
        Ok(Some((
            axis,
            position,
            self.editor.can_split(id, axis, position)?,
        )))
    }

    fn selection_closure(
        &self,
        origin: usize,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<Vec<usize>> {
        let left = x0.min(x1);
        let right = x0.max(x1);
        let top = y0.min(y1);
        let bottom = y0.max(y1);
        let mut indices = vec![origin];
        for zone in self.zones() {
            // Inclusive pointer segments allow a horizontal or vertical drag.
            if zone.rect.x <= right
                && left < zone.rect.x + zone.rect.width
                && zone.rect.y <= bottom
                && top < zone.rect.y + zone.rect.height
            {
                indices.push(zone.id);
            }
        }
        self.editor.merge_closure(&indices)
    }

    fn buttons(&self) -> Vec<Button> {
        let button = |rect, text: &str, action| Button {
            rect,
            text: text.into(),
            action,
            enabled: true,
        };
        let width = f64::from(self.size.width);
        let height = f64::from(self.size.height);
        if self.switch_prompt {
            let left = (width - 444.0).max(0.0) / 2.0;
            let top = height / 2.0 + 18.0;
            return vec![
                button(
                    RectF::new(left, top, 140.0, 42.0),
                    "Save and switch",
                    Action::SwitchSave,
                ),
                button(
                    RectF::new(left + 152.0, top, 140.0, 42.0),
                    "Discard",
                    Action::SwitchDiscard,
                ),
                button(
                    RectF::new(left + 304.0, top, 140.0, 42.0),
                    "Stay",
                    Action::SwitchStay,
                ),
            ];
        }
        let mut result = vec![
            button(
                RectF::new(16.0, 12.0, (width - 232.0).clamp(80.0, 340.0), 40.0),
                &format!("{}  >", self.output_name),
                Action::NextOutput,
            ),
            button(
                RectF::new((width - 208.0).max(108.0), 12.0, 88.0, 40.0),
                "Save",
                Action::Save,
            ),
            button(
                RectF::new((width - 108.0).max(208.0), 12.0, 92.0, 40.0),
                "Cancel",
                Action::Cancel,
            ),
        ];
        if !self.merge.is_empty()
            && !matches!(self.gesture, Some(Gesture::Body { dragging: true, .. }))
        {
            result.clear();
            let left = (width - 236.0).max(0.0) / 2.0;
            result.push(button(
                RectF::new(left, height / 2.0 + 18.0, 112.0, 42.0),
                "Merge",
                Action::Merge,
            ));
            result.push(button(
                RectF::new(left + 124.0, height / 2.0 + 18.0, 112.0, 42.0),
                "Cancel",
                Action::CancelMerge,
            ));
        }
        result
    }

    fn button_at(&self, x: f64, y: f64) -> Option<Action> {
        self.buttons()
            .into_iter()
            .rev()
            .find(|b| b.enabled && b.rect.contains(x, y))
            .map(|b| b.action)
    }

    pub fn scene(&self) -> Scene {
        let pending_merge = !self.merge.is_empty()
            && !matches!(self.gesture, Some(Gesture::Body { dragging: true, .. }));
        let mut scene = Scene {
            zones: self.zones(),
            buttons: self.buttons(),
            modal: self.switch_prompt || pending_merge,
            ..Scene::default()
        };
        let hover_divider = self.pointer.and_then(|(x, y)| {
            if y >= TOOLBAR_HEIGHT {
                self.divider_at(x, y)
            } else {
                None
            }
        });
        for index in 0..self.editor.resizers().len() {
            scene.dividers.push(DividerVisual { rect: self.divider_rect(index),
                active: !scene.modal && (hover_divider == Some(index)
                    || matches!(self.gesture, Some(Gesture::Resize { axis, negative_zone, .. })
                        if self.editor.resizers()[index].axis == axis
                            && self.editor.resizers()[index].negative_side_indices.contains(&negative_zone))),
            });
        }
        if self.gesture.is_none()
            && !scene.modal
            && let Some((x, y)) = self.pointer
            && y >= TOOLBAR_HEIGHT
            && hover_divider.is_none()
            && let Some(id) = self.zone_at(x, y)
            && let Ok(Some((axis, position, enabled))) = self.split_at(id, x, y)
        {
            let zone = scene.zones.iter().find(|zone| zone.id == id).unwrap();
            let thickness = self.spacing().max(1.0);
            let rect = if axis == Axis::X {
                RectF::new(
                    (f64::from(position) / f64::from(GRID_MULTIPLIER) * f64::from(self.size.width)
                        - thickness / 2.0)
                        .clamp(
                            zone.rect.x,
                            (zone.rect.x + zone.rect.width - thickness).max(zone.rect.x),
                        ),
                    zone.rect.y,
                    thickness.min(zone.rect.width),
                    zone.rect.height,
                )
            } else {
                RectF::new(
                    zone.rect.x,
                    (f64::from(position) / f64::from(GRID_MULTIPLIER)
                        * f64::from(self.size.height)
                        - thickness / 2.0)
                        .clamp(
                            zone.rect.y,
                            (zone.rect.y + zone.rect.height - thickness).max(zone.rect.y),
                        ),
                    zone.rect.width,
                    thickness.min(zone.rect.height),
                )
            };
            scene.split = Some((rect, enabled));
        }
        if !self.merge.is_empty() {
            let selected: Vec<_> = scene
                .zones
                .iter()
                .filter(|z| self.merge.contains(&z.id))
                .collect();
            if let Some(first) = selected.first() {
                let left = selected.iter().fold(first.rect.x, |v, z| v.min(z.rect.x));
                let top = selected.iter().fold(first.rect.y, |v, z| v.min(z.rect.y));
                let right = selected
                    .iter()
                    .fold(first.rect.x, |v, z| v.max(z.rect.x + z.rect.width));
                let bottom = selected
                    .iter()
                    .fold(first.rect.y, |v, z| v.max(z.rect.y + z.rect.height));
                scene.selection = Some(RectF::new(left, top, right - left, bottom - top));
            }
        }
        let mut label = |x, y, size, text: String| scene.labels.push(Label { x, y, size, text });
        label(18.0, 64.0, 15.0, "Click to split | Shift: rotate | Drag zones to merge | Drag dividers to resize | Esc: cancel".into());
        label(
            18.0,
            f64::from(self.size.height) - 26.0,
            15.0,
            if self.message.is_empty() {
                if self.is_dirty() {
                    "Unsaved changes".into()
                } else {
                    "Grid editor".into()
                }
            } else {
                self.message.clone()
            },
        );
        if self.switch_prompt {
            label(
                (f64::from(self.size.width) / 2.0 - 215.0).max(12.0),
                f64::from(self.size.height) / 2.0 - 26.0,
                21.0,
                "Save changes before switching outputs?".into(),
            );
        } else if pending_merge {
            label(
                (f64::from(self.size.width) / 2.0 - 130.0).max(12.0),
                f64::from(self.size.height) / 2.0 - 26.0,
                21.0,
                format!("Merge {} selected zones?", self.merge.len()),
            );
        }
        scene
    }

    pub fn key(&mut self, key: EditorKey) -> Result<()> {
        if self.gesture.is_some() || self.switch_prompt || !self.merge.is_empty() {
            return Ok(());
        }
        let zones = self.zones();
        let current = self.focus.unwrap_or(0).min(zones.len() - 1);
        match key {
            EditorKey::Tab => {
                self.focus = Some(if self.focus.is_none() {
                    0
                } else {
                    (current + 1) % zones.len()
                });
                self.pointer = None;
            }
            EditorKey::Split => {
                let rect = zones[current].rect;
                if let Some((axis, position, true)) = self.split_at(
                    current,
                    rect.x + rect.width / 2.0,
                    rect.y + rect.height / 2.0,
                )? {
                    self.editor.split(current, axis, position)?;
                    self.focus = Some(current);
                }
            }
            EditorKey::Delete => {
                let zone = self.editor.zones()[current];
                if let Some(other) = self.editor.zones().iter().find(|other| {
                    other.index != current
                        && (((zone.left == other.right || zone.right == other.left)
                            && zone.top < other.bottom
                            && other.top < zone.bottom)
                            || ((zone.top == other.bottom || zone.bottom == other.top)
                                && zone.left < other.right
                                && other.left < zone.right))
                }) {
                    self.merge = self.editor.merge_closure(&[current, other.index])?;
                }
            }
            direction => {
                let (axis, sign) = match direction {
                    EditorKey::Left => (Axis::X, -1.0),
                    EditorKey::Right => (Axis::X, 1.0),
                    EditorKey::Up => (Axis::Y, -1.0),
                    EditorKey::Down => (Axis::Y, 1.0),
                    _ => unreachable!(),
                };
                if let Some((x, y)) = self.pointer
                    && y >= TOOLBAR_HEIGHT
                    && let Some(index) = self.divider_at(x, y)
                    && self.editor.resizers()[index].axis == axis
                {
                    let extent = if axis == Axis::X {
                        self.size.width
                    } else {
                        self.size.height
                    };
                    let delta = (sign * if self.shift { 1.0 } else { 10.0 } / f64::from(extent)
                        * f64::from(GRID_MULTIPLIER))
                    .round_ties_even() as i32;
                    if self.editor.can_drag_resizer(index, delta)? {
                        let mut next = self.editor.clone();
                        next.drag_resizer(index, delta)?;
                        if calculate_grid(next.layout(), self.size).is_ok() {
                            self.editor = next;
                            let distance =
                                f64::from(delta) / f64::from(GRID_MULTIPLIER) * f64::from(extent);
                            self.pointer = Some(if axis == Axis::X {
                                (x + distance, y)
                            } else {
                                (x, y + distance)
                            });
                        }
                    }
                    return Ok(());
                }
                let center = |r: RectF| (r.x + r.width / 2.0, r.y + r.height / 2.0);
                let (cx, cy) = center(zones[current].rect);
                let next = zones
                    .iter()
                    .filter(|zone| zone.id != current)
                    .filter_map(|zone| {
                        let (x, y) = center(zone.rect);
                        let along = if axis == Axis::X { x - cx } else { y - cy };
                        let across = if axis == Axis::X { y - cy } else { x - cx };
                        (along * sign > 0.0).then_some((zone.id, along * along + across * across))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                self.focus = Some(next.map(|(id, _)| id).unwrap_or(current));
                self.pointer = None;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> GridLayout {
        GridLayout::new(1, 2, vec![10000], vec![5000, 5000], vec![vec![0, 1]]).unwrap()
    }
    fn controller(layout: GridLayout) -> Controller {
        Controller::new(layout, Size::new(1200, 800), "Test output".into()).unwrap()
    }
    fn click_action(c: &mut Controller, action: Action) -> Option<Action> {
        let b = c
            .scene()
            .buttons
            .into_iter()
            .find(|b| b.action == action)
            .unwrap();
        let (x, y) = (b.rect.x + 8.0, b.rect.y + 8.0);
        c.press(x, y).unwrap();
        c.release(x, y).unwrap()
    }

    #[test]
    fn cumulative_resize_has_no_drift_and_rejects_then_recovers() {
        let mut c = controller(columns());
        c.press(600.0, 300.0).unwrap();
        c.pointer_move(660.0, 300.0).unwrap();
        assert_eq!(c.editor.layout().columns_percentage, vec![5500, 4500]);
        c.pointer_move(720.0, 300.0).unwrap();
        assert_eq!(c.editor.layout().columns_percentage, vec![6000, 4000]);
        c.pointer_move(1190.0, 300.0).unwrap();
        assert_eq!(c.editor.layout().columns_percentage, vec![6000, 4000]);
        c.pointer_move(630.0, 300.0).unwrap();
        assert_eq!(c.editor.layout().columns_percentage, vec![5250, 4750]);
        c.release(600.0, 300.0).unwrap();
        assert_eq!(c.editor.layout().columns_percentage, vec![5000, 5000]);
        assert!(!c.is_dirty());
    }

    #[test]
    fn pointer_loss_restores_resize_and_requires_a_fresh_press() {
        let mut c = controller(columns());
        let initial = c.editor.layout().clone();
        c.press(600.0, 300.0).unwrap();
        c.pointer_move(720.0, 300.0).unwrap();
        assert!(c.is_dirty());

        c.pointer_lost();
        assert_eq!(c.editor.layout(), &initial);
        assert!(!c.is_dirty());
        assert!(c.gesture.is_none());
        assert!(c.pointer.is_none());
        c.pointer_move(800.0, 300.0).unwrap();
        c.release(800.0, 300.0).unwrap();
        assert_eq!(c.editor.layout(), &initial);

        c.press(600.0, 300.0).unwrap();
        c.pointer_move(660.0, 300.0).unwrap();
        c.release(660.0, 300.0).unwrap();
        assert_eq!(c.editor.layout().columns_percentage, vec![5500, 4500]);
        assert!(c.is_dirty());
        // Canceling a later gesture restores its checkpoint, preserving edits
        // that were already completed before capture was lost.
        c.press(660.0, 300.0).unwrap();
        c.pointer_move(720.0, 300.0).unwrap();
        c.pointer_lost();
        assert_eq!(c.editor.layout().columns_percentage, vec![5500, 4500]);
        assert!(c.is_dirty());
    }

    #[test]
    fn pointer_loss_cancels_unreleased_body_selection_and_click() {
        let mut c = controller(columns());
        c.press(300.0, 400.0).unwrap();
        c.pointer_move(900.0, 400.0).unwrap();
        assert_eq!(c.merge.len(), 2);
        c.pointer_lost();
        assert!(c.merge.is_empty());
        assert!(c.gesture.is_none());
        assert!(c.pointer.is_none());
        assert!(c.scene().selection.is_none());
        c.pointer_move(900.0, 400.0).unwrap();
        c.release(900.0, 400.0).unwrap();
        assert!(!c.scene().modal);
        assert!(!c.is_dirty());

        c.press(300.0, 400.0).unwrap();
        c.pointer_lost();
        c.release(300.0, 400.0).unwrap();
        assert_eq!(c.editor.zones().len(), 2);
        assert!(!c.is_dirty());
    }

    #[test]
    fn pointer_loss_preserves_released_merge_confirmation() {
        let mut c = controller(columns());
        c.press(300.0, 400.0).unwrap();
        c.release(900.0, 400.0).unwrap();
        assert!(c.scene().modal);
        c.pointer_lost();
        assert_eq!(c.merge, vec![0, 1]);
        assert!(c.scene().modal);
        assert!(c.pointer.is_none());

        // Losing capture while pressing the popup must cancel the button
        // activation without dismissing the completed merge selection.
        let button = c
            .scene()
            .buttons
            .into_iter()
            .find(|b| b.action == Action::Merge)
            .unwrap();
        let (x, y) = (button.rect.x + 8.0, button.rect.y + 8.0);
        c.press(x, y).unwrap();
        c.pointer_lost();
        c.release(x, y).unwrap();
        assert!(c.scene().modal);
        assert_eq!(c.editor.zones().len(), 2);
        assert_eq!(click_action(&mut c, Action::Merge), None);
        assert_eq!(c.editor.zones().len(), 1);
    }

    #[test]
    fn stationary_shift_rotates_hover_and_click_split() {
        let mut c = controller(GridLayout::default());
        c.pointer_move(600.0, 400.0).unwrap();
        let vertical = c.scene().split.unwrap().0;
        assert!(vertical.height > vertical.width);
        c.modifiers(true).unwrap();
        let horizontal = c.scene().split.unwrap().0;
        assert!(horizontal.width > horizontal.height);
        c.press(600.0, 400.0).unwrap();
        c.release(600.0, 400.0).unwrap();
        assert_eq!(c.editor.layout().rows, 2);
        assert_eq!(c.editor.layout().columns, 1);
        assert!(c.is_dirty());
        c.mark_saved();
        assert!(!c.is_dirty());
    }

    #[test]
    fn merge_drag_requires_confirmation_and_cancel_never_splits() {
        let mut c = controller(columns());
        c.press(300.0, 400.0).unwrap();
        c.release(900.0, 400.0).unwrap();
        assert_eq!(c.editor.zones().len(), 2);
        assert!(c.scene().modal);
        assert_eq!(click_action(&mut c, Action::CancelMerge), None);
        assert_eq!(c.editor.zones().len(), 2);
        c.press(300.0, 400.0).unwrap();
        c.release(900.0, 400.0).unwrap();
        assert_eq!(click_action(&mut c, Action::Merge), None);
        assert_eq!(c.editor.zones().len(), 1);
        assert!(c.is_dirty());
    }

    #[test]
    fn escape_restores_resize_then_cancels_merge_and_switch_prompt() {
        let mut c = controller(columns());
        c.press(600.0, 400.0).unwrap();
        c.pointer_move(750.0, 400.0).unwrap();
        assert!(c.is_dirty());
        assert!(!c.escape());
        assert!(!c.is_dirty());
        c.release(750.0, 400.0).unwrap();
        assert_eq!(c.editor.zones().len(), 2);
        c.press(300.0, 400.0).unwrap();
        c.release(900.0, 400.0).unwrap();
        assert!(!c.escape());
        assert!(!c.scene().modal);
        c.set_switch_prompt(true);
        assert!(!c.escape());
        assert!(!c.scene().modal);
        assert!(c.escape());
    }

    #[test]
    fn toolbar_and_prompt_block_underlying_grid_hits() {
        let mut c = controller(GridLayout::default());
        assert_eq!(click_action(&mut c, Action::Save), Some(Action::Save));
        c.press(600.0, 76.0).unwrap();
        c.release(600.0, 76.0).unwrap();
        assert_eq!(c.editor.zones().len(), 1);
        c.set_switch_prompt(true);
        c.press(600.0, 200.0).unwrap();
        c.release(600.0, 200.0).unwrap();
        assert_eq!(c.editor.zones().len(), 1);
        assert_eq!(
            click_action(&mut c, Action::SwitchDiscard),
            Some(Action::SwitchDiscard)
        );
        assert_eq!(
            click_action(&mut c, Action::SwitchStay),
            Some(Action::SwitchStay)
        );
        assert!(!c.scene().modal);
    }

    #[test]
    fn resizing_output_cancels_gesture_and_recomputes_logical_minimum() {
        let mut c = controller(columns());
        c.press(600.0, 400.0).unwrap();
        c.pointer_move(720.0, 400.0).unwrap();
        c.resize(Size::new(1600, 1000)).unwrap();
        assert!(!c.is_dirty());
        assert_eq!(c.editor.minimum_zone_size(), (825, 1320));
        assert!(c.escape());
    }

    #[test]
    fn snapping_uses_global_cumulative_grid_boundaries() {
        let layout = GridLayout::new(
            2,
            2,
            vec![5000, 5000],
            vec![5000, 5000],
            vec![vec![0, 0], vec![1, 2]],
        )
        .unwrap();
        let mut c = controller(layout);
        c.pointer_move(588.0, 200.0).unwrap();
        let split = c.scene().split.unwrap();
        assert!(split.1);
        assert_eq!(split.0.x + split.0.width / 2.0, 600.0);
    }

    #[test]
    fn resize_reuses_original_index_after_boundaries_join() {
        let layout = GridLayout::new(
            3,
            2,
            vec![3333, 3333, 3334],
            vec![5000, 5000],
            vec![vec![0, 1], vec![2, 1], vec![2, 3]],
        )
        .unwrap();
        let mut c = Controller::new(layout, Size::new(1200, 10000), "Tall".into()).unwrap();
        // Initially Y=3333 has index 0, Y=6666 has index 1. Aligning the
        // latter with the former removes a row and joins both resizers.
        c.press(900.0, 6666.0).unwrap();
        c.pointer_move(900.0, 3333.0).unwrap();
        assert_eq!(c.editor.layout().rows, 2);
        assert_eq!(c.editor.resizers()[0].axis, Axis::Y);
        // Index 1 now refers to X, but the next event must still move Y from
        // the original checkpoint, recovering the original third row.
        c.pointer_move(900.0, 5000.0).unwrap();
        assert_eq!(c.editor.layout().rows_percentage, vec![3333, 1667, 5000]);
        assert_eq!(c.editor.layout().columns_percentage, vec![5000, 5000]);
        assert!(!c.escape());
        assert!(!c.is_dirty());
    }
}
