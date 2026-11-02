//! Standalone Zones model/editor CLI. It never controls other applications.

use std::error::Error;
use std::fs;
use std::str::FromStr;

use serde_json::json;
use waytoys_zones::canvas_editor::{CanvasEditor, ResizeHandle};
use waytoys_zones::geometry::Size;
use waytoys_zones::grid_editor::{Axis, GridEditor};
use waytoys_zones::layout::calculate_custom;
use waytoys_zones::model::{CustomLayoutData, CustomLayouts};
use waytoys_zones::settings::ZonesSettings;

const HELP: &str = "waytoys-zones: independent layout and editor model tool

  settings [SETTINGS.json]                Print defaults or validate native settings
  validate CUSTOM-LAYOUTS.json            Validate FancyZones custom layouts
  preview FILE INDEX WIDTH HEIGHT        Calculate work-area-local zone rectangles
  edit FILE INDEX grid-split ZONE x|y POSITION
  edit FILE INDEX grid-merge ZONE ZONE [ZONE...]
  edit FILE INDEX grid-resize RESIZER DELTA
  edit FILE INDEX canvas-add
  edit FILE INDEX canvas-delete ZONE
  edit FILE INDEX canvas-move ZONE DX DY [--magnetic]
  edit FILE INDEX canvas-resize ZONE HANDLE DX DY [--magnetic]

Edit results are JSON on stdout; input files are never overwritten.
Grid positions/deltas are in 0..10000 ratio units. Canvas coordinates are
reference-work-area units. Resize handles: n,ne,e,se,s,sw,w,nw.
No GUI, window hooks, window placement, shared host, or other feature required.";

fn number<T: FromStr>(args: &[String], index: usize, label: &str) -> Result<T, Box<dyn Error>> {
    args.get(index)
        .ok_or_else(|| format!("missing {label}"))?
        .parse()
        .map_err(|_| format!("invalid {label}").into())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("waytoys-zones: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        println!("{HELP}");
        return Ok(());
    };
    if matches!(command, "--help" | "-h" | "help") {
        println!("{HELP}");
        return Ok(());
    }
    if command == "settings" {
        if args.len() > 2 {
            return Err("settings accepts at most one file".into());
        }
        let settings = if let Some(file) = args.get(1) {
            ZonesSettings::from_json(&fs::read_to_string(file)?)?
        } else {
            ZonesSettings::default()
        };
        println!("{}", settings.to_json()?);
        return Ok(());
    }
    let file = args.get(1).ok_or("missing custom-layouts file")?;
    let mut layouts = CustomLayouts::from_json(&fs::read_to_string(file)?)?;
    match command {
        "validate" => {
            if args.len() != 2 {
                return Err("validate requires one file".into());
            }
            println!(
                "{}",
                json!({"valid":true,"layout_count":layouts.custom_layouts.len()})
            );
        }
        "preview" => {
            if args.len() != 5 {
                return Err("preview requires FILE INDEX WIDTH HEIGHT".into());
            }
            let index: usize = number(&args, 2, "layout index")?;
            let size = Size::new(number(&args, 3, "width")?, number(&args, 4, "height")?);
            let layout = layouts
                .custom_layouts
                .get(index)
                .ok_or("unknown layout index")?;
            let zones = calculate_custom(layout, size)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"uuid":layout.uuid,"name":layout.name,"work_area":size,"zones":zones})
                )?
            );
        }
        "edit" => {
            let index: usize = number(&args, 2, "layout index")?;
            let operation = args.get(3).ok_or("missing edit operation")?.as_str();
            let layout = layouts
                .custom_layouts
                .get_mut(index)
                .ok_or("unknown layout index")?;
            layout.data = match &layout.data {
                CustomLayoutData::Grid(grid) => {
                    let mut editor = GridEditor::new(grid.clone())?;
                    match operation {
                        "grid-split" if args.len() == 7 => {
                            let axis = match args[5].as_str() {
                                "x" => Axis::X,
                                "y" => Axis::Y,
                                _ => return Err("axis must be x or y".into()),
                            };
                            editor.split(
                                number(&args, 4, "zone")?,
                                axis,
                                number(&args, 6, "position")?,
                            )?;
                        }
                        "grid-merge" if args.len() >= 6 => {
                            let ids = (4..args.len())
                                .map(|i| number(&args, i, "zone"))
                                .collect::<Result<Vec<usize>, _>>()?;
                            editor.merge(&ids)?;
                        }
                        "grid-resize" if args.len() == 6 => editor.drag_resizer(
                            number(&args, 4, "resizer")?,
                            number(&args, 5, "delta")?,
                        )?,
                        _ => return Err("invalid grid edit or arguments; see --help".into()),
                    }
                    CustomLayoutData::Grid(editor.into_layout())
                }
                CustomLayoutData::Canvas(canvas) => {
                    let mut editor = CanvasEditor::new(canvas.clone())?;
                    let magnetic = args.last().is_some_and(|s| s == "--magnetic");
                    let count = args.len() - usize::from(magnetic);
                    match operation {
                        "canvas-add" if count == 4 && !magnetic => {
                            editor.add_default_zone()?;
                        }
                        "canvas-delete" if count == 5 && !magnetic => {
                            editor.remove_zone(number(&args, 4, "zone")?)?;
                        }
                        "canvas-move" if count == 7 => editor.move_zone(
                            number(&args, 4, "zone")?,
                            number(&args, 5, "dx")?,
                            number(&args, 6, "dy")?,
                            magnetic,
                        )?,
                        "canvas-resize" if count == 8 => {
                            let handle = match args[5].as_str() {
                                "n" => ResizeHandle::North,
                                "ne" => ResizeHandle::NorthEast,
                                "e" => ResizeHandle::East,
                                "se" => ResizeHandle::SouthEast,
                                "s" => ResizeHandle::South,
                                "sw" => ResizeHandle::SouthWest,
                                "w" => ResizeHandle::West,
                                "nw" => ResizeHandle::NorthWest,
                                _ => return Err("unknown resize handle".into()),
                            };
                            editor.resize_zone(
                                number(&args, 4, "zone")?,
                                handle,
                                number(&args, 6, "dx")?,
                                number(&args, 7, "dy")?,
                                magnetic,
                            )?;
                        }
                        _ => return Err("invalid canvas edit or arguments; see --help".into()),
                    }
                    CustomLayoutData::Canvas(editor.into_layout())
                }
            };
            println!("{}", layouts.to_json()?);
        }
        _ => return Err("unknown command; see --help".into()),
    }
    Ok(())
}
