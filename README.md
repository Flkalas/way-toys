# Waytoys

KDE Plasma Wayland를 우선 대상으로 검토하는 데스크톱 도구 모음입니다.

- **Zones**: FancyZones 형태의 영역 기반 창 배치
- **Capture**: 화면 캡처, 영역 선택, 주석
- **Paste**: 클립보드 기록과 선택
- **설정 UI**: 기능 활성화와 설정
- **Waytoys 공통 UI 호스트**: 모니터별 오버레이와 기능 간 화면·입력 전환

현재는 조사·설계 단계이며 구현 스택과 프로세스 구조는 미확정입니다.

전체 대화 요약, 조사 출처, 설계 방향과 검증 계획은 [조사 및 계획 문서](docs/research-and-plan.ko.md)에 정리되어 있습니다.

## Rust 개발 환경

[rustup](https://rustup.rs/)을 설치하면 저장소의 `rust-toolchain.toml`이 Rust 버전과 개발 도구를 선택합니다. 최초 실행 시 해당 도구가 자동으로 설치됩니다. 새 터미널에서 `cargo`가 보이지 않으면 `source "$HOME/.cargo/env"`를 실행하세요.

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Cargo 워크스페이스는 현재 공통 라이브러리 골격만 포함합니다. UI 및 Wayland 시스템 라이브러리는 구현 방식을 검증한 뒤 추가합니다. `Cargo.lock`은 저장소에서 관리하고 빌드 산출물 `target/`은 제외합니다.

오버레이 프로토타입의 실행 방법과 검증 결과는 [Wayland 오버레이 기술 검증](docs/overlay-poc.ko.md)을 참고하세요.
