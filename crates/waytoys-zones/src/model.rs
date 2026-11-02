// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license. See LICENSE-MIT-PowerToys in the repository root.
// Rust adaptation of PowerToys 8827f848fbbac4afeedcae2907c71cd05fd5aa86:
// FancyZonesEditorCommon/Data/{CustomLayouts,LayoutDefaultSettings,Constants}.cs,
// editor/FancyZonesEditor/Models/{GridLayoutModel,LayoutModel}.cs.

//! Feature-owned custom layouts. JSON uses the original FancyZones kebab-case
//! names, including capital `X`/`Y` and the `type`/`info` discriminated union.
//! `from_json`/`to_json` validate; direct serde deserialization only decodes data.
//! Validation intentionally adds safety constraints to the source JSON reader:
//! at most 128 rows/columns/zones, contiguous zero-based IDs, rectangular merged
//! zones, positive dimensions, and the editor's spacing/sensitivity bounds.
//! IDs are opaque nonempty strings; no Windows GUID or monitor identity is needed.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::{Result, ZonesError};

pub const GRID_MULTIPLIER: i32 = 10_000;
pub const MAX_ZONES: usize = 128;
pub const DEFAULT_SPACING: i32 = 16;
pub const DEFAULT_SENSITIVITY_RADIUS: i32 = 20;
pub const DEFAULT_ZONE_COUNT: usize = 3;
pub type ZoneId = usize;

fn default_show_spacing() -> bool {
    true
}
fn default_spacing() -> i32 {
    DEFAULT_SPACING
}
fn default_sensitivity() -> i32 {
    DEFAULT_SENSITIVITY_RADIUS
}

fn invalid(message: impl Into<String>) -> ZonesError {
    ZonesError::InvalidData(message.into())
}

pub(crate) fn validate_spacing(spacing: i32) -> Result<()> {
    if !(-10..=1000).contains(&spacing) {
        return Err(invalid("spacing must be between -10 and 1000"));
    }
    Ok(())
}

pub(crate) fn validate_sensitivity(radius: i32) -> Result<()> {
    if !(0..=1000).contains(&radius) {
        return Err(invalid("sensitivity radius must be between 0 and 1000"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutType {
    #[default]
    Blank,
    Focus,
    Columns,
    Rows,
    Grid,
    PriorityGrid,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasZone {
    #[serde(rename = "X")]
    pub x: i32,
    #[serde(rename = "Y")]
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl CanvasZone {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn validate(&self) -> Result<()> {
        Rect::from(*self).validate()
    }
}

impl From<Rect> for CanvasZone {
    fn from(rect: Rect) -> Self {
        Self::new(rect.x, rect.y, rect.width, rect.height)
    }
}

impl From<CanvasZone> for Rect {
    fn from(zone: CanvasZone) -> Self {
        Self::new(zone.x, zone.y, zone.width, zone.height)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CanvasLayout {
    pub ref_width: i32,
    pub ref_height: i32,
    pub zones: Vec<CanvasZone>,
    #[serde(default = "default_sensitivity")]
    pub sensitivity_radius: i32,
}

impl Default for CanvasLayout {
    fn default() -> Self {
        Self {
            ref_width: 1,
            ref_height: 1,
            zones: Vec::new(),
            sensitivity_radius: DEFAULT_SENSITIVITY_RADIUS,
        }
    }
}

impl CanvasLayout {
    /// An empty editable canvas. The reference dimensions must be positive.
    pub fn new(ref_width: i32, ref_height: i32) -> Result<Self> {
        let value = Self {
            ref_width,
            ref_height,
            ..Self::default()
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<()> {
        if self.ref_width <= 0 || self.ref_height <= 0 {
            return Err(invalid("canvas reference dimensions must be positive"));
        }
        if self.zones.len() > MAX_ZONES {
            return Err(invalid("canvas exceeds the maximum of 128 zones"));
        }
        validate_sensitivity(self.sensitivity_radius)?;
        // Negative origins and zones extending outside the reference canvas are
        // retained: the source canvas model permits these editing states.
        for zone in &self.zones {
            zone.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GridLayout {
    pub rows: usize,
    pub columns: usize,
    pub rows_percentage: Vec<i32>,
    pub columns_percentage: Vec<i32>,
    pub cell_child_map: Vec<Vec<ZoneId>>,
    #[serde(default = "default_show_spacing")]
    pub show_spacing: bool,
    #[serde(default = "default_spacing")]
    pub spacing: i32,
    #[serde(default = "default_sensitivity")]
    pub sensitivity_radius: i32,
}

impl Default for GridLayout {
    fn default() -> Self {
        Self {
            rows: 1,
            columns: 1,
            rows_percentage: vec![GRID_MULTIPLIER],
            columns_percentage: vec![GRID_MULTIPLIER],
            cell_child_map: vec![vec![0]],
            show_spacing: true,
            spacing: DEFAULT_SPACING,
            sensitivity_radius: DEFAULT_SENSITIVITY_RADIUS,
        }
    }
}

impl GridLayout {
    pub fn new(
        rows: usize,
        columns: usize,
        rows_percentage: Vec<i32>,
        columns_percentage: Vec<i32>,
        cell_child_map: Vec<Vec<ZoneId>>,
    ) -> Result<Self> {
        let value = Self {
            rows,
            columns,
            rows_percentage,
            columns_percentage,
            cell_child_map,
            ..Self::default()
        };
        value.validate()?;
        Ok(value)
    }

    /// Count distinct IDs. Call `validate` before using IDs as array indices.
    pub fn zone_count(&self) -> usize {
        self.cell_child_map
            .iter()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
            .len()
    }

    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_ZONES).contains(&self.rows) || !(1..=MAX_ZONES).contains(&self.columns) {
            return Err(invalid("grid dimensions must each be between 1 and 128"));
        }
        for (name, count, percentages) in [
            ("row", self.rows, &self.rows_percentage),
            ("column", self.columns, &self.columns_percentage),
        ] {
            if percentages.len() != count
                || percentages.iter().any(|&value| value <= 0)
                || percentages
                    .iter()
                    .map(|&value| i64::from(value))
                    .sum::<i64>()
                    != i64::from(GRID_MULTIPLIER)
            {
                return Err(invalid(format!(
                    "{name} percentages must match dimension, be positive, and sum to 10000"
                )));
            }
        }
        if self.cell_child_map.len() != self.rows
            || self
                .cell_child_map
                .iter()
                .any(|row| row.len() != self.columns)
        {
            return Err(invalid(
                "grid cell map must match its rectangular dimensions",
            ));
        }
        validate_spacing(self.spacing)?;
        validate_sensitivity(self.sensitivity_radius)?;

        // Store each ID's bounding box and cell count. A rectangle is full iff
        // its count equals its bounding-box area; disconnected/L-shaped IDs fail.
        let mut bounds = vec![None::<(usize, usize, usize, usize, usize)>; MAX_ZONES];
        for (r, row) in self.cell_child_map.iter().enumerate() {
            for (c, &id) in row.iter().enumerate() {
                if id >= MAX_ZONES {
                    return Err(invalid("zone ID must be below 128"));
                }
                bounds[id] = Some(match bounds[id] {
                    Some((r0, c0, r1, c1, count)) => {
                        (r0.min(r), c0.min(c), r1.max(r), c1.max(c), count + 1)
                    }
                    None => (r, c, r, c, 1),
                });
            }
        }
        let count = bounds.iter().filter(|bound| bound.is_some()).count();
        if bounds[..count].iter().any(Option::is_none) {
            return Err(invalid("zone IDs must be contiguous starting at zero"));
        }
        for &(r0, c0, r1, c1, cells) in bounds.iter().flatten() {
            if (r1 - r0 + 1) * (c1 - c0 + 1) != cells {
                return Err(invalid("each grid zone must occupy a complete rectangle"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "info", rename_all = "lowercase")]
pub enum CustomLayoutData {
    Grid(GridLayout),
    Canvas(CanvasLayout),
}

impl CustomLayoutData {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Grid(grid) => grid.validate(),
            Self::Canvas(canvas) => canvas.validate(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomLayout {
    pub uuid: String,
    pub name: String,
    #[serde(flatten)]
    pub data: CustomLayoutData,
}

impl CustomLayout {
    pub fn new(
        uuid: impl Into<String>,
        name: impl Into<String>,
        data: CustomLayoutData,
    ) -> Result<Self> {
        let value = Self {
            uuid: uuid.into(),
            name: name.into(),
            data,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<()> {
        if self.uuid.trim().is_empty() {
            return Err(invalid("custom layout ID cannot be empty"));
        }
        self.data.validate()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CustomLayouts {
    pub custom_layouts: Vec<CustomLayout>,
}

impl CustomLayouts {
    pub fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        for layout in &self.custom_layouts {
            layout.validate()?;
            if !ids.insert(&layout.uuid) {
                return Err(invalid("duplicate custom layout ID"));
            }
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
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_custom_layout_json_roundtrips() {
        let json = r#"{"custom-layouts":[
            {"uuid":"{72B98B71-5098-4ED4-A604-FE76E5773F4A}","name":"Two columns","type":"grid","info":{"rows":1,"columns":2,"rows-percentage":[10000],"columns-percentage":[5000,5000],"cell-child-map":[[0,1]],"show-spacing":true,"spacing":16,"sensitivity-radius":20}},
            {"uuid":"{5AF0823C-12AA-4F08-8A74-568C20968DBC}","name":"Canvas","type":"canvas","info":{"ref-width":1920,"ref-height":1080,"zones":[{"X":-10,"Y":40,"width":800,"height":600}],"sensitivity-radius":20}}
        ]}"#;
        let decoded = CustomLayouts::from_json(json).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&decoded.to_json().unwrap()).unwrap(),
            serde_json::from_str::<serde_json::Value>(json).unwrap()
        );
        assert_eq!(
            CustomLayouts::from_json(&decoded.to_json().unwrap()).unwrap(),
            decoded
        );
    }

    #[test]
    fn absent_optional_values_use_source_defaults() {
        let grid: GridLayout = serde_json::from_str(r#"{"rows":1,"columns":1,"rows-percentage":[10000],"columns-percentage":[10000],"cell-child-map":[[0]]}"#).unwrap();
        assert_eq!(grid, GridLayout::default());
        let canvas: CanvasLayout =
            serde_json::from_str(r#"{"ref-width":1920,"ref-height":1080,"zones":[]}"#).unwrap();
        assert_eq!(canvas.sensitivity_radius, 20);
        assert!(canvas.validate().is_ok());
        assert_eq!(
            serde_json::to_string(&LayoutType::PriorityGrid).unwrap(),
            "\"priority-grid\""
        );
    }

    #[test]
    fn malformed_grids_reject_shape_percentages_ids_and_nonrectangular_zones() {
        let valid = GridLayout::new(
            2,
            2,
            vec![5000, 5000],
            vec![5000, 5000],
            vec![vec![0, 1], vec![0, 1]],
        )
        .unwrap();
        for map in [
            vec![vec![0], vec![1]],
            vec![vec![0, 2], vec![0, 2]],
            vec![vec![0, 0], vec![0, 1]],
            vec![vec![0, 128], vec![0, 128]],
        ] {
            let mut bad = valid.clone();
            bad.cell_child_map = map;
            assert!(bad.validate().is_err());
        }
        for percentages in [
            vec![5000],
            vec![5000, 4999],
            vec![0, 10000],
            vec![-1, 10001],
            vec![i32::MAX, i32::MAX],
        ] {
            let mut bad = valid.clone();
            bad.rows_percentage = percentages;
            assert!(bad.validate().is_err());
        }
        let mut grid = valid;
        grid.spacing = -10;
        assert!(grid.validate().is_ok());
        grid.spacing = -11;
        assert!(grid.validate().is_err());
    }

    #[test]
    fn invalid_canvas_and_duplicate_ids_reject() {
        assert!(CanvasLayout::new(0, 1080).is_err());
        let mut canvas = CanvasLayout::new(1920, 1080).unwrap();
        canvas.zones.push(CanvasZone::new(0, 0, -1, 10));
        assert!(canvas.validate().is_err());
        let layout = CustomLayout::new(
            "portable-id",
            "",
            CustomLayoutData::Grid(GridLayout::default()),
        )
        .unwrap();
        let layouts = CustomLayouts {
            custom_layouts: vec![layout.clone(), layout],
        };
        assert!(layouts.to_json().is_err());
        assert!(CustomLayouts::from_json(r#"{}"#).is_err());
        assert!(
            CustomLayouts::from_json(
                r#"{"custom-layouts":[{"uuid":"id","name":"n","type":"unknown","info":{}}]}"#
            )
            .is_err()
        );
    }
}
