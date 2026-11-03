mod autostart;
mod overlay;
mod wayland_overlay;
mod x11;
mod zones;
use ksni::blocking::TrayMethods;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::{Child, Command as Process, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use waytoys_core::{
    Paths, RuntimeStatus, WaytoysSettings, client,
    storage::{FileLock, JsonStore},
};
use waytoys_zones_config::PreferencesWatch;
use zbus::blocking::{Connection, Proxy};

type Reply = async_channel::Sender<Result<String, String>>;
enum Command {
    Event(String),
    X11Event(String, String, Reply),
    Change(String, String, Reply),
    Open(String),
    Capture,
    Paste,
    Quit,
}
struct Api {
    tx: mpsc::Sender<Command>,
    status: Arc<Mutex<RuntimeStatus>>,
}
#[zbus::interface(name = "org.waytoys.Runtime1")]
impl Api {
    async fn x11_event(
        &self,
        value: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let (tx, rx) = async_channel::bounded(1);
        let sender = header
            .sender()
            .map(|s| s.as_str())
            .unwrap_or_default()
            .to_string();
        self.tx
            .send(Command::X11Event(value.into(), sender, tx))
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        rx.recv()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?
            .map_err(zbus::fdo::Error::Failed)
    }
    fn status(&self) -> String {
        serde_json::to_string(&*self.status.lock().unwrap()).unwrap()
    }
    async fn set_zones_enabled(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("zones", value).await
    }
    async fn set_run_at_login(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("login", value).await
    }
    async fn reload(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("reload", value).await
    }
    async fn show_settings(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("open", value).await
    }
    async fn capture(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("capture", value).await
    }
    async fn show_paste(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("show_paste", value).await
    }
    async fn set_paste_enabled(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("paste", value).await
    }
    async fn suspend_for_capture(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("capture_pause", value).await
    }
    async fn quit(&self, value: &str) -> zbus::fdo::Result<String> {
        self.change("quit", value).await
    }
}
impl Api {
    async fn change(&self, kind: &str, value: &str) -> zbus::fdo::Result<String> {
        let (tx, rx) = async_channel::bounded(1);
        self.tx
            .send(Command::Change(kind.into(), value.into(), tx))
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        rx.recv()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?
            .map_err(zbus::fdo::Error::Failed)
    }
}
struct Tray {
    tx: mpsc::Sender<Command>,
}
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "waytoys".into()
    }
    fn title(&self) -> String {
        "Waytoys".into()
    }
    fn icon_name(&self) -> String {
        "preferences-system-windows".into()
    }
    fn activate(&mut self, _: i32, _: i32) {
        let _ = self.tx.send(Command::Open("general".into()));
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::menu::StandardItem {
                label: "클립보드 기록".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(Command::Paste);
                }),
                ..Default::default()
            }
            .into(),
            ksni::menu::StandardItem {
                label: "설정 열기".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(Command::Open("general".into()));
                }),
                ..Default::default()
            }
            .into(),
            ksni::menu::StandardItem {
                label: "영역 캡처".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(Command::Capture);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            ksni::menu::StandardItem {
                label: "Waytoys 종료".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(Command::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn bridge<'a>(connection: &'a Connection) -> Result<Proxy<'a>, String> {
    Proxy::new(
        connection,
        "org.kde.KWin",
        "/Waytoys/Zones",
        "org.waytoys.ZonesBridge1",
    )
    .map_err(|e| e.to_string())
}
fn configure(connection: &Connection, value: serde_json::Value) -> Result<(), String> {
    let ok: bool = bridge(connection)?
        .call("Configure", &(value.to_string(),))
        .map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else {
        Err("KWin 연결이 설정 요청을 거절했습니다".into())
    }
}
fn disable(connection: &Connection) {
    let _ = configure(
        connection,
        serde_json::json!({"protocol":1,"enabled":false}),
    );
}

struct Runtime {
    paths: Paths,
    settings: WaytoysSettings,
    store: Option<JsonStore<WaytoysSettings>>,
    status: Arc<Mutex<RuntimeStatus>>,
    preferences: PreferencesWatch,
    layouts: BTreeMap<String, zones::Layout>,
    drag: zones::Drag,
    overlay: Option<overlay::Overlay>,
    x11_bridge: Option<x11::Bridge>,
    settings_window: Option<Child>,
    capture: Option<Child>,
    paste_daemon: Option<Child>,
    paste_picker: Option<Child>,
    paste_recording: Option<bool>,
    paste_retry: Instant,
    active: bool,
    bridge_epoch: String,
}
impl Runtime {
    fn status(&self, error: Option<String>, ready: bool) {
        let mut s = self.status.lock().unwrap();
        s.settings = self.settings.clone();
        s.blocked = self.store.is_none();
        s.error = error;
        s.ready = ready;
    }
    fn open_settings(&mut self, page: &str) -> Result<(), String> {
        if client::settings_control("Activate", page).unwrap_or(false) {
            return Ok(());
        }
        if let Some(child) = self.settings_window.as_mut()
            && child.try_wait().map_err(|e| e.to_string())?.is_none()
        {
            return Ok(());
        }
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("waytoys-settings");
        self.settings_window = Some(
            Process::new(exe)
                .args(self.paths.args())
                .arg("--page")
                .arg(match page {
                    "zones" => "zones",
                    "capture" => "capture",
                    "paste" => "paste",
                    _ => "general",
                })
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn capture(&mut self) -> Result<(), String> {
        if let Some(child) = self.capture.as_mut()
            && child.try_wait().map_err(|e| e.to_string())?.is_none()
        {
            return Ok(());
        }
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("waytoys-capture");
        self.capture = Some(
            Process::new(exe)
                .stdin(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn paste(&mut self) -> Result<(), String> {
        if let Some(child) = self.paste_picker.as_mut()
            && child.try_wait().map_err(|e| e.to_string())?.is_none()
        {
            return Ok(());
        }
        self.paste_picker = Some(
            Process::new(
                std::env::current_exe()
                    .map_err(|e| e.to_string())?
                    .with_file_name("waytoys-paste"),
            )
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn refresh_paste(&mut self, connection: &Connection) -> Result<(), String> {
        if let Some(child) = self.paste_picker.as_mut()
            && child.try_wait().map_err(|e| e.to_string())?.is_some()
        {
            self.paste_picker = None;
        }
        if let Some(child) = self.paste_daemon.as_mut()
            && child.try_wait().map_err(|e| e.to_string())?.is_some()
        {
            self.paste_daemon = None;
            self.paste_recording = None;
        }
        let bus = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(|e| e.to_string())?;
        let exists: bool = bus
            .call("NameHasOwner", &("org.waytoys.Paste1",))
            .map_err(|e| e.to_string())?;
        if !exists {
            if self.paste_daemon.is_none() && Instant::now() >= self.paste_retry {
                self.paste_retry = Instant::now() + Duration::from_secs(3);
                self.paste_daemon = Some(
                    Process::new(
                        std::env::current_exe()
                            .map_err(|e| e.to_string())?
                            .with_file_name("waytoys-paste"),
                    )
                    .arg("--daemon")
                    .arg("--data-dir")
                    .arg(&self.paths.paste)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()
                    .map_err(|e| e.to_string())?,
                );
            }
            self.status.lock().unwrap().paste_ready = false;
            return Ok(());
        }
        let proxy = Proxy::new(
            connection,
            "org.waytoys.Paste1",
            "/Paste",
            "org.waytoys.Paste1",
        )
        .map_err(|e| e.to_string())?;
        let directory: String = proxy.call("Directory", &()).map_err(|e| e.to_string())?;
        if std::path::Path::new(&directory) != self.paths.paste {
            return Err("다른 데이터 경로의 클립보드 실행기가 있습니다".into());
        }
        let recording = self.store.is_some() && self.settings.enabled.paste;
        if self.paste_recording != Some(recording) {
            let _: () = proxy
                .call("SetRecording", &(recording,))
                .map_err(|e| e.to_string())?;
            self.paste_recording = Some(recording);
        }
        let (ready, error): (bool, String) =
            proxy.call("Health", &()).map_err(|e| e.to_string())?;
        let mut status = self.status.lock().unwrap();
        status.paste_ready = ready;
        status.paste_error = (!error.is_empty()).then_some(error);
        Ok(())
    }
    fn stop_zones(&mut self, connection: &Connection) {
        if self.x11_bridge.take().is_none() {
            disable(connection);
        }
        self.drag.clear();
        self.overlay.take();
        self.active = false;
    }
    fn refresh(&mut self, connection: &Connection) -> Result<(), String> {
        if let Err(error) = self.refresh_paste(connection) {
            let mut status = self.status.lock().unwrap();
            status.paste_ready = false;
            status.paste_error = Some(error);
        }
        if let Some(child) = self.capture.as_mut()
            && child.try_wait().map_err(|e| e.to_string())?.is_some()
        {
            self.capture = None;
        }
        let bus = Proxy::new(
            connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(|e| e.to_string())?;
        let capturing: bool = bus
            .call("NameHasOwner", &("org.waytoys.Capture1",))
            .map_err(|e| e.to_string())?;
        let picking: bool = bus
            .call("NameHasOwner", &("org.waytoys.PastePicker1",))
            .map_err(|e| e.to_string())?;
        if capturing || picking {
            if self.active {
                self.stop_zones(connection);
            }
            self.status(None, false);
            return Ok(());
        }
        if self.store.is_none() {
            return Err("일반 설정을 복구한 뒤 다시 읽어 주세요".into());
        }
        if !self.settings.enabled.zones {
            if self.active {
                self.stop_zones(connection);
            }
            self.status(None, false);
            return Ok(());
        }
        let previous = self.preferences.preferences.clone();
        self.preferences.refresh();
        if let Some(e) = &self.preferences.error {
            return Err(format!("Zones 설정 읽기 실패 · 원본 보존: {e}"));
        }
        if waytoys_desktop::Session::current()? == waytoys_desktop::Session::X11 {
            return self.refresh_x11(connection, previous);
        }
        let snapshot: Result<String, _> = bridge(connection)?.call("Snapshot", &());
        let snapshot = match snapshot {
            Ok(s) => s,
            Err(_) => {
                let plugins = Proxy::new(
                    connection,
                    "org.kde.KWin",
                    "/Plugins",
                    "org.kde.KWin.Plugins",
                )
                .map_err(|e| e.to_string())?;
                let loaded: bool = plugins
                    .call("LoadPlugin", &("waytoys_zones_bridge",))
                    .map_err(|e| e.to_string())?;
                if !loaded {
                    return Err(
                        "KWin Zones 플러그인을 불러오지 못했습니다. 설치와 KWin 버전을 확인하세요"
                            .into(),
                    );
                }
                bridge(connection)?
                    .call("Snapshot", &())
                    .map_err(|e| e.to_string())?
            }
        };
        let snapshot: zones::Snapshot =
            serde_json::from_str(&snapshot).map_err(|e| e.to_string())?;
        if snapshot.protocol != 1 {
            return Err("KWin 연결 프로토콜이 호환되지 않습니다".into());
        }
        if self.bridge_epoch != snapshot.epoch {
            self.bridge_epoch = snapshot.epoch;
            self.drag = zones::Drag::default();
            self.show();
        }
        let layouts = zones::load_layouts(&self.paths.zones, &snapshot.outputs)?;
        let changed = self.layouts != layouts || previous != self.preferences.preferences;
        self.layouts = layouts;
        if self.overlay.is_none() {
            self.overlay = Some(overlay::Overlay::start()?);
        }
        if let Some(error) = self.overlay.as_ref().and_then(overlay::Overlay::error) {
            return Err(format!("영역 표시 연결 오류: {error}"));
        }
        let p = &self.preferences.preferences;
        configure(
            connection,
            serde_json::json!({"protocol":1,"enabled":true,"outputs":self.layouts.keys().collect::<Vec<_>>(),"excluded":p.excluded_apps.lines().filter(|v|!v.is_empty()).collect::<Vec<_>>(),"transparent":p.make_dragged_window_transparent}),
        )?;
        self.active = true;
        if changed {
            self.drag.refresh(&self.layouts, p)?;
            self.show();
        }
        self.status(None, true);
        Ok(())
    }
    fn refresh_x11(
        &mut self,
        connection: &Connection,
        previous: waytoys_zones_config::ZonesPreferences,
    ) -> Result<(), String> {
        let saver = Proxy::new(
            connection,
            "org.freedesktop.ScreenSaver",
            "/ScreenSaver",
            "org.freedesktop.ScreenSaver",
        )
        .map_err(|e| e.to_string())?;
        let locked: bool = saver.call("GetActive", &()).map_err(|e| e.to_string())?;
        if locked {
            self.stop_zones(connection);
            self.status(None, false);
            return Ok(());
        }
        if let Some(bridge) = &self.x11_bridge
            && !bridge.healthy()?
        {
            self.stop_zones(connection);
        }
        if self.x11_bridge.is_none() {
            self.x11_bridge = Some(x11::Bridge::start(connection)?);
            self.bridge_epoch = self.x11_bridge.as_ref().unwrap().epoch.clone();
            self.drag = zones::Drag::default();
        }
        // Drain the selected XI2 events during idle periods as well as moves,
        // so an enabled runtime never accumulates a session's input backlog.
        self.x11_bridge.as_mut().unwrap().input(false)?;
        let outputs = self.x11_bridge.as_ref().unwrap().outputs()?;
        let layouts = zones::load_layouts(&self.paths.zones, &outputs)?;
        let changed = self.layouts != layouts || previous != self.preferences.preferences;
        self.layouts = layouts;
        if self.overlay.is_none() {
            self.overlay = Some(overlay::Overlay::start()?);
        }
        if let Some(e) = self.overlay.as_ref().and_then(overlay::Overlay::error) {
            return Err(e);
        }
        self.active = true;
        if changed {
            self.drag
                .refresh(&self.layouts, &self.preferences.preferences)?;
            self.show();
        }
        self.status(None, true);
        Ok(())
    }
    fn x11_event(&mut self, value: &str, sender: &str) -> Result<String, String> {
        let Some(bridge) = &mut self.x11_bridge else {
            return Ok("{}".into());
        };
        if !self.active || sender != bridge.owner {
            return Ok("{}".into());
        }
        let input: x11::Input = serde_json::from_str(value).map_err(|e| e.to_string())?;
        let mut event = input.event;
        if event.epoch != bridge.epoch {
            return Ok("{}".into());
        }
        let excluded = self
            .preferences
            .preferences
            .excluded_apps
            .lines()
            .any(|a| !a.is_empty() && (a == input.app || a == input.desktop_file));
        let (shift, ctrl, cancelled) = bridge.input(event.kind == "drop")?;
        (event.shift, event.ctrl) = (shift, ctrl);
        if cancelled && event.kind == "drop" {
            self.drag.cancel(&event);
            self.show();
            return Ok(serde_json::json!({"epoch":event.epoch,"id":event.id,"sequence":event.sequence,"cancelled":true}).to_string());
        }
        if excluded {
            event.shift = false;
        }
        event.output = self
            .layouts
            .values()
            .find(|l| {
                let [x, y, w, h] = l.output.geometry;
                event.position[0] >= f64::from(x)
                    && event.position[0] < f64::from(x + w)
                    && event.position[1] >= f64::from(y)
                    && event.position[1] < f64::from(y + h)
            })
            .map(|l| l.output.name.clone())
            .unwrap_or_default();
        if event.kind == "cancel" {
            self.drag.cancel(&event);
            self.show();
            return Ok("{}".into());
        }
        let changed = self
            .drag
            .update(event, &self.layouts, &self.preferences.preferences)?;
        if let Some(target) = self.drag.target(&self.layouts)? {
            let reply = serde_json::to_string(&target).map_err(|e| e.to_string())?;
            self.drag.clear();
            self.show();
            return Ok(reply);
        }
        if changed {
            self.show();
        }
        Ok("{}".into())
    }
    fn show(&mut self) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.show(
                self.drag
                    .visual(&self.layouts, &self.preferences.preferences),
            );
        }
    }
    fn event(&mut self, value: &str, connection: &Connection) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        let e: zones::Event = serde_json::from_str(value).map_err(|e| e.to_string())?;
        if e.epoch != self.bridge_epoch {
            return Ok(());
        }
        if e.kind == "outputs" {
            self.drag.clear();
            self.show();
            return self.refresh(connection);
        }
        if e.kind == "cancel" {
            if self.drag.cancel(&e) {
                self.show();
            }
            return Ok(());
        }
        let changed = self
            .drag
            .update(e, &self.layouts, &self.preferences.preferences)?;
        if let Some(target) = self.drag.target(&self.layouts)? {
            // Drop must be answered even when the last pointer stayed in the same zone.
            let accepted: bool = bridge(connection)?
                .call(
                    "Target",
                    &(serde_json::to_string(&target).map_err(|e| e.to_string())?,),
                )
                .map_err(|e| e.to_string())?;
            eprintln!(
                "zone drop id={} sequence={} accepted={accepted} rect={:?}",
                target.id, target.sequence, target.rect
            );
            self.drag.clear();
            self.show();
        } else if changed {
            self.show();
        }
        Ok(())
    }
    fn change(&mut self, kind: &str, value: &str, connection: &Connection) -> Result<(), String> {
        if kind == "reload" {
            let (store, settings) = JsonStore::open(self.paths.app.join("settings.json"))?;
            self.store = Some(store);
            self.settings = settings;
        } else {
            let enabled: bool = serde_json::from_str(value).map_err(|e| e.to_string())?;
            let mut next = self.settings.clone();
            match kind {
                "zones" => next.enabled.zones = enabled,
                "paste" => next.enabled.paste = enabled,
                "login" => next.run_at_login = enabled,
                _ => return Err("unknown setting".into()),
            };
            let store = self
                .store
                .as_mut()
                .ok_or("일반 설정 파일을 복구한 뒤 다시 읽어 주세요")?;
            if next.run_at_login != self.settings.run_at_login {
                autostart::configure(&self.paths, next.run_at_login)?;
            }
            if let Err(e) = store.save(&next) {
                if next.run_at_login != self.settings.run_at_login {
                    let _ = autostart::configure(&self.paths, self.settings.run_at_login);
                }
                return Err(e);
            }
            self.settings = next;
        }
        if let Err(e) = self.refresh(connection) {
            self.stop_zones(connection);
            self.status(Some(e), false);
        }
        Ok(())
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = None;
    let mut zones = None;
    let mut paste = None;
    let mut background = false;
    let mut action = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--app-config-dir") => {
                app = Some(PathBuf::from(
                    args.next().ok_or("--app-config-dir requires a path")?,
                ))
            }
            Some("--config-dir") => {
                zones = Some(PathBuf::from(
                    args.next().ok_or("--config-dir requires a path")?,
                ))
            }
            Some("--background") => background = true,
            Some("--paste-data-dir") => {
                paste = Some(PathBuf::from(
                    args.next().ok_or("--paste-data-dir requires a path")?,
                ))
            }
            Some("--paste") => action = Some(("ShowPaste", "".to_string())),
            Some("--status") => action = Some(("Status", "".to_string())),
            Some("--quit") => action = Some(("Quit", "".to_string())),
            Some("--capture") => action = Some(("Capture", "".to_string())),
            Some("--zones") => {
                let v = args.next().ok_or("--zones on|off")?;
                action = Some((
                    "SetZonesEnabled",
                    match v.to_str() {
                        Some("on") => "true",
                        Some("off") => "false",
                        _ => return Err("--zones on|off".into()),
                    }
                    .into(),
                ));
            }
            Some("--help" | "-h") => {
                println!(
                    "waytoys [--background|--status|--quit|--capture|--paste|--zones on|off] [--app-config-dir PATH] [--config-dir ZONES_PATH] [--paste-data-dir PATH]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {}", arg.to_string_lossy()).into()),
        }
    }
    let paths = Paths::resolve_with_paste(app, zones, paste)?;
    if let Some((method, value)) = action {
        let status = client::call(method, &value)?;
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }
    if !background {
        client::ensure_running(&paths)?;
        client::call("ShowSettings", "general")?;
        return Ok(());
    }
    let _lock = FileLock::acquire(&paths.app, "runtime")?;
    let (store, settings, error) =
        match JsonStore::<WaytoysSettings>::open(paths.app.join("settings.json")) {
            Ok((s, v)) => (Some(s), v, None),
            Err(e) => (None, WaytoysSettings::default(), Some(e)),
        };
    let status = Arc::new(Mutex::new(RuntimeStatus {
        paths: paths.clone(),
        settings: settings.clone(),
        ready: false,
        blocked: store.is_none(),
        error,
        paste_ready: false,
        paste_error: None,
    }));
    let (tx, rx) = mpsc::channel();
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(Duration::from_secs(2))
        .name(client::BUS)?
        .serve_at(
            client::PATH,
            Api {
                tx: tx.clone(),
                status: status.clone(),
            },
        )?
        .build()?;
    let listener = connection.clone();
    let events = tx.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let proxy = bridge(&listener)?;
            for signal in proxy.receive_signal("Event")? {
                let value: String = signal.body().deserialize()?;
                if events.send(Command::Event(value)).is_err() {
                    break;
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("bridge event stream: {e}");
        }
    });
    let tray = Tray { tx: tx.clone() }
        .assume_sni_available(true)
        .spawn()
        .map_err(|e| format!("KDE 트레이 연결 실패: {e}"))?;
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, stop.clone())?;
    }
    let mut runtime = Runtime {
        preferences: PreferencesWatch::new(&paths.zones),
        paths,
        settings,
        store,
        status,
        layouts: BTreeMap::new(),
        drag: zones::Drag::default(),
        overlay: None,
        x11_bridge: None,
        settings_window: None,
        capture: None,
        paste_daemon: None,
        paste_picker: None,
        paste_recording: None,
        paste_retry: Instant::now(),
        active: false,
        bridge_epoch: String::new(),
    };
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next {
            if let Err(e) = runtime.refresh(&connection) {
                runtime.stop_zones(&connection);
                runtime.status(Some(e), false);
            }
            next = Instant::now() + Duration::from_millis(500);
        }
        let command = match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(c) => c,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        match command {
            Command::X11Event(value, sender, reply) => {
                let result = runtime.x11_event(&value, &sender);
                if let Err(error) = &result {
                    runtime.stop_zones(&connection);
                    runtime.status(Some(error.clone()), false);
                }
                let _ = reply.send_blocking(result);
            }
            Command::Event(e) => {
                if let Err(error) = runtime.event(&e, &connection) {
                    runtime.stop_zones(&connection);
                    runtime.status(Some(error), false);
                }
            }
            Command::Open(page) => {
                if let Err(e) = runtime.open_settings(&page) {
                    eprintln!("settings: {e}");
                }
            }
            Command::Capture => {
                if let Err(e) = runtime.capture() {
                    eprintln!("capture: {e}");
                }
            }
            Command::Paste => {
                if let Err(e) = runtime.paste() {
                    eprintln!("paste: {e}");
                }
            }
            Command::Quit => {
                if client::settings_control("Close", "").unwrap_or(false) {
                    break;
                }
            }
            Command::Change(kind, value, reply) => {
                let result = match kind.as_str() {
                    "open" => runtime.open_settings(&value),
                    "capture" => runtime.capture(),
                    "show_paste" => runtime.paste(),
                    "capture_pause" => {
                        runtime.stop_zones(&connection);
                        runtime.status(None, false);
                        Ok(())
                    }
                    "quit" => client::settings_control("Close", "").and_then(|closed| {
                        if closed {
                            stop.store(true, Ordering::Relaxed);
                            Ok(())
                        } else {
                            Err("편집기 또는 저장 오류를 먼저 확인하세요".into())
                        }
                    }),
                    _ => runtime.change(&kind, &value, &connection),
                };
                let response = result.and_then(|()| {
                    serde_json::to_string(&*runtime.status.lock().unwrap())
                        .map_err(|e| e.to_string())
                });
                let _ = reply.send_blocking(response);
            }
        }
    }
    runtime.stop_zones(&connection);
    for (name, path, method) in [
        ("org.waytoys.PastePicker1", "/Picker", "Close"),
        ("org.waytoys.Paste1", "/Paste", "Quit"),
    ] {
        if let Ok(proxy) = Proxy::new(&connection, name, path, name) {
            let _: Result<(), _> = proxy.call(method, &());
        }
    }
    for child in [&mut runtime.paste_picker, &mut runtime.paste_daemon] {
        if let Some(mut child) = child.take() {
            let until = Instant::now() + Duration::from_secs(1);
            while child.try_wait().ok().flatten().is_none() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    if let Ok(capture) = Proxy::new(
        &connection,
        "org.waytoys.Capture1",
        "/Capture",
        "org.waytoys.Capture1",
    ) {
        let _: Result<(), _> = capture.call("Cancel", &());
    }
    if let Some(mut child) = runtime.capture.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    tray.shutdown();
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("waytoys: {e}");
        std::process::exit(1);
    }
}
