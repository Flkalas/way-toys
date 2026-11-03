# Zones 실제 창 배치

아래 네이티브 플러그인 설치·입력 필터·투명화 설명은 **Wayland** 경로에 해당한다. **X11**에서는 실행기가 KWin 스크립트와 XInput 2.1을 자동으로 연결한다. 플러그인 설치 없이 같은 설정 창과 레이아웃을 사용하며 드래그 창 투명화는 적용하지 않는다. [X11 실행·검증 안내](x11.ko.md)를 참고한다.

KWin이 진행하는 창 이동을 유지하면서, Waytoys가 처리하는 Shift 드래그에 한해 KDE 기본 Shift 타일 선택을 억제한다. Rust 실행기가 기존 Grid/Canvas 배치 계산과 렌더러를 사용하고, 드롭하면 KWin 플러그인이 실제 창의 위치와 크기를 적용한다. 기존 전체 화면 오버레이 편집기를 다시 만드는 작업은 포함하지 않는다.

## 설치와 실행

현재 네이티브 연결은 **KWin 6.3.6**용이다. KWin 플러그인 ABI는 버전에 종속되므로 다른 버전에서 로드하도록 버전 검사를 우회하지 않는다. Debian 13 개발 VM에서 다음으로 빌드했다.

```bash
sudo apt-get install kwin-dev qt6-base-dev extra-cmake-modules cmake ninja-build fonts-noto-cjk
cd ~/projects/way-toys
cargo build --workspace --locked
sh scripts/install-kwin-zones.sh
./target/debug/waytoys-settings
```

설정 창의 **Zones → Zones 사용**을 켠다. 플러그인을 이미 불러온 상태에서 업데이트했다면 로그아웃 후 다시 로그인한다. 설치 스크립트는 해당 라이브러리 하나만 원자적으로 교체하며 `kwinrc`의 타일·단축키 설정은 수정하지 않는다. 실행기가 필요한 때 플러그인을 로드한다.

실제 사용자 설정과 분리하려면 두 경로를 지정한다. 동일 세션의 실행기는 한 개만 허용하며, 다른 경로로 실행 중이면 오류를 표시한다.

```bash
./target/debug/waytoys-settings \
  --app-config-dir /tmp/waytoys-demo/app --config-dir /tmp/waytoys-demo/zones
./target/debug/waytoys --status
./target/debug/waytoys --zones on
./target/debug/waytoys --zones off
./target/debug/waytoys --quit
```

`--config-dir`은 기존처럼 Zones 디렉터리다. 일반 설정은 `--app-config-dir`의 `settings.json`에 저장하며 기본 경로는 `$XDG_CONFIG_HOME/waytoys/settings.json`이다. 파일에는 `run_at_login`, `enabled.zones`, `enabled.paste`가 있고 기본값은 모두 false다. 기존 파일에 `enabled.paste`가 없으면 false로 읽는다. Zones의 기존 `preferences.json` 21개 필드와 모니터별 레이아웃 JSON은 별도로 유지한다. 클립보드 데이터는 별도 SQLite 파일이며 [클립보드 문서](paste.ko.md)의 경로 규칙을 따른다.

## 조작과 수명

- 창 제목 표시줄을 왼쪽 버튼으로 끈 상태에서 Shift를 누른다. Shift를 먼저 눌러도 된다. 앱 본문에서 새로운 이동을 시작하는 단축키는 등록하지 않는다.
- Ctrl을 누르면 같은 모니터의 여러 영역을 합친다. 다른 화면으로 넘어가면 선택을 초기화하고 해당 화면에서 다시 선택한다.
- 영역 좌표는 **패널을 포함한 모니터 전체 크기**를 기준으로 한다. 화면을 가로지르는 영역은 만들지 않는다.
- Shift를 놓으면 영역 표시와 투명화를 해제하고 일반 이동을 계속한다. Esc는 창 이동을 취소한다.
- 설정 창을 닫아도 실행기는 트레이에 남는다. 트레이의 ‘설정 열기’는 기존 창을 활성화하고, ‘Waytoys 종료’는 실행기를 종료한다. 종료해도 사용 설정 값은 바뀌지 않는다.
- 로그인 실행은 Waytoys가 관리하는 `~/.config/systemd/user/waytoys.service`로 등록한다. 일반 설정에서 끄면 다음 로그인부터 자동 시작하지 않는다.
- 트레이의 ‘영역 캡처’ 또는 `waytoys --capture`로 [이미지 캡처](capture.ko.md)를 시작한다. 캡처 중에는 Zones의 드래그 처리를 일시 정지하고 종료·취소 후 다시 연결한다. Zones 사용 설정과 레이아웃 파일은 바뀌지 않는다.
- 트레이의 ‘클립보드 기록’, `waytoys --paste` 또는 Meta+V로 기록 창을 연다. 실행기가 기록 수집기를 관리한다. 기록 창을 닫으면 UI 프로세스는 종료하고 수집기는 계속 동작한다. 실행기를 종료하면 수집기도 종료한다. 기록 창 표시 중에는 Zones를 일시 정지하고 닫힌 뒤 복구한다.

표시 오버레이에는 빈 입력 영역을 지정하며 키보드 포커스를 요청하지 않는다. 창 투명화는 원래 불투명도의 50%이며 종료·취소·비활성화·화면 잠금·실행기 연결 해제 때 원래 값으로 복구한다. 실행기 정지는 4초 heartbeat lease로 감지한다. 최소 크기가 선택한 영역보다 큰 창은 강제로 줄이지 않는다.

제외 앱은 한 줄에 하나씩 입력하며 앱 ID 또는 WM_CLASS를 **대소문자 구분해 정확히** 비교한다. 키보드 영역 이동, 빠른 레이아웃 전환, 편집기 실행 화면 자동 선택 정책과 전역 단축키 등록은 아직 제공하지 않는다. 관련 UI에는 미적용 안내가 남아 있다.

## 연결과 검증

세션 버스 `org.waytoys.Runtime1`이 설정 창과 KWin 연결을 조정한다. KWin의 `/Waytoys/Zones`는 같은 버스 이름의 실제 소유자만 설정·배치 요청에 접근하도록 확인한다. 플러그인 인스턴스 ID, 드래그 ID, 순번을 비교하여 오래된 드롭을 거절한다. 드롭 응답은 150ms 동안 기다리며 응답이 없으면 배치하지 않는다. 설정·레이아웃·표시 연결이 준비되지 않은 상태에서는 기본 이동 동작을 가로채지 않는다.

공식 [KWin 6.3.6 플러그인 로더](https://github.com/KDE/kwin/blob/v6.3.6/src/pluginmanager.cpp)와 설치된 `kwin-dev` 헤더를 기준으로 구현했다. 전역 Shift 설정이나 사용자 타일 트리를 변경하는 방식이 아니다.

구현 과정은 [23회차](experiments/2026-10-31-23.md), 최종 빌드·실제 입력·로그인·종료·메모리 측정은 [24회차](experiments/2026-10-31-24.md)에 기록한다. 재시험 도구 `scripts/test-zones-runtime.py`는 격리된 개발 VM 설정과 QMP 입력 도구를 사용한다. 테스트용 KWin 스크립트 `scripts/kwin-window-probe.py`는 실행 후 즉시 해제된다.
