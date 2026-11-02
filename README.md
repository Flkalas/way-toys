# Waytoys

KDE Plasma Wayland를 우선 대상으로 검토하는 데스크톱 도구 모음입니다.

- **Zones**: FancyZones 형태의 영역 기반 창 배치
- **Capture**: 선택 영역을 이미지로 클립보드에 복사
- **Paste**: 텍스트·이미지 클립보드 기록, 검색과 고정
- **설정 UI**: 기능 활성화와 설정
- **Waytoys 공통 UI 호스트**: 모니터별 오버레이와 기능 간 화면·입력 전환

각 부가기능은 자기 데이터·설정·실행 진입점을 소유합니다. 현재 Zones의 Rust 모델·전체 화면 Grid 에디터, 한국어 통합 설정 창, 트레이 실행기와 KWin 6.3.6용 Shift 드래그 창 배치 연결이 있습니다. Capture는 Meta+Shift+S로 영역을 선택해 PNG 이미지를 클립보드에 복사합니다. 기존 오버레이 편집기는 그대로 사용합니다.

전체 대화 요약, 조사 출처, 설계 방향과 검증 계획은 [조사 및 계획 문서](docs/research-and-plan.ko.md)에 정리되어 있습니다.

## Rust 개발 환경

[rustup](https://rustup.rs/)을 설치하면 저장소의 `rust-toolchain.toml`이 Rust 버전과 개발 도구를 선택합니다. 최초 실행 시 해당 도구가 자동으로 설치됩니다. 새 터미널에서 `cargo`가 보이지 않으면 `source "$HOME/.cargo/env"`를 실행하세요.

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Cargo 워크스페이스의 `waytoys-zones`는 독립 라이브러리와 CLI이며 `waytoys-core`나 `waytoys-overlay`에 의존하지 않습니다. `waytoys-overlay`는 별도의 Wayland 기술 검증 실행 파일입니다. `waytoys-core`는 일반 설정·저장·실행기 통신을, `waytoys-runtime`은 트레이·KWin 연결·표시 오버레이를 담당합니다. `Cargo.lock`은 저장소에서 관리하고 빌드 산출물 `target/`은 제외합니다.

Zones만 빌드하고 실행할 수 있습니다. 실행 파일 자체는 다른 Waytoys 기능이나 데스크톱 세션을 요구하지 않습니다.

```bash
cargo build -p waytoys-zones --locked
cargo run -p waytoys-zones --locked -- settings
cargo run -p waytoys-zones --locked -- preview crates/waytoys-zones/examples/custom-layouts.json 0 1280 720
```

가져온 데이터·설정·편집 조작의 범위와 좌표 계약은 [Zones 개발 문서](crates/waytoys-zones/README.md)에 있습니다.

모니터 위에서 Grid를 분할·병합·조절하려면 [전체 화면 Grid 에디터](docs/zones-grid-editor.ko.md)를 사용하세요. `cargo run -p waytoys-zones-editor --locked -- --output Virtual-2`로 실행할 수 있습니다.

[통합 설정 창](docs/zones-settings.ko.md)은 `waytoys-settings`로 실행합니다. `waytoys-zones-settings`도 호환 진입점으로 유지합니다. Iced 0.14.0의 소프트웨어 렌더러와 Noto Sans CJK KR을 사용하며, 유효한 변경을 자동 저장합니다. `waytoys-zones-config`는 GUI 의존성이 없는 공통 모델·저장 라이브러리입니다.

실제 창을 Shift 드래그로 배치하려면 [KWin 연결 설치·실행 방법](docs/zones-runtime.ko.md)을 따라 플러그인을 설치하고 설정 창에서 **Zones 사용**을 켭니다. 기본값은 꺼짐입니다. 설정 창을 닫아도 트레이 실행기는 계속 동작합니다.

[영역 캡처 설치·사용법](docs/capture.ko.md)에 따라 `sh scripts/install-capture.sh --shortcut`을 실행하면 **Meta(Windows 키)+Shift+S → 드래그 → 마우스 놓기 → Ctrl+V**로 사용할 수 있습니다. Esc 또는 오른쪽 클릭으로 취소합니다. 트레이의 ‘영역 캡처’와 설정의 ‘캡처’ 페이지에서도 시작할 수 있습니다.

[클립보드 기록 설치·사용법](docs/paste.ko.md)에 따라 `sh scripts/install-paste.sh --shortcut`을 실행하고 설정의 ‘클립보드 → 기록 수집’을 켭니다. **Meta+V → 항목 선택 → Ctrl+V**로 이전 텍스트와 이미지를 다시 사용합니다. 고정한 항목은 재시작과 최근 기록 정리 후에도 남습니다. 캡처한 PNG도 기록에 들어갑니다.

오버레이 프로토타입의 실행 방법과 검증 결과는 [Wayland 오버레이 기술 검증](docs/overlay-poc.ko.md)을 참고하세요.

VM에서 실행한 화면을 Windows로 가져와 탭으로 확인하려면 [VM 화면 캡처 도구](docs/vm-capture.ko.md)를 사용하세요.

Zones 구현 참고 자료: [PowerToys FancyZones 소스 분석](docs/fancyzones-source-analysis.ko.md).

설정 UI를 추려서 구성할 때는 [FancyZones 원본 설정 항목 목록](docs/fancyzones-settings-inventory.ko.md)을 참고하세요. 메인 설정 43개와 별도 레이아웃 편집기 항목을 구분해 번호를 붙였습니다.
