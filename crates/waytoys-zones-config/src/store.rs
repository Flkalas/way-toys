use crate::ZonesPreferences;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub type ConfigResult<T> = Result<T, String>;
pub const SAVE_DELAY: Duration = Duration::from_millis(200);
pub const RELOAD_INTERVAL: Duration = Duration::from_millis(500);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn default_config_dir() -> ConfigResult<PathBuf> {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

pub fn config_dir_from(xdg: Option<OsString>, home: Option<OsString>) -> ConfigResult<PathBuf> {
    let base = if let Some(xdg) = xdg.filter(|v| !v.is_empty()) {
        let path = PathBuf::from(xdg);
        if !path.is_absolute() {
            return Err("XDG_CONFIG_HOME must be absolute".into());
        }
        path
    } else {
        let path = home
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or("HOME is required when XDG_CONFIG_HOME is unset")?;
        if !path.is_absolute() {
            return Err("HOME must be absolute".into());
        }
        path.join(".config")
    };
    Ok(base.join("waytoys/zones"))
}

fn read(path: &Path) -> ConfigResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn decode(bytes: &Option<Vec<u8>>) -> ConfigResult<ZonesPreferences> {
    match bytes {
        None => Ok(ZonesPreferences::default()),
        Some(bytes) => {
            ZonesPreferences::from_json(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        }
    }
}

/// Kernel lock lives until drop (including after an abnormal process exit).
/// Keep the lock inode in place: unlinking it would permit competing locks.
pub struct InstanceGuard {
    _file: File,
}
impl InstanceGuard {
    pub fn acquire(directory: &Path, name: &str) -> ConfigResult<Self> {
        if !matches!(name, "editor" | "settings" | "preferences") {
            return Err("invalid lock name".into());
        }
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(format!(".{name}.lock")))
            .map_err(|e| e.to_string())?;
        file.try_lock()
            .map_err(|e| format!("이미 실행 중이거나 잠금을 사용할 수 없습니다 ({name}): {e}"))?;
        Ok(Self { _file: file })
    }
}

pub struct PreferencesStore {
    directory: PathBuf,
    expected: Option<Vec<u8>>,
}

impl PreferencesStore {
    pub fn open(directory: &Path) -> ConfigResult<(Self, ZonesPreferences)> {
        let expected = read(&directory.join("preferences.json"))?;
        let preferences = decode(&expected)?;
        Ok((
            Self {
                directory: directory.into(),
                expected,
            },
            preferences,
        ))
    }

    pub fn save(&mut self, preferences: &ZonesPreferences) -> ConfigResult<()> {
        let json = preferences.to_json().map_err(|e| e.to_string())?;
        let _lock = InstanceGuard::acquire(&self.directory, "preferences")?;
        let destination = self.directory.join("preferences.json");
        if fs::symlink_metadata(&destination).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("설정 파일이 심볼릭 링크이므로 교체하지 않았습니다".into());
        }
        let current = read(&destination)?;
        decode(&current).map_err(|e| format!("기존 설정 파일을 보존했습니다: {e}"))?;
        if current != self.expected {
            return Err("다른 프로그램이 설정을 변경했습니다. 파일 다시 읽기로 확인하세요".into());
        }
        atomic_write(&destination, json.as_bytes()).map_err(|e| format!("저장 실패: {e}"))?;
        self.expected = Some(json.into_bytes());
        Ok(())
    }
}

/// Same-directory rename, file fsync, and directory fsync on the Linux target.
fn atomic_write(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = destination
        .parent()
        .ok_or_else(|| io::Error::other("missing directory"))?;
    let (temporary, mut file) = loop {
        let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(".preferences-{}-{seq}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, destination)?;
        #[cfg(unix)]
        File::open(directory)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub struct Autosave {
    pub preferences: ZonesPreferences,
    pub error: Option<String>,
    store: Option<PreferencesStore>,
    changed: Option<Instant>,
    directory: PathBuf,
}

impl Autosave {
    pub fn open(directory: PathBuf) -> Self {
        let mut s = Self {
            preferences: ZonesPreferences::default(),
            error: None,
            store: None,
            changed: None,
            directory,
        };
        s.reload();
        s
    }
    pub fn reload(&mut self) {
        match PreferencesStore::open(&self.directory) {
            Ok((store, preferences)) => {
                self.store = Some(store);
                self.preferences = preferences;
                self.changed = None;
                self.error = None;
            }
            Err(e) => {
                self.store = None;
                self.error = Some(format!("설정을 읽지 못했습니다. 원본을 보존합니다: {e}"));
            }
        }
    }
    pub fn blocked(&self) -> bool {
        self.store.is_none()
    }
    pub fn pending(&self) -> bool {
        self.changed.is_some()
    }
    pub fn change(&mut self, preferences: ZonesPreferences, now: Instant) -> ConfigResult<()> {
        if self.blocked() {
            return Err("설정 파일을 먼저 복구하고 다시 읽어 주세요".into());
        }
        preferences.validate().map_err(|e| e.to_string())?;
        if preferences != self.preferences {
            self.preferences = preferences;
            self.changed = Some(now);
        }
        Ok(())
    }
    pub fn tick(&mut self, now: Instant) {
        if self.error.is_none()
            && self
                .changed
                .is_some_and(|t| now.saturating_duration_since(t) >= SAVE_DELAY)
        {
            let _ = self.flush();
        }
    }
    pub fn flush(&mut self) -> ConfigResult<()> {
        if !self.pending() {
            return Ok(());
        }
        let result = self
            .store
            .as_mut()
            .ok_or_else(|| "설정을 읽을 수 없습니다".to_string())?
            .save(&self.preferences);
        match result {
            Ok(()) => {
                self.changed = None;
                self.error = None;
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }
}

pub struct PreferencesWatch {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
    pub preferences: ZonesPreferences,
    pub error: Option<String>,
}

impl PreferencesWatch {
    pub fn new(directory: &Path) -> Self {
        let mut s = Self {
            path: directory.join("preferences.json"),
            bytes: None,
            preferences: ZonesPreferences::default(),
            error: None,
        };
        s.refresh();
        s
    }
    /// Invalid input preserves the last valid value; only real preference changes redraw.
    pub fn refresh(&mut self) -> bool {
        let bytes = match read(&self.path) {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(e);
                return false;
            }
        };
        if self.bytes == bytes && self.error.is_none() {
            return false;
        }
        self.bytes = bytes;
        match decode(&self.bytes) {
            Ok(p) => {
                let changed = p != self.preferences;
                self.preferences = p;
                self.error = None;
                changed
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }
}
