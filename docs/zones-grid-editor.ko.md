# 전체 화면 Grid 에디터

`waytoys-zones-editor`는 선택한 모니터 한 대를 덮는 독립 실행 파일이다. 기존 `waytoys-zones` 모델을 사용하며, 다른 Waytoys 기능이나 공통 호스트를 실행할 필요가 없다. 현재 편집 기준은 패널을 포함한 모니터 전체 논리 영역이다. 실제 앱 창 이동과 KWin 스크립트는 연결하지 않았다.

## 실행

로그인한 KDE Plasma Wayland 또는 X11 세션의 저장소 루트에서 실행한다. X11에서는 XRandR 1.5와 KWin 합성이 필요하다. `--list-outputs`에 나온 이름을 그대로 `--output`에 전달한다. [X11 안내](x11.ko.md).

```bash
cargo build -p waytoys-zones-editor --locked
cargo run -p waytoys-zones-editor --locked -- --list-outputs
cargo run -p waytoys-zones-editor --locked -- --list-outputs --json
cargo run -p waytoys-zones-editor --locked -- --output Virtual-2
```

출력을 지정하지 않으면 이름순 첫 출력을 편집한다. 저장 파일이 없으면 기본 3열로 시작한다. `fc-match`와 시스템 sans-serif 글꼴이 필요하며, 현재 컨트롤 문구는 영어다. 문자는 `fontdue`로 그리며 별도 GPU·위젯 프레임워크를 사용하지 않는다.

Windows PowerShell에서 VM의 기존 Wayland 세션에 띄우려면 다음 명령을 사용한다. 소스 전송과 빌드는 먼저 완료되어 있어야 한다.

```powershell
ssh debian@192.168.50.125 'systemd-run --user --unit=waytoys-zones-editor-dev --collect --setenv=WAYLAND_DISPLAY=wayland-0 /home/debian/projects/way-toys/target/debug/waytoys-zones-editor --output Virtual-2'
```

에디터에서 저장/취소로 닫거나 `systemctl --user stop waytoys-zones-editor-dev`로 종료한다. 같은 unit이 실행 중이면 중복 실행하지 않는다. 선택한 화면을 클릭하면 키보드 포커스를 얻는다.

## 조작

- 구역 위에 포인터를 올리면 분할 미리보기가 보인다. 짧게 클릭하면 해당 위치에서 분할한다. 가로가 긴 구역은 세로선으로, 세로가 긴 구역은 가로선으로 나뉜다. Shift를 누르면 방향이 바뀐다.
- 파란 경계를 끌면 양쪽 구역 크기가 함께 바뀐다. 최소 크기를 넘는 이동은 적용하지 않으며, 다시 가능한 위치로 돌아오면 계속 조절할 수 있다.
- 구역 안에서 다른 구역까지 드래그하면 병합 범위가 표시된다. 놓은 뒤 `Merge`를 눌러 확정한다. 필요한 경우 직사각형이 되도록 선택 범위가 확장된다.
- 상단 모니터 버튼은 다음 모니터로 전환한다. 수정한 상태이면 `Save and switch`·`Discard`·`Stay`를 선택한다.
- `Save` 또는 Ctrl+S는 저장 후 닫는다. `Cancel`은 저장하지 않고 닫는다. Esc는 먼저 진행 중인 드래그·선택·확인창을 취소하고, 그런 상태가 없을 때 에디터를 닫는다.
- Tab으로 구역을 선택하고 S로 중앙 분할할 수 있다. 포인터가 경계에 있으면 해당 축의 방향키로 논리 1픽셀씩 조절한다. Delete는 이웃과의 병합 확인을 연다.

조작 패널이 놓이는 상단 96 논리 픽셀은 Grid 클릭보다 컨트롤 입력이 우선한다. 구역 자체의 계산은 전체 모니터 크기를 사용한다. 구역 번호는 화면에서 1부터 표시하지만 내부 모델 ID는 0부터 시작한다.

## 저장과 좌표

기본 저장 위치는 `$XDG_CONFIG_HOME/waytoys/zones/outputs/output-<출력 이름>.json`이며, XDG 경로가 없으면 `~/.config/waytoys/zones`를 사용한다. 출력 이름의 경로 구분자와 특수 바이트는 percent-encoding한다. `--config-dir PATH`로 개발용 저장 위치를 분리할 수 있다.

파일 하나는 기존 `CustomLayouts` JSON 형식의 Grid 하나를 담는다. 잘못된 파일·Canvas·복수 레이아웃은 오류로 중단하며 자동으로 초기화하지 않는다. 저장 전 유효성 검사와 직렬화를 수행한 뒤 같은 디렉터리의 임시 파일을 동기화하고 rename으로 교체한다. rename 이전 실패는 기존 파일을 유지한다. Linux의 원자적 파일 교체를 대상으로 하며 저장 장치 고장까지 복구를 보장하지 않는다.

클릭·레이아웃은 surface 로컬 논리 좌표이고 Grid 내부는 전체 길이 10000의 비율이다. Wayland에서는 버퍼의 물리 크기와 배율을 프런트엔드에서만 처리한다. fractional-scale과 viewporter가 있으면 선호 분수 배율을, 없으면 정수 배율을 사용한다. 선택 출력의 제거는 자동 저장 없이 종료하고, 크기 변경과 포인터 상실은 진행 중 제스처를 취소한다.

X11에서는 XRandR 모니터 좌표와 루트 픽셀이 편집 좌표이며 `integer_scale=1`을 반환한다. 모니터 구성 변경 시 저장하지 않고 종료한다. Wayland와 X11의 출력 이름이 다르면 해당 이름의 레이아웃 파일을 각각 사용한다.

기존 모델·설정 CLI는 GUI 의존성 없이 유지된다. Wayland 프로토타입에서 재사용한 표시·입력 코드는 편집기 내부에 있으며 아직 공통 UI 프레임워크로 추출하지 않았다.

[공통 설정 창](zones-settings.ko.md)과 같은 디렉터리의 `preferences.json`을 읽는다. Grid 에디터와 `--preview` 모두 500ms 간격으로 변경을 확인하고, 외형이 달라진 경우에만 다시 그린다. 잘못된 설정은 마지막 유효값을 유지하고 stderr에 알린다. 영역 구조·간격·치수 표시는 유지하며 번호·색·불투명도·레이아웃 이름 표시만 공통 설정을 따른다. 이름은 도구 모음과 하단 상태 영역을 피해서 표시하고 지정 시간이 지나면 숨긴다. 기존 에디터의 시스템 sans-serif 글꼴 경로는 유지한다.

같은 설정 디렉터리에서는 커널 파일 잠금으로 에디터/표시 프로세스 중복 실행을 막는다. `--list-outputs --json`은 잠금을 잡거나 파일을 쓰지 않으며, 이름·논리 크기·논리 위치·정수 배율의 JSON 배열을 stdout에 출력한다. 정수 배율을 실제 fractional scale로 해석하면 안 된다. 실제 표시 배율은 surface의 fractional-scale 이벤트로 결정된다.

## Canvas 겹침 예제 표시

Canvas/Grid 사용자 레이아웃을 편집 없이 표시하려면 `--preview`를 사용한다. 예제에는 일부가 겹치는 1·2번 및 2·3번 사각형과, 1번 안에 포함되는 4번 사각형이 있다.

```bash
cargo run -p waytoys-zones-editor --locked -- --output Virtual-2 --preview crates/waytoys-zones-editor/examples/canvas-overlap.json
```

`--layout-index N`으로 문서 안의 다른 레이아웃을 선택할 수 있으며 기본값은 0이다. 미리보기는 `Next output`·`Close`·Esc만 사용하고, 클릭·Ctrl+S로 원본이나 Grid 설정을 수정하지 않는다. 현재 Canvas는 표시 전용이며 Canvas의 이동·크기 조절 UI는 아직 연결하지 않았다. [검증 기록](experiments/2026-10-31-17.md).

## 개발 검사

```bash
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

VM의 실제 Wayland 세션에서 분할·Shift 방향 전환·경계 이동·병합·저장·재실행·수정 버리기를 원격 입력으로 확인했다. 캡처는 기존 [VM 캡처 도구](vm-capture.ko.md)를 사용하며 Windows 클립보드를 읽거나 변경하지 않는다. 자세한 입력·로그·환경·한계는 [실험일지 2026-10-31-16](experiments/2026-10-31-16.md)에 있다.
