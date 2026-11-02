use iced::{
    Element, Font, Length, Subscription, Task, Theme, keyboard,
    widget::{
        button, checkbox, column, container, image, row, scrollable, space, text, text_input,
    },
    window,
};
use std::{
    collections::{HashMap, HashSet},
    process::Command,
    sync::OnceLock,
    time::Duration,
};
use waytoys_core::client;
use waytoys_paste::{
    PICKER, Result,
    service::{self, Snapshot},
    store::PAGE_SIZE,
};
const FONT: Font = Font::with_name("Noto Sans CJK KR");
static CLOSE: OnceLock<async_channel::Receiver<()>> = OnceLock::new();
struct Control {
    tx: async_channel::Sender<()>,
}
#[zbus::interface(name = "org.waytoys.PastePicker1")]
impl Control {
    fn close(&self) {
        let _ = self.tx.try_send(());
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Text,
    Image,
    Pinned,
}
impl Filter {
    fn id(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Text => "text",
            Self::Image => "image",
            Self::Pinned => "pinned",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::All => "전체",
            Self::Text => "텍스트",
            Self::Image => "이미지",
            Self::Pinned => "고정",
        }
    }
}
#[derive(Debug, Clone)]
enum Action {
    Pin(i64, bool),
    Delete(i64),
    Clear,
    Recording(bool),
    Reload,
}
#[derive(Debug, Clone)]
struct Loaded {
    snapshot: Snapshot,
    thumbs: Vec<(i64, u32, u32, Vec<u8>)>,
}
#[derive(Debug, Clone)]
enum Message {
    Query(String),
    Filter(Filter),
    Page(usize),
    Loaded(u64, std::result::Result<Loaded, String>),
    Action(Action),
    Done(std::result::Result<(), String>),
    Copy(i64),
    Copied(std::result::Result<(), String>),
    ClearConfirm,
    ClearCancel,
    Up,
    Down,
    Enter,
    Tick,
    Close,
    Focused,
    Unfocused,
}
struct App {
    snapshot: Snapshot,
    query: String,
    filter: Filter,
    page: usize,
    selected: usize,
    thumbs: HashMap<i64, image::Handle>,
    loading: bool,
    working: bool,
    version: u64,
    error: Option<String>,
    confirm: bool,
    focused: bool,
}
impl App {
    fn new() -> (Self, Task<Message>) {
        let mut app = Self {
            snapshot: Snapshot::default(),
            query: String::new(),
            filter: Filter::All,
            page: 0,
            selected: 0,
            thumbs: HashMap::new(),
            loading: false,
            working: false,
            version: 0,
            error: None,
            confirm: false,
            focused: false,
        };
        let load = app.load();
        (
            app,
            Task::batch([load, iced::widget::operation::focus("paste-search")]),
        )
    }
    fn load(&mut self) -> Task<Message> {
        if self.loading {
            return Task::none();
        }
        self.loading = true;
        let (query, filter, page, version) = (
            self.query.clone(),
            self.filter.id(),
            self.page as u32,
            self.version,
        );
        let known: HashSet<_> = self.thumbs.keys().copied().collect();
        Task::perform(
            async move {
                (|| -> std::result::Result<Loaded, String> {
                    let connection = service::connection()?;
                    let p = service::proxy(&connection)?;
                    let value: String = p
                        .call("List", &(query, filter, page))
                        .map_err(|e| e.to_string())?;
                    let snapshot: Snapshot =
                        serde_json::from_str(&value).map_err(|e| e.to_string())?;
                    let mut thumbs = vec![];
                    for entry in &snapshot.listing.entries {
                        if entry.width > 0 && !known.contains(&entry.id) {
                            let (w, h, bytes): (u32, u32, Vec<u8>) = p
                                .call("Thumbnail", &(entry.id,))
                                .map_err(|e| e.to_string())?;
                            if w <= 128 && h <= 80 && bytes.len() == (w * h * 4) as usize {
                                thumbs.push((entry.id, w, h, bytes));
                            }
                        }
                    }
                    Ok(Loaded { snapshot, thumbs })
                })()
            },
            move |r| Message::Loaded(version, r),
        )
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Close => return iced::exit(),
            Message::Focused => self.focused = true,
            Message::Unfocused => {
                if self.focused && !self.working {
                    return iced::exit();
                }
            }
            Message::Query(value) => {
                self.query = value;
                self.page = 0;
                self.selected = 0;
                self.version += 1;
                return self.load();
            }
            Message::Filter(filter) => {
                self.filter = filter;
                self.page = 0;
                self.selected = 0;
                self.version += 1;
                return self.load();
            }
            Message::Page(page) => {
                self.page = page;
                self.selected = 0;
                self.version += 1;
                return self.load();
            }
            Message::Tick => {
                if !self.working {
                    return self.load();
                }
            }
            Message::Loaded(version, result) => {
                self.loading = false;
                if version != self.version {
                    return self.load();
                }
                match result {
                    Ok(loaded) => {
                        if loaded.snapshot.locked {
                            return iced::exit();
                        }
                        let ids: HashSet<_> = loaded
                            .snapshot
                            .listing
                            .entries
                            .iter()
                            .map(|e| e.id)
                            .collect();
                        self.thumbs.retain(|id, _| ids.contains(id));
                        for (id, w, h, bytes) in loaded.thumbs {
                            self.thumbs
                                .insert(id, image::Handle::from_rgba(w, h, bytes));
                        }
                        self.snapshot = loaded.snapshot;
                        self.selected = self
                            .selected
                            .min(self.snapshot.listing.entries.len().saturating_sub(1));
                        if self.page > 0 && self.snapshot.listing.entries.is_empty() {
                            self.page -= 1;
                            self.version += 1;
                            return self.load();
                        }
                    }
                    Err(e) => self.error = Some(format!("클립보드 연결 실패: {e}")),
                }
            }
            Message::Action(action) => {
                if self.working {
                    return Task::none();
                }
                self.working = true;
                self.confirm = false;
                self.error = None;
                return Task::perform(
                    async move {
                        (|| -> std::result::Result<(), String> {
                            if let Action::Recording(enabled) = action {
                                client::call("SetPasteEnabled", &enabled.to_string())?;
                                return Ok(());
                            }
                            let connection = service::connection()?;
                            let p = service::proxy(&connection)?;
                            let result: std::result::Result<(), _> = match action {
                                Action::Pin(id, v) => p.call("Pin", &(id, v)),
                                Action::Delete(id) => p.call("Delete", &(id,)),
                                Action::Clear => p.call("ClearRecent", &()),
                                Action::Reload => p.call("Reload", &()),
                                Action::Recording(_) => unreachable!(),
                            };
                            result.map_err(|e| e.to_string())
                        })()
                    },
                    Message::Done,
                );
            }
            Message::Done(result) => {
                self.working = false;
                if let Err(e) = result {
                    self.error = Some(e);
                }
                self.version += 1;
                return self.load();
            }
            Message::Copy(id) => {
                if self.working {
                    return Task::none();
                }
                self.working = true;
                self.error = None;
                return Task::perform(
                    async move {
                        let connection = service::connection()?;
                        let p = service::proxy(&connection)?;
                        p.call("Copy", &(id,)).map_err(|e| e.to_string())
                    },
                    Message::Copied,
                );
            }
            Message::Copied(result) => {
                self.working = false;
                match result {
                    Ok(()) => return iced::exit(),
                    Err(e) => self.error = Some(e),
                }
            }
            Message::ClearConfirm => self.confirm = true,
            Message::ClearCancel => self.confirm = false,
            Message::Up => self.selected = self.selected.saturating_sub(1),
            Message::Down => {
                self.selected =
                    (self.selected + 1).min(self.snapshot.listing.entries.len().saturating_sub(1))
            }
            Message::Enter => {
                if let Some(entry) = self.snapshot.listing.entries.get(self.selected) {
                    return self.update(Message::Copy(entry.id));
                }
            }
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Message> {
        let status = if self.snapshot.enabled {
            "기록 중"
        } else {
            "기록 꺼짐"
        };
        let heading = row![
            text("클립보드").size(23),
            space().width(Length::Fill),
            checkbox(self.snapshot.enabled)
                .label("기록 수집")
                .on_toggle_maybe(
                    (!self.working).then_some(|v| Message::Action(Action::Recording(v)))
                )
                .text_size(13)
        ]
        .align_y(iced::Alignment::Center);
        let mut filters = row![].spacing(6);
        for filter in [Filter::All, Filter::Text, Filter::Image, Filter::Pinned] {
            filters = filters.push(
                button(filter.label())
                    .on_press(Message::Filter(filter))
                    .style(if filter == self.filter {
                        button::primary
                    } else {
                        button::text
                    }),
            );
        }
        let mut list = column![].spacing(7);
        if self.snapshot.listing.entries.is_empty() {
            list = list.push(
                container(
                    text(if self.loading {
                        "기록을 불러오는 중…"
                    } else if !self.query.is_empty() {
                        "검색 결과가 없습니다"
                    } else if !self.snapshot.enabled {
                        "기록 수집을 켠 뒤 텍스트나 이미지를 복사해 보세요"
                    } else {
                        "아직 기록이 없습니다. 텍스트나 이미지를 복사해 보세요"
                    })
                    .size(14),
                )
                .padding(24),
            );
        }
        for (index, entry) in self.snapshot.listing.entries.iter().enumerate() {
            let preview: String = entry.preview.lines().take(3).collect::<Vec<_>>().join("\n");
            let caption = if entry.width > 0 {
                format!(
                    "{} × {} · {:.1} KiB",
                    entry.width,
                    entry.height,
                    entry.size as f64 / 1024.
                )
            } else {
                format!("텍스트 · {} bytes", entry.size)
            };
            let label = column![text(preview).size(14), text(caption).size(11)]
                .spacing(5)
                .width(Length::Fill);
            let body: Element<'_, Message> = if let Some(handle) = self.thumbs.get(&entry.id) {
                row![
                    image(handle.clone())
                        .width(100)
                        .height(64)
                        .content_fit(iced::ContentFit::Contain),
                    label
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center)
                .into()
            } else {
                label.into()
            };
            list = list.push(
                row![
                    button(body)
                        .width(Length::Fill)
                        .padding(10)
                        .style(if index == self.selected {
                            button::primary
                        } else {
                            button::secondary
                        })
                        .on_press_maybe((!self.working).then_some(Message::Copy(entry.id))),
                    column![
                        button(
                            text(if entry.pinned {
                                "고정 해제"
                            } else {
                                "고정"
                            })
                            .size(13)
                        )
                        .width(76)
                        .on_press_maybe(
                            (!self.working)
                                .then_some(Message::Action(Action::Pin(entry.id, !entry.pinned)))
                        ),
                        button(text("삭제").size(13)).width(76).on_press_maybe(
                            (!self.working && !entry.pinned)
                                .then_some(Message::Action(Action::Delete(entry.id)))
                        )
                    ]
                    .spacing(4)
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
        }
        let pages = self.snapshot.listing.total.div_ceil(PAGE_SIZE).max(1);
        let footer = row![
            text(format!(
                "{status} · 최근 {} · 고정 {}",
                self.snapshot.listing.recent, self.snapshot.listing.pinned
            ))
            .size(12),
            space().width(Length::Fill),
            button("이전").on_press_maybe(
                (self.page > 0).then_some(Message::Page(self.page.saturating_sub(1)))
            ),
            text(format!("{} / {pages}", self.page + 1)).size(12),
            button("다음")
                .on_press_maybe((self.page + 1 < pages).then_some(Message::Page(self.page + 1)))
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);
        let mut content = column![
            heading,
            text_input("기록 검색", &self.query)
                .id("paste-search")
                .on_input(Message::Query)
                .on_submit(Message::Enter)
                .padding(9),
            filters,
            scrollable(list).height(Length::Fill),
            footer,
            text("선택하면 복사하고 닫습니다 · Ctrl+V 붙여넣기 · Esc 닫기").size(12)
        ]
        .spacing(10);
        if let Some(error) = self.error.as_ref().or(self.snapshot.error.as_ref()) {
            content = content
                .push(
                    text(error)
                        .size(12)
                        .color(iced::Color::from_rgb8(170, 55, 35)),
                )
                .push(button("다시 읽기").on_press(Message::Action(Action::Reload)));
        }
        if self.confirm {
            content = content.push(
                row![
                    text("고정 항목을 제외한 기록을 모두 지울까요?").size(13),
                    button("지우기").on_press(Message::Action(Action::Clear)),
                    button("취소").on_press(Message::ClearCancel)
                ]
                .spacing(8),
            );
        } else {
            content = content.push(row![
                button("고정 제외 기록 지우기").on_press_maybe(
                    (!self.working && self.snapshot.listing.recent > 0)
                        .then_some(Message::ClearConfirm)
                ),
                space().width(Length::Fill),
                button("닫기").on_press(Message::Close)
            ]);
        }
        container(content).padding(14).into()
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run(|| {
                iced::futures::stream::unfold((), |()| async {
                    futures_timer::Delay::new(Duration::from_secs(1)).await;
                    Some((Message::Tick, ()))
                })
            }),
            Subscription::run(|| {
                iced::futures::stream::unfold(CLOSE.get().unwrap().clone(), |rx| async move {
                    rx.recv().await.ok().map(|()| (Message::Close, rx))
                })
            }),
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(key),
                    ..
                }) => match key {
                    keyboard::key::Named::Escape => Some(Message::Close),
                    keyboard::key::Named::ArrowUp => Some(Message::Up),
                    keyboard::key::Named::ArrowDown => Some(Message::Down),
                    _ => None,
                },
                iced::Event::Window(window::Event::Focused) => Some(Message::Focused),
                iced::Event::Window(window::Event::Unfocused) => Some(Message::Unfocused),
                _ => None,
            }),
            window::close_requests().map(|_| Message::Close),
        ])
    }
}
pub fn run() -> Result<()> {
    let bus = service::connection()?;
    let dbus = zbus::blocking::Proxy::new(
        &bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let exists: bool = dbus.call("NameHasOwner", &(PICKER,))?;
    if exists {
        let p = zbus::blocking::Proxy::new(&bus, PICKER, "/Picker", PICKER)?;
        let _: () = p.call("Close", &())?;
        return Ok(());
    }
    let capturing: bool = dbus.call("NameHasOwner", &("org.waytoys.Capture1",))?;
    if capturing {
        return Err("영역 캡처를 완료하거나 취소한 뒤 클립보드 기록을 여세요".into());
    }
    let (tx, rx) = async_channel::bounded(4);
    let _ = CLOSE.set(rx);
    let _control = zbus::blocking::connection::Builder::session()?
        .name(PICKER)?
        .serve_at("/Picker", Control { tx })?
        .build()?;
    let output = Command::new("fc-match")
        .args(["-f", "%{file}", "Noto Sans CJK KR"])
        .output()?;
    if !output.status.success() {
        return Err("Noto Sans CJK KR 글꼴을 찾지 못했습니다".into());
    }
    let font = std::fs::read(String::from_utf8(output.stdout)?.trim())?;
    iced::application(App::new, App::update, App::view)
        .title("Waytoys Clipboard")
        .theme(Theme::Light)
        .subscription(App::subscription)
        .font(font)
        .default_font(FONT)
        .window(window::Settings {
            size: iced::Size::new(560., 400.),
            min_size: Some(iced::Size::new(440., 320.)),
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
        .map_err(|e| e.to_string())?;
    Ok(())
}
