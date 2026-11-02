# Wayland 오버레이 기술 검증

이 문서는 Waytoys 공통 UI 호스트의 기반을 선택하기 위한 Rust 오버레이 프로토타입의 실행·검증 기록이다. 프로토타입은 완성된 UI 호스트가 아니며, 창 배치, 화면 캡처, 클립보드 기능을 구현하지 않는다.

## 검증 환경

2026-10-29 조사 당시 현재 로그인 세션은 Debian 13, KDE Plasma/KWin 6.3.6의 **X11**이다. `kwin_wayland`는 설치되어 있지만 현재 세션에 연결된 Wayland 디스플레이는 없다. 물리 모니터는 2560×1440과 1920×1080 두 대이고 모두 배율 1이다.

따라서 현재 세션에서는 X11 위의 별도 D-Bus 세션에서 중첩 KWin Wayland를 띄워 먼저 확인한다. 중첩 KWin에서의 결과는 실제 Wayland 로그인 세션의 물리 모니터·입력 검증을 대체하지 않는다. 물리 Wayland 세션의 혼합 배율은 미검증이다.

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

## 실험일지

- [2026-10-29-01 초기 중첩 테스트](experiments/2026-10-29-01.md)
- [2026-10-29-02 XRDP 혼합 배율 테스트](experiments/2026-10-29-02.md)
- [2026-10-30-01 로컬 X11 재시험](experiments/2026-10-30-01.md)
- [2026-10-31-01 재현 스크립트 검증](experiments/2026-10-31-01.md)
- [2026-10-31-02 격리 X11 시험 환경 준비 실패](experiments/2026-10-31-02.md)
- [2026-10-31-03 창 메타데이터 호환성 재시험 실패](experiments/2026-10-31-03.md)
- [2026-10-31-04 X11 중첩 입력 오류 재현과 원인 분석](experiments/2026-10-31-04.md)
- [2026-10-31-05 VM 기본 Wayland 세션의 출력별 오버레이 캡처](experiments/2026-10-31-05.md)

## 과거 대화의 실험 명령과 결과

이 절은 2026-10-29~30 대화 및 당시 남은 로컬 스크립트·로그를 정리한 것이다. 실험일지의 일자-회차는 관련 대조 실험을 묶어 대화에서 확인된 순서대로 부여했으며, 개별 실행 횟수 전체를 뜻하지 않는다. XRDP 실험의 원본 임시 스크립트와 로그는 보존되지 않았으므로 재현 명령의 세부 사항은 로컬 X11 실험의 원본에 근거한다. 다음 기록은 Waytoys 선택 오버레이 실험이며 Flameshot 캡처 이미지를 재현한 실험이 아니다.

### 공통 조건과 정확한 입력

- Debian 13, KWin 6.3.6, Rust 디버그 빌드 `waytoys-overlay --select`; 구현 기준 커밋 `3e9dbcd`, 로컬 시험 당시 `f28c06f`.
- 호스트는 X11이고, 독립 `dbus-run-session` 안에서 임시 `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`을 사용했다. `kscreen-doctor` 호출에는 반드시 **별도 세션의** `WAYLAND_DISPLAY`, `XDG_SESSION_TYPE=wayland`, `QT_QPA_PLATFORM=wayland`가 적용되어야 한다. 호스트 세션의 디스플레이 설정에 실행해서는 안 된다.
- 중첩 컴포지터 시작 명령:

```sh
kwin_wayland --x11-display "$DISPLAY" --socket waytoys-local \
  --width 800 --height 600 --output-count 2 \
  --no-lockscreen --no-global-shortcuts --no-kactivities
```

- 같은 격리 세션에서 첫 출력은 100%, 둘째는 비교에 따라 100% 또는 150%로 지정했다.

```sh
kscreen-doctor output.X11-0.scale.1 output.X11-1.scale.1     # 대조군
kscreen-doctor output.X11-0.scale.1 output.X11-1.scale.1.5   # 혼합 배율
```

- 당시 `wmctrl -lp`에서 중첩 KWin PID에 해당하는 첫 번째·두 번째 X11 창 ID를 찾아 `first`, `second`에 넣었다. 첫 출력은 800×600, 둘째는 창을 600×400으로 조정했다. X11 창 위치는 겹치지 않도록 옮겼다.

```sh
xdotool windowsize "$second" 600 400
xdotool windowmove "$first" 700 200
xdotool windowmove "$second" 1600 200
kscreen-doctor -o
./target/debug/waytoys-overlay --select
```

- 오버레이는 별도 프로세스로 실행하고, 둘째 창의 **X11 창 기준 좌표** `(150,100)`에서 `(450,300)`으로 합성 드래그했다. 버튼을 놓기 전 각 중첩 창의 이미지를 캡처했다. `import`는 ImageMagick 도구다.

```sh
xdotool windowactivate --sync "$second" mousemove --window "$second" 150 100 \
  mousedown 1 mousemove --sync --window "$second" 450 300 sleep 1
import -window "$first" first.png
import -window "$second" second.png
xdotool mouseup 1
```

위 명령을 별도 터미널에서 무작정 순서대로 실행하면 격리된 D-Bus·Wayland 환경 변수나 창 ID가 전달되지 않는다. 자동 재현은 [재현 스크립트 절차](experiments/2026-10-31-01.md)를 사용한다. 스크립트는 마우스 포인터와 창 포커스를 실제로 움직이므로 테스트 중에는 호스트 입력을 사용하지 않는다.

### 대화 순서대로 결과

1. **초기 중첩 테스트 (2026-10-29):** 각각 640×480인 두 출력을 생성했다. Wayland registry에서 `zwlr_layer_shell_v1` v5와 `wl_output` v4 두 개가 관측됐다. 첫 출력에서 `(100,120)`→`(300,260)` 드래그 시 `output=0 x=100 y=120 width=200 height=140`; 별도 실행에서 `Esc` 취소를 확인했다. 표시 모드의 실제 입력 통과, 출력 추가·제거, 물리 Wayland 세션은 확인하지 않았다. 이 프로브와 실행 스크립트의 원본은 현재 보존되지 않았다.
2. **메모리:** 중첩 출력 두 개의 디버그 빌드에서 초기 표시 모드 PSS 2,960 KiB, 이후 선택 모드 PSS 3,696 KiB를 각각 한 차례 기록했다. 빌드·모드가 달라 직접 비교할 수 없고, `smaps_rollup` 접근 거부로 KWin PSS는 측정하지 못했다. GPU와 종료 전후는 미측정이다.
3. **XRDP X11 혼합 배율 (2026-10-29):** 800×600·100%와 600×400·150% 출력의 둘째 창에 합성 드래그했을 때 첫 화면 오른쪽에 테두리가 나타났다. 로그 `output=0 x=633.33203125 y=66.6640625 width=165.66796875 height=133.3359375`. 둘 다 100%인 대조군은 `output=1 x=150 y=100 width=300 height=200`. 둘째 창을 호스트에서 200픽셀 옮겨도 150% 결과는 같았다. 실제 캡처는 [XRDP 실험일지의 그림](experiments/2026-10-29-02.md)이다.
4. **로컬 X11 재시험 (2026-10-30):** `DISPLAY=:0`, `Remote=no`. 물리 화면 배치는 DP-1-1 3600×2400+0+1440, DP-1-2 2560×1440+668+0이었다. 위의 동일 배율 및 혼합 배율 실험에서 XRDP와 동일한 로그를 얻었다. `wmctrl -lpG`의 중첩 창 실제 위치는 첫 창 `701,228 800×600`, 둘째 `1601,228 600×400`; 장식과 창 관리자에 따라 달라질 수 있다. 150%일 때 `kscreen-doctor`는 둘째 화면 물리 600×400, 논리 400×267, 배율 1.5를 보고했고 Waytoys는 정수 배율 2, 버퍼 800×534를 기록했다. 혼합 배율 PNG는 XRDP 캡처와 SHA-256이 같았다.

**해석의 한계:** xdotool 합성 입력과 중첩 KWin의 X11 백엔드가 포함된다. 입력이 실제 둘째 출력으로 올바르게 전달됐는지 독립적인 증거가 없으므로 KWin 자체나 Waytoys의 버그로 단정할 수 없다. 로컬 시험 재현도 물리 Wayland 로그인 세션 검증을 대신하지 않는다. 실험 세션의 창 제목, 출력 이름, 위치, 해상도와 배율, 앱 로그를 함께 보존하고, 사용 중인 KWin·Qt·배포판·코드 버전을 기록해야 한다.

재현 스크립트 사용법과 검증 결과는 [2026-10-31-01 실험일지](experiments/2026-10-31-01.md)에 정리되어 있다.
