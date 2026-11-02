use serde::{Deserialize, Serialize};
pub use waytoys_zones::settings::{LayoutNameLabelPlacement, OverlappingZonesAlgorithm};
use waytoys_zones::{Result, ZonesError, settings::ZonesSettings};

/// Deliberately separate from the source-compatible ZonesSettings schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ZonesPreferences {
    pub use_cursorpos_editor_startupscreen: bool,
    pub show_zones_on_all_monitors: bool,
    pub overlapping_zones_algorithm: OverlappingZonesAlgorithm,
    pub show_zone_number: bool,
    pub zone_highlight_opacity: i32,
    pub zone_highlight_color: String,
    pub zone_color: String,
    pub zone_border_color: String,
    pub zone_number_color: String,
    pub layout_name_label_enabled: bool,
    pub layout_name_label_placement: LayoutNameLabelPlacement,
    pub layout_name_label_text_color: String,
    pub layout_name_label_background_color: String,
    pub layout_name_label_font_size: i32,
    pub layout_name_label_padding: i32,
    pub layout_name_label_duration: i32,
    pub make_dragged_window_transparent: bool,
    pub move_windows_based_on_position: bool,
    pub move_window_across_monitors: bool,
    pub flash_zones_on_quick_switch: bool,
    /// App IDs are case-sensitive on Linux; retain the entered text.
    pub excluded_apps: String,
}

impl Default for ZonesPreferences {
    fn default() -> Self {
        let s = ZonesSettings::default();
        Self {
            use_cursorpos_editor_startupscreen: s.use_cursorpos_editor_startupscreen,
            show_zones_on_all_monitors: s.show_zones_on_all_monitors,
            overlapping_zones_algorithm: s.overlapping_zones_algorithm,
            show_zone_number: s.show_zone_number,
            zone_highlight_opacity: s.zone_highlight_opacity,
            zone_highlight_color: s.zone_highlight_color,
            zone_color: s.zone_color,
            zone_border_color: s.zone_border_color,
            zone_number_color: s.zone_number_color,
            layout_name_label_enabled: s.layout_name_label_enabled,
            layout_name_label_placement: s.layout_name_label_placement,
            layout_name_label_text_color: s.layout_name_label_text_color,
            layout_name_label_background_color: s.layout_name_label_background_color,
            layout_name_label_font_size: s.layout_name_label_font_size,
            layout_name_label_padding: s.layout_name_label_padding,
            layout_name_label_duration: s.layout_name_label_duration,
            make_dragged_window_transparent: s.make_dragged_window_transparent,
            move_windows_based_on_position: s.move_windows_based_on_position,
            move_window_across_monitors: s.move_window_across_monitors,
            flash_zones_on_quick_switch: s.flash_zones_on_quick_switch,
            excluded_apps: s.excluded_apps,
        }
    }
}

impl ZonesPreferences {
    /// Fields consumed by the native display renderer; compositor policies do
    /// not invalidate its frame or restart its name-label timeout.
    pub fn same_appearance(&self, other: &Self) -> bool {
        self.show_zone_number == other.show_zone_number
            && self.zone_highlight_opacity == other.zone_highlight_opacity
            && self.zone_highlight_color == other.zone_highlight_color
            && self.zone_color == other.zone_color
            && self.zone_border_color == other.zone_border_color
            && self.zone_number_color == other.zone_number_color
            && self.layout_name_label_enabled == other.layout_name_label_enabled
            && self.layout_name_label_placement == other.layout_name_label_placement
            && self.layout_name_label_text_color == other.layout_name_label_text_color
            && self.layout_name_label_background_color == other.layout_name_label_background_color
            && self.layout_name_label_font_size == other.layout_name_label_font_size
            && self.layout_name_label_padding == other.layout_name_label_padding
            && self.layout_name_label_duration == other.layout_name_label_duration
    }

    pub fn validate(&self) -> Result<()> {
        for (name, value, min, max) in [
            ("불투명도", self.zone_highlight_opacity, 0, 100),
            ("글자 크기", self.layout_name_label_font_size, 8, 128),
            ("여백", self.layout_name_label_padding, 0, 64),
            ("표시 시간", self.layout_name_label_duration, 300, 10_000),
        ] {
            if !(min..=max).contains(&value) {
                return Err(ZonesError::InvalidData(format!(
                    "{name}: {min}–{max} 범위가 필요합니다"
                )));
            }
        }
        for value in [
            &self.zone_highlight_color,
            &self.zone_color,
            &self.zone_border_color,
            &self.zone_number_color,
            &self.layout_name_label_text_color,
            &self.layout_name_label_background_color,
        ] {
            if parse_color(value).is_none() {
                return Err(ZonesError::InvalidData(
                    "색상은 #RRGGBB 형식이어야 합니다".into(),
                ));
            }
        }
        if self.excluded_apps.len() > 65_536
            || self
                .excluded_apps
                .chars()
                .any(|c| c.is_control() && c != '\r' && c != '\n')
        {
            return Err(ZonesError::InvalidData(
                "앱 ID 목록은 64 KiB 이하이며 줄바꿈 외 제어 문자를 포함할 수 없습니다".into(),
            ));
        }
        Ok(())
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let value: Self = serde_json::from_str(json)?;
        value.validate()?;
        Ok(value)
    }

    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }
}

pub fn parse_color(value: &str) -> Option<[u8; 3]> {
    if value.len() != 7
        || !value.starts_with('#')
        || !value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
    {
        return None;
    }
    Some([
        u8::from_str_radix(&value[1..3], 16).ok()?,
        u8::from_str_radix(&value[3..5], 16).ok()?,
        u8::from_str_radix(&value[5..7], 16).ok()?,
    ])
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputDescription {
    pub name: String,
    pub logical_size: Option<(i32, i32)>,
    pub logical_position: Option<(i32, i32)>,
    pub integer_scale: i32,
}

impl std::fmt::Display for OutputDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some((w, h)) = self.logical_size {
            write!(f, "{} · {w}×{h}", self.name)
        } else {
            f.write_str(&self.name)
        }
    }
}
