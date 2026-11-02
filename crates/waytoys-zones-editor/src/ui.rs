//! Logical-coordinate draw data shared by the editor controller and SHM frontend.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl RectF {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, x: f64, y: f64) -> bool {
        self.x <= x && x < self.x + self.width && self.y <= y && y < self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Save,
    Cancel,
    NextOutput,
    Merge,
    CancelMerge,
    SwitchSave,
    SwitchDiscard,
    SwitchStay,
}

#[derive(Clone, Debug)]
pub struct Button {
    pub rect: RectF,
    pub text: String,
    pub action: Action,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct ZoneVisual {
    pub rect: RectF,
    pub id: usize,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub struct DividerVisual {
    pub rect: RectF,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub x: f64,
    pub y: f64,
    pub size: f32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// Display-only runtime scenes have no editor dimensions or toolbar margins.
    pub runtime_overlay: bool,
    pub preferences: waytoys_zones_config::ZonesPreferences,
    pub layout_name: Option<String>,
    pub zones: Vec<ZoneVisual>,
    pub dividers: Vec<DividerVisual>,
    pub split: Option<(RectF, bool)>,
    pub selection: Option<RectF>,
    pub buttons: Vec<Button>,
    pub labels: Vec<Label>,
    pub modal: bool,
}
