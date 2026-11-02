# FancyZones 원본 설정 UI 목록

기준: PowerToys `8827f848fbbac4afeedcae2907c71cd05fd5aa86`. 원본 화면의 항목을 골라 Waytoys에 가져오기 위한 조사 목록이다. 한국어 명칭은 영문 리소스의 의미를 요약한 것이며 공식 한국어 번역이 아니다. 아래에 현재까지의 선별 결정을 기록하고 원본 43개 목록은 비교용으로 보존한다.

## Waytoys 1차 범위 선별 — 2026-10-31

사용자가 구현 난도 검토 뒤 지정한 범위다. 설정 UI와 실제 KWin 동작의 구현 완료를 뜻하지 않는다.

| 상태 | 항목 | 원본 대응·범위 |
|---|---|---|
| 핵심으로 유지 | 다른 앱 창을 끌면서 Shift로 영역 표시·배치 | M04의 동작. 정상 드롭과 Esc 취소를 함께 검증해야 함. Shift 없는 자동 활성화 모드를 추가한다는 결정은 아님 |
| 제외 | 보조·가운데 마우스 버튼 조작 | M05·M06. 버튼으로 활성화 전환하거나 여러 영역 선택을 전환하는 동작을 추가하지 않음 |
| 제외 | 드래그 중 휠로 레이아웃 전환 | M07 |
| 제외 | 배율이 다른 모니터를 가로지르는 하나의 영역 | M09와 관련된 확장 범위. 서로 다른 배율의 모니터를 각각 독립적으로 사용하는 것은 유지 |
| 제외 | 자동 배치·마지막 영역 기억 | M25–M28. 해상도/작업 영역·레이아웃 변경 시 기존 창 자동 재배치와 새 창의 마지막 영역/활성 모니터 자동 배치를 1차 범위에서 제외 |
| 제외 | 배치 해제 시 원래 창 크기 복원 | M29. Waytoys가 배치 전 크기를 기억했다가 배치 해제 후 복원하는 기능은 1차 범위에서 제외. 진행 중 드래그의 Esc 취소는 별도로 유지 |
| 제외 | 자식·메뉴·팝업 등 특수 창 배치 | M31 관련. 우선 일반 최상위 앱 창 대상 |

기존 선별 방향은 Windows 전용 설정(M32의 Windows 11 모서리 제어, M38의 Windows Snap 대체 등)을 제외하고 전역 키 지정은 KDE에 맡기는 것이다. 전역 동작의 기본 키는 미지정으로 두며 별도 키 입력 UI를 만들지 않는다. 키 할당 여부와 동작별 키보드 호출 활성화를 연결하되, Zones 전체의 마우스 동작까지 키 할당에 종속시키지는 않는 구성을 권고했다.

원래 크기 복원(M29)도 후속 사용자 결정으로 1차 제외에 추가했다. 드래그 창 투명화(M30), Ctrl 기반 다중 영역 선택 등 아직 결정하지 않은 항목은 후보로 남긴다. 같은 배율의 모니터를 가로지르는 영역도 이번에 새로 채택하지 않았다. 나머지 설정 전체가 자동 채택됐다는 뜻은 아니다.

현재 Rust 원본 대응 모델과 JSON 데이터는 변경하지 않았다. 이후 제품 설정 UI/실행 경로에서 확정 범위를 적용하며 원본 데이터 호환성과 범위 밖 기능을 구분한다.

## 메인 설정 화면: 43개

원본 `FancyZonesPage.xaml`의 TwoWay 바인딩 43개와 고유 속성 43개를 확인했다. PowerToys 전체 설정에 저장되는 기능 활성화 1개와 FancyZones 모듈 설정 42개다. 조건부 표시 항목도 포함하며, 편집기 열기 버튼·모양 미리보기·도움말 링크는 개수에서 제외했다.

원본 근거: [설정 화면 XAML](https://github.com/microsoft/PowerToys/blob/8827f848fbbac4afeedcae2907c71cd05fd5aa86/src/settings-ui/Settings.UI/SettingsXAML/Views/FancyZonesPage.xaml), [ViewModel](https://github.com/microsoft/PowerToys/blob/8827f848fbbac4afeedcae2907c71cd05fd5aa86/src/settings-ui/Settings.UI/ViewModels/FancyZonesViewModel.cs), [영문 리소스](https://github.com/microsoft/PowerToys/blob/8827f848fbbac4afeedcae2907c71cd05fd5aa86/src/settings-ui/Settings.UI/Strings/en-us/Resources.resw). 아래 행 번호는 이 커밋의 XAML 기준이다.

### 기본·편집기 진입 (3개, 18–54행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M01 | FancyZones 사용 | 켜기/끄기. 관리 정책이 있으면 변경 제한 | IsEnabled |
| M02 | 편집기 실행 단축키 | 키 조합 지정 | EditorHotkey |
| M03 | 편집기를 띄울 화면 | 활성 창이 있는 화면 / 포인터가 있는 화면 | UseCursorPosEditorStartupScreen |

별도로 **레이아웃 편집기 열기** 실행 버튼이 있다. 기능을 끄면 나머지 설정 그룹은 비활성화된다.

### 영역 활성화·선택 (7개, 58–93행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M04 | 드래그 중 Shift로 영역 활성화 | 켜기/끄기 | ShiftDrag |
| M05 | 기본 버튼 이외의 마우스 버튼으로 활성화 전환 | 켜기/끄기 | MouseSwitch |
| M06 | 가운데 클릭으로 여러 영역에 걸친 배치 전환 | 켜기/끄기 | MouseMiddleClickSpanningMultipleZones |
| M07 | 드래그 중 휠로 레이아웃 전환 | 켜기/끄기 | MouseWheelLayoutSwitch |
| M08 | 드래그 중 모든 모니터에 영역 표시 | 켜기/끄기 | ShowOnAllMonitors |
| M09 | 여러 모니터에 걸친 영역 허용 | 원본은 동일 DPI 배율 요구 | SpanZonesAcrossMonitors |
| M10 | 겹친 영역의 선택 방식 | 가장 작은 영역 / 가장 큰 영역 / 겹친 부분을 활성화 대상으로 나눔 / 중심이 포인터에 가장 가까운 영역 | OverlappingZonesAlgorithmIndex |

M10은 네 가지다. ViewModel의 오래된 내부 enum에는 세 이름만 남아 있지만, 실제 XAML 선택지와 런타임 `Settings.h`에는 ClosestCenter가 네 번째로 존재한다.

### 영역 모양 (7개, 96–154행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M11 | 색상 방식 | 사용자 지정 / Windows 기본 | SystemTheme |
| M12 | 영역 번호 표시 | 켜기/끄기 | ShowZoneNumber |
| M13 | 불투명도 | 0–100% | HighlightOpacity |
| M14 | 강조 영역 색 | 사용자 지정 색상일 때 표시 | ZoneHighlightColor |
| M15 | 비활성 영역 색 | 사용자 지정 색상일 때 표시 | ZoneInActiveColor |
| M16 | 테두리 색 | 사용자 지정 색상일 때 표시 | ZoneBorderColor |
| M17 | 번호 색 | 사용자 지정 색상일 때 표시 | ZoneNumberColor |

현재 설정을 반영하는 작은 미리보기가 있다. M17에는 번호 표시 여부에 따른 추가 숨김 조건은 없다.

### 레이아웃 이름 표시기 (7개, 156–223행)

창을 끌며 레이아웃을 바꿀 때 잠깐 보여주는 이름 표시기다.

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M18 | 이름 표시기 사용 | 켜기/끄기 | LayoutNameLabelEnabled |
| M19 | 표시 위치 | 위/아래의 왼쪽·중앙·오른쪽, 화면 중앙: 총 7곳 | LayoutNameLabelPlacement |
| M20 | 글자 색 | 색상 선택 | LayoutNameLabelTextColor |
| M21 | 배경 색 | 색상 선택 | LayoutNameLabelBackgroundColor |
| M22 | 글자 크기 | 8–128 | LayoutNameLabelFontSize |
| M23 | 여백 | 0–64 | LayoutNameLabelPadding |
| M24 | 표시 시간 | 300–10,000ms, 증감 100ms | LayoutNameLabelDuration |

M19–24는 M18을 켰을 때 조작 가능하다.

### 창 배치 동작 (8개, 227–261행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M25 | 해상도·작업 영역 변경 시 창을 기존 영역에 유지 | 켜기/끄기 | DisplayOrWorkAreaChangeMoveWindows |
| M26 | 레이아웃 변경 시 창을 새 영역 위치·크기에 맞춤 | 켜기/끄기 | ZoneSetChangeMoveWindows |
| M27 | 새 창을 해당 앱이 마지막 사용한 영역에 배치 | 켜기/끄기 | AppLastZoneMoveWindows |
| M28 | 새 창을 활성 모니터로 이동 | 원문에 Experimental 표시 | OpenWindowOnActiveMonitor |
| M29 | 배치 해제 시 원래 창 크기 복원 | 켜기/끄기 | RestoreSize |
| M30 | 드래그 중인 창을 투명하게 표시 | 켜기/끄기 | MakeDraggedWindowsTransparent |
| M31 | 자식 창의 영역 배치 허용 | 켜기/끄기 | AllowChildWindowSnap |
| M32 | 배치된 창의 둥근 모서리 끄기 | Windows 11에서만 표시 | DisableRoundCornersOnWindowSnap |

### 같은 영역의 창 전환 (3개, 263–288행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M33 | 같은 영역에 있는 창 사이 전환 | 켜기/끄기 | WindowSwitching |
| M34 | 다음 창 단축키 | M33에 종속 | NextTabHotkey |
| M35 | 이전 창 단축키 | M33에 종속 | PrevTabHotkey |

### 모니터 사이 창 순환 (2개, 290–309행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M36 | 모니터 사이로 창 순환 이동 | 켜기/끄기 | MonitorRotation |
| M37 | 순환 모드 단축키 | M36에 종속. 누른 채 좌우 방향키로 방향 선택 | MonitorRotationHotkey |

### Windows Snap 단축키 대체 (3개, 311–351행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M38 | Windows Snap을 FancyZones로 대체 | Win+방향키 동작 변경 | OverrideSnapHotkeys |
| M39 | 창을 옮길 기준 | 영역 번호 순서 / 상대 위치. M38에 종속 | MoveWindowsBasedOnPosition |
| M40 | 모든 모니터의 영역 사이 이동 허용 | M38에 종속 | MoveWindowsAcrossMonitors |

원본의 번호 순서는 Win+좌우, 상대 위치는 Win+상하좌우를 사용한다. Waytoys에서는 KWin의 기존 단축키와 어떻게 공존할지 별도 설계가 필요하다.

### 빠른 레이아웃 전환 (2개, 354–372행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M41 | 빠른 레이아웃 전환 사용 | 실제 레이아웃별 숫자는 편집기에서 지정 | QuickLayoutSwitch |
| M42 | 전환 시 영역을 잠깐 표시 | M41에 종속 | FlashZonesOnQuickSwitch |

### 제외 앱 (1개, 375–398행)

| ID | 화면 항목 | 선택지·조건 | 바인딩 |
|---|---|---|---|
| M43 | 적용하지 않을 앱 목록 | 여러 줄 텍스트, 한 줄에 앱 이름 하나 | ExcludedApps |

## 별도 레이아웃 편집기

메인 동작 설정과 구분해야 한다. 원본 근거는 [MainWindow.xaml](https://github.com/microsoft/PowerToys/blob/8827f848fbbac4afeedcae2907c71cd05fd5aa86/src/modules/fancyzones/editor/FancyZonesEditor/MainWindow.xaml), [LayoutModel.cs](https://github.com/microsoft/PowerToys/blob/8827f848fbbac4afeedcae2907c71cd05fd5aa86/src/modules/fancyzones/editor/FancyZonesEditor/Models/LayoutModel.cs), [GridLayoutModel.cs](https://github.com/microsoft/PowerToys/blob/8827f848fbbac4afeedcae2907c71cd05fd5aa86/src/modules/fancyzones/editor/FancyZonesEditor/Models/GridLayoutModel.cs)다.

| ID | 항목 | 원본 UI의 값·조건 |
|---|---|---|
| E01 | 대상 모니터 | 모니터 카드 선택. 번호·해상도·배율은 표시 정보 |
| E02 | 적용할 레이아웃 | 템플릿/사용자 레이아웃 선택. 선택 시 즉시 적용 |
| E03 | 템플릿 종류 | No layout, Focus, Columns, Rows, Grid, Priority Grid |
| E04 | 사용자 레이아웃 이름 | 만들 때 입력하고 속성 팝업에서 수정 |
| E05 | 사용자 레이아웃 방식 | 만들 때 Grid 또는 Canvas 선택 |
| E06 | 템플릿 영역 개수 | 1–128, 기본 3. 사용자 레이아웃에는 개수 슬라이더 숨김 |
| E07 | 영역 간격 표시 | 기본 켜짐. Canvas/Focus에서는 숨김 |
| E08 | 영역 간격 크기 | −10–1000px, 기본 16. 간격 표시가 켜졌을 때 활성화 |
| E09 | 강조 표시 거리 | 0–1000px, 기본 20. 코드 속성 SensitivityRadius, 영문 UI는 Highlight distance |
| E10 | 사용자 레이아웃별 빠른 전환 숫자 | None 또는 0–9. Win+Ctrl+Alt+숫자로 적용. 이미 사용 중인 숫자는 제외 |
| E11 | 가로 모니터 기본 레이아웃 | 지정/기본값 복구. 복구 대상 Priority Grid |
| E12 | 세로 모니터 기본 레이아웃 | 지정/기본값 복구. 복구 대상 Rows |

개수·간격·거리의 기본값은 `FancyZonesEditorCommon/Data/LayoutDefaultSettings.cs`의 상수 기준이며, 사용자가 이미 저장한 현재 값과는 다를 수 있다. No layout은 편집 메뉴를 숨기고 개수/감지 거리를 0으로 초기화하는 예외다.

MainWindow.xaml의 근거 행은 모니터 84–120/337–347, 적용 레이아웃 208–225/264–280, 이름 453–476/810–814, 새 방식 815–889, 영역 개수 553–588, 간격 609–670, 거리 675–718, 단축키 480–519, 가로/세로 기본값 729–782다.

### 설정값과 별도로 존재하는 명령

- 새 레이아웃 만들기, 적용, 속성 편집, 영역 편집.
- 사용자 레이아웃 복제·삭제, 템플릿에서 사용자 레이아웃 만들기.
- Grid의 분할·병합·경계 이동·경계 삭제.
- Canvas의 영역 추가·삭제·이동·크기 변경.
- 속성/영역 편집의 저장·취소.

Grid 구조와 Canvas 사각형은 저장 데이터이지만 위 조작 자체를 별도 설정 스위치로 세면 안 된다. 원본 Canvas에는 X/Y/Width/Height 직접 숫자 입력 필드가 없고 마우스·키보드로 편집한다. 레이아웃별 모서리 둥글기 설정도 없다. M32는 실제 창의 모서리를 다루는 별도 메인 설정이다.

## 현재 Rust 모델과 구분

`waytoys-zones/src/settings.rs`에는 위 42개 모듈 설정의 대응 값이 있다. 데이터가 있다는 사실이 설정 UI와 KWin 동작이 구현됐다는 뜻은 아니다. 기능 활성화 M01은 원본도 공통 일반 설정 소유이며 현재 Rust Zones 모델에 없다.

Rust의 `zone_set_change_flash_zones`는 원본 내부 값으로 메인 UI 항목이 아니다. M42와 다른 값이다. Rust `editor`의 개수·간격·민감도 기본값은 편집기 측 항목이며 메인 43개에 더하면 안 된다.

Waytoys에 가져올 때는 M09의 동일 DPI 제약, M11의 Windows 색상 방식, M32의 Windows 11 모서리 제어, M38–40의 Windows Snap 단축키를 그대로 UI 계약으로 확정하지 말고 KDE에 맞게 검토해야 한다.

조사 방법과 한계는 [2026-10-31-18](experiments/2026-10-31-18.md)에 있다.
