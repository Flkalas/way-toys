mod wayland;
mod x11;

use std::{error::Error, path::PathBuf};
use waytoys_zones::model::{CustomLayout, CustomLayouts};
use waytoys_zones_editor::store;

#[derive(Default)]
pub struct Options {
    pub output: Option<String>,
    pub config_dir: PathBuf,
    pub list: bool,
    pub json: bool,
    pub preview: Option<CustomLayout>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("waytoys-zones-editor: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut options = Options::default();
    let mut config_dir = None;
    let mut preview_file = None;
    let mut layout_index: usize = 0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => options.output = Some(args.next().ok_or("--output needs a name")?),
            "--config-dir" => {
                config_dir = Some(PathBuf::from(
                    args.next().ok_or("--config-dir needs a path")?,
                ))
            }
            "--list-outputs" => options.list = true,
            "--json" => options.json = true,
            "--preview" => {
                preview_file = Some(
                    args.next()
                        .ok_or("--preview needs a custom-layouts JSON file")?,
                )
            }
            "--layout-index" => {
                layout_index = args
                    .next()
                    .ok_or("--layout-index needs a number")?
                    .parse()?
            }
            "--help" | "-h" => {
                println!(
                    "waytoys-zones-editor [--output NAME] [--config-dir PATH] [--list-outputs [--json]]\n  [--preview CUSTOM-LAYOUTS.json [--layout-index N]]\n\nClick to split; Shift changes orientation; drag dividers to resize.\nDrag across zones then click Merge. Ctrl+S saves and closes; Esc cancels.\nPreview mode shows Canvas/Grid rectangles read-only; Esc closes.\nOnly the selected output is covered. No application windows are moved."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}; see --help").into()),
        }
    }
    if options.json && !options.list {
        return Err("--json requires --list-outputs".into());
    }
    options.config_dir = match config_dir {
        Some(path) => path,
        None => store::default_config_dir()?,
    };
    if let Some(file) = preview_file {
        let layouts = CustomLayouts::from_json(&std::fs::read_to_string(file)?)?;
        options.preview = Some(
            layouts
                .custom_layouts
                .get(layout_index)
                .ok_or("unknown preview layout index")?
                .clone(),
        );
    } else if layout_index != 0 {
        return Err("--layout-index requires --preview".into());
    }
    let _instance = if options.list {
        None
    } else {
        Some(waytoys_zones_config::InstanceGuard::acquire(
            &options.config_dir,
            "editor",
        )?)
    };
    match waytoys_desktop::Session::current()? {
        waytoys_desktop::Session::X11 => x11::run(options),
        waytoys_desktop::Session::Wayland => wayland::run(options),
    }
}
