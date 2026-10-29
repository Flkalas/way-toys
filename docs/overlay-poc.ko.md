# Wayland 오버레이 기술 검증

이 문서는 Waytoys 공통 UI 호스트의 기반을 선택하기 위한 Rust 오버레이 프로토타입의 실행·검증 기록이다. 프로토타입은 완성된 UI 호스트가 아니며, 창 배치, 화면 캡처, 클립보드 기능을 구현하지 않는다.

## 검증 환경

2026-10-29 조사 당시 현재 로그인 세션은 Debian 13, KDE Plasma/KWin 6.3.6의 **X11**이다. `kwin_wayland`는 설치되어 있지만 현재 세션에 연결된 Wayland 디스플레이는 없다. 물리 모니터는 2560×1440과 1920×1080 두 대이고 모두 배율 1이다.

따라서 현재 세션에서는 X11 위의 별도 D-Bus 세션에서 중첩 KWin Wayland를 띄워 먼저 확인한다. 중첩 KWin에서의 결과는 실제 Wayland 로그인 세션의 물리 모니터·입력 검증을 대체하지 않는다. 혼합 배율은 환경을 갖추기 전까지 미검증으로 기록한다.

## 빌드와 자동 검사

저장소 루트에서 다음 명령을 실행한다.

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

테스트 결과와 적용된 도구 버전을 결과 기록에 포함한다. 최초 빌드 시 Rust 도구와 Cargo 의존성을 다운로드할 수 있다.

## 중첩 KWin 실행

중첩 KWin은 실제 데스크톱을 교체하지 않고 X11 창 안에서 Wayland 출력과 컴포지터를 제공한다. 설치된 `kwin_wayland --help`에서 `--x11-display`, `--width`, `--height`, `--output-count`, `--socket`, `--no-lockscreen`, `--no-global-shortcuts` 옵션을 확인했다. 독립 D-Bus 세션 및 별도의 Wayland 소켓을 사용한다.

X11 세션의 터미널에서 아래 명령을 실행한다. 별도의 임시 런타임과 설정 디렉터리, D-Bus 세션을 사용하며 종료 시 삭제한다. 표시 모드는 Ctrl+C로 종료하고, 선택 모드는 드래그 완료 또는 Esc로 종료한다.

```sh
cargo build -p waytoys-overlay
WAYTOYS_BIN="$PWD/target/debug/waytoys-overlay" dbus-run-session -- sh -c '
  set -eu
  test_dir=$(mktemp -d)
  chmod 700 "$test_dir"
  export XDG_RUNTIME_DIR="$test_dir" XDG_CONFIG_HOME="$test_dir/config"
  export XDG_CACHE_HOME="$test_dir/cache" XDG_DATA_HOME="$test_dir/data"
  export WAYLAND_DISPLAY=waytoys-test
  mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME"
  kwin_pid=
  cleanup() {
    if [ -n "$kwin_pid" ]; then kill "$kwin_pid" 2>/dev/null || true; wait "$kwin_pid" 2>/dev/null || true; fi
    rm -rf "$test_dir"
  }
  trap cleanup EXIT HUP INT TERM
  kwin_wayland --x11-display "$DISPLAY" --socket "$WAYLAND_DISPLAY" \
    --width 640 --height 480 --output-count 2 \
    --no-lockscreen --no-global-shortcuts --no-kactivities >"$test_dir/kwin.log" 2>&1 &
  kwin_pid=$!
  count=0
  while [ ! -S "$test_dir/$WAYLAND_DISPLAY" ] && kill -0 "$kwin_pid" 2>/dev/null && [ "$count" -lt 100 ]; do
    sleep 0.1; count=$((count + 1))
  done
  if [ ! -S "$test_dir/$WAYLAND_DISPLAY" ]; then cat "$test_dir/kwin.log"; exit 1; fi
  "$WAYTOYS_BIN" --select
'
```

표시 모드를 확인할 때는 마지막 `--select`를 `--display`로 바꾼다. 중첩 KWin이 시작되지 않거나 필요한 프로토콜을 제공하지 않으면 오류 메시지와 환경을 기록하며, 그 결과를 통과로 표시하지 않는다.

## 수동 검증

| 시나리오 | 확인할 결과 |
|---|---|
| 출력 열거 | 출력마다 이름, 논리 크기, 배율이 기록되고 구분 가능한 표시가 나타난다. |
| 표시 모드 | 오버레이가 보이면서 아래 창의 포인터 입력이 동작하고 키보드 포커스를 빼앗지 않는다. |
| 선택 모드 | 드래그로 선택한 사각형이 표시되고 출력된 논리 좌표가 선택 위치와 맞는다. |
| 선택 취소 | `Esc`가 선택을 취소하고 오버레이를 닫는다. |
| 화면 변경 | 가능한 환경에서 출력의 추가·제거 및 크기·배율 변경을 기록하고 화면이 갱신되는지 확인한다. |
| 반복 실행 | 오버레이 생성·종료를 반복한 뒤 고아 화면과 입력 점유가 남지 않는다. |

선택 좌표의 역방향 드래그, 화면 가장자리와 여러 출력에서의 동작도 확인한다. 실제 Wayland 세션에서는 물리 모니터마다 별도로 수행한다. KWin 창 드래그와 오버레이의 공존은 별도 검증이며, 위 테스트의 성공만으로 판단하지 않는다.

## 메모리 측정

같은 빌드 및 출력 구성에서 프로토타입과 중첩 KWin의 PID를 각각 확인하고 `/proc/<pid>/smaps_rollup`의 `Pss:`를 읽는다. 시작 직후, 오버레이 표시 중, 오버레이를 닫은 뒤를 각각 기록한다. 프로세스가 종료된 뒤에는 측정할 수 없으므로 종료 직전의 측정과 프로세스 종료 여부를 구분한다. 여러 번 반복해 증가 추세를 확인하고 테스트 조건·시각을 함께 남긴다.

GPU 메모리는 PSS에 포함되는 전체 비용이라고 가정하지 않는다. 신뢰할 수 있는 측정 수단이 없으면 **미측정**으로 기록한다. 다른 UI 툴킷을 동일 조건에서 측정하기 전에는 메모리 우위를 주장하지 않는다.

## 결과 기록

2026-10-29 Debian 13, X11, KWin 6.3.6 중첩 환경에서 각각 640×480인 출력 두 개를 사용했다. Wayland registry는 layer-shell v5, wl_output v4 두 개를 보고했다. 자동 입력으로 첫 출력의 (100,120)에서 (300,260)까지 드래그하여 `x=100 y=120 width=200 height=140` 로그를 확인했고, 별도 실행에서 Esc 취소와 종료를 확인했다. 이 결과는 실제 물리 Wayland 세션을 대체하지 않는다.

최종 디버그 빌드의 선택 모드에서 중첩 KWin 2출력 표시 중 프로토타입 PSS는 약 3,696 KiB였다(한 차례 측정). 표시 모드 초기 빌드에서는 2,960 KiB를 관측했으나 빌드가 다르므로 직접 비교할 수 없다. KWin의 `smaps_rollup`은 접근이 거부되어 측정하지 못했다. 화면 닫기 전후 비교 및 GPU 메모리는 측정하지 않았다.

| 항목 | 결과 |
|---|---|
| 빌드·자동 검사 | 통과: 빌드, 테스트 3개, fmt, clippy |
| 중첩 KWin 기동·프로토콜 | 통과: 출력 2개, layer-shell v5 |
| 모니터별 표시 | 부분 확인: 출력별 surface 구성 로그 확인 |
| 표시 모드 입력 통과 | 미검증 |
| 선택 모드·취소 | 통과: 첫 출력 드래그 좌표 및 Esc |
| 출력 변경·반복 종료 | 미검증 |
| 실제 Wayland 세션 | 미검증 |
| 혼합 배율 | 미검증 |
| CPU PSS / GPU 메모리 | 최종 앱 선택 모드 3,696 KiB 한 차례 / 미측정; KWin은 접근 거부 |

## XRDP 중첩 KWin의 혼합 배율 시각 검증

2026-10-29 XRDP의 X11 세션 안에 격리된 중첩 KWin을 띄워 `X11-0` 800×600·100%, `X11-1` 600×400·150%로 설정하고 두 X11 창을 분리했다. 두 번째 창에서 (150,100)→(450,300)을 `xdotool`로 드래그하는 동안 각 창을 X11에서 캡처했다. 이 그림은 앱이 캡처한 화면이 아니라 테스트 환경의 실제 출력 창이다.

| 첫 번째 출력 | 두 번째 출력 |
|---|---|
| ![첫 번째 출력에서 나타난 선택 테두리](images/nested-first-drag.png) | ![선택 테두리가 나타나지 않은 두 번째 출력](images/nested-second-drag.png) |

관찰: 두 번째 창을 드래그했는데 첫 번째 출력의 오른쪽 끝에 선택 사각형이 나타났다. 로그는 `selection output=0 x=633.33203125 y=66.6640625 width=165.66796875 height=133.3359375`를 기록했다. 기대와 다르므로 **혼합 배율 선택 검증 실패**다. 중첩 KWin의 X11 백엔드, XRDP, 자동 입력과 클라이언트 중 어느 부분에서 좌표가 잘못되었는지는 확인되지 않았다. Flameshot의 실제 Wayland 세션 문제와 원인이 같다는 증거도 없다. 실제 로그인 세션, 수동 입력 및 캡처 이미지-출력 매핑은 별도 검증이 필요하다.

같은 창 배치와 드래그를 두 출력 모두 100%로 맞춰 반복하자 `selection output=1 x=150 y=100 width=300 height=200`으로 정상 출력됐다. 이 실험에서는 배율 변경과 현상이 함께 나타났지만, 그것만으로 결함이 발생하는 계층은 특정할 수 없다.
