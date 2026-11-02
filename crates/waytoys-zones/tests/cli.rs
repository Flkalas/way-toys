use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;
use waytoys_zones::model::{CustomLayoutData, CustomLayouts};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/custom-layouts.json")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_waytoys-zones"))
        .args(args)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .unwrap()
}

#[test]
fn preview_and_settings_run_without_a_desktop_session() {
    let path = fixture();
    let output = run(&["preview", path.to_str().unwrap(), "0", "1280", "720"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let preview: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(preview["zones"].as_array().unwrap().len(), 3);
    assert_eq!(preview["zones"][0]["rect"]["x"], 16);
    assert_eq!(preview["zones"][2]["rect"]["width"], 296);
    let settings = run(&["settings"]);
    assert!(settings.status.success());
    let value: Value = serde_json::from_slice(&settings.stdout).unwrap();
    assert_eq!(value["shift_drag"], true);
    assert_eq!(value["editor"]["spacing"], 16);
}

#[test]
fn editors_emit_importable_json_and_preserve_the_input_file() {
    let path = fixture();
    let original = std::fs::read(&path).unwrap();
    let split = run(&[
        "edit",
        path.to_str().unwrap(),
        "0",
        "grid-split",
        "1",
        "x",
        "5000",
    ]);
    assert!(
        split.status.success(),
        "{}",
        String::from_utf8_lossy(&split.stderr)
    );
    let document = CustomLayouts::from_json(std::str::from_utf8(&split.stdout).unwrap()).unwrap();
    let CustomLayoutData::Grid(grid) = &document.custom_layouts[0].data else {
        panic!("expected grid");
    };
    assert_eq!(grid.zone_count(), 4);
    let moved = run(&[
        "edit",
        path.to_str().unwrap(),
        "1",
        "canvas-move",
        "0",
        "10",
        "-20",
    ]);
    assert!(
        moved.status.success(),
        "{}",
        String::from_utf8_lossy(&moved.stderr)
    );
    let document = CustomLayouts::from_json(std::str::from_utf8(&moved.stdout).unwrap()).unwrap();
    let CustomLayoutData::Canvas(canvas) = &document.custom_layouts[1].data else {
        panic!("expected canvas");
    };
    assert_eq!((canvas.zones[0].x, canvas.zones[0].y), (110, 80));
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn invalid_cli_edits_fail_without_partial_json() {
    let path = fixture();
    let output = run(&[
        "edit",
        path.to_str().unwrap(),
        "0",
        "grid-split",
        "1",
        "x",
        "10001",
    ]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
