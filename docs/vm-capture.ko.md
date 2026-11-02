# VM 화면 캡처와 확인

Windows PowerShell에서 저장소 루트 기준으로 실행한다.

드래그 중이거나 포커스를 유지해야 할 때는 네이티브 파일 캡처를 사용한다. 잠금이 해제된 Plasma 세션에서 실행하며, 창을 열거나 키·마우스 입력을 보내지 않는다. 저장 후 게스트와 로컬 파일의 SHA-256을 비교한다.

```powershell
.\scripts\capture-wayland-file.ps1 -OutputDirectory .artifacts\captures\runtime-drag
```

이 경로는 `spectacle --background --fullscreen --nonotify --output`을 사용하고 `screen.png`, 출력 구성·시각·로그·해시를 저장한다. 자동으로 탭을 열지는 않는다. 기존 PNG가 있으면 다른 디렉터리를 요구한다. 잠긴 세션에서는 KWin이 캡처 요청을 거절할 수 있다. 실제 드래그 상태의 검증은 [24회차](experiments/2026-10-31-24.md)를 참조한다.

일반 창을 클립보드로 캡처하고 탭을 여는 기존 도구는 다음과 같다. 드래그 중에는 아래 도구의 Spectacle 창이 포커스와 입력 상태를 바꿀 수 있으므로 위 파일 캡처를 사용한다.

```powershell
# VM의 현재 화면을 KDE Spectacle로 캡처하고 여기의 브라우저 탭으로 연다.
.\scripts\capture-vm.ps1

# 빌드한 오버레이를 각 출력에 띄운 상태로 캡처한다.
.\scripts\capture-vm.ps1 -Overlay -Build

# VM 클립보드에 이미 복사한 PNG만 가져와 연다. 캡처 키는 보내지 않는다.
.\scripts\capture-vm.ps1 -ClipboardOnly

# 탭을 열지 않고 PNG와 로그만 저장한다.
.\scripts\capture-vm.ps1 -Overlay -NoOpen
```

기본 대상은 `debian@192.168.50.125`, PVE SSH 별칭 `INOPS-PVE`, VM 700이며, 게스트 저장소는 `/home/debian/projects/way-toys`다. 주소가 바뀌거나 다른 개발 VM을 사용할 때는 인수를 지정한다.

```powershell
.\scripts\capture-vm.ps1 -Guest debian@192.168.50.125 -PveHost INOPS-PVE -VmId 700 `
    -GuestRepo /home/debian/projects/way-toys -Overlay -Build
```

## 수행 내용

1. 기존 실제 Plasma Wayland 세션에서 Spectacle의 전체 화면 캡처 창을 연다. `-Overlay`를 지정하면 먼저 오버레이를 실행한다. `-Build`는 이미 VM에 동기화된 소스를 빌드하며, 소스 전송은 하지 않는다.
2. PVE를 통해 VM에 `Ctrl+C`를 보내 Spectacle의 캡처 이미지를 VM 클립보드로 복사한다. 이 동안 VM 키보드나 창 포커스를 따로 조작하지 않는다. 기존 Spectacle 창이 있으면 중단하므로 닫거나 `-ClipboardOnly`를 사용한다.
3. `wl-paste --type image/png`로 실제 클립보드 바이트를 저장하고 PNG 헤더를 검증한다. SCP로 가져온 뒤 게스트·로컬 SHA-256 일치를 확인한다.
4. `clipboard.png`, `manifest.json`, 출력 구성과 앱 로그를 `.artifacts/captures/<실행 식별자>/`에 저장한다. 이 경로는 Git에서 제외한다. `-OutputDirectory`로 다른 위치를 지정할 수 있다.
5. Orca 내장 브라우저의 새 탭으로 원본 PNG를 연다. `-NoOpen`이면 저장만 한다. 도구가 시작한 Spectacle과 오버레이만 종료한다.

Windows 클립보드는 읽거나 변경하지 않는다. 이미지를 자르거나 크기·색을 편집하지 않으며 QEMU 프레임버퍼 캡처를 사용하지 않는다. `-ClipboardOnly`는 현재 VM 클립보드의 PNG를 그대로 가져오므로, 그 이미지의 캡처 방법은 호출자가 결정한다.

## 준비

- Windows: PowerShell, `ssh`, `scp`, 탭을 열 경우 실행 중인 Orca와 CLI.
- VM: 로그인된 Plasma Wayland 세션, `kde-spectacle`, `wl-clipboard`, `kscreen`, `python3`, `systemd-run`, 공개키 SSH 인증. 잠긴 세션을 해제할 경우 비대화형 `sudo loginctl unlock-session` 권한이 필요하다.
- PVE: 해당 VM의 QEMU 모니터에 키 입력을 보낼 수 있는 SSH 권한. `-ClipboardOnly`에서는 PVE에 접속하지 않는다.

실제 도구 검증 결과: [2026-10-31-12](experiments/2026-10-31-12.md).
