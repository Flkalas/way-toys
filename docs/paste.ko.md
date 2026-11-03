# 클립보드 기록

**Meta(Windows 키)+V**로 기록 창을 연다. 텍스트나 이미지 항목을 누르면 클립보드에 다시 복사하고 창을 닫는다. 원래 앱에서 **Ctrl+V**로 붙여넣는다. 검색한 뒤 위·아래 방향키와 Enter로 선택할 수도 있다. Esc, 창 밖 클릭 또는 Meta+V를 다시 누르면 닫는다.

‘고정’을 누른 항목은 목록 위에 표시되며, Waytoys를 재시작하거나 최근 기록을 지워도 남는다. 삭제하려면 먼저 고정을 해제한다. 텍스트·이미지·고정 필터와 검색을 함께 사용할 수 있다. 검색은 텍스트 원문을 대상으로 하며 이미지 OCR은 하지 않는다.

## 설치와 시작

검증 환경은 Debian 13.7 / KWin 6.3.6의 Wayland 및 격리된 X11 세션이다. 로그인한 데스크톱 사용자의 터미널에서 저장소 루트를 작업 디렉터리로 사용한다. Rust 빌드 의존성에 더해 실행 시 Wayland는 `wl-clipboard`, X11은 `xclip`, Noto CJK·fontconfig, 설치 시 Python D-Bus와 `kbuildsycoca6`가 필요하다.

```bash
sudo apt-get install wl-clipboard xclip python3-dbus fonts-noto-cjk
cargo build --workspace --locked
sh scripts/install-paste.sh --shortcut
./target/debug/waytoys-settings --page paste
```

설정의 **클립보드 → 클립보드 기록 수집**을 켠다. 처음 설치할 때는 꺼져 있다. 기록 창 위쪽 체크박스로도 수집을 켜고 끌 수 있다. 수집을 꺼도 저장한 기록과 고정 항목은 유지되며 선택할 수 있다. 실행기가 켜져 있는 동안 수집한다. 다음 로그인부터 자동으로 수집하려면 일반 설정의 **로그인 실행**도 켠다.

설정 창과 실행 파일은 같은 빌드를 사용한다. release 빌드로 바꾸면 사용자 desktop entry의 실행 경로도 갱신한다.

```bash
cargo build --workspace --release --locked
sh scripts/install-paste.sh --binary "$PWD/target/release/waytoys-paste" --shortcut
```

단축키 외에도 트레이의 **클립보드 기록**, 설정 페이지의 **클립보드 기록 열기**, `./target/debug/waytoys --paste`로 연다. `waytoys-paste`를 직접 실행하면 필요한 경우 실행기를 시작한다. `waytoys-paste --status`는 현재 기록 페이지의 JSON을 출력하므로 내용이 포함된 출력물을 공개 로그에 남기지 않는다.

Windows의 PVE 웹 콘솔은 호스트 Windows가 Meta+V를 먼저 처리할 수 있다. 그 경우 VM의 설정 버튼이나 트레이를 사용한다. VM에 직접 보내는 QMP 키 입력은 검증했으며, Windows 브라우저를 통한 Meta+V 전달은 이번에 검증하지 않았다.

## 단축키 충돌과 복구

설치 스크립트는 `~/.local/share/applications/org.waytoys.Paste.desktop`을 만들고 **Meta+V만** Waytoys에 할당한다. KDE Plasma의 기존 `show-on-mouse-pos` 동작은 백업 후 해당 키만 해제한다. 다른 앱이 쓰고 있으면 덮어쓰지 않고 오류를 낸다. Meta+Shift+S와 PrintScreen은 건드리지 않는다. `--shortcut` 없이 설치하면 키 할당은 바뀌지 않는다.

백업은 `$XDG_STATE_HOME/waytoys/paste-shortcut.json`(기본 `~/.local/state/waytoys/`)에 있다. 이전 할당으로 되돌리려면 다음을 실행한다.

```bash
sh scripts/install-paste.sh --restore-shortcut
```

설치 후 다른 프로그램이나 사용자가 해당 할당을 바꿨으면 복구도 거부하고 현재 값을 보존한다. 복구가 성공하면 백업 파일을 보관용 이름으로 옮기며 desktop entry는 남긴다. KDE의 기존 기록을 가져오거나 삭제하지 않는다.

## 저장과 한도

기본 경로는 `$XDG_DATA_HOME/waytoys/paste/history.sqlite3`이며 XDG 값이 없으면 `~/.local/share/waytoys/paste/history.sqlite3`다. `--paste-data-dir`로 실행기·설정 창에 별도 절대 경로를 전달한다. `--app-config-dir`을 명시하고 기록 경로를 생략하면 `<app-config-dir>/paste`에 저장해 개발 기록을 분리한다.

```bash
./target/debug/waytoys-settings --page paste \
  --app-config-dir /tmp/waytoys-demo/app \
  --config-dir /tmp/waytoys-demo/zones \
  --paste-data-dir /tmp/waytoys-demo/history
```

디렉터리는 0700, DB 파일은 0600이다. SQLite 트랜잭션에 원문·이미지·축소 이미지·고정 상태·순서를 함께 저장한다. 지원하지 않는 버전이나 손상된 DB는 원본을 보존하고 오류를 표시한다. 파일을 복구한 뒤 기록 창의 **다시 읽기**를 사용한다. 일반 설정의 `enabled.paste`는 수집 여부만 저장한다. 기존 Zones 설정·레이아웃은 바꾸지 않는다.

| 종류 | 한도와 처리 |
|---|---|
| 일반 기록 | 100개 또는 원본 합계 64 MiB. 초과하면 오래된 일반 항목부터 제거 |
| 고정 기록 | 별도 100개, 원본 합계 128 MiB. 초과 시 새 고정을 거부하고 기존 항목 유지 |
| 텍스트 | UTF-8 일반 텍스트, 항목당 1 MiB. 한글·줄바꿈 보존 |
| 이미지 | PNG/JPEG, 항목당 16 MiB·24메가픽셀 제한. 원본 형식으로 다시 복사 |
| 중복 | 텍스트 바이트 또는 디코딩한 이미지 픽셀이 같으면 같은 항목으로 처리 |

HTML 서식, 파일 목록, primary selection은 기록하지 않는다. 암호 관리자가 `x-kde-passwordManagerHint` 또는 `application/x-keepassxc` 형식으로 표시한 내용도 제외한다. 일반 텍스트에서 비밀번호를 추측하는 기능은 아니다. 화면이 잠기면 수집을 멈추고 기록 창을 닫는다. 기록은 이 컴퓨터의 로컬 파일에만 저장한다.

**최근 기록 지우기**는 확인 후 고정되지 않은 항목만 지운다. 현재 시스템 클립보드까지 비우지는 않는다. 같은 내용을 다시 복사하면 새 일반 기록으로 추가할 수 있다. SQLite의 원본 데이터와 WAL을 정리하지만 파일 시스템 백업이나 다른 클립보드 관리자의 기록까지 삭제하는 기능은 아니다.

## 캡처·실행기 연결

수집을 켜 둔 상태에서 Meta+Shift+S로 캡처하면 PNG가 기록에 들어온다. 캡처 선택 중에는 기록 창을 열지 않아 포커스를 유지한다. 기록 창 표시 중에는 Zones 드래그를 잠시 멈추고 닫힌 뒤 다시 연결한다.

X11 수집기는 XFixes 소유권 변경 알림을 받고, `xclip`의 TARGETS/INCR 전송으로 내용을 읽는다. 수집 중 소유권이 바뀌면 이전 요청의 결과를 버린다. 텍스트 재복사는 X11의 `UTF8_STRING`으로 제공한다.

기록 창은 Iced 0.14.0·tiny-skia 소프트웨어 렌더링을 사용한다. 한 페이지에 최대 12개를 표시하며 화면에 필요한 축소 이미지만 가져온다. 창을 닫으면 해당 UI 프로세스와 캐시가 종료되고 수집기만 남는다. 실행기를 종료하면 수집기도 종료한다. 마지막으로 선택한 클립보드 데이터를 제공하는 Wayland의 `wl-copy` 또는 X11의 `xclip`은 별도 사용자 scope에서 다음 복사까지 남아 붙여넣기를 유지한다.

구현·실제 입력·복원·측정 및 검증 한계는 [27회차 실험일지](experiments/2026-10-31-27.md)에 기록한다.

X11 기록 창·한글/이미지 재복사·고정 복원 검증은 [2026-11-03-08](experiments/2026-11-03-08.md)에 있다. 설치와 세션 선택은 [X11 안내](x11.ko.md)를 참고한다.
