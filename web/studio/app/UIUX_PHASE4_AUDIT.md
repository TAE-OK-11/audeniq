# AUDENIQ Studio UX 가이드 대조 4차 감사

## 최상단 결정 사항 — 모두 수정하지 않음

1. **규칙 19의 빠름 기준**: 가이드는 시트 0.46/0.34s, 첫 등장 0.6s·탭 이동 0.36s·스크롤 등장 0.48s·떠오름 16px(예외)이다. 현재 1차 리빌은 360/240/280ms, 2차 시트는 0.40/0.30s다. 사용자의 “더 빠르게 느껴지도록” 지시에 따른 값이다. **가이드를 새 빠름 기준으로 업데이트할지, 코드를 가이드 값으로 되돌릴지 사용자 결정 필요.** 이번에는 훅·시트·가이드를 수정하지 않았다.
2. **공통 견본과 규칙 충돌(4·7·13·15)**: `DesignSystem.tsx:223` 자체가 동그란 `.ui-choice-mark`를 보여 주고, `ui.css:166`·`enhance.css:1358`·`ChipPicker.tsx`·`AddonOptions.tsx`에 동그란 선택 표시가 남는다. `enhance.css:1136`도 라디오 점을 직접 그린다. `--aqc-*` 정의를 enhance.css에서 다시 덮어쓴다. `ui.css`의 mono Tag 및 코드·금액용 고정폭 글꼴, CAPTCHA Georgia도 Audeniq Sans 단일 규칙과 충돌한다. 견본/공통 컨트롤까지 통일할지 예외를 명시할지 결정 필요.
3. **시각 변경이 필요한 잔여(1~5·8·9·11·12·14·16·17·22)**: 직접 색·크기, 34px 칩·54px 선택 줄, 별도 상태 태그, 문자 아이콘, 단계 밖 글자, 기본 날짜 입력, 입력 묶음, 오류 면 덮어쓰기 및 브라우저별 검색 지우기 차이가 있다. 아래 근거대로 보류했다. 색·크기·배치·글꼴·컨트롤 동작 변경을 이번 최소 수정에 섞지 않았다.
4. **추가 모션 충돌(18·19)**: `.ui-tab-panel.is-from-*` 0.34s, `lib/expand.ts` 최대 380ms, `lib/motion.ts` 흔들기 380ms·별도 곡선, 화면 CSS의 직접 시간·곡선, 보완 이동의 직접 smooth scroll이 남는다. 3차 감사서의 팬 응원·저장·결제 연출 미결정 사항도 유지한다. 검증·포커스 순서와 기존 연출을 바꾸지 않았다.
5. **영어 제품 표기(10)**: `Layout.tsx`·`AuthLayout.tsx`의 STUDIO, `AdminApp.tsx`의 ADMIN은 제품명 예외인지 메뉴 라벨인지 결정 필요. 고유명사 AUDENIQ·Spotify·UPC·ISRC는 위반으로 세지 않았다.

## 범위와 방법

기준은 `docs/DESIGN_SYSTEM.md` 전체(규칙 1~23·토큰·컴포넌트 표·다른 앱 안내)다. GAP_AUDIT는 기능 개발 근거로 쓰지 않았다. 지정 체크아웃의 `web/studio/app/src` TS/TSX/CSS를 rg로 검색하고 공통 토큰·입력·배지·확인창·Picker·press·motion·expand·rowNav 및 검색에 걸린 화면을 정적으로 대조했다. CSS 로드 순서(App.tsx:15~23), 후속 덮어쓰기, 실제 컴포넌트 사용처도 확인했다. “준수”는 정적 검토에서 위반을 확인하지 못했다는 뜻이며 전 화면 실기기 보증이 아니다. 렌더링·스크린리더·axe 검사는 하지 않았다.

시작 브랜치: `codex-studio-uiux-20261028`. 기존 tracked 변경 12개와 untracked 4개를 기준으로 분리했다. 상위 경로에서 시작한 AGENTS 파일명 검색은 즉시 중단했고 다른 체크아웃 파일을 열거나 수정하지 않았다. 이후 저장소 안만 검색했다. 환경 runtime 스킬은 로컬 실행 안내 확인용으로 읽었고 네트워크 도구·AWS·라이브 서버·실제 도메인은 사용하지 않았다.

## 4차에서 바꾼 파일만

- `src/styles/ai-connect.css`: 기존 값과 정확히 같은 글자 크기·굵기·40/56px 크기·18px 모서리를 토큰으로 치환했다. 17/20/24/26px·11.5px·500 굵기 등 같은 토큰이 없는 값은 그대로 남겼다.
- `src/admin/pages/Ops.tsx`: 인증 코드 확인창과 지급 기록창의 닫기 버튼 두 곳을 ‘취소’에서 ‘닫기’로 바꿨다. 핸들러·검증·API·실행 순서는 그대로다.
- `UIUX_PHASE4_AUDIT.md`: 결정 사항·규칙별 근거·검증·미확인 범위를 기록했다.

1~3차의 모든 수정 파일과 추가 파일은 재수정하지 않았다. 공통 ui.css/tokens.css를 수정하지 않아 독립 관리자 앱의 복사본 동기화 변경도 없다.

## 규칙별 결과

| 규칙 | 결과 | 정적 근거와 남은 범위 |
|---|---|---|
| 1 | 위반-미결정 | ai-connect.css의 동일 값 일부는 토큰화했지만 `.aq-connect-tag`, `.aq-flow-note` 등 화면 CSS가 여전히 색·크기를 정한다. 공통 컴포넌트 이관은 구조 변경이라 보류. |
| 2 | 위반-수정 | ai-connect.css의 13/14/15/16/22px·600/700/800·40/56px·18px을 동일 토큰으로 치환. 전체 CSS 직접값 제거는 미완료(규칙 1·9 참조). |
| 3 | 위반-미결정 | ai-connect.css:42 칩 높이 34px, ui.css:824 선택 줄 최소 54px, ui.css:658 메뉴 CTA 최소 50px. 32/40/48/56으로 바꾸면 크기가 달라진다. |
| 4 | 위반-미결정 | 공통 입력은 면 포커스·파란 라벨 적용(ui.css:111~116). 그러나 ui-choice-mark·aq-pick-mark.is-radio는 원형, aq-connect-always는 점선, ui-group 포커스는 가이드의 blue-100 대신 field-focus-bg(blue-150)를 사용한다. |
| 5 | 위반-미결정 | ui.css:617~624 발매 상태는 회색/보라/주황/초록/빨강과 aq-status 사용. AiConnect.tsx:206의 ‘확인됨’은 별도 각진 aq-connect-tag.is-ok로 Badge 규칙과 충돌. 정보 ‘내 앱’과 위험 고지의 의미는 보존. |
| 6 | 준수 | ui.css의 ui-card/ui-dark와 ui-tone-warn/danger/success가 버튼 배경·글자·아이콘 의미 변수를 전달한다. |
| 7 | 위반-미결정 | index.html은 Audeniq Sans 커밋 고정 로드, design.css body는 font-body. ai-connect.css의 font-mono, ui.css:777 Georgia, enhance.css/admin.css의 직접 monospace는 단일 서체와 충돌. |
| 8 | 위반-미결정 | TSX 검색의 기호 대부분은 주석이며 Glyph/Check SVG 사용. ai-connect.css:54 summary::after의 실제 content ‘›’는 문자 아이콘. SVG mask로 바꾸면 모양·크기가 달라져 보류. 문장 속 →는 예외. |
| 9 | 위반-미결정 | ai-connect.css:12/14/20/44/72의 17·11.5·26·20·24px, 500 굵기 잔존. ui.css:848/877·docs.css:70의 gray 계열 글자도 의미 텍스트 토큰 5단계 밖. 동일 값만 치환하고 시각 변경은 보류. |
| 10 | 위반-미결정 | eyebrow·관리자 메뉴 묶음은 한글을 확인. STUDIO/ADMIN 제품 표기 예외 여부는 최상단 결정 사항. Spotify·UPC·ISRC 등 고유명사는 유지. |
| 11 | 위반-미결정 | App.tsx:190 SelectHost, Picker의 Date/DateTime/Time/MonthField 존재. AddonOptions.tsx:210, Ops.tsx:1047~1048의 기본 date 입력 잔존. 컨트롤 교체는 동작 변경이라 보류. |
| 12 | 위반-미결정 | PressKitManager는 fieldset.ui-group·aq-optional·하단 도움말 사용. RightsReport.tsx:73/87 fieldset은 ui-group 없음. AiConnections.tsx:176 summary에 ‘(선택)’ 잔존. 묶음 스타일/문구 맥락 변경 보류. |
| 13 | 위반-미결정 | 새 역할은 ui.css/src/ui/DesignSystem 경로가 마련돼 있으나 aq-connect-tag 등 화면 자작 역할 잔존. 규칙 1·5의 공통화와 함께 결정 필요. 4차에는 새 역할을 만들지 않음. |
| 14 | 위반-미결정 | enhance.css:482/484는 invalid 면을 포커스 중에도 유지, ui.css:1093 이후 비활성 보조 버튼 회색 처리. 하지만 design-align.css:1019의 aq-al-link-fields input[aria-invalid]가 배경 transparent·그림자 none으로 덮는다. disabled 입력은 회색 글자만 보장해 화면별 면·흐림은 미확인. |
| 15 | 위반-미결정 | enhance.css:1120 이후 aqc-box/blue/focus 변수를 다시 정의하고 radio gradient·22px 등을 직접 정함. controls.css 단일 기준과 충돌. 공통 컨트롤 시각 변경 보류. |
| 16 | 위반-미결정 | ui.css:1069 이후 모바일 공통 입력 16px·작은 버튼 터치 44px, design-align.css:738 헤더 터치 확장. 검색 지우기는 ui.css:1087 WebKit 의사요소만 구현해 Firefox 등에서는 제공 보장 안 됨. 전 화면 터치 중첩·기기 확대는 미확인. |
| 17 | 준수 | Confirm.tsx danger 실행 is-danger-solid·닫기 data-autofocus, Modal의 초기 포커스 연계 확인. rowNav의 aq-more-left/right와 design-align.css의 흐림, ui.css:826 선택 줄 말줄임·858 날짜 입력 공통 모양 존재. 실기기 긴 문자열은 미확인. |
| 18 | 위반-미결정 | PickerSurface.tsx:30~49 닫힘 복제·animationend 제거(600ms 예비 정리), expand 접힘, rowNav 방향 진입·reduced 방어 확인. Upload.tsx:1757 검사 실패는 revealField 사용하지만 :2132~2137 보완 이동은 직접 smooth scroll 후 focus/flash; 순서 변경은 보류. |
| 19 | 위반-미결정 | 1·2차 빠름 충돌을 그대로 유지. press.ts 최소70ms, ui.css:1058~1063의 :is(:active,.is-pressed)·0.04/0.16s 토큰 확인. ai-connect.css:54 직접 .15s, ui.css:1111~1112 탭 .34s, 별도 곡선·직접 시간이 남는다. 3차 미결정 연출도 유지. |
| 20 | 위반-수정 | Ops.tsx:105/908 창 닫기 ‘취소’ → ‘닫기’. Dashboard.tsx:350 이후 보완 우선 홈 분기와 Upload 선택 추가 정보 접힘 확인. AiConnect 페이지 이탈/업무 편집 ‘취소’는 창 닫기와 구분해 유지. 전 화면 판단 수의 사용성 평가는 미확인. |
| 21 | 준수 | Rights.tsx 권리·허락·위임 원문, RightsReport 책임 고지, Settlement 수수료·금액 결과 문구 유지. 4차에는 돈·권리·법적 문구를 줄이거나 순화하지 않음. |
| 22 | 위반-미결정 | 새 색 없음. tokens.css의 대비 보정 의미 색 유지. gray-300/400 글자와 opacity가 있는 작은 안내 글자는 4.5:1 의심 후보. axe-core color-contrast 실제 실행 및 상태·면별 대비는 미확인으로 남김. |
| 23 | 위반-수정 | 창 닫기 문구 두 곳 통일. 기존 lib/writing.test.ts의 금지 표현 검사 통과(법적 문서·관리자 제외). 전체 해요체·버튼 의미·캐주얼 경어의 수동 문맥 평가는 테스트만으로 보증하지 않음. |

## 고친 UX 문제

- 화면 CSS가 같은 값을 직접 관리해 토큰과 따로 변할 수 있었음 → 규칙 1·2·9 → 값이 같은 항목만 기존 토큰 참조로 바꿈. 색·배치·최종 글자 크기·굵기는 바뀌지 않음.
- 실행을 취소하는 것으로 읽힐 수 있는 창 닫기 버튼 → 규칙 20·23 → 모달 두 곳에 ‘닫기’ 사용. 지급 취소·권한 변경·법적 의미는 건드리지 않음.

## 검증

- `web/studio/app`에서 `npm run check`: 통과(tsc --noEmit, 종료 0). 첫 실행은 루트에 package.json이 없어 ENOENT였으며 올바른 app 경로에서 다시 실행했다.
- 관련 기존 테스트: `timeout 45s npm test -- src/admin/pages/Ops.test.tsx src/pages/AiConnect.test.tsx src/pages/AiApprove.test.tsx src/lib/writing.test.ts` → 4파일·24검사 통과, 15.26s.
- 전체 Vitest: `timeout 45s npm test` 한 번만 시도. RUN 뒤 결과 없이 제한 시간 종료(124). **전체 결과 미확인**, 재시도하지 않음.
- CSS 동등성: tokens.css 정의를 재귀로 풀어 변경 전후 ai-connect.css 전체 문자열 비교 → 일치. 일회성 정적 검사이며 브라우저 스크린샷 동등성 검사는 아님. 저위험 치환/문구 수정이라 구현을 복제하는 새 테스트는 추가하지 않음.
- `git diff --check`: 통과. 4차 코드 diff는 두 파일 26추가/26삭제. 선언 치환과 버튼 문구 외 CSS 속성·구조·핸들러 변경 없음. 기존 1~3차 tracked diff는 시작 스냅샷과 동일함을 비교했다.
- 최종 상태: 기존 tracked 수정12/untracked4 보존 + 4차 tracked 수정2/감사서 untracked1 = 수정14·미추적5. 가이드·정적 산출물 변경 없음. 빌드 안 함.
- Vite는 향후 native configLoader에서 확장자 없는 import를 지원하지 않을 수 있다는 경고를 출력했다. 이번 UX 범위 밖이며 테스트 통과와 구분한다.

## 미완성·미결정

최상단 항목 및 표의 위반-미결정은 보존했다. 전체 테스트·브라우저별 검색 지우기·모바일 터치/입력·실제 대비/axe·동작 줄이기 실기기·전 화면 사용성 검사는 미확인이다. 기능 격차 개발은 하지 않았다. 커밋·푸시·PR 생성·병합·라이브 배포는 하지 않았고 모든 변경은 미커밋 상태다.
