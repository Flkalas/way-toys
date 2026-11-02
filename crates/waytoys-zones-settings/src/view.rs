use crate::*;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, space, text, text_editor,
    text_input,
};
use iced::{Element, Length};

impl App {
    fn input(&self, f: Field) -> Element<'_, Message> {
        let mut field = column![
            text(f.label()).size(13),
            text_input(
                if f.is_color() { "#RRGGBB" } else { "숫자" },
                &self.draft.values[&f]
            )
            .on_input(move |v| Message::Edit(f, v))
            .padding(6)
            .size(15)
        ]
        .spacing(3);
        if let Some(error) = self.draft.errors.get(&f) {
            field = field.push(
                text(error)
                    .size(11)
                    .color(iced::Color::from_rgb8(170, 55, 35)),
            );
        }
        field.into()
    }
    fn toggle(&self, label: &'static str, value: bool, field: Toggle) -> Element<'_, Message> {
        checkbox(value)
            .label(label)
            .on_toggle(move |v| Message::Toggle(field, v))
            .text_size(14)
            .into()
    }
    fn options(&self, category: Category) -> Element<'_, Message> {
        let p = &self.session.preferences;
        let mut c = column![text(category.title()).size(20)].spacing(12);
        match category {
            Category::General => {
                c=c.push(text("편집기 실행 화면 기준").size(14))
                    .push(pick_list(["마우스 포인터가 있는 화면","현재 활성 창의 화면"],Some(if p.use_cursorpos_editor_startupscreen{"마우스 포인터가 있는 화면"}else{"현재 활성 창의 화면"}),Message::Startup).width(Length::Fill))
                    .push(text(WAITING).size(12).color(iced::Color::from_rgb8(135,90,35)))
                    .push(self.toggle("모든 모니터에 영역 표시",p.show_zones_on_all_monitors,Toggle::AllMonitors))
                    .push(text("겹친 영역 선택 방식").size(14))
                    .push(pick_list(["가장 작은 영역","가장 큰 영역","포인터 위치","가장 가까운 중심"],Some(match p.overlapping_zones_algorithm {Overlap::Smallest=>"가장 작은 영역",Overlap::Largest=>"가장 큰 영역",Overlap::Positional=>"포인터 위치",Overlap::ClosestCenter=>"가장 가까운 중심"}),Message::Overlap).width(Length::Fill))
                    .push(text("창 제목 표시줄을 Shift와 함께 드래그합니다. Ctrl을 누르면 여러 영역을 선택합니다.").size(13))
                    .push(text("전역 단축키는 KDE에서 관리할 예정입니다.").size(13))
                    .push(text("위의 모니터 선택은 이번 편집기 실행에 사용됩니다. 자동 선택 정책은 연동 대기 중입니다.").size(12));
            }
            Category::Appearance => {
                c = c.push(self.toggle("영역 번호 표시", p.show_zone_number, Toggle::Numbers));
                for f in [
                    Field::Opacity,
                    Field::Highlight,
                    Field::Inactive,
                    Field::Border,
                    Field::Number,
                ] {
                    c = c.push(self.input(f));
                }
            }
            Category::Name => {
                c = c
                    .push(self.toggle(
                        "레이아웃 이름 표시",
                        p.layout_name_label_enabled,
                        Toggle::Name,
                    ))
                    .push(text("표시 위치").size(14))
                    .push(
                        pick_list(
                            [
                                "왼쪽 위",
                                "가운데 위",
                                "오른쪽 위",
                                "가운데",
                                "왼쪽 아래",
                                "가운데 아래",
                                "오른쪽 아래",
                            ],
                            Some(match p.layout_name_label_placement {
                                Placement::TopLeft => "왼쪽 위",
                                Placement::TopCenter => "가운데 위",
                                Placement::TopRight => "오른쪽 위",
                                Placement::Center => "가운데",
                                Placement::BottomLeft => "왼쪽 아래",
                                Placement::BottomCenter => "가운데 아래",
                                Placement::BottomRight => "오른쪽 아래",
                            }),
                            Message::Placement,
                        )
                        .width(Length::Fill),
                    );
                for f in [
                    Field::TextColor,
                    Field::Background,
                    Field::FontSize,
                    Field::Padding,
                    Field::Duration,
                ] {
                    c = c.push(self.input(f));
                }
            }
            Category::Window => {
                c = c
                    .push(self.toggle(
                        "드래그 창 투명화",
                        p.make_dragged_window_transparent,
                        Toggle::Transparent,
                    ))
                    .push(text("아래 옵션은 키보드 이동·빠른 레이아웃 전환용입니다.").size(12))
                    .push(
                        text(WAITING)
                            .size(12)
                            .color(iced::Color::from_rgb8(135, 90, 35)),
                    )
                    .push(text("영역 이동 기준").size(14))
                    .push(
                        pick_list(
                            ["영역 번호", "영역 위치"],
                            Some(if p.move_windows_based_on_position {
                                "영역 위치"
                            } else {
                                "영역 번호"
                            }),
                            Message::MoveBy,
                        )
                        .width(Length::Fill),
                    )
                    .push(self.toggle(
                        "모니터 사이 이동 허용",
                        p.move_window_across_monitors,
                        Toggle::Across,
                    ))
                    .push(self.toggle(
                        "레이아웃 전환 시 영역 표시",
                        p.flash_zones_on_quick_switch,
                        Toggle::Flash,
                    ));
            }
            Category::Excluded => {
                c = c
                    .push(
                        text("앱 ID를 한 줄에 하나씩 입력하세요. 대소문자를 유지합니다.").size(13),
                    )
                    .push(
                        text_editor(&self.excluded)
                            .on_action(Message::Excluded)
                            .height(140)
                            .padding(8)
                            .size(15),
                    );
            }
        }
        c.padding([4, 10]).into()
    }
    pub(super) fn view(&self) -> Element<'_, Message> {
        let editor = row![
            pick_list(
                self.outputs.clone(),
                self.selected.clone(),
                Message::SelectOutput
            )
            .placeholder("모니터 선택")
            .text_size(13),
            button(
                text(if self.child.is_some() {
                    "편집기 실행 중"
                } else {
                    "에디터 열기"
                })
                .size(13)
            )
            .on_press_maybe(
                (self.selected.is_some() && self.child.is_none() && !self.session.blocked())
                    .then_some(Message::OpenEditor)
            ),
            button(text("새로고침").size(12))
                .on_press_maybe((!self.outputs_loading).then_some(Message::RefreshOutputs))
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let mut nav = column![text("Waytoys").size(22), space().height(12)]
            .spacing(5)
            .width(110);
        for (page, title) in [
            (Page::General, "일반"),
            (Page::Zones, "Zones"),
            (Page::Capture, "캡처"),
            (Page::Paste, "클립보드"),
        ] {
            nav = nav.push(
                button(text(title).size(15))
                    .width(Length::Fill)
                    .padding([10, 8])
                    .style(if page == self.page {
                        button::primary
                    } else {
                        button::text
                    })
                    .on_press(Message::Page(page)),
            );
        }
        let usable = self.runtime.as_ref().is_some_and(|s| !s.blocked)
            && !self.runtime_busy
            && !self.runtime_offline;
        let state = self.runtime.as_ref();
        let mut runtime_info = column![
            text(if self.runtime_offline {
                "Waytoys 실행기 연결이 끊겼습니다"
            } else {
                state.map_or("Waytoys 연결 중…", RuntimeStatus::label)
            })
            .size(13)
        ]
        .spacing(6);
        if let Some(error) = self
            .runtime_error
            .as_ref()
            .or_else(|| state.and_then(|s| s.error.as_ref()))
        {
            runtime_info = runtime_info.push(
                text(error)
                    .size(12)
                    .color(iced::Color::from_rgb8(170, 55, 35)),
            );
        }
        if self.runtime_offline || state.is_none() {
            runtime_info = runtime_info.push(
                button("Waytoys 연결 재시도")
                    .on_press_maybe((!self.runtime_busy).then_some(Message::RuntimeConnect)),
            );
        } else if state.is_some_and(|s| s.blocked) || self.runtime_error.is_some() {
            runtime_info = runtime_info.push(
                button("일반 설정 다시 읽기")
                    .on_press_maybe((!self.runtime_busy).then_some(Message::RuntimeReload)),
            );
        }
        let body: Element<'_, Message> = if self.page == Page::General {
            let enabled = state.is_some_and(|s| s.settings.run_at_login);
            scrollable(column![
                text("일반").size(24),
                checkbox(enabled).label("로그인할 때 Waytoys 실행").on_toggle_maybe(usable.then_some(Message::SetLogin)).text_size(16),
                text("설정 창을 닫아도 Waytoys는 트레이에서 계속 실행됩니다. 트레이의 ‘Waytoys 종료’로 종료할 수 있습니다.").size(14),
                text("종료해도 로그인 실행과 Zones 사용 설정은 유지됩니다.").size(13),
                runtime_info,
            ].spacing(20).padding(12)).id("options").into()
        } else if self.page == Page::Paste {
            let mut content=column![
                text("클립보드").size(24),
                checkbox(state.is_some_and(|s|s.settings.enabled.paste)).label("클립보드 기록 수집").on_toggle_maybe(usable.then_some(Message::SetPaste)).text_size(16),
                text("복사한 텍스트와 이미지가 기록됩니다. 고정 항목은 최근 기록 정리에서 보존됩니다.").size(14),
                button("클립보드 기록 열기").padding([10,16]).on_press_maybe((state.is_some()&&!self.runtime_busy&&!self.runtime_offline).then_some(Message::OpenPaste)),
                text("기본 단축키: Meta(Windows 키) + V").size(14),
                text("항목을 선택하면 클립보드에 복사합니다. Ctrl+V로 붙여넣으세요.").size(13),
                text("최근 기록: 최대 100개 · 64 MiB / 고정: 최대 100개 · 128 MiB").size(12),
                text("수집을 꺼도 저장한 기록과 고정 항목은 유지됩니다.").size(12),
            ].spacing(18).padding(12);
            if let Some(error) = state
                .and_then(|s| s.paste_error.as_ref())
                .or(self.runtime_error.as_ref())
            {
                content = content.push(
                    text(error)
                        .size(12)
                        .color(iced::Color::from_rgb8(170, 55, 35)),
                );
            }
            if self.runtime_offline || state.is_none() {
                content = content.push(runtime_info);
            }
            scrollable(content).id("options").into()
        } else if self.page == Page::Capture {
            let mut content = column![
                text("영역 캡처").size(24),
                text("드래그한 영역을 이미지로 클립보드에 복사합니다.").size(16),
                text("마우스를 놓으면 복사됩니다. Esc 또는 오른쪽 클릭으로 취소합니다.").size(14),
                button("영역 캡처 시작").padding([10, 16]).on_press_maybe(
                    (state.is_some() && !self.runtime_busy && !self.runtime_offline)
                        .then_some(Message::Capture)
                ),
                text("시작하면 설정 창이 닫힙니다. 트레이에서 다시 열 수 있습니다.").size(13),
                text("기본 단축키: Meta(Windows 키) + Shift + S").size(14),
                text("단축키는 KDE 시스템 설정에서 변경할 수 있습니다.").size(13),
            ]
            .spacing(18)
            .padding(12);
            if self.runtime_offline || state.is_none() {
                content = content.push(runtime_info);
            } else if let Some(error) = &self.runtime_error {
                content = content.push(
                    text(error)
                        .size(13)
                        .color(iced::Color::from_rgb8(170, 55, 35)),
                );
            }
            scrollable(content).id("options").into()
        } else if self.session.blocked() {
            scrollable(container(column![text("설정 파일 확인이 필요합니다").size(20),text("잘못된 파일을 기본값으로 덮어쓰지 않습니다. 파일을 복구한 뒤 다시 읽어 주세요.").size(14),text(self.directory.join("preferences.json").display().to_string()).size(12),button("파일 다시 읽기").on_press(Message::Reload)].spacing(12)).padding(12)).height(Length::Fill).into()
        } else {
            let enabled = state.is_some_and(|s| s.settings.enabled.zones);
            let mut options = column![].spacing(24);
            for category in Category::ALL {
                options = options.push(self.options(category));
            }
            column![
                text("Zones").size(24),
                checkbox(enabled)
                    .label("Zones 사용")
                    .on_toggle_maybe(usable.then_some(Message::SetZones))
                    .text_size(16),
                runtime_info,
                editor,
                scrollable(options)
                    .id("options")
                    .width(Length::Fill)
                    .height(Length::Fill)
            ]
            .spacing(10)
            .height(Length::Fill)
            .into()
        };
        let status = if self.session.blocked() {
            "설정 읽기 오류 · 원본 보존"
        } else if self.session.error.is_some() {
            "저장 오류 · 원본 보존"
        } else if !self.draft.errors.is_empty() {
            "입력 미완성 · 마지막 유효값 유지"
        } else if self.session.pending() {
            "저장 대기 중…"
        } else {
            "모든 유효한 변경이 저장되었습니다"
        };
        let mut footer = column![
            row![
                text(status).size(12),
                space().width(Length::Fill),
                button("닫기").on_press(Message::Close)
            ]
            .align_y(iced::Alignment::Center)
        ]
        .spacing(4);
        if let Some(error) = &self.session.error {
            footer = footer
                .push(
                    text(error)
                        .size(12)
                        .color(iced::Color::from_rgb8(170, 55, 35)),
                )
                .push(
                    row![
                        button("저장 재시도")
                            .on_press_maybe(self.session.pending().then_some(Message::Retry)),
                        button("파일 다시 읽기").on_press(Message::Reload),
                        button("저장하지 않고 닫기").on_press(Message::DiscardClose)
                    ]
                    .spacing(6),
                );
        }
        if let Some(error) = &self.launch_error {
            footer = footer.push(
                text(error)
                    .size(12)
                    .color(iced::Color::from_rgb8(170, 55, 35)),
            );
        }
        container(column![row![nav, body].spacing(12).height(Length::Fill), footer].spacing(10))
            .padding(12)
            .into()
    }
}
