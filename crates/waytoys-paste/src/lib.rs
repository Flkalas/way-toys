pub mod monitor;
pub mod service;
pub mod store;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const BUS: &str = "org.waytoys.Paste1";
pub const PATH: &str = "/Paste";
pub const PICKER: &str = "org.waytoys.PastePicker1";
