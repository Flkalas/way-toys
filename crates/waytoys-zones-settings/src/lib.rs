mod draft;
mod view;

use draft::{Draft, Field};
use iced::{Font, Subscription, Task, Theme, widget::text_editor, window};
use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use waytoys_core::{Paths, RuntimeStatus, client};
use waytoys_zones_config::{
    Autosave, InstanceGuard, LayoutNameLabelPlacement as Placement, OutputDescription,
    OverlappingZonesAlgorithm as Overlap, ZonesPreferences,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Zones,
    Capture,
    Paste,
}
#[derive(Debug, Clone)]
enum External {
    Activate(String),
    Close(async_channel::Sender<bool>),
}
static EXTERNAL: std::sync::OnceLock<async_channel::Receiver<External>> =
    std::sync::OnceLock::new();
struct Control {
    tx: async_channel::Sender<External>,
}
#[zbus::interface(name = "org.waytoys.Settings1")]
impl Control {
    fn activate(&self, page: &str) -> bool {
        self.tx.try_send(External::Activate(page.into())).is_ok()
    }
    async fn close(&self, _value: &str) -> bool {
        let (tx, rx) = async_channel::bounded(1);
        if self.tx.send(External::Close(tx)).await.is_err() {
            return false;
        }
        rx.recv().await.unwrap_or(false)
    }
}
fn external_events() -> impl iced::futures::Stream<Item = Message> {
    iced::futures::stream::unfold(
        EXTERNAL.get().expect("settings control").clone(),
        |rx| async move {
            rx.recv()
                .await
                .ok()
                .map(|event| (Message::External(event), rx))
        },
    )
}

const WAITING: &str = "연동 준비 중 · 현재 창 동작에는 미적용";
const FONT: Font = Font::with_name("Noto Sans CJK KR");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    General,
    Appearance,
    Name,
    Window,
    Excluded,
}
impl Category {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Appearance,
        Self::Name,
        Self::Window,
        Self::Excluded,
    ];
    fn title(self) -> &'static str {
        match self {
            Self::General => "일반·선택",
            Self::Appearance => "영역 모양",
            Self::Name => "레이아웃 이름 표시",
            Self::Window => "창 동작",
            Self::Excluded => "제외 앱",
        }
    }
}
#[derive(Debug, Clone, Copy)]
enum Toggle {
    AllMonitors,
    Numbers,
    Name,
    Transparent,
    Across,
    Flash,
}
#[derive(Debug, Clone)]
enum Message {
    Page(Page),
    Capture,
    CaptureStarted(Result<RuntimeStatus, String>),
    SetZones(bool),
    SetPaste(bool),
    OpenPaste,
    SetLogin(bool),
    RuntimeConnect,
    RuntimeReload,
    Runtime(u64, bool, Result<RuntimeStatus, String>),
    External(External),
    ExitReady,
    Toggle(Toggle, bool),
    Startup(&'static str),
    MoveBy(&'static str),
    Overlap(&'static str),
    Placement(&'static str),
    Edit(Field, String),
    Excluded(text_editor::Action),
    Tick,
    Retry,
    Reload,
    Close,
    DiscardClose,
    Outputs(Result<Vec<OutputDescription>, String>),
    RefreshOutputs,
    SelectOutput(OutputDescription),
    OpenEditor,
}

struct App {
    session: Autosave,
    draft: Draft,
    excluded: text_editor::Content,
    page: Page,
    paths: Paths,
    runtime: Option<RuntimeStatus>,
    runtime_error: Option<String>,
    runtime_offline: bool,
    runtime_busy: bool,
    runtime_epoch: u64,
    last_runtime_poll: Instant,
    outputs: Vec<OutputDescription>,
    selected: Option<OutputDescription>,
    outputs_loading: bool,
    editor_path: PathBuf,
    directory: PathBuf,
    child: Option<Child>,
    launch_error: Option<String>,
    _instance: InstanceGuard,
}

fn query_outputs(editor: PathBuf) -> Result<Vec<OutputDescription>, String> {
    let mut child = Command::new(editor)
        .args(["--list-outputs", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("모니터 조회 실행 실패: {e}"))?;
    let start = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("모니터 조회 시간이 초과되었습니다".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("모니터 응답 오류: {e}"))
}

fn ticks(interval: &u64) -> impl iced::futures::Stream<Item = Message> + use<> {
    iced::futures::stream::unfold(*interval, |interval| async move {
        futures_timer::Delay::new(Duration::from_millis(interval)).await;
        Some((Message::Tick, interval))
    })
}

impl App {
    fn new(
        paths: Paths,
        editor_path: PathBuf,
        instance: InstanceGuard,
        page: Page,
    ) -> (Self, Task<Message>) {
        let directory = paths.zones.clone();
        let session = Autosave::open(directory.clone());
        let draft = Draft::new(&session.preferences);
        let excluded = text_editor::Content::with_text(&session.preferences.excluded_apps);
        let app = Self {
            session,
            draft,
            excluded,
            page,
            paths: paths.clone(),
            runtime: None,
            runtime_error: None,
            runtime_offline: false,
            runtime_busy: true,
            runtime_epoch: 1,
            last_runtime_poll: Instant::now(),
            outputs: Vec::new(),
            selected: None,
            outputs_loading: true,
            editor_path: editor_path.clone(),
            directory,
            child: None,
            launch_error: None,
            _instance: instance,
        };
        (
            app,
            Task::batch([
                Task::perform(async move { query_outputs(editor_path) }, Message::Outputs),
                Task::perform(async move { client::ensure_running(&paths) }, |r| {
                    Message::Runtime(1, true, r)
                }),
            ]),
        )
    }
    fn change(&mut self, p: ZonesPreferences) {
        if let Err(e) = self.session.change(p, Instant::now()) {
            self.launch_error = Some(e);
        }
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        let mut p = self.session.preferences.clone();
        match message {
            Message::Capture => {
                if self.runtime_busy || !self.close_allowed() {
                    return Task::none();
                }
                self.runtime_epoch += 1;
                self.runtime_busy = true;
                return Task::perform(
                    async { client::call("Capture", "") },
                    Message::CaptureStarted,
                );
            }
            Message::CaptureStarted(result) => {
                self.runtime_busy = false;
                match result.and_then(|s| client::check_paths(s, &self.paths)) {
                    // Wayland cannot unminimize a window through winit. Close
                    // after flushing instead; the tray can open a fresh window.
                    Ok(_) => return iced::exit(),
                    Err(error) => self.runtime_error = Some(error),
                }
            }
            Message::Page(page) => {
                self.page = page;
                return iced::widget::operation::snap_to(
                    "options",
                    iced::widget::scrollable::RelativeOffset::START,
                );
            }
            Message::SetZones(v) => return self.runtime_action("SetZonesEnabled", v.to_string()),
            Message::SetPaste(v) => return self.runtime_action("SetPasteEnabled", v.to_string()),
            Message::OpenPaste => return self.runtime_action("ShowPaste", String::new()),
            Message::SetLogin(v) => return self.runtime_action("SetRunAtLogin", v.to_string()),
            Message::RuntimeReload => return self.runtime_action("Reload", String::new()),
            Message::RuntimeConnect => {
                self.runtime_epoch += 1;
                let epoch = self.runtime_epoch;
                let paths = self.paths.clone();
                self.runtime_busy = true;
                return Task::perform(async move { client::ensure_running(&paths) }, move |r| {
                    Message::Runtime(epoch, true, r)
                });
            }
            Message::Runtime(epoch, action, result) => {
                if epoch != self.runtime_epoch {
                    return Task::none();
                }
                self.runtime_busy = false;
                match result.and_then(|s| client::check_paths(s, &self.paths)) {
                    Ok(status) => {
                        self.runtime = Some(status);
                        self.runtime_offline = false;
                        if action {
                            self.runtime_error = None;
                        }
                    }
                    Err(error) => {
                        self.runtime_error = Some(error);
                        if !action {
                            self.runtime_offline = true;
                        }
                    }
                }
            }
            Message::External(External::Activate(page)) => {
                self.page = match page.as_str() {
                    "zones" => Page::Zones,
                    "capture" => Page::Capture,
                    "paste" => Page::Paste,
                    _ => Page::General,
                };
                return window::oldest().then(|id| id.map_or_else(Task::none, window::gain_focus));
            }
            Message::External(External::Close(reply)) => {
                let close = self.close_allowed();
                let _ = reply.try_send(close);
                if close {
                    return Task::perform(
                        async {
                            futures_timer::Delay::new(Duration::from_millis(80)).await;
                        },
                        |()| Message::ExitReady,
                    );
                }
            }
            Message::ExitReady => return iced::exit(),
            Message::Toggle(field, v) => {
                match field {
                    Toggle::AllMonitors => p.show_zones_on_all_monitors = v,
                    Toggle::Numbers => p.show_zone_number = v,
                    Toggle::Name => p.layout_name_label_enabled = v,
                    Toggle::Transparent => p.make_dragged_window_transparent = v,
                    Toggle::Across => p.move_window_across_monitors = v,
                    Toggle::Flash => p.flash_zones_on_quick_switch = v,
                }
                self.change(p);
            }
            Message::Startup(v) => {
                p.use_cursorpos_editor_startupscreen = v == "마우스 포인터가 있는 화면";
                self.change(p);
            }
            Message::MoveBy(v) => {
                p.move_windows_based_on_position = v == "영역 위치";
                self.change(p);
            }
            Message::Overlap(v) => {
                p.overlapping_zones_algorithm = match v {
                    "가장 큰 영역" => Overlap::Largest,
                    "포인터 위치" => Overlap::Positional,
                    "가장 가까운 중심" => Overlap::ClosestCenter,
                    _ => Overlap::Smallest,
                };
                self.change(p);
            }
            Message::Placement(v) => {
                p.layout_name_label_placement = match v {
                    "왼쪽 위" => Placement::TopLeft,
                    "오른쪽 위" => Placement::TopRight,
                    "가운데" => Placement::Center,
                    "왼쪽 아래" => Placement::BottomLeft,
                    "가운데 아래" => Placement::BottomCenter,
                    "오른쪽 아래" => Placement::BottomRight,
                    _ => Placement::TopCenter,
                };
                self.change(p);
            }
            Message::Edit(f, value) => {
                if let Some(next) = self.draft.edit(f, value, &p) {
                    self.change(next);
                }
            }
            Message::Excluded(action) => {
                self.excluded.perform(action);
                p.excluded_apps = self.excluded.text();
                self.change(p);
            }
            Message::Tick => {
                self.session.tick(Instant::now());
                if let Some(child) = self.child.as_mut() {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            self.launch_error = None;
                            if !status.success() {
                                let mut error = String::new();
                                if let Some(stderr) = child.stderr.as_mut() {
                                    let _ = stderr.read_to_string(&mut error);
                                }
                                self.launch_error = Some(format!("편집기 종료: {}", error.trim()));
                            }
                            self.child = None;
                        }
                        Err(e) => self.launch_error = Some(e.to_string()),
                        Ok(None) => {}
                    }
                }
            }
            Message::Retry => {
                let _ = self.session.flush();
            }
            Message::Reload => {
                self.session.reload();
                self.draft = Draft::new(&self.session.preferences);
                self.excluded =
                    text_editor::Content::with_text(&self.session.preferences.excluded_apps);
            }
            Message::Close => {
                if self.close_allowed() {
                    return iced::exit();
                }
            }
            Message::DiscardClose => {
                if self.child.is_none() {
                    return iced::exit();
                }
            }
            Message::Outputs(result) => {
                self.outputs_loading = false;
                match result {
                    Ok(outputs) => {
                        self.selected = self
                            .selected
                            .take()
                            .and_then(|old| outputs.iter().find(|o| o.name == old.name).cloned())
                            .or_else(|| outputs.first().cloned());
                        self.outputs = outputs;
                        self.launch_error = if self.outputs.is_empty() {
                            Some("사용 가능한 모니터가 없습니다".into())
                        } else {
                            None
                        };
                    }
                    Err(e) => {
                        self.outputs.clear();
                        self.selected = None;
                        self.launch_error = Some(e);
                    }
                }
            }
            Message::RefreshOutputs => {
                self.outputs_loading = true;
                let path = self.editor_path.clone();
                return Task::perform(async move { query_outputs(path) }, Message::Outputs);
            }
            Message::SelectOutput(o) => self.selected = Some(o),
            Message::OpenEditor => {
                if self.child.is_none()
                    && self.session.flush().is_ok()
                    && let Some(output) = &self.selected
                {
                    match Command::new(&self.editor_path)
                        .arg("--config-dir")
                        .arg(&self.directory)
                        .arg("--output")
                        .arg(&output.name)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::piped())
                        .spawn()
                    {
                        Ok(child) => {
                            self.child = Some(child);
                            self.launch_error = None;
                        }
                        Err(e) => self.launch_error = Some(format!("편집기 실행 실패: {e}")),
                    }
                }
            }
        }
        if self.last_runtime_poll.elapsed() >= Duration::from_secs(1) && !self.runtime_busy {
            self.last_runtime_poll = Instant::now();
            self.runtime_epoch += 1;
            let epoch = self.runtime_epoch;
            return Task::perform(async { client::call("Status", "") }, move |r| {
                Message::Runtime(epoch, false, r)
            });
        }
        Task::none()
    }
    fn close_allowed(&mut self) -> bool {
        if self.child.is_some() {
            self.launch_error =
                Some("편집기를 먼저 닫아 주세요. 편집 중인 레이아웃을 보존합니다".into());
            false
        } else {
            self.session.flush().is_ok()
        }
    }
    fn runtime_action(&mut self, method: &'static str, value: String) -> Task<Message> {
        if self.runtime_busy {
            return Task::none();
        }
        self.runtime_epoch += 1;
        let epoch = self.runtime_epoch;
        self.runtime_busy = true;
        Task::perform(async move { client::call(method, &value) }, move |r| {
            Message::Runtime(epoch, true, r)
        })
    }
    fn subscription(&self) -> Subscription<Message> {
        let timer = if self.session.pending() && self.session.error.is_none() {
            Subscription::run_with(40_u64, ticks)
        } else if self.child.is_some() {
            Subscription::run_with(500_u64, ticks)
        } else {
            Subscription::run_with(1000_u64, ticks)
        };
        Subscription::batch([
            timer,
            window::close_requests().map(|_| Message::Close),
            Subscription::run(external_events),
        ])
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn main_entry(zones_page: bool) {
    if let Err(error) = run(zones_page) {
        eprintln!("waytoys-zones-settings: {error}");
        std::process::exit(1);
    }
}
fn run(zones_page: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut directory = None;
    let mut app_directory = None;
    let mut paste_directory = None;
    let mut page = if zones_page {
        Page::Zones
    } else {
        Page::General
    };
    let mut editor = std::env::current_exe()?.with_file_name("waytoys-zones-editor");
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--paste-data-dir") => {
                paste_directory = Some(PathBuf::from(
                    args.next().ok_or("--paste-data-dir requires a path")?,
                ))
            }
            Some("--app-config-dir") => {
                app_directory = Some(PathBuf::from(
                    args.next().ok_or("--app-config-dir requires a path")?,
                ))
            }
            Some("--page") => {
                page = match args.next().and_then(|v| v.into_string().ok()).as_deref() {
                    Some("general") => Page::General,
                    Some("zones") => Page::Zones,
                    Some("capture") => Page::Capture,
                    Some("paste") => Page::Paste,
                    _ => return Err("--page general|zones|capture|paste".into()),
                }
            }
            Some("--config-dir") => {
                directory = Some(PathBuf::from(
                    args.next().ok_or("--config-dir requires a path")?,
                ))
            }
            Some("--editor") => {
                editor = PathBuf::from(args.next().ok_or("--editor requires a path")?)
            }
            Some("--help" | "-h") => {
                println!(
                    "waytoys-settings [--app-config-dir PATH] [--config-dir ZONES_PATH] [--paste-data-dir PATH] [--page general|zones|capture|paste] [--editor PATH]\n한국어 Waytoys 설정 · 유효한 변경 자동 저장"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {}", arg.to_string_lossy()).into()),
        }
    }
    let paths = Paths::resolve_with_paste(app_directory, directory, paste_directory)?;
    if let Ok(status) = client::call("Status", "")
        && status.paths == paths
        && client::settings_control(
            "Activate",
            match page {
                Page::Zones => "zones",
                Page::Capture => "capture",
                Page::Paste => "paste",
                Page::General => "general",
            },
        )
        .unwrap_or(false)
    {
        return Ok(());
    }
    let instance = InstanceGuard::acquire(&paths.app, "settings")?;
    let (tx, rx) = async_channel::unbounded();
    EXTERNAL
        .set(rx)
        .map_err(|_| "settings control initialized twice")?;
    let _control = zbus::blocking::connection::Builder::session()?
        .name("org.waytoys.Settings1")?
        .serve_at("/Settings", Control { tx })?
        .build()?;
    let output = Command::new("fc-match")
        .args(["-f", "%{family}\n%{file}", "Noto Sans CJK KR"])
        .output()?;
    let font_info = String::from_utf8(output.stdout)?;
    let mut lines = font_info.lines();
    if !output.status.success() || !lines.next().is_some_and(|s| s.contains("Noto Sans CJK KR")) {
        return Err("Noto Sans CJK KR 글꼴이 필요합니다 (Debian: fonts-noto-cjk)".into());
    }
    let font = std::fs::read(lines.next().ok_or("font file missing")?)?;
    let boot = std::cell::RefCell::new(Some((paths, editor, instance)));
    iced::application(
        move || {
            let (dir, editor, guard) = boot.borrow_mut().take().expect("single boot");
            App::new(dir, editor, guard, page)
        },
        App::update,
        App::view,
    )
    .title("Waytoys")
    .theme(Theme::Light)
    .subscription(App::subscription)
    .font(font)
    .default_font(FONT)
    .window(window::Settings {
        size: iced::Size::new(760.0, 380.0),
        min_size: Some(iced::Size::new(560.0, 300.0)),
        exit_on_close_request: false,
        ..Default::default()
    })
    .run()?;
    Ok(())
}
