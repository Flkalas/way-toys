# 영역 캡처

**Meta(Windows 키)+Shift+S**를 누르고 영역을 드래그한다. 마우스를 놓으면 선택한 부분이 PNG 이미지로 클립보드에 복사된다. 이미지 붙여넣기를 지원하는 앱에서 **Ctrl+V**로 붙여넣는다. **Esc 또는 오른쪽 클릭**은 취소하며 기존 클립보드를 유지한다.

동영상, 주석, 파일 저장 UI와 별도 캡처 기록은 제공하지 않는다. 캡처 도구는 이미지 파일을 저장하지 않는다. [Waytoys 클립보드 기록](paste.ko.md)을 켜 두면 캡처한 PNG를 Meta+V에서 다시 선택하거나 고정할 수 있다. KDE 클립보드 관리자의 기록 정책은 별도로 적용된다.

## 설치

현재 검증 환경은 Debian 13.7의 KDE Plasma Wayland / KWin 6.3.6이다. 로그인한 데스크톱 사용자의 터미널에서 저장소 루트를 작업 디렉터리로 사용한다. 기존 Rust/Wayland 빌드 의존성에 더해 실행 시 `wl-clipboard`, 설치 시 Python D-Bus와 KDE의 `kbuildsycoca6`가 필요하다.

```bash
sudo apt-get install wl-clipboard python3-dbus
cargo build --workspace --locked
sh scripts/install-capture.sh --shortcut
./target/debug/waytoys-capture --check
```

설치는 사용자 경로 `~/.local/share/applications/org.waytoys.Capture.desktop`에 실행 파일과 KWin 화면 획득 인터페이스 권한을 등록한다. `sudo`로 설치 스크립트를 실행하지 않는다. 실행 파일 경로를 바꾸거나 release 빌드를 사용하면 다시 등록한다.

```bash
cargo build --workspace --release --locked
sh scripts/install-capture.sh --binary "$PWD/target/release/waytoys-capture" --shortcut
```

설정·실행기의 캡처 버튼은 자신의 실행 파일과 같은 디렉터리에 있는 `waytoys-capture`를 실행하므로, release 실행 시 설정·실행기도 같은 빌드를 사용한다. KWin의 화면 획득 권한 검사를 우회하지 않는다. 등록하지 않은 다른 경로의 실행 파일은 화면 획득이 거부된다.

`--shortcut`은 Spectacle이 사용하던 **Meta+Shift+S만** Waytoys로 옮기고 PrintScreen은 보존한다. 다른 앱과 충돌하면 변경하지 않고 오류를 낸다. 기존 할당은 `$XDG_STATE_HOME/waytoys/capture-shortcut.json`(기본 `~/.local/state/waytoys/`)에 기록한다. KDE 시스템 설정의 단축키에서 이후 변경할 수 있다. 이전 할당으로 복구하려면 다음을 실행한다.

```bash
sh scripts/install-capture.sh --restore-shortcut
```

복구는 설치 후 사용자가 단축키를 바꾸지 않았을 때만 적용하며, 기록 파일을 보관용 이름으로 옮긴다. 실행 파일 등록은 유지한다. `--shortcut` 없이 설치하면 단축키를 변경하지 않는다.

## 시작 경로와 동작

- **Meta+Shift+S**: 설정 창이나 Waytoys 실행기가 꺼져 있어도 시작한다.
- **트레이 → 영역 캡처** 또는 `./target/debug/waytoys --capture`: 실행 중인 Waytoys를 통해 시작한다.
- **설정 → 캡처 → 영역 캡처 시작**: 저장 후 설정 창을 닫고 시작한다. 설정은 트레이에서 다시 연다.
- `./target/debug/waytoys-capture`: 직접 실행한다.
- `./target/debug/waytoys-capture --check`: 화면 획득과 모니터 정보를 확인하고 종료한다. 영역 선택과 클립보드 변경은 하지 않는다.

시작할 때 각 화면의 정지 이미지를 가져오고 선택 영역 밖을 어둡게 표시한다. 결과에는 어두운 효과·테두리·마우스 포인터가 포함되지 않는다. 역방향 드래그와 모니터를 넘는 선택을 지원한다. 한 화면 안에서는 원본 물리 픽셀을 그대로 잘라낸다. 배율이 다른 여러 화면을 선택하면 가장 높은 픽셀 밀도로 합성하며 낮은 배율의 화면은 최근접 보간으로 확대한다. 모니터 사이의 빈 공간은 투명하게 처리한다.

잠긴 화면에서는 캡처를 시작하지 않는다. 선택 중 잠금·화면 구성 변경·포커스 상실·종료 신호가 발생하면 취소한다. 중복 호출은 새 선택 창을 만들지 않는다. Waytoys가 실행 중이면 캡처하는 동안 Zones 드래그 처리를 멈추고 종료 후 복구한다.

영역 선택 프로그램은 복사 후 종료한다. Wayland에서 클립보드 데이터를 제공하는 `wl-copy`는 별도의 사용자 scope에 남아 Waytoys 종료 후에도 붙여넣을 수 있게 한다. 새 클립보드 소유자가 생기면 이전 소유자는 종료한다. 이는 선택 오버레이가 계속 실행되는 것과 구분된다.

Windows의 PVE 웹 콘솔에서는 호스트가 같은 Windows+Shift+S 조합을 먼저 처리할 수 있다. 그런 경우 VM의 설정 버튼이나 트레이에서 캡처를 시작한다. VM에 키를 직접 전달하는 QMP 테스트는 통과했지만 Windows 브라우저를 통한 이 단축키 전달은 이번에 검증하지 않았다.

구현·실제 입력·붙여넣기·배율·수명 검증과 실행 기록은 [26회차](experiments/2026-10-31-26.md)에 있다.
