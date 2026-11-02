use std::{
    fs,
    time::{Duration, Instant},
};
use waytoys_zones_config::*;

#[test]
fn compositor_only_options_do_not_invalidate_the_native_frame() {
    let p = ZonesPreferences::default();
    let mut changed = p.clone();
    changed.excluded_apps = "org.example.App".into();
    changed.make_dragged_window_transparent = !p.make_dragged_window_transparent;
    changed.overlapping_zones_algorithm = OverlappingZonesAlgorithm::Largest;
    assert!(p.same_appearance(&changed));
    changed.zone_number_color = "#112233".into();
    assert!(!p.same_appearance(&changed));
}

#[test]
fn schema_is_exactly_the_approved_common_options_and_matches_source_defaults() {
    let p = ZonesPreferences::default();
    p.validate().unwrap();
    let value: serde_json::Value = serde_json::from_str(&p.to_json().unwrap()).unwrap();
    let original = serde_json::to_value(waytoys_zones::settings::ZonesSettings::default()).unwrap();
    let mut keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    let mut expected = vec![
        "use_cursorpos_editor_startupscreen",
        "show_zones_on_all_monitors",
        "overlapping_zones_algorithm",
        "show_zone_number",
        "zone_highlight_opacity",
        "zone_highlight_color",
        "zone_color",
        "zone_border_color",
        "zone_number_color",
        "layout_name_label_enabled",
        "layout_name_label_placement",
        "layout_name_label_text_color",
        "layout_name_label_background_color",
        "layout_name_label_font_size",
        "layout_name_label_padding",
        "layout_name_label_duration",
        "make_dragged_window_transparent",
        "move_windows_based_on_position",
        "move_window_across_monitors",
        "flash_zones_on_quick_switch",
        "excluded_apps",
    ];
    expected.sort();
    assert_eq!(keys, expected);
    for key in keys {
        assert_eq!(value[key], original[key], "default for {key}");
    }
    assert_eq!(ZonesPreferences::from_json("{}").unwrap(), p);
    for forbidden in [
        "enabled",
        "shift_drag",
        "ctrl_multi_select",
        "editor_hotkey",
        "editor",
        "restore_size",
        "span_zones_across_monitors",
        "mouse_switch",
        "mouse_wheel_layout_switch",
        "system_theme",
        "allow_snap_child_windows",
    ] {
        assert!(ZonesPreferences::from_json(&format!("{{\"{forbidden}\":true}}")).is_err());
    }
}

#[test]
fn validates_numbers_colors_and_app_ids_without_normalizing_valid_text() {
    for json in [
        r##"{"zone_color":"#123"}"##,
        r##"{"zone_color":"#GG0000"}"##,
        r#"{"zone_highlight_opacity":101}"#,
        r#"{"layout_name_label_font_size":7}"#,
        r#"{"layout_name_label_padding":65}"#,
        r#"{"layout_name_label_duration":299}"#,
        r#"{"excluded_apps":"bad\u0000id"}"#,
    ] {
        assert!(ZonesPreferences::from_json(json).is_err(), "{json}");
    }
    assert_eq!(parse_color("#aB12Ff"), Some([171, 18, 255]));
    assert_eq!(parse_color("#가나"), None);
    let p = ZonesPreferences {
        excluded_apps: "org.Example.App\n한글앱\n".into(),
        ..Default::default()
    };
    assert_eq!(
        ZonesPreferences::from_json(&p.to_json().unwrap()).unwrap(),
        p
    );
}

#[test]
fn missing_and_invalid_files_are_never_replaced_with_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("preferences.json");
    let (mut store, p) = PreferencesStore::open(temp.path()).unwrap();
    assert!(!path.exists());
    let invalid = b"{broken original bytes";
    fs::write(&path, invalid).unwrap();
    assert!(PreferencesStore::open(temp.path()).is_err());
    assert!(store.save(&p).is_err());
    let mut session = Autosave::open(temp.path().into());
    assert!(session.blocked());
    assert!(session.change(p, Instant::now()).is_err());
    session.tick(Instant::now() + Duration::from_secs(2));
    assert_eq!(fs::read(path).unwrap(), invalid);
}

#[test]
fn atomic_replacement_keeps_layouts_and_other_writers_changes() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join("settings.json");
    let layouts = temp.path().join("outputs");
    fs::create_dir(&layouts).unwrap();
    fs::write(&legacy, b"legacy untouched").unwrap();
    fs::write(layouts.join("output-DP-1.json"), b"layout untouched").unwrap();
    let (mut first, mut p) = PreferencesStore::open(temp.path()).unwrap();
    let (mut stale, _) = PreferencesStore::open(temp.path()).unwrap();
    p.zone_highlight_opacity = 80;
    first.save(&p).unwrap();
    assert!(stale.save(&ZonesPreferences::default()).is_err());
    assert_eq!(fs::read(legacy).unwrap(), b"legacy untouched");
    assert_eq!(
        fs::read(layouts.join("output-DP-1.json")).unwrap(),
        b"layout untouched"
    );
    assert_eq!(PreferencesStore::open(temp.path()).unwrap().1, p);
    assert!(
        !fs::read_dir(temp.path()).unwrap().any(|e| e
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "tmp"))
    );
}

#[test]
fn debounces_valid_edits_flushes_on_close_and_recovers_on_restart() {
    let temp = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let path = temp.path().join("preferences.json");
    let mut s = Autosave::open(temp.path().into());
    let mut p = s.preferences.clone();
    p.zone_highlight_opacity = 70;
    s.change(p.clone(), now).unwrap();
    s.tick(now + Duration::from_millis(199));
    assert!(!path.exists());
    p.zone_highlight_opacity = 80;
    s.change(p.clone(), now + Duration::from_millis(180))
        .unwrap();
    s.tick(now + Duration::from_millis(200));
    assert!(!path.exists());
    s.tick(now + Duration::from_millis(380));
    assert!(!s.pending());
    assert!(path.exists());
    assert_eq!(Autosave::open(temp.path().into()).preferences, p);
    p.show_zone_number = false;
    s.change(p.clone(), now + Duration::from_millis(400))
        .unwrap();
    s.flush().unwrap();
    assert_eq!(Autosave::open(temp.path().into()).preferences, p);
}

#[test]
fn failed_save_is_retryable_and_does_not_drop_pending_edits() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("preferences.json");
    let mut s = Autosave::open(temp.path().into());
    let now = Instant::now();
    let mut p = s.preferences.clone();
    p.show_zone_number = false;
    s.change(p.clone(), now).unwrap();
    fs::create_dir(&path).unwrap();
    s.tick(now + SAVE_DELAY);
    assert!(s.error.is_some());
    assert!(s.pending());
    assert!(path.is_dir());
    fs::remove_dir(&path).unwrap();
    s.flush().unwrap();
    assert!(s.error.is_none());
    assert!(!s.pending());
    assert_eq!(PreferencesStore::open(temp.path()).unwrap().1, p);
}

#[test]
fn watch_only_signals_effective_changes_and_keeps_last_valid_values() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("preferences.json");
    let mut w = PreferencesWatch::new(temp.path());
    assert!(!w.refresh());
    let p = ZonesPreferences {
        show_zone_number: false,
        ..Default::default()
    };
    fs::write(&path, p.to_json().unwrap()).unwrap();
    assert!(w.refresh());
    assert!(!w.refresh());
    fs::write(&path, b"{").unwrap();
    assert!(!w.refresh());
    assert!(w.error.is_some());
    assert_eq!(w.preferences, p);
    fs::write(&path, p.to_json().unwrap()).unwrap();
    assert!(!w.refresh());
    assert!(w.error.is_none());
}

#[test]
fn instance_lock_prevents_duplicates_and_releases_on_drop() {
    let temp = tempfile::tempdir().unwrap();
    let guard = InstanceGuard::acquire(temp.path(), "editor").unwrap();
    assert!(InstanceGuard::acquire(temp.path(), "editor").is_err());
    drop(guard);
    assert!(InstanceGuard::acquire(temp.path(), "editor").is_ok());
}

#[cfg(unix)]
#[test]
fn concurrent_readers_never_see_partial_json() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("preferences.json");
    let (mut store, mut p) = PreferencesStore::open(temp.path()).unwrap();
    store.save(&p).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    let reader = std::thread::spawn(move || {
        let mut count = 0;
        while !flag.load(Ordering::Relaxed) || count < 20 {
            ZonesPreferences::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
            count += 1;
        }
    });
    for i in 0..40 {
        p.zone_highlight_opacity = i;
        store.save(&p).unwrap();
    }
    done.store(true, Ordering::Relaxed);
    reader.join().unwrap();
}
