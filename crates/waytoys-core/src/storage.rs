use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct FileLock {
    _file: File,
}
impl FileLock {
    pub fn acquire(directory: &Path, name: &str) -> Result<Self, String> {
        if !matches!(
            name,
            "editor" | "settings" | "preferences" | "application" | "runtime"
        ) {
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

pub fn atomic_write(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = destination
        .parent()
        .ok_or_else(|| io::Error::other("missing directory"))?;
    let (temporary, mut file) = loop {
        let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(".waytoys-{}-{seq}.tmp", std::process::id()));
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

fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
pub struct JsonStore<T> {
    path: PathBuf,
    expected: Option<Vec<u8>>,
    marker: std::marker::PhantomData<T>,
}
impl<T: Default + DeserializeOwned + Serialize> JsonStore<T> {
    pub fn open(path: PathBuf) -> Result<(Self, T), String> {
        let expected = read(&path)?;
        let value = match &expected {
            Some(b) => {
                serde_json::from_slice(b).map_err(|e| format!("설정 읽기 실패 · 원본 보존: {e}"))?
            }
            None => T::default(),
        };
        Ok((
            Self {
                path,
                expected,
                marker: std::marker::PhantomData,
            },
            value,
        ))
    }
    pub fn save(&mut self, value: &T) -> Result<(), String> {
        let directory = self.path.parent().ok_or("설정 디렉터리가 없습니다")?;
        let _lock = FileLock::acquire(directory, "application")?;
        if fs::symlink_metadata(&self.path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("설정 심볼릭 링크를 교체하지 않았습니다".into());
        }
        let current = read(&self.path)?;
        if let Some(bytes) = &current {
            let _: T = serde_json::from_slice(bytes)
                .map_err(|e| format!("손상된 설정을 보존했습니다: {e}"))?;
        }
        if current != self.expected {
            return Err("외부에서 설정이 변경되었습니다. 다시 읽어 주세요".into());
        }
        let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        atomic_write(&self.path, &bytes).map_err(|e| format!("저장 실패: {e}"))?;
        self.expected = Some(bytes);
        Ok(())
    }
}
