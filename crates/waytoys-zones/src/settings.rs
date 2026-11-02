// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license. See LICENSE-MIT-PowerToys in the repository root.
// Rust adaptation of PowerToys 8827f848fbbac4afeedcae2907c71cd05fd5aa86:
// FancyZonesLib/Settings.{h,cpp}, FancyZonesEditorCommon/Data/LayoutDefaultSettings.cs,
// editor/FancyZonesEditor/Models/{GridLayoutModel,LayoutModel}.cs.

//! Passive settings and source defaults, with no hooks, file watchers or OS APIs.
//!
//! The native portable JSON schema is a flat object of snake_case field names,
//! with nested `editor` and hotkey objects. Enum values are snake_case strings;
//! colors are `#RRGGBB`; hotkeys use portable key names plus modifier booleans.
//! Missing settings inherit the source defaults. Unknown fields are rejected.
//! This is deliberately NOT a PowerToys `settings.json` wrapper importer.
//! Use `from_json`/`to_json` to validate; direct serde decoding does not validate.
//! Bounds follow the editor UI or Settings.cpp; out-of-range values are rejected
//! rather than clamped. Strict color/key validation is an additional safety rule.

use serde::{Deserialize, Serialize};

use crate::model::{
    DEFAULT_SENSITIVITY_RADIUS, DEFAULT_SPACING, DEFAULT_ZONE_COUNT, MAX_ZONES,
    validate_sensitivity, validate_spacing,
};
use crate::{Result, ZonesError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlappingZonesAlgorithm {
    #[default]
    Smallest,
    Largest,
    Positional,
    ClosestCenter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutNameLabelPlacement {
    TopLeft,
    #[default]
    TopCenter,
    TopRight,
    Center,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Portable shortcut intent, not a Windows virtual-key code. The eventual backend
/// resolves `key` (e.g. `backquote`, `page_down`, `page_up`, `x`) to its key map.
/// `super_key` represents the source's Windows-logo modifier. Layout-dependent
/// OEM_3 translation is intentionally deferred to the backend.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hotkey {
    pub super_key: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Hotkey {
    pub fn new(
        super_key: bool,
        ctrl: bool,
        alt: bool,
        shift: bool,
        key: impl Into<String>,
    ) -> Self {
        Self {
            super_key,
            ctrl,
            alt,
            shift,
            key: key.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        // An empty key with no modifiers is an explicitly unassigned shortcut.
        if self.key.is_empty() {
            if self.super_key || self.ctrl || self.alt || self.shift {
                return Err(ZonesError::InvalidData(
                    "a hotkey with modifiers requires a key".into(),
                ));
            }
        } else if self.key.len() > 128
            || self
                .key
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
        {
            return Err(ZonesError::InvalidData(
                "hotkey key must be a non-whitespace name of at most 128 bytes".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorSettings {
    pub show_spacing: bool,
    pub spacing: i32,
    pub zone_count: usize,
    pub sensitivity_radius: i32,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            show_spacing: true,
            spacing: DEFAULT_SPACING,
            zone_count: DEFAULT_ZONE_COUNT,
            sensitivity_radius: DEFAULT_SENSITIVITY_RADIUS,
        }
    }
}

impl EditorSettings {
    pub fn validate(&self) -> Result<()> {
        validate_spacing(self.spacing)?;
        validate_sensitivity(self.sensitivity_radius)?;
        if !(1..=MAX_ZONES).contains(&self.zone_count) {
            return Err(ZonesError::InvalidData(
                "editor zone count must be between 1 and 128".into(),
            ));
        }
        Ok(())
    }
}

/// All Settings.h persisted values, plus editor defaults. `excluded_apps_array`
/// is derived on demand instead of maintaining a second, potentially stale copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ZonesSettings {
    pub shift_drag: bool,
    pub mouse_switch: bool,
    pub mouse_middle_click_spanning_multiple_zones: bool,
    pub mouse_wheel_layout_switch: bool,
    pub display_or_work_area_change_move_windows: bool,
    /// Retained from Settings.h, although Settings.cpp does not load this field.
    pub zone_set_change_flash_zones: bool,
    pub zone_set_change_move_windows: bool,
    pub override_snap_hotkeys: bool,
    pub move_window_across_monitors: bool,
    pub move_windows_based_on_position: bool,
    pub app_last_zone_move_windows: bool,
    pub open_window_on_active_monitor: bool,
    pub restore_size: bool,
    pub quick_layout_switch: bool,
    pub flash_zones_on_quick_switch: bool,
    pub use_cursorpos_editor_startupscreen: bool,
    pub show_zones_on_all_monitors: bool,
    pub span_zones_across_monitors: bool,
    pub make_dragged_window_transparent: bool,
    pub system_theme: bool,
    pub show_zone_number: bool,
    pub allow_snap_child_windows: bool,
    pub disable_round_corners: bool,
    pub zone_color: String,
    pub zone_border_color: String,
    pub zone_highlight_color: String,
    pub zone_number_color: String,
    pub zone_highlight_opacity: i32,
    pub layout_name_label_enabled: bool,
    pub layout_name_label_placement: LayoutNameLabelPlacement,
    pub layout_name_label_text_color: String,
    pub layout_name_label_background_color: String,
    pub layout_name_label_font_size: i32,
    pub layout_name_label_padding: i32,
    /// Milliseconds.
    pub layout_name_label_duration: i32,
    pub overlapping_zones_algorithm: OverlappingZonesAlgorithm,
    pub editor_hotkey: Hotkey,
    pub window_switching: bool,
    pub next_tab_hotkey: Hotkey,
    pub prev_tab_hotkey: Hotkey,
    pub monitor_rotation: bool,
    pub monitor_rotation_hotkey: Hotkey,
    /// Original newline-delimited text, preserved without normalization.
    pub excluded_apps: String,
    pub editor: EditorSettings,
}

impl Default for ZonesSettings {
    fn default() -> Self {
        Self {
            shift_drag: true,
            mouse_switch: false,
            mouse_middle_click_spanning_multiple_zones: false,
            mouse_wheel_layout_switch: false,
            display_or_work_area_change_move_windows: true,
            zone_set_change_flash_zones: false,
            zone_set_change_move_windows: false,
            override_snap_hotkeys: false,
            move_window_across_monitors: false,
            move_windows_based_on_position: false,
            app_last_zone_move_windows: false,
            open_window_on_active_monitor: false,
            restore_size: false,
            quick_layout_switch: true,
            flash_zones_on_quick_switch: true,
            use_cursorpos_editor_startupscreen: true,
            show_zones_on_all_monitors: false,
            span_zones_across_monitors: false,
            make_dragged_window_transparent: true,
            system_theme: true,
            show_zone_number: true,
            allow_snap_child_windows: false,
            disable_round_corners: false,
            zone_color: "#AACDFF".into(),
            zone_border_color: "#FFFFFF".into(),
            zone_highlight_color: "#008CFF".into(),
            zone_number_color: "#000000".into(),
            zone_highlight_opacity: 50,
            layout_name_label_enabled: true,
            layout_name_label_placement: LayoutNameLabelPlacement::TopCenter,
            layout_name_label_text_color: "#FFFFFF".into(),
            layout_name_label_background_color: "#262626".into(),
            layout_name_label_font_size: 36,
            layout_name_label_padding: 12,
            layout_name_label_duration: 1500,
            overlapping_zones_algorithm: OverlappingZonesAlgorithm::Smallest,
            editor_hotkey: Hotkey::new(true, false, false, true, "backquote"),
            window_switching: true,
            next_tab_hotkey: Hotkey::new(true, false, false, false, "page_down"),
            prev_tab_hotkey: Hotkey::new(true, false, false, false, "page_up"),
            monitor_rotation: false,
            monitor_rotation_hotkey: Hotkey::new(false, false, true, false, "x"),
            excluded_apps: String::new(),
            editor: EditorSettings::default(),
        }
    }
}

impl ZonesSettings {
    pub fn validate(&self) -> Result<()> {
        self.editor.validate()?;
        for hotkey in [
            &self.editor_hotkey,
            &self.next_tab_hotkey,
            &self.prev_tab_hotkey,
            &self.monitor_rotation_hotkey,
        ] {
            hotkey.validate()?;
        }
        for (name, value, min, max) in [
            (
                "zone_highlight_opacity",
                self.zone_highlight_opacity,
                0,
                100,
            ),
            (
                "layout_name_label_font_size",
                self.layout_name_label_font_size,
                8,
                128,
            ),
            (
                "layout_name_label_padding",
                self.layout_name_label_padding,
                0,
                64,
            ),
            (
                "layout_name_label_duration",
                self.layout_name_label_duration,
                300,
                10_000,
            ),
        ] {
            if !(min..=max).contains(&value) {
                return Err(ZonesError::InvalidData(format!(
                    "{name} must be between {min} and {max}"
                )));
            }
        }
        for (name, value) in [
            ("zone_color", &self.zone_color),
            ("zone_border_color", &self.zone_border_color),
            ("zone_highlight_color", &self.zone_highlight_color),
            ("zone_number_color", &self.zone_number_color),
            (
                "layout_name_label_text_color",
                &self.layout_name_label_text_color,
            ),
            (
                "layout_name_label_background_color",
                &self.layout_name_label_background_color,
            ),
        ] {
            if value.len() != 7
                || !value.starts_with('#')
                || !value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
            {
                return Err(ZonesError::InvalidData(format!(
                    "{name} must be a #RRGGBB color"
                )));
            }
        }
        Ok(())
    }

    /// Source-style CR/LF splitting and uppercase lookup entries. Unicode case
    /// folding is Rust's portable Unicode mapping, not Win32 locale casing.
    pub fn excluded_apps_array(&self) -> Vec<String> {
        let uppercase = self.excluded_apps.to_uppercase();
        let mut remaining = uppercase.trim();
        let mut result = Vec::new();
        while !remaining.is_empty() {
            let end = remaining.find(['\r', '\n']).unwrap_or(remaining.len());
            result.push(remaining[..end].to_owned());
            remaining = remaining[end..].trim();
        }
        result
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let value: Self = serde_json::from_str(json)?;
        value.validate()?;
        Ok(value)
    }

    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_source_values_and_hotkey_modifiers() {
        let value = ZonesSettings::default();
        value.validate().unwrap();
        assert!(
            value.shift_drag
                && value.display_or_work_area_change_move_windows
                && value.quick_layout_switch
                && value.flash_zones_on_quick_switch
        );
        assert!(
            value.use_cursorpos_editor_startupscreen
                && value.make_dragged_window_transparent
                && value.system_theme
                && value.show_zone_number
                && value.window_switching
        );
        assert!(
            !value.override_snap_hotkeys
                && !value.monitor_rotation
                && !value.span_zones_across_monitors
        );
        assert_eq!(
            (
                &*value.zone_color,
                &*value.zone_highlight_color,
                value.zone_highlight_opacity
            ),
            ("#AACDFF", "#008CFF", 50)
        );
        assert_eq!(
            (
                value.layout_name_label_font_size,
                value.layout_name_label_padding,
                value.layout_name_label_duration
            ),
            (36, 12, 1500)
        );
        assert_eq!(
            value.editor_hotkey,
            Hotkey::new(true, false, false, true, "backquote")
        );
        assert_eq!(
            value.next_tab_hotkey,
            Hotkey::new(true, false, false, false, "page_down")
        );
        assert_eq!(
            value.prev_tab_hotkey,
            Hotkey::new(true, false, false, false, "page_up")
        );
        assert_eq!(
            value.monitor_rotation_hotkey,
            Hotkey::new(false, false, true, false, "x")
        );
        assert_eq!(
            (
                value.editor.zone_count,
                value.editor.spacing,
                value.editor.sensitivity_radius
            ),
            (3, 16, 20)
        );
    }

    #[test]
    fn portable_json_supports_partial_defaults_and_roundtrip() {
        assert_eq!(
            ZonesSettings::from_json("{}").unwrap(),
            ZonesSettings::default()
        );
        let settings = ZonesSettings::from_json(r#"{"shift_drag":false,"editor":{"spacing":-10},"overlapping_zones_algorithm":"closest_center"}"#).unwrap();
        assert!(!settings.shift_drag);
        assert_eq!(settings.editor.spacing, -10);
        assert_eq!(settings.editor.zone_count, 3);
        assert_eq!(
            settings.overlapping_zones_algorithm,
            OverlappingZonesAlgorithm::ClosestCenter
        );
        assert_eq!(
            ZonesSettings::from_json(&settings.to_json().unwrap()).unwrap(),
            settings
        );
    }

    #[test]
    fn invalid_values_and_powertoys_wrappers_are_rejected() {
        for json in [
            r#"{"properties":{"fancyzones_shiftDrag":{"value":false}}}"#,
            r#"{"zone_highlight_opacity":101}"#,
            r#"{"layout_name_label_font_size":7}"#,
            r#"{"layout_name_label_padding":65}"#,
            r#"{"layout_name_label_duration":299}"#,
            r#"{"layout_name_label_placement":"nowhere"}"#,
            r##"{"zone_color":"#GG0000"}"##,
            r##"{"zone_color":"#💥"}"##,
            r#"{"editor":{"zone_count":129}}"#,
            r#"{"editor":{"sensitivity_radius":-1}}"#,
            r#"{"editor_hotkey":{"super_key":true}}"#,
        ] {
            assert!(ZonesSettings::from_json(json).is_err(), "accepted {json}");
        }
        assert!(Hotkey::default().validate().is_ok());
    }

    #[test]
    fn excluded_apps_preserve_original_and_derive_source_style_entries() {
        let settings = ZonesSettings {
            excluded_apps: " \r\nfoo.exe \r\n\r\n bar.exe\n ".into(),
            ..ZonesSettings::default()
        };
        assert_eq!(settings.excluded_apps_array(), vec!["FOO.EXE ", "BAR.EXE"]);
        assert_eq!(settings.excluded_apps, " \r\nfoo.exe \r\n\r\n bar.exe\n ");
    }
}
