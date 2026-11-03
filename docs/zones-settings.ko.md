# Zones 공통 설정 창

`waytoys-settings`는 일반·Zones·캡처·클립보드 페이지를 제공하는 통합 앱 창이다. 기존 `waytoys-zones-settings`는 Zones 페이지로 여는 호환 진입점이다. 한국어 UI, Noto Sans CJK KR, 초기 논리 크기 760×380, 최소 560×300을 사용한다. 세부 설정은 한 열로 배치하며 스크롤할 수 있다. 별도의 Grid/Canvas 예시 미리보기는 두지 않고, ‘에디터 열기’에서 기존 전체 화면 오버레이 편집기를 실행한다.

캡처 페이지의 ‘영역 캡처 시작’은 유효한 설정을 저장하고 창을 닫은 뒤 영역 선택을 시작한다. 저장 오류가 있거나 편집기가 열려 있으면 창을 유지한다. 트레이에서 설정 창을 다시 열 수 있다. 캡처 설치와 단축키는 [영역 캡처 문서](capture.ko.md)를 따른다.

클립보드 페이지에서는 기록 수집을 켜고 끄거나 기록 창을 연다. 기본값은 꺼짐이다. `--page paste`로 이 페이지를 바로 열 수 있다. 텍스트·이미지 검색과 고정은 별도 기록 창에서 제공한다. `--paste-data-dir /absolute/path`로 기록 경로를 지정하며, `--app-config-dir`만 지정하면 그 아래 `paste/`를 사용해 개발 기록을 분리한다. 자세한 동작과 Meta+V 설치는 [클립보드 문서](paste.ko.md)를 따른다.

## 실행

Linux의 로그인된 Plasma Wayland 또는 X11 세션에서 저장소 루트를 작업 디렉터리로 사용한다. Rust/Cargo 1.99.1, fontconfig, Noto CJK가 필요하다. Debian 개발 VM에는 `sudo apt-get install fonts-noto-cjk`로 글꼴을 설치했다. 기존 에디터가 사용하는 Wayland·xkbcommon 개발 의존성도 필요하다.

```bash
cargo build --workspace --locked
./target/debug/waytoys-settings

# 실제 사용자 설정과 분리한 개발 실행
./target/debug/waytoys-settings --app-config-dir /tmp/waytoys-demo/app --config-dir /tmp/waytoys-demo/zones

# 에디터 실행 파일이 같은 디렉터리에 없을 때
./target/debug/waytoys-zones-settings --config-dir /tmp/waytoys-settings-demo \
  --editor /absolute/path/waytoys-zones-editor

./target/debug/waytoys-zones-editor --list-outputs --json
```

Iced는 `=0.14.0`, `default-features = false`, `tiny-skia`, `wayland`, `x11`, `thread-pool`을 활성화한다. [Iced 기능 문서](https://docs.rs/crate/iced/0.14.0/features). 글꼴 파일을 프로그램 안에 복제하지 않고 fontconfig에서 찾은 Noto 폰트를 로드한다. 창 관리자 제목은 `Waytoys`이며 본문은 한국어다.

Windows PowerShell의 작업 폴더 `D:\Github\utils\way-toys`에서 개발 VM으로 전송하고 실행하는 경로:

```powershell
wsl.exe -d Ubuntu-24.04 -e rsync -rt --exclude=.git/ --exclude=target/ --exclude=.artifacts/ -e /mnt/c/Windows/System32/OpenSSH/ssh.exe /mnt/d/Github/utils/way-toys/ debian@192.168.50.125:/home/debian/projects/way-toys/
ssh debian@192.168.50.125 '. ~/.cargo/env; cd ~/projects/way-toys; cargo build --workspace --locked'

# 기존 로그인 세션에 실행: 같은 unit이 이미 열려 있으면 다시 시작하지 않는다.
.\scripts\zones-settings-vm.ps1 -Action Start -Unit waytoys-settings-demo -ConfigDir /home/debian/waytoys-test-results/settings-demo/config

# 실제 VM 캡처를 로컬 파일로 가져오기
.\scripts\zones-settings-vm.ps1 -Action Capture -OutputDirectory .artifacts\settings-demo-capture

# 전체 테스트, fmt, Clippy, 기능 트리, 출력 JSON 확인
.\scripts\zones-settings-vm.ps1 -Action Check -OutputDirectory .artifacts\settings-demo-check
```

창의 닫기 또는 창 관리자 닫기는 마지막 유효값을 즉시 저장한다. 저장 실패 시 창을 유지하고 재시도/파일 다시 읽기/저장하지 않고 닫기를 제공한다. 설정 창에서 시작한 편집기가 열려 있으면 편집기를 먼저 닫도록 안내한다. 비정상 종료 시 소유한 자식 프로세스는 Drop 경로에서 정리하며, SIGKILL 같은 강제 종료는 이 경로를 보장하지 않는다. 개발 unit을 강제로 정리하려면 `ssh debian@192.168.50.125 'systemctl --user stop waytoys-settings-demo'`를 사용한다.

X11에서는 드래그 창 투명화 체크박스를 비활성화하고 Wayland 적용 안내를 표시한다. 저장된 옵션 값은 유지하며 다른 설정은 같은 모델을 사용한다. 세션별 의존성과 실제 검증 범위는 [X11 안내](x11.ko.md)에 있다.

## 옵션과 적용 범위

| 범주 | 입력 | 현재 적용 |
|---|---|---|
| 일반·선택 | 편집기 화면 기준, 모든 모니터에 영역 표시, 겹침 선택 4종 | 겹침 판정과 모든 모니터 표시는 실제 드래그에 적용. 편집기 자동 화면 선택만 연결 대기 |
| 영역 모양 | 번호, 불투명도, 강조·비활성·테두리·번호 색 | 기존 전체 화면 표시/에디터와 실제 드래그에 적용 |
| 레이아웃 이름 표시 | 표시 여부, 위치 7종, 글자·배경 색, 크기, 여백, 시간 | 전체 화면에서 표시하고 지정 시간 후 숨김 |
| 창 동작 | 드래그 투명화, 이동 기준, 모니터 간 이동, 전환 시 영역 표시 | 투명화는 실제 드래그에 적용. 키보드 이동·빠른 전환 옵션은 연결 대기 |
| 제외 앱 | 한 줄에 앱 ID 하나 | 앱 ID/WM_CLASS를 대소문자 구분해 일치하는 앱을 제외 |

아직 연결하지 않은 항목에는 **연동 준비 중 · 현재 창 동작에는 미적용**을 표시한다. 위쪽 모니터 선택은 이번 에디터 실행에 명시적으로 전달된다. 포인터/활성 창에 따른 자동 실행 화면 정책은 아직 연결하지 않았다. 모니터 새로고침은 에디터의 JSON 목록 인터페이스를 다시 호출하며 5초 제한을 둔다.

Shift 드래그와 Ctrl 다중 영역 선택은 고정 동작이며 별도 스위치를 만들지 않는다. Windows 전용 항목, 보조 버튼·휠, 여러 모니터를 가로지르는 영역, 자동 배치·기억·복원, 특수 창 옵션은 표시하지 않는다. 전역 단축키는 KDE 관리 예정이라는 안내만 제공한다. **Zones 사용**과 **로그인 실행**은 별도 일반 설정 파일에 저장한다. 실제 배치·트레이·자동 실행 방법은 [KWin 연결 문서](zones-runtime.ko.md)를 따른다.

## 저장과 오류 처리

기본 디렉터리는 `$XDG_CONFIG_HOME/waytoys/zones`, XDG가 없으면 `$HOME/.config/waytoys/zones`다. 상대 XDG/HOME은 오류로 처리한다. 공통 설정은 그 아래 `preferences.json` 한 파일이며 `--config-dir`가 설정 창과 에디터에 동일하게 전달된다.

`ZonesPreferences`는 승인된 21개 공통 필드만 직렬화한다. 초기값은 기존 `ZonesSettings::default()`의 해당 값과 테스트로 대조한다. 기존 원본 대응 `ZonesSettings`, `outputs/output-*.json` 및 레이아웃 구조·간격·치수는 자동 변환하거나 덮어쓰지 않는다.

- 유효한 입력은 즉시 설정 모델에 적용하고, 마지막 유효 변경 후 200ms가 지나면 저장한다. 스케줄링 오차는 있을 수 있다.
- 비어 있는 숫자, 범위 밖 숫자, 미완성 HEX는 입력 버퍼에만 남기고 마지막 유효값을 유지한다. 다른 유효한 필드의 저장은 계속 가능하다.
- 저장 전 검증, 같은 디렉터리 임시 파일, 파일 fsync, rename, 디렉터리 fsync를 사용한다. Linux 파일 시스템을 검증 대상으로 한다.
- 기존 파일이 손상되었거나 외부에서 바뀌면 저장을 거절한다. 잘못된 파일을 기본값으로 자동 복구하지 않는다. 파일을 직접 복구한 뒤 다시 읽는다. 다시 읽기는 메모리의 미저장 변경을 대신해 파일 내용을 사용한다.
- 설정 창은 일반 설정 디렉터리, 에디터는 Zones 디렉터리에 단일 인스턴스 잠금을 사용한다. 실행 중인 설정 창은 D-Bus로 활성화한다. 남은 `.lock` 파일 자체는 실행 중이라는 뜻이 아니다. 커널 잠금은 프로세스 종료 후 해제된다.
- 에디터는 500ms마다 검사하고 외형 설정이 바뀐 경우에만 다시 그린다. 손상된 외부 변경은 마지막 유효값을 유지한다. 창 동작/제외 앱 같은 정책 변경은 에디터 프레임이나 이름 표시 시간을 갱신하지 않는다.

## 검증

[구현·입력 검증 21회차](experiments/2026-10-31-21.md)와 [설정 검증 22회차](experiments/2026-10-31-22.md)에 실행 명령, 원격 입력 JSON, 설정 파일, 화면, 실패와 수정, 측정 결과를 기록했다. 통합 설정과 실제 창 배치 연결은 [최종 검증 24회차](experiments/2026-10-31-24.md)를 참조한다. 검증 환경은 VM 700의 Debian 13.7 / KWin 6.3.6 Wayland, Virtual-1 848×480·100%, Virtual-2 1280×720·115%다. 이 결과는 해당 환경의 디버그 빌드에 한정한다.

설정 창의 예시 미리보기 제거와 기존 편집기 실행·스크롤 재확인은 [25회차](experiments/2026-10-31-25.md)에 기록했다.
