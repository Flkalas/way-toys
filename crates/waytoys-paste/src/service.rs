use crate::{
    BUS, PATH, Result, monitor,
    store::{Listing, Store},
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use zbus::blocking::{Connection, Proxy, connection::Builder};

pub struct State {
    pub store: Option<Store>,
    pub error: Option<String>,
    pub monitor_ready: bool,
}
pub struct Shared {
    pub state: Mutex<State>,
    pub enabled: AtomicBool,
    pub locked: AtomicBool,
    pub epoch: AtomicU64,
    pub stop: Arc<AtomicBool>,
    directory: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub listing: Listing,
    pub enabled: bool,
    pub locked: bool,
    pub ready: bool,
    pub error: Option<String>,
    pub directory: String,
}
fn failed(e: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}
struct Api {
    shared: Arc<Shared>,
}
impl Api {
    fn unlocked(&self) -> zbus::fdo::Result<()> {
        if self.shared.locked.load(Ordering::SeqCst) {
            Err(failed("잠금 중에는 기록을 열 수 없습니다"))
        } else {
            Ok(())
        }
    }
    fn with_store<T>(&self, action: impl FnOnce(&mut Store) -> Result<T>) -> zbus::fdo::Result<T> {
        self.unlocked()?;
        let mut state = self.shared.state.lock().unwrap();
        let store = state.store.as_mut().ok_or_else(|| {
            failed("기록 파일 오류 · 원본 보존. 파일을 복구한 뒤 다시 읽어 주세요")
        })?;
        action(store).map_err(failed)
    }
}
#[zbus::interface(name = "org.waytoys.Paste1")]
impl Api {
    fn directory(&self) -> String {
        self.shared.directory.to_string_lossy().into_owned()
    }
    fn health(&self) -> (bool, String) {
        let state = self.shared.state.lock().unwrap();
        (
            state.monitor_ready && state.store.is_some(),
            state.error.clone().unwrap_or_default(),
        )
    }
    fn set_recording(&self, enabled: bool) {
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        self.shared.enabled.store(enabled, Ordering::SeqCst);
    }
    fn list(&self, query: &str, filter: &str, page: u32) -> zbus::fdo::Result<String> {
        let state = self.shared.state.lock().unwrap();
        let locked = self.shared.locked.load(Ordering::SeqCst);
        let listing = if locked {
            Listing::default()
        } else {
            state
                .store
                .as_ref()
                .map(|s| s.list(query, filter, page as usize))
                .transpose()
                .map_err(failed)?
                .unwrap_or_default()
        };
        serde_json::to_string(&Snapshot {
            listing,
            enabled: self.shared.enabled.load(Ordering::SeqCst),
            locked,
            ready: state.monitor_ready && state.store.is_some(),
            error: state.error.clone(),
            directory: self.directory(),
        })
        .map_err(failed)
    }
    fn thumbnail(&self, id: i64) -> zbus::fdo::Result<(u32, u32, Vec<u8>)> {
        self.with_store(|s| s.thumbnail(id))
    }
    fn pin(&self, id: i64, pinned: bool) -> zbus::fdo::Result<()> {
        self.with_store(|s| s.pin(id, pinned))
    }
    fn delete(&self, id: i64) -> zbus::fdo::Result<()> {
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        self.with_store(|s| s.delete(id))
    }
    fn clear_recent(&self) -> zbus::fdo::Result<()> {
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        self.with_store(Store::clear_recent)
    }
    fn copy(&self, id: i64) -> zbus::fdo::Result<()> {
        let (mime, bytes) = self.with_store(|s| s.data(id))?;
        self.unlocked()?;
        copy(&mime, &bytes).map_err(failed)?;
        self.with_store(|s| s.touch(id))
    }
    fn reload(&self) -> zbus::fdo::Result<()> {
        self.unlocked()?;
        let mut state = self.shared.state.lock().unwrap();
        match Store::open(&self.shared.directory) {
            Ok(store) => {
                state.store = Some(store);
                state.error = None;
                Ok(())
            }
            Err(e) => {
                state.error = Some(e.to_string());
                Err(failed(e))
            }
        }
    }
    fn quit(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}
pub fn run(directory: PathBuf) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, stop.clone())?;
    }
    let (store, error) = match Store::open(&directory) {
        Ok(s) => (Some(s), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            store,
            error,
            monitor_ready: false,
        }),
        enabled: AtomicBool::new(false),
        locked: AtomicBool::new(true),
        epoch: AtomicU64::new(0),
        stop: stop.clone(),
        directory,
    });
    let connection = Builder::session()?
        .method_timeout(Duration::from_secs(3))
        .name(BUS)?
        .serve_at(
            PATH,
            Api {
                shared: shared.clone(),
            },
        )?
        .build()?;
    let watcher = shared.clone();
    let watch_bus = connection.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let saver = Proxy::new(
                &watch_bus,
                "org.freedesktop.ScreenSaver",
                "/ScreenSaver",
                "org.freedesktop.ScreenSaver",
            )?;
            let signals = saver.receive_signal("ActiveChanged")?;
            let locked: bool = saver.call("GetActive", &())?;
            watcher.locked.store(locked, Ordering::SeqCst);
            for event in signals {
                let locked = event.body().deserialize::<bool>().unwrap_or(true);
                watcher.epoch.fetch_add(1, Ordering::SeqCst);
                watcher.locked.store(locked, Ordering::SeqCst);
            }
            Ok(())
        })();
        if let Err(e) = result {
            watcher.locked.store(true, Ordering::SeqCst);
            watcher.state.lock().unwrap().error = Some(format!("화면 잠금 상태 연결 실패: {e}"));
        }
    });
    let watcher = shared.clone();
    let monitor = std::thread::spawn(move || {
        while !watcher.stop.load(Ordering::SeqCst) {
            if let Err(e) = monitor::run(watcher.clone()) {
                let mut state = watcher.state.lock().unwrap();
                state.monitor_ready = false;
                state.error = Some(format!("클립보드 연결 오류: {e}"));
            }
            for _ in 0..10 {
                if watcher.stop.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = monitor.join();
    drop(connection);
    Ok(())
}

static COPY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub fn copy(mime: &str, bytes: &[u8]) -> Result<()> {
    let unit = format!(
        "waytoys-clipboard-paste-{}-{}",
        std::process::id(),
        COPY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let mut child = Command::new("systemd-run")
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--unit",
            &unit,
            "wl-copy",
            "--type",
            mime,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = child.stdin.take().unwrap();
    let bytes = bytes.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(input.write_all(&bytes));
    });
    let result = (|| -> Result<()> {
        rx.recv_timeout(Duration::from_secs(3))??;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                if status.success() {
                    return Ok(());
                }
                return Err("클립보드에 복사하지 못했습니다".into());
            }
            if start.elapsed() > Duration::from_secs(3) {
                return Err("클립보드 복사 시간이 초과됐습니다".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &format!("{unit}.scope")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    result
}
pub fn connection() -> std::result::Result<Connection, String> {
    Builder::session()
        .and_then(|b| b.method_timeout(Duration::from_secs(4)).build())
        .map_err(|e| e.to_string())
}
pub fn proxy(connection: &Connection) -> std::result::Result<Proxy<'_>, String> {
    Proxy::new(connection, BUS, PATH, BUS).map_err(|e| e.to_string())
}
