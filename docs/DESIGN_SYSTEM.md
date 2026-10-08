# AUDENIQ 디자인 시스템

스튜디오(아티스트 화면)·관리자·업무 화면이 함께 쓰는 **토큰**과 **공통 컴포넌트**입니다.
같은 역할의 요소는 화면이 달라도 이 한 벌만 씁니다. 미리보기: 관리자 `/admin/design`.

| 위치 | 내용 |
|---|---|
| `web/studio/app/src/styles/tokens.css` | 색·글자·모서리·높이·간격·그림자·움직임 토큰 (단 하나의 기준) |
| `web/studio/app/src/styles/ui.css` | 공통 컴포넌트 클래스 `ui-*` |
| `web/studio/app/src/ui/` | React 컴포넌트 (`import { Button, Badge, … } from '../ui'`) |
| `web/studio/app/src/admin/pages/DesignSystem.tsx` | 살아 있는 견본 화면 |

## 규칙
1. 화면 CSS는 **배치(폭·여백·정렬·그리드)** 만 바꿉니다. 색·크기·모서리는 컴포넌트와 토큰이 정합니다.
2. 숫자 대신 토큰: `var(--aq-text-3)`, `var(--aq-r-xl)`, `var(--aq-h-sm)` …
3. 누르는 요소 높이는 8px 단위 4단계(32·40·48·56). 모서리는 높이에 비례.
4. 윤곽선·그림자 없이 면 색으로 구분합니다. 호버는 면 색만 바뀝니다.
5. 색 규칙: 보완 필요 = 주황, 거절·오류 = 빨강, 검토 중 = 보라, 완료 = 초록.
6. 버튼은 놓인 면에 맞춰 스스로 바뀝니다 (`.ui-card` 위에서는 한 단계 진하게, `.ui-dark` 위에서는 반전).
7. 새 역할이 생기면 화면 CSS에 만들지 말고 `ui.css` + `src/ui`에 더하고 견본 화면에 올립니다.

## 컴포넌트
| 역할 | React | 클래스 |
|---|---|---|
| 버튼 | `Button`, `buttonClass()` | `ui-btn` + `is-primary·is-neutral·is-ghost·is-white·is-warn·is-warn-solid·is-danger·is-danger-solid·is-text` / `is-xs·is-sm·is-lg·is-block·is-icon·is-busy` |
| 상태 배지 | `Badge` | `ui-badge is-{blue·violet·amber·green·red·gray}` |
| 정보 태그 · 숫자 | `Tag`, `Count` | `ui-tag(is-mono)`, `ui-count(is-hot·is-alert)` |
| 필터 칩 | `ChipGroup` | `ui-chips` / `ui-chip[aria-pressed]` |
| 고르기 토글 | `Segmented` | `ui-seg` (흰 알약) |
| 탭 | `Tabs`, `LinkTabs` | `ui-tabs` / `ui-tab` |
| 화면 머리 · 섹션 | `PageHeader`, `Section`, `SectionHeader` | `ui-page-head`, `ui-section-head` |
| 면 | `Card` | `ui-card`, `ui-box`, `ui-hero` |
| 숫자 칸 | `StatTile`, `StatGrid` | `ui-stat`, `ui-stats` |
| 목록 줄 | — | `ui-list(is-inset)` / `ui-row` |
| 아바타 · 커버 · 아이콘 칸 | `Avatar`, `Cover`, `IconTile` | `ui-avatar`, `ui-cover`, `ui-icon` |
| 입력칸 | `Field` | `ui-input`, `ui-select`, `ui-textarea`, `ui-field` (`.ui-form-lg` 안은 56px) |
| 선택 줄 · 체크 줄 | — | `ui-choice`, `ui-check(is-plain)` |
| 알림 상자 · 빈 상태 · 로딩 | `Callout`, `EmptyState`, `SkeletonList` | `ui-callout`, `ui-empty`, `ui-skel-rows` |
| 타임라인 · 항목/값 | `KeyValue` | `ui-timeline`, `ui-kv` |

## 이번에 하나로 합친 것 (요약)
버튼 5벌 → 1, 상태 배지 4벌 → 1, 필터 칩 7벌 → 1, 고르기 토글 2벌(흰/파란 알약) → 흰 알약, 탭 2벌 → 미끄러지는 밑줄,
화면 머리·섹션 머리·카드·숫자 칸·빈 상태·알림 상자·타임라인·체크 줄 각 2~4벌 → 1, 스튜디오 목록 카드 6벌 + 관리자 목록 줄 → `ui-row`,
입력칸 바탕 3색·포커스 3가지 → 1, 앨범 커버 2벌 → 발매별 그라데이션, 앱 상단 높이·로고·제품 표시 위치 통일,
고유 색 587개 → 토큰 47개, 모서리 30여 가지 → 7+4단계. 자세한 비교와 이유는 `/admin/design` › 통일 기록.

함께 고친 버그: 보드 화면 가로 스크롤, 플랫폼별 조건 카드 글자 겹침, 리포트 금액 세로 줄바꿈, 선택 상자 글자와 화살표 겹침, 팀 대화 본문 글자 크기.

## 다른 앱
- `web/admin`(독립 관리자 콘솔)은 `tokens.css`·`ui.css`·`src/ui`를 같은 내용으로 복사해 씁니다. 스튜디오 쪽을 바꾸면 같이 맞춰 주세요.
- 랜딩·설문은 같은 브랜드 토큰 값(파랑 #3B63F3, 잉크 #171B29)을 씁니다.
