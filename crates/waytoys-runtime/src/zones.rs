use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};
use waytoys_zones::{
    geometry::{Point, Size},
    layout::{Zone, calculate_custom, grid_template},
    model::{CustomLayout, CustomLayoutData, CustomLayouts, DEFAULT_ZONE_COUNT},
    selection::{HighlightedZones, combined_zones_rect},
};
use waytoys_zones_config::ZonesPreferences;
use waytoys_zones_editor::{
    store::output_path,
    ui::{RectF, Scene, ZoneVisual},
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Output {
    pub name: String,
    pub geometry: [i32; 4],
}
#[derive(Deserialize)]
pub struct Snapshot {
    pub protocol: u32,
    pub epoch: String,
    pub outputs: Vec<Output>,
}
#[derive(Clone, Default, Debug, Deserialize)]
#[serde(default)]
pub struct Event {
    pub kind: String,
    pub epoch: String,
    pub id: u64,
    pub sequence: u64,
    pub output: String,
    pub position: [f64; 2],
    pub shift: bool,
    pub ctrl: bool,
}
#[derive(Debug, Serialize)]
pub struct Target {
    pub epoch: String,
    pub id: u64,
    pub sequence: u64,
    pub rect: Option<[i32; 4]>,
}
#[derive(Clone)]
pub struct Visual {
    pub scenes: BTreeMap<String, Scene>,
    pub label_until: Option<Instant>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Layout {
    pub output: Output,
    pub name: String,
    pub zones: Vec<Zone>,
    pub sensitivity: i32,
}

pub fn load_layouts(
    directory: &Path,
    outputs: &[Output],
) -> Result<BTreeMap<String, Layout>, String> {
    let mut layouts = BTreeMap::new();
    for output in outputs {
        let layout = match std::fs::read_to_string(output_path(directory, &output.name)) {
            Ok(json) => {
                let mut parsed = CustomLayouts::from_json(&json).map_err(|e| e.to_string())?;
                if parsed.custom_layouts.len() != 1 {
                    return Err(format!("{}: 레이아웃 한 개가 필요합니다", output.name));
                }
                parsed.custom_layouts.remove(0)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => CustomLayout::new(
                format!("waytoys-grid:{}", output.name),
                format!("{} grid", output.name),
                CustomLayoutData::Grid(
                    grid_template(DEFAULT_ZONE_COUNT, false).map_err(|e| e.to_string())?,
                ),
            )
            .map_err(|e| e.to_string())?,
            Err(e) => return Err(format!("{}: {e}", output.name)),
        };
        let zones = calculate_custom(&layout, Size::new(output.geometry[2], output.geometry[3]))
            .map_err(|e| e.to_string())?;
        let sensitivity = match &layout.data {
            CustomLayoutData::Grid(grid) => grid.sensitivity_radius,
            CustomLayoutData::Canvas(canvas) => canvas.sensitivity_radius,
        };
        layouts.insert(
            output.name.clone(),
            Layout {
                output: output.clone(),
                name: layout.name,
                zones,
                sensitivity,
            },
        );
    }
    Ok(layouts)
}

#[derive(Default)]
pub struct Drag {
    pub event: Option<Event>,
    selection: HighlightedZones,
    label_until: Option<Instant>,
    last: (u64, u64),
    closed: u64,
}
impl Drag {
    pub fn cancel(&mut self, event: &Event) -> bool {
        if (event.id, event.sequence) <= self.last {
            return false;
        }
        self.last = (event.id, event.sequence);
        self.clear();
        true
    }
    pub fn clear(&mut self) {
        self.closed = self.closed.max(self.last.0);
        self.event = None;
        self.selection.reset();
        self.label_until = None;
    }
    /// Reject stale messages before either painting or committing a different drag.
    pub fn update(
        &mut self,
        event: Event,
        layouts: &BTreeMap<String, Layout>,
        p: &ZonesPreferences,
    ) -> Result<bool, String> {
        if event.id <= self.closed || (event.id, event.sequence) <= self.last {
            return Ok(false);
        }
        self.last = (event.id, event.sequence);
        if !matches!(event.kind.as_str(), "update" | "drop") || !event.shift {
            let changed = self.event.as_ref().is_some_and(|e| e.shift);
            self.selection.reset();
            self.label_until = None;
            self.event = Some(event);
            return Ok(changed);
        }
        if !event.position.iter().all(|v| v.is_finite()) {
            return Err("invalid drag pointer".into());
        }
        let reset = self
            .event
            .as_ref()
            .is_none_or(|e| e.id != event.id || e.output != event.output || !e.shift);
        if reset {
            self.selection.reset();
            self.label_until =
                Some(Instant::now() + Duration::from_millis(p.layout_name_label_duration as u64));
        }
        self.event = Some(event);
        Ok(self.select(layouts, p)? || reset)
    }
    pub fn refresh(
        &mut self,
        layouts: &BTreeMap<String, Layout>,
        p: &ZonesPreferences,
    ) -> Result<(), String> {
        self.selection.reset();
        self.select(layouts, p)?;
        Ok(())
    }
    fn select(
        &mut self,
        layouts: &BTreeMap<String, Layout>,
        p: &ZonesPreferences,
    ) -> Result<bool, String> {
        let Some(e) = self.event.as_ref().filter(|e| e.shift) else {
            return Ok(false);
        };
        let Some(layout) = layouts.get(&e.output) else {
            self.selection.reset();
            return Ok(true);
        };
        self.selection
            .update(
                &layout.zones,
                Point::new(
                    e.position[0] - f64::from(layout.output.geometry[0]),
                    e.position[1] - f64::from(layout.output.geometry[1]),
                ),
                layout.sensitivity,
                p.overlapping_zones_algorithm,
                e.ctrl,
            )
            .map_err(|e| e.to_string())
    }
    pub fn target(&self, layouts: &BTreeMap<String, Layout>) -> Result<Option<Target>, String> {
        let Some(e) = self.event.as_ref().filter(|e| e.kind == "drop") else {
            return Ok(None);
        };
        let rect = if e.shift {
            match layouts.get(&e.output) {
                Some(layout) => combined_zones_rect(&layout.zones, self.selection.zones())
                    .map_err(|e| e.to_string())?
                    .map(|r| {
                        [
                            r.x + layout.output.geometry[0],
                            r.y + layout.output.geometry[1],
                            r.width,
                            r.height,
                        ]
                    }),
                None => None,
            }
        } else {
            None
        };
        Ok(Some(Target {
            epoch: e.epoch.clone(),
            id: e.id,
            sequence: e.sequence,
            rect,
        }))
    }
    pub fn visual(
        &self,
        layouts: &BTreeMap<String, Layout>,
        p: &ZonesPreferences,
    ) -> Option<Visual> {
        let e = self
            .event
            .as_ref()
            .filter(|e| e.shift && e.kind == "update")?;
        if !layouts.contains_key(&e.output) {
            return None;
        }
        let scenes = layouts
            .iter()
            .filter(|(name, _)| p.show_zones_on_all_monitors || **name == e.output)
            .map(|(name, layout)| {
                let mut scene = Scene {
                    preferences: p.clone(),
                    runtime_overlay: true,
                    ..Scene::default()
                };
                scene.layout_name = p.layout_name_label_enabled.then(|| layout.name.clone());
                scene.zones = layout
                    .zones
                    .iter()
                    .map(|z| ZoneVisual {
                        id: z.id,
                        rect: RectF::new(
                            f64::from(z.rect.x),
                            f64::from(z.rect.y),
                            f64::from(z.rect.width),
                            f64::from(z.rect.height),
                        ),
                        selected: *name == e.output && self.selection.zones().contains(&z.id),
                    })
                    .collect();
                (name.clone(), scene)
            })
            .collect();
        Some(Visual {
            scenes,
            label_until: self.label_until,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> BTreeMap<String, Layout> {
        let temp = tempfile::tempdir().unwrap();
        load_layouts(
            temp.path(),
            &[
                Output {
                    name: "A".into(),
                    geometry: [0, 0, 900, 600],
                },
                Output {
                    name: "B".into(),
                    geometry: [900, 0, 900, 600],
                },
            ],
        )
        .unwrap()
    }
    fn e(id: u64, sequence: u64, output: &str, x: f64, ctrl: bool) -> Event {
        Event {
            kind: "update".into(),
            id,
            sequence,
            output: output.into(),
            position: [x, 200.],
            shift: true,
            ctrl,
            ..Event::default()
        }
    }
    #[test]
    fn spans_locally_resets_on_output_and_rejects_stale_targets() {
        let layouts = fixture();
        let p = ZonesPreferences::default();
        let mut d = Drag::default();
        d.update(e(1, 1, "A", 150., true), &layouts, &p).unwrap();
        d.update(e(1, 2, "A", 750., true), &layouts, &p).unwrap();
        assert_eq!(d.selection.zones().len(), 3);
        d.update(e(1, 3, "B", 1050., true), &layouts, &p).unwrap();
        assert_eq!(d.selection.zones().len(), 1);
        assert!(!d.update(e(1, 2, "A", 750., true), &layouts, &p).unwrap());
        let mut drop = e(1, 4, "B", 1050., false);
        drop.kind = "drop".into();
        d.update(drop, &layouts, &p).unwrap();
        assert!(d.target(&layouts).unwrap().unwrap().rect.unwrap()[0] >= 900);
        assert!(d.visual(&layouts, &p).is_none());
        d.update(e(2, 1, "A", 150., false), &layouts, &p).unwrap();
        assert!(!d.update(e(1, 99, "B", 1050., false), &layouts, &p).unwrap());
    }
    #[test]
    fn stationary_shift_release_clears_overlay_and_selection() {
        let layouts = fixture();
        let p = ZonesPreferences::default();
        let mut d = Drag::default();
        d.update(e(1, 1, "A", 150., false), &layouts, &p).unwrap();
        assert!(d.visual(&layouts, &p).is_some());
        let mut release = e(1, 2, "A", 150., false);
        release.shift = false;
        d.update(release, &layouts, &p).unwrap();
        assert!(d.visual(&layouts, &p).is_none());
        assert!(d.selection.zones().is_empty());
        d.update(e(1, 3, "A", 150., false), &layouts, &p).unwrap();
        assert!(d.visual(&layouts, &p).is_some());
        d.clear();
        assert!(!d.update(e(1, 99, "A", 150., false), &layouts, &p).unwrap());
        assert!(d.visual(&layouts, &p).is_none());
    }
}
