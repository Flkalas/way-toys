//! Independent Zones feature model and editor logic.
//!
//! Coordinates are work-area-local logical units. This crate owns its settings
//! and layout data; it has no dependency on the shared host, other features,
//! window hooks, or a GUI toolkit.

pub mod canvas_editor;
pub mod geometry;
pub mod grid_editor;
pub mod layout;
pub mod magnetic_snap;
pub mod model;
pub mod selection;
pub mod settings;

pub type Result<T> = std::result::Result<T, ZonesError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZonesError {
    InvalidData(String),
    InvalidOperation(String),
    Json(String),
}

impl std::fmt::Display for ZonesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidData(message) => write!(f, "invalid Zones data: {message}"),
            Self::InvalidOperation(message) => write!(f, "invalid edit: {message}"),
            Self::Json(message) => write!(f, "JSON error: {message}"),
        }
    }
}

impl std::error::Error for ZonesError {}

impl From<serde_json::Error> for ZonesError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}
