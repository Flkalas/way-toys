//! Per-output settings. Replacement by rename is atomic on the Linux target;
//! this does not promise Windows replacement semantics.

use std::error::Error;
#[cfg(test)]
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use waytoys_zones::ZonesError;
use waytoys_zones::layout::grid_template;
use waytoys_zones::model::{CustomLayout, CustomLayoutData, CustomLayouts, DEFAULT_ZONE_COUNT};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn default_config_dir() -> Result<PathBuf, Box<dyn Error>> {
    waytoys_zones_config::default_config_dir().map_err(Into::into)
}

#[cfg(test)]
fn config_dir_from(
    xdg: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, Box<dyn Error>> {
    waytoys_zones_config::config_dir_from(xdg, home).map_err(Into::into)
}
/// Encode UTF-8 bytes, including path separators and percent signs. The prefix
/// also keeps names consisting of dots from becoming directory components.
pub fn output_path(config_dir: &Path, output_name: &str) -> PathBuf {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut name = String::from("output-");
    for byte in output_name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            name.push(char::from(byte));
        } else {
            name.push('%');
            name.push(char::from(HEX[usize::from(byte >> 4)]));
            name.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    name.push_str(".json");
    config_dir.join("outputs").join(name)
}

fn validate_grid(layout: &CustomLayout) -> Result<(), Box<dyn Error>> {
    layout.validate()?;
    if !matches!(&layout.data, CustomLayoutData::Grid(_)) {
        return Err(
            ZonesError::InvalidData("the grid editor requires a Grid layout".into()).into(),
        );
    }
    Ok(())
}

pub fn load(config_dir: &Path, output_name: &str) -> Result<CustomLayout, Box<dyn Error>> {
    let json = match fs::read_to_string(output_path(config_dir, output_name)) {
        Ok(json) => json,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(CustomLayout::new(
                format!("waytoys-grid:{output_name}"),
                format!("{output_name} grid"),
                CustomLayoutData::Grid(grid_template(DEFAULT_ZONE_COUNT, false)?),
            )?);
        }
        Err(error) => return Err(error.into()),
    };
    let mut layouts = CustomLayouts::from_json(&json)?;
    if layouts.custom_layouts.len() != 1 {
        return Err(ZonesError::InvalidData(
            "the grid editor requires exactly one custom layout".into(),
        )
        .into());
    }
    let layout = layouts.custom_layouts.remove(0);
    validate_grid(&layout)?;
    Ok(layout)
}

pub fn save(
    config_dir: &Path,
    output_name: &str,
    layout: &CustomLayout,
) -> Result<(), Box<dyn Error>> {
    // Validate and serialize before creating directories or touching any file.
    validate_grid(layout)?;
    let json = CustomLayouts {
        custom_layouts: vec![layout.clone()],
    }
    .to_json()?;
    let destination = output_path(config_dir, output_name);
    let directory = destination
        .parent()
        .ok_or_else(|| io::Error::other("missing settings directory"))?;
    fs::create_dir_all(directory)?;
    atomic_write(&destination, json.as_bytes())?;
    Ok(())
}

fn atomic_write(destination: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = destination
        .parent()
        .ok_or_else(|| io::Error::other("missing settings directory"))?;
    let (temporary, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(".waytoys-{}-{sequence}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (path, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, destination)?;
        // Linux supports directory fsync for durable directory-entry updates.
        #[cfg(unix)]
        File::open(directory)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use waytoys_zones::model::CanvasLayout;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            loop {
                let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "waytoys-store-test-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn output_names_cannot_escape_the_output_directory() {
        let directory = TempDir::new();
        for name in [
            "../../escape",
            "..",
            "",
            "C:\\escape",
            "/tmp/file",
            "화면/%",
        ] {
            let path = output_path(&directory.0, name);
            assert_eq!(path.parent(), Some(directory.0.join("outputs").as_path()));
            assert!(
                path.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("output-")
            );
        }
        assert_eq!(
            output_path(&directory.0, "../%"),
            directory.0.join("outputs/output-..%2F%25.json")
        );
        assert_ne!(
            output_path(&directory.0, "/"),
            output_path(&directory.0, "%2F")
        );
    }

    #[test]
    fn missing_file_defaults_to_three_columns_without_writing() {
        let directory = TempDir::new();
        let layout = load(&directory.0, "DP-1").unwrap();
        assert_eq!(layout.uuid, "waytoys-grid:DP-1");
        assert_eq!(layout.name, "DP-1 grid");
        let CustomLayoutData::Grid(grid) = layout.data else {
            panic!("expected grid")
        };
        assert_eq!((grid.rows, grid.columns), (1, 3));
        assert_eq!(grid.columns_percentage, vec![3333, 3333, 3334]);
        assert_eq!(grid.cell_child_map, vec![vec![0, 1, 2]]);
        assert!(!output_path(&directory.0, "DP-1").exists());
    }

    #[test]
    fn grid_json_roundtrip_preserves_metadata_and_settings() {
        let directory = TempDir::new();
        let mut layout = load(&directory.0, "DP-1").unwrap();
        layout.uuid = "original identity".into();
        layout.name = "한글 custom name".into();
        let CustomLayoutData::Grid(grid) = &mut layout.data else {
            unreachable!()
        };
        grid.spacing = -5;
        grid.show_spacing = false;
        grid.sensitivity_radius = 37;
        save(&directory.0, "DP-1", &layout).unwrap();
        assert_eq!(load(&directory.0, "DP-1").unwrap(), layout);
        let saved = fs::read_to_string(output_path(&directory.0, "DP-1")).unwrap();
        assert_eq!(
            CustomLayouts::from_json(&saved).unwrap().custom_layouts,
            vec![layout]
        );
    }

    #[test]
    fn unsupported_or_malformed_files_are_refused_unchanged() {
        let directory = TempDir::new();
        let path = output_path(&directory.0, "DP-1");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let canvas = CustomLayout::new(
            "canvas",
            "canvas",
            CustomLayoutData::Canvas(CanvasLayout::default()),
        )
        .unwrap();
        let grid = load(&directory.0, "DP-1").unwrap();
        for json in [
            "malformed".to_owned(),
            CustomLayouts::default().to_json().unwrap(),
            CustomLayouts {
                custom_layouts: vec![canvas],
            }
            .to_json()
            .unwrap(),
            CustomLayouts {
                custom_layouts: vec![
                    grid.clone(),
                    CustomLayout {
                        uuid: "second".into(),
                        ..grid
                    },
                ],
            }
            .to_json()
            .unwrap(),
        ] {
            fs::write(&path, &json).unwrap();
            assert!(load(&directory.0, "DP-1").is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), json);
        }
    }

    #[test]
    fn failed_save_preserves_previous_file() {
        let directory = TempDir::new();
        let mut layout = load(&directory.0, "DP-1").unwrap();
        save(&directory.0, "DP-1", &layout).unwrap();
        let path = output_path(&directory.0, "DP-1");
        let original = fs::read(&path).unwrap();
        layout.uuid.clear();
        assert!(save(&directory.0, "DP-1", &layout).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        let blocked = path.parent().unwrap().join("blocked");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("marker"), "retained").unwrap();
        assert!(atomic_write(&blocked, b"replacement").is_err());
        assert_eq!(
            fs::read_to_string(blocked.join("marker")).unwrap(),
            "retained"
        );
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
    }

    #[test]
    fn config_resolution_requires_absolute_paths() {
        let directory = TempDir::new();
        let home = directory.0.as_os_str().to_owned();
        assert_eq!(
            config_dir_from(None, Some(home.clone())).unwrap(),
            directory.0.join(".config/waytoys/zones")
        );
        assert_eq!(
            config_dir_from(Some(home.clone()), None).unwrap(),
            directory.0.join("waytoys/zones")
        );
        assert_eq!(
            config_dir_from(Some(OsString::new()), Some(home)).unwrap(),
            directory.0.join(".config/waytoys/zones")
        );
        assert!(config_dir_from(Some("relative".into()), None).is_err());
        assert!(config_dir_from(None, None).is_err());
    }
}
