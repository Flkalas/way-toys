# Waytoys Zones

Zones가 소유하는 Rust 데이터 모델·설정·편집 로직과 단독 CLI입니다. PowerToys FancyZones의 MIT 소스를 참고해 이식했습니다. 기준 커밋은 `8827f848fbbac4afeedcae2907c71cd05fd5aa86`이며, 출처와 라이선스는 루트의 [서드파티 고지](../../THIRD_PARTY_NOTICES.md)에 있습니다.

## 기능 경계

이 크레이트가 Zones의 설정, 사용자 레이아웃, 템플릿, 편집 상태를 소유합니다. 직접 의존성은 `serde`와 `serde_json`이며 `waytoys-core`, 오버레이, 다른 부가기능, GUI 라이브러리를 요구하지 않습니다. Cargo 워크스페이스에서 Zones만 빌드할 수 있고, 생성된 실행 파일은 다른 Waytoys 프로세스 없이 실행됩니다. Cargo 메타데이터와 도구 버전은 루트 워크스페이스를 공유하므로 이 디렉터리만 복사한 별도 Cargo 프로젝트라는 의미는 아닙니다.

공통 설정 화면은 기능의 활성화와 해당 기능 설정 화면으로의 연결을 담당합니다. Zones 설정의 해석·검증·수정은 Zones에 남깁니다. 추후 공통 UI 호스트와 연결할 때도 작업 영역 정보·입력 이벤트를 받아 표시할 구역을 반환하는 경계를 유지합니다. 현재 CLI는 파일을 명시적으로 읽고 JSON을 표준 출력으로 반환하며, 자동 저장이나 호스트 연결은 구현하지 않았습니다.

## 현재 제공하는 로직

- `model`: Grid/Canvas 사용자 레이아웃, 기본값, 원본 `custom-layouts.json` 형식의 읽기·쓰기와 검증.
- `settings`: FancyZones 설정 기본값, 편집기 기본값, 단축키 의도, 겹친 구역 선택 방식. OS 동작 관련 값도 수동 데이터로 보존합니다.
- `layout`: Focus/Columns/Rows/Grid/PriorityGrid 템플릿과 사용자 레이아웃을 작업 영역 안의 사각형으로 계산.
- `grid_editor`: 분할, 직사각형 병합 범위 확장, 공유 경계 이동, 스냅샷 복구. 잘못된 편집은 상태 변경 없이 실패합니다.
- `canvas_editor`: 구역 추가·삭제·이동, 여덟 방향 크기 조절, 키보드 조작, 자석 보정, 드래그 및 편집 세션 확정·취소, undo.
- `selection`: 포인터 위치와 민감도에 따른 구역 선택, 겹침 처리, 여러 구역 선택 범위, Shift/Ctrl·마우스 버튼 상태 해석.
- `magnetic_snap`: Grid의 픽셀/비율 변환과 Canvas 자석 보정 계산.

Grid 편집 좌표는 전체 길이 `10000`의 정수 비율입니다. 분할의 `POSITION`은 구역 내부의 비율이 아니라 작업 영역 전체의 해당 축 좌표입니다. 경계의 `DELTA`도 같은 단위입니다. 편집 이후 구역 번호와 경계 번호가 바뀔 수 있으므로 다시 조회해야 합니다.

Canvas는 `ref-width`/`ref-height` 기준의 정수 좌표와 크기를 저장합니다. 계산 결과는 호출자가 지정한 작업 영역의 로컬 논리 좌표입니다. 전역 모니터 위치와 배율·DPI 변환은 호출 측 책임입니다. Canvas는 원본처럼 기준 영역 밖의 구역도 저장할 수 있습니다.

사용자 레이아웃의 JSON 필드명과 Grid/Canvas 태그는 원본 형식을 따릅니다. ID는 비어 있지 않은 불투명 문자열로 다루며 Windows GUID 처리까지 이식하지 않았습니다. 설정 JSON은 `snake_case`의 자체 형식이므로 PowerToys의 `properties`/`value` 래퍼가 있는 `settings.json`을 직접 가져오지는 않습니다. `from_json`/`to_json`이 유효성을 검사하며, 직접 Serde 역직렬화를 쓸 때는 별도로 `validate()`를 호출해야 합니다.

## 실행과 검증

저장소 루트에서 실행합니다. 아래 상대 경로는 PowerShell과 Linux 셸에서 모두 사용할 수 있습니다. 실제 빌드·실행 검증 환경은 Debian VM의 Rust 1.99.1이며 Windows 네이티브 빌드는 확인하지 않았습니다.

```text
cargo build -p waytoys-zones --locked
cargo test -p waytoys-zones --locked
cargo run -p waytoys-zones --locked -- settings
cargo run -p waytoys-zones --locked -- validate crates/waytoys-zones/examples/custom-layouts.json
cargo run -p waytoys-zones --locked -- preview crates/waytoys-zones/examples/custom-layouts.json 0 1280 720
cargo run -p waytoys-zones --locked -- edit crates/waytoys-zones/examples/custom-layouts.json 0 grid-split 1 x 5000
cargo run -p waytoys-zones --locked -- edit crates/waytoys-zones/examples/custom-layouts.json 1 canvas-move 0 10 -20
```

인덱스는 모두 0부터 시작합니다. `edit`는 변경된 문서 전체를 표준 출력에 쓰며 입력 파일은 덮어쓰지 않습니다. 파일 저장은 호출자가 맡습니다. 원본 파일을 입력으로 사용하면서 같은 파일에 셸 리다이렉션하면 셸이 먼저 파일을 비울 수 있으므로 저장할 때는 다른 출력 경로를 사용하세요. `--help`에 크기 조절·병합 등 전체 CLI 조작을 정리했습니다.

## 이식 범위와 차이

원본의 기본값·레이아웃 형식·편집 알고리즘을 참고했지만 PowerToys 전체 동작과의 동등성을 뜻하지는 않습니다. WPF 설정/편집 화면, 포인터 이벤트 연결, Grid 분할의 UI 스냅, 모니터별 저장·적용 레이아웃, 앱 배치 기록, 전역 단축키 등록, 실제 창 이동과 오버레이 연결은 포함하지 않습니다.

원본의 특성 중 Canvas 기본 구역 40%, 100/50 위치 오프셋, 64×72 크기 조절 최소값, 자석 곡선과 Grid 병합 범위 확장을 보존했습니다. PriorityGrid는 원본 코드의 1~10개 전용 패턴과 11개부터의 일반 Grid 전환을 따릅니다. Canvas의 계산은 논리 좌표에서 정수 비율로 수행하므로 Windows 런타임의 float/DPI 변환과 픽셀 단위로 동일하다고 가정하지 않습니다.

잘못된 Grid 편집은 원본의 조용한 무시 대신 오류를 반환합니다. 병합은 확장된 병합 범위의 최소 구역 ID에 삽입하도록 처리했고, 빈 구역·좌표 오버플로·불가능한 크기 조절을 거부합니다. Canvas의 조작별 undo와 드래그 묶음 취소는 이번 이식의 확장 기능입니다. 원본이 같은 undo 스택을 제공한다는 뜻은 아닙니다.

실제 검증 기록은 [2026-10-31-15](../../docs/experiments/2026-10-31-15.md)에 있습니다.
