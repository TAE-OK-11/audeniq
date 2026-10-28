# AUDENIQ Studio UI/UX 3차 감사 (2026-10-29)

범위: `web/studio/app`만. 브랜치 `codex-studio-uiux-20261028`. 1·2차의 리빌 훅 3개, revealMotion.test.tsx, design-align.css, wizard-motion.css는 이번에 수정하지 않았다. 빌드·커밋·푸시·PR·배포·외부 서비스 접속 없음.

## A. 판단과 조치

| 위치(src 기준) | 이유 | 조치 | 전 → 후 |
|---|---|---|---|
| styles/ui.css:789 `.aq-verify-pulse` | 인증 대기 화면의 반복 주의 유도 | 수정 | 1.6s 무한 글로우 → 없음 |
| styles/enhance.css:1066 `.aq-cadmin-pulse` | 긴급 상태는 색·문구로 이미 표시, 반복 글로우 불필요 | 수정 | 1.6s 무한 → 없음 |
| styles/admin.css:717 `.adm-claim.is-mine .adm-claim-who i` | 담당 상태에 반복 호흡 불필요 | 수정 | 2.4s 무한 → 없음; 기존 정적 점·그림자 유지 |
| styles/admin.css:714 `.adm-check-mark` | 팝과 지연 글로우가 중첩 | 수정 | 0.32s 등장 + 지연 0.4s/글로우 1.2s → 등장만 |
| styles/enhance.css:233 `.aq-auth-glow` | 배경 장식의 등장이 본문보다 오래 지속 | 수정 | 1s → 0.24s |
| enhance.css:40,157,177,245; ui.css:90,508; design-align.css:1308; admin.css:382,671 | 부팅·로딩·저장·갱신 상태의 진행 표시 | 유지 | 부팅 1s, 스켈레톤 1.4s, 스핀 0.7~0.9s 무한 유지; reduced-motion에서 정지 |
| ui.css:764 OTP 커서 | 입력 위치 표시 | 유지 | 1s 무한 점멸; reduced-motion 정지 |
| design-align.css:1298,1301,1338,1340 팬 응원·저장 피드백 | 등장+1.8s 글로우, 0.9s 저장 강조·하트 0.45~0.5s는 길지만 사용자 행동 피드백 의도 가능 | 유지·미결정 | 기존 값 유지; reduced-motion 정지 |
| enhance.css:659 / wizard-motion.css:18,53 | 오류 위치 강조·입력 검증; 기능상 강조 의도 가능 | 유지·미결정 | 일반 플래시 2s, 위자드 플래시 1.8s, 입력 강조 1.2s 각 1회; reduced-motion 정지 |
| enhance.css:334~425 결제 성공·오류 | 성공 그림·문구·영수증 순차 등장, 오류 등장+흔들기 | 유지·미결정 | 최대 0.45s 지연 + 0.32s 등장; 오류 0.22s 등장 + 0.38s 흔들기, reduced-motion 정지 |
| enhance.css:102,293; live.css:617; design-align.css:1768~1814 차트·준비 표시 | 값·상태를 표현하는 단발 움직임 | 유지 | 0.4~0.7s, 일부 지연/팝 유지 |
| admin.css:702~712 관리자 등장 | 숫자·본문·고정 바 등장 중첩 후보이나 서로 다른 정보 | 유지·미결정 | 0.28~0.32s, 지연 0.04~0.15s 유지 |
| controls.css:90 | 자동완성 배경을 유지하는 CSS 우회 처리 | 유지 | 100000s transition은 장식 연출이 아님 |

정지 상태의 CSS 속성은 변경하지 않고 animation 선언만 수정했다. 전역 `live.css:551`의 reduced-motion `animation:none!important; transition:none!important` 및 개별 방어 규칙을 확인했다. 이번에 제거한 연출은 모든 설정에서 꺼지며, 단축한 인증 배경도 전역 규칙으로 꺼진다. 리빌·모달·레일은 1·2차 범위로 재수정하지 않았다.

### 훅·JS 모션 검토

- `useSmoothHeight`: 320ms 높이 보간, 시작 시 reduced-motion 확인. 유지.
- `useAnimations`: 차트 grow(관찰 후 CSS), 진행 구간 280ms WAAPI(reduced-motion 확인). 유지.
- `useSegPill`: rAF는 위치 측정·활성화용, 실제 이동은 CSS transition이며 reduced-motion에서 정지. 유지.
- `ui/CountUp`: 기본 600ms 수치 보간, reduced-motion이면 최종 수치 직접 표시. 유지.
- `ui/Reveal`, `lib/expand`: 펼침·접힘, 최대 380ms, reduced-motion 확인. 유지.
- `lib/motion`: 오류 위치 이동·380ms 흔들기, reduced-motion 확인. 유지.
- `lib/rowNav`: 탭·가로 목록의 선택 위치 이동, reduced-motion 확인. 유지.
- `Layout`, `Modal`, `topCover`, `JumpNav`의 rAF는 위치 계산·보정용으로 장식 반복과 구분했다. 모달 연출은 기존 작업 범위.
- `Upload`의 명시적 smooth scroll 호출은 JS에서 매번 reduced-motion을 분기하지 않는다. CSS 전역 scroll-behavior 방어만으로 모든 브라우저의 명시적 JS smooth가 차단되는지는 미확인. 검증·포커스 호출 순서를 바꾸지 않고 남겼다.

## B·C. 확인 결과와 수정

- 준비 개수는 오류가 아님: 빈 draft에서 완료(요청 없는 권리 서류) 1, 사용자 할 일 4, AUDENIQ 대기 2 = 7. 계산을 바꾸지 않았다. 제목을 '발매 준비 항목', 설명을 '내가 할 일 4개 · 대기 2개', 접힌 항목 버튼을 '완료·대기 3개 보기'로 바꿔 단계 1/4와 단위를 구분했다.
- 음원·크레딧은 모두 같은 신청서로 연결되지만 요구사항이 다르다. 각각 음원 파일 업로드 / 작곡·작사 입력으로 설명을 구분했다. 트랙 등록 버튼은 직접 입력 경로라 유지했다.
- 빈 draft의 읽기 전용 배급·계약, 빈 관리, 빈 기록 탭을 숨겼다. URL로 숨긴 탭을 열어도 정보 패널이 즉시 표시되며 선택 상태도 보정된다.
- 수익 분배는 SplitsCard의 주석·editable 흐름상 접수 전에만 수정 가능하므로 트랙이 있으면 작성 단계 관리 탭을 유지했다. 신청서 작성 버튼과 준비 항목 입력 경로도 유지.
- 삭제는 `handleDelete`에서 danger 확인을 기다린 뒤 API를 부른다. 변경 없음.
- 권리 격자는 실제 3항목(마스터 권리자, ℗, ©)의 2열 구성이다. 홀수 항목이 다음 행에 남는 것이며 배치 버그 근거가 없어 변경 없음.
- 최근 수정 줄바꿈: 좁은 2열에서 긴 날짜의 자연스러운 줄바꿈. 강제 한 줄·축약은 레이아웃/정보 표현 변경이므로 유지.
- 별도 B 범위의 디자인·기능 확장은 하지 않았다. 공통 Tabs의 화살표/Home/End 이동 부재는 접근성 개선 후보로 남겼다(전체 탭 상호작용 정책과 관련 테스트가 필요).

## 탭·섹션 노출 판단

○ 노출, 조건은 실제 자료/하위 컴포넌트의 기존 조건을 따름. 작성 이외 상태의 기존 조건은 변경하지 않았다.

| 탭·섹션 | 작성 | 검토(ready/review/needs 포함) | 배급 승인 | 발매 완료 | 이유 |
|---|---|---|---|---|---|
| 정보: 발매 정보·앨범 소개·커버·트랙 | ○ | ○ | ○ | ○ | 작성 입력 경로·발매 자료; 커버는 이미지 있을 때만 |
| 정보: 준비 항목 | ○ | ○(needs 숨김) | ○ | 발매 3일 뒤 숨김 | 기존 보완 카드 중복·발매 후 조건 유지 |
| 정보: 제출 전 파일 안내 | ○ | needs만 | 숨김 | 숨김 | 기존 접수 전 안내 유지 |
| 성과 | 숨김 | 숨김 | 숨김 | ○(closed 포함) | 기존 리포트 조건 유지 |
| 배급·계약 탭 | 신청서/실제 문서 있을 때만 | ○ | ○ | ○ | 빈 draft는 읽기 전용 요약뿐; 예외 문서 접근 보존 |
| 배급 설정 | 숨김 | ○ | ○ | ○ | 신청서에서 입력, 상세는 읽기 전용 |
| 플랫폼 진행 | 데이터 있을 때만 | 데이터 있을 때만 | 데이터 있을 때만 | 데이터 있을 때만 | 기존 API·표시 조건 유지 |
| 권리 요약 | 탭 열림 + 권리 값 있을 때 | ○ | ○ | ○ | 모두 '입력 전'인 draft 카드 숨김; 값 있는 예외 보존 |
| 계약 묶음 | 신청서 또는 계약 문서 있을 때 | ○ | ○ | ○ | 접수 후 의미 있는 문서, 실제 문서 예외 보존 |
| 권리 서류 | 실제 권리 문서 있을 때 | 같음 | 같음 | 같음 | 요청·첨부 경로 보존 |
| 관리 탭·수익 분배 | 트랙 있을 때 | ○(카드 자체 조건) | 같음 | 같음 | 접수 전 입력이 필요; 데이터 없는 draft 탭은 숨김 |
| 관리: 사전 공개·스마트링크·피칭 | 숨김 | 기존 조건 | 기존 조건 | 기존 조건 | 작성에서는 원래 없었음; 세부 서비스 단계 의도 미결정 |
| 관리: Content ID·침해 신고·부가서비스 | 숨김 | 기존 조건 | 기존 조건 | 기존 조건 | 작성에서는 원래 없었음; 기능 조건 보존 |
| 관리: 정보 수정·내림·계약 기간 | 숨김 | 기존 조건 | 기존 조건 | 기존 조건 | 작성에서는 원래 없었음; 하위 컴포넌트 판단 유지 |
| 기록 | 이력 있을 때 | ○ | ○ | ○ | 빈 draft 안내만 있는 탭 숨김; 실제 이력 보존 |

## 검증

- `npm run check`: 통과 (app 디렉터리). 처음 저장소 루트에서 실행한 명령은 package.json 부재로 실행되지 않았고 올바른 위치에서 다시 수행했다.
- 관련 테스트: albumPrep, releaseDetailVisibility, 1차 revealMotion, CountUp: 4파일 26개 통과. 기존 준비 계산·크레딧, 새 빈 draft 개수 및 단계별 노출, 기존 리빌 10개 포함.
- 전체 Vitest: `timeout 60s npm test` 한 번 실행. RUN 이후 테스트 결과가 나오지 않아 60초에 종료(exit 124). 전체 결과는 미확인, 재시도하지 않음.
- `git diff --check`: 통과. 3차 CSS diff는 animation 값만 변경. 색·레이아웃·타이포 선언 변경 없음. C의 탭/섹션 노출과 문구만 변경. 브라우저 실제 모바일 재촬영/시각 비교는 수행하지 않아 문구 줄바꿈은 미확인.
- API 함수·요청 순서·검증 로직은 변경하지 않았다. 숨긴 탭의 컴포넌트 미마운트에 따라 해당 탭에 들어갈 때 실행되던 요청은 발생하지 않을 수 있다(C 노출 변경의 결과).
- 기존 1·2차 6개 경로 유지, 3차 10개 경로(보고서 포함) 추가/수정. 정적 산출물 변경 없음. 모든 변경 미커밋.

## 전체 소스 모션 색인

최종 소스 기준. CSS는 keyframes 정의 및 모든 animation/transition 선언(덮어쓰기·비활성 규칙 포함), TS/TSX는 WAAPI·rAF·모션 속성·smooth scroll 사용 위치를 수집했다. 테스트 파일은 연출이 아니므로 제외했다. rAF에는 위치 측정도 포함된다. 선언 전체가 현재 활성 연출이라는 뜻은 아니며 앞의 판단표 및 로드 순서와 함께 읽는다.

### src/admin/JumpNav.tsx

- L29: `const onScroll = () => { if (!raf) raf = requestAnimationFrame(update); };`
- L41: `nav.scrollTo({ left: Math.max(0, left), behavior: 'smooth' });`
- L50: `el.scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'start' });`

### src/admin/pages/Inquiries.tsx

- L49: `paneRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' });`

### src/admin/work/board/Column.tsx

- L88: `requestAnimationFrame(() => addBtn.current?.focus());`

### src/admin/work/docs/DocTree.tsx

- L112: `requestAnimationFrame(() => box.current?.querySelector<HTMLElement>('[data-id="${CSS.escape(id)}"]')?.focus());`

### src/admin/work/pages/Board.tsx

- L164: `const focusCard = (id: string) => requestAnimationFrame(() => {`
- L298: `const scrollToColumn = (id: string) => document.getElementById('kbcol-${id}')?.scrollIntoView?.({ inline: 'start', block: 'nearest', behavior: window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });`

### src/admin/work/pages/Chat.tsx

- L225: `requestAnimationFrame(() => { if (el) el.scrollTop = el.scrollHeight - before; });`

### src/admin/work/pages/Exceptions.tsx

- L223: `document.getElementById('ex-${focus}')?.scrollIntoView({ block: 'center', behavior: 'smooth' });`

### src/admin/work/ui.tsx

- L204: `requestAnimationFrame(() => { area.current?.focus(); area.current?.setSelectionRange(pos, pos); });`

### src/components/ArtistCommunity.tsx

- L34: `requestAnimationFrame(() => {`
- L36: `el.animate([{ height: '${from}px' }, { height: '${el.scrollHeight}px' }], { duration: 320, easing: 'cubic-bezier(.32,.72,0,1)' });`
- L96: `<li key={i} className="aq-cheer" style={{ animationDelay: '${i * 70}ms' }}>`

### src/components/Layout.tsx

- L155: `requestAnimationFrame(() => {`
- L204: `raf = requestAnimationFrame(again);`
- L206: `raf = requestAnimationFrame(again);`

### src/components/Modal.tsx

- L126: `const schedule = () => { if (!raf) raf = requestAnimationFrame(place); };`
- L130: `const follow = () => { place(); if (performance.now() < until) raf = requestAnimationFrame(follow); else raf = 0; };`
- L131: `const burst = () => { until = performance.now() + 600; if (!raf) raf = requestAnimationFrame(follow); };`
- L132: `raf = requestAnimationFrame(follow);`
- L233: `const onEnd = (e: AnimationEvent) => { if (e.target === inner && /out|down/.test(e.animationName)) finish(); };`
- L328: `el.style.animation = 'none';`

### src/components/PaymentSetupModal.tsx

- L128: `const frame = requestAnimationFrame(reset);`

### src/components/PickerSurface.tsx

- L109: `el.style.animation = 'none';`
- L119: `el.style.animation = '';`

### src/components/PublicPage.tsx

- L43: `{[0, 1, 2].map(i => <span key={i} className="aq-skel aq-pub-loading-row" style={{ animationDelay: '${i * 70}ms' }} />)}`

### src/components/Skeleton.tsx

- L6: `<div key={i} className="aq-skeleton-row" style={{ animationDelay: '${i * 70}ms' }}>`

### src/hooks/useAnimations.ts

- L59: `newSpan.animate(`

### src/hooks/useLandingReveal.ts

- L64: `const a = el.animate(first ? motion.keyframes : SCROLL_RISE, { duration: first ? motion.ms : SCROLL_MS, easing: EASING, fill: 'backwards', delay: delays.get(el) ?? 0 });`
- L77: `if (cssEntrance.has(el)) el.style.animation = 'none';`

### src/hooks/usePublicReveal.ts

- L38: `const a = el.animate(KEYFRAMES, { duration, easing: EASING, fill: 'backwards', delay });`
- L56: `el.style.animation = 'none';`
- L144: `touched.forEach(el => { el.style.animation = ''; });`

### src/hooks/useScrollReveal.ts

- L52: `return cs.animationName !== 'none' && !cs.animationIterationCount.split(',').some(v => v.trim() === 'infinite');`
- L74: `const a = el.animate(m.first ? entry.keyframes : SCROLL_RISE, { duration: m.first ? entry.ms : SCROLL_MS, easing: EASING, fill: 'backwards', delay: m.delay });`
- L103: `if (cssEntrance[i]) el.style.animation = 'none';`
- L138: `view.querySelectorAll<HTMLElement>('.ui-tab-panel:not(.is-from-right):not(.is-from-left)').forEach(el => { el.style.animation = 'none'; });`

### src/hooks/useSegPill.ts

- L30: `raf = requestAnimationFrame(() => { setLive(true); if (remembered) measure(); });`

### src/hooks/useSmoothHeight.ts

- L20: `const anim = el.animate([{ height: '${from}px' }, { height: '${h}px' }], { duration: 320, easing: 'cubic-bezier(.32,.72,0,1)' });`

### src/lib/expand.ts

- L54: `prev.animate([{ marginBottom: opening ? shut : mb }, { marginBottom: opening ? mb : shut }], timing);`
- L71: `const anim = el.animate([`
- L98: `const anim = ghost.animate([`
- L169: `window.scrollTo({ top: target, behavior: reduced() ? 'instant' as ScrollBehavior : 'smooth' });`

### src/lib/motion.ts

- L33: `(el as HTMLElement).animate(`
- L48: `el.scrollIntoView({ behavior: 'smooth', block: 'center' });`

### src/lib/printStamp.tsx

- L58: `const nextFrame = () => new Promise<void>(r => requestAnimationFrame(() => requestAnimationFrame(() => r())));`

### src/lib/rowNav.ts

- L31: `box.scrollTo({ left: box.scrollLeft + dx, behavior: smooth && !reduce ? 'smooth' : 'auto' });`
- L75: `if (t) requestAnimationFrame(() => { if (t.isConnected) revealInRow(t); });`
- L101: `pending = requestAnimationFrame(() => { pending = 0; revealSelected(document); updateFades(document); });`
- L128: `requestAnimationFrame(() => {`
- L134: `window.scrollTo({ top: window.scrollY + top - head - 8, behavior: reduce ? 'auto' : 'smooth' });`

### src/lib/topCover.ts

- L21: `const schedule = () => { if (!raf) raf = requestAnimationFrame(place); };`

### src/pages/AiAction.tsx

- L105: `window.scrollTo({ top: 0, behavior: 'smooth' });`

### src/pages/AiApprove.tsx

- L101: `window.scrollTo({ top: 0, behavior: 'smooth' });`

### src/pages/ArtistNews.tsx

- L17: `{Array.from({ length: rows }, (_, i) => <span key={i} className="aq-skel" style={{ animationDelay: '${i * 70}ms' }} />)}`

### src/pages/Notices.tsx

- L95: `document.getElementById('aqNoticeAll')?.scrollIntoView({ behavior: 'smooth', block: 'start' });`

### src/pages/Sign.tsx

- L94: `window.scrollTo({ top: 0, behavior: 'smooth' });`

### src/pages/Terms.tsx

- L31: `<select id="aqTermsJump" key={doc} defaultValue="" onChange={e => { document.getElementById(e.target.value)?.scrollIntoView({ behavior: 'smooth', block: 'start' }); }}>`

### src/pages/Upload.tsx

- L1752: `requestAnimationFrame(() => {`
- L1768: `document.getElementById('wizardError')?.scrollIntoView({ behavior: 'smooth', block: 'center' });`
- L1978: `requestAnimationFrame(() => document.getElementById('wizardSubmitError')?.scrollIntoView({ behavior: 'smooth', block: 'center' }));`
- L2133: `box.scrollIntoView({ behavior: 'smooth', block: 'center' });`
- L2368: `requestAnimationFrame(() => document.getElementById('f-genre-custom')?.focus());`
- L2480: `requestAnimationFrame(() => document.getElementById('tr-${form.tracks.length}-title')?.focus());`
- L2564: `requestAnimationFrame(() => document.getElementById('aqExpressReason')?.closest('.aq-option-detail')?.scrollIntoView({ behavior: 'smooth', block: 'center' }));`

### src/styles/admin.css

- L22: `transition:background-color .18s var(--adm-ease),color .18s var(--adm-ease)`
- L99: `transition:transform .2s`
- L192: `transition:transform .18s var(--adm-ease)`
- L319: `transition:transform .2s ease`
- L330: `animation:none!important`
- L331: `animation:none!important`
- L379: `transition:background-color .15s,color .15s`
- L382: `animation:adm-spin .9s linear infinite`
- L383: `@keyframes adm-spin`
- L384: `animation:none`
- L385: `transition:background-color .15s`
- L420: `transition:transform .2s var(--adm-ease)`
- L430: `transition:transform .2s var(--adm-ease)`
- L440: `transition:background-color .15s,color .15s`
- L472: `transition:transform .2s var(--adm-ease)`
- L523: `transition:transform .2s var(--adm-ease)`
- L626: `transition:transform .2s var(--adm-ease)`
- L644: `transition:background-color .18s var(--adm-ease),color .18s var(--adm-ease),transform .16s var(--adm-ease)`
- L671: `animation:adm-spin .7s cubic-bezier(.45,.05,.35,1) infinite`
- L673: `animation:adm-pop .32s var(--adm-ease) both`
- L674: `@keyframes adm-pop`
- L675: `animation:none!important`
- L695: `@keyframes adm-rise`
- L696: `@keyframes adm-reveal`
- L697: `@keyframes adm-bar-up`
- L698: `@keyframes adm-scale-in`
- L699: `@keyframes adm-glow`
- L702: `animation:adm-rise var(--aq-motion-long) var(--adm-ease) both`
- L704: `animation:adm-rise var(--aq-motion-long) var(--adm-ease) .04s both`
- L705: `animation:adm-scale-in var(--aq-motion-long) var(--adm-ease) .12s both`
- L707: `animation:adm-reveal .28s var(--adm-ease) both`
- L709: `animation:adm-bar-up var(--aq-motion-long) var(--adm-ease) .15s both`
- L712: `animation:adm-scale-in var(--aq-motion-long) var(--adm-ease) both`
- L714: `animation:adm-scale-in var(--aq-motion-long) var(--adm-ease) both`
- L715: `animation:adm-scale-in var(--aq-motion-long) var(--adm-ease) both`
- L717: `animation:none`
- L718: `@keyframes adm-breathe`
- L720: `transition:background-color .22s var(--adm-ease),color .22s var(--adm-ease),transform .16s var(--adm-ease),box-shadow .22s var(--adm-ease)`
- L723: `transition:transform .2s var(--adm-ease)`

### src/styles/ai-connect.css

- L54: `transition:transform .15s`

### src/styles/controls.css

- L23: `transition:background-color .18s ease,box-shadow .18s ease,transform .12s ease`
- L34: `animation:aqc-check-pop .22s var(--aq-enter)`
- L35: `@keyframes aqc-check-pop`
- L73: `transition:background-color .18s ease`
- L90: `transition:background-color 100000s 0s,color 100000s 0s`

### src/styles/design-align.css

- L58: `transition:background-color .2s var(--aq-e-ease)`
- L64: `transition:background-color .2s var(--aq-e-ease)`
- L107: `transition:background-color .2s ease`
- L126: `animation:none`
- L127: `animation:aq-fade-up .22s var(--aq-e-ease) both`
- L129: `animation-duration:.45s`
- L164: `transition:background-color .2s ease`
- L175: `transition:background-color .2s ease`
- L236: `transition:background-color .3s ease`
- L287: `transition:background-color .2s ease`
- L335: `animation:aq-fade-up .32s var(--aq-e-spring) both`
- L342: `animation:none`
- L354: `@keyframes aq-menu-in`
- L355: `@keyframes aq-menu-out`
- L356: `@keyframes aq-menu-group`
- L357: `animation:aq-menu-in var(--aq-motion-base) var(--aq-enter) both!important`
- L358: `animation:aq-menu-out .15s cubic-bezier(.4,0,1,1) forwards!important`
- L359: `animation:aq-menu-group .26s cubic-bezier(.2,.8,.2,1) both`
- L360: `animation:none`
- L361: `animation:none!important`
- L365: `animation-name:aq-menu-in-full!important`; `animation-duration:var(--aq-motion-short)!important`
- L366: `animation-name:aq-menu-out-full!important`
- L367: `animation-delay:.14s`
- L369: `@keyframes aq-menu-in-full`
- L370: `@keyframes aq-menu-out-full`
- L371: `animation:none!important`
- L377: `@keyframes aq-view-fade`
- L378: `animation:aq-view-fade var(--aq-motion-short) ease-out backwards`
- L380: `animation:none!important`
- L381: `animation:none`
- L395: `animation:aq-fade-in var(--aq-motion-base) var(--aq-enter) both`
- L397: `animation:none`
- L460: `@keyframes aq-modal-nudge`
- L461: `animation:aq-modal-nudge .3s var(--aq-enter)!important`
- L464: `transition:transform .2s var(--aq-enter),opacity .2s ease`
- L500: `animation:none!important`
- L537: `transition:background-color var(--aq-dur) var(--aq-ease),transform var(--aq-press-out,.34s) var(--aq-press-spring,ease)`
- L539: `transition-duration:var(--aq-press-in,.06s)`
- L556: `transition:background-color var(--aq-dur) var(--aq-ease),transform var(--aq-press-out,.34s) var(--aq-press-spring,ease)`
- L558: `transition-duration:var(--aq-press-in,.06s)`
- L648: `animation:aq-overlay-enter .32s ease-out both`
- L649: `animation:aq-sheet-up var(--aq-motion-sheet) var(--aq-sheet-ease) both`
- L650: `animation:aq-overlay-out var(--aq-sheet-out,var(--aq-motion-sheet-out)) ease forwards!important`
- L651: `animation:aq-sheet-down var(--aq-sheet-out,var(--aq-motion-sheet-out)) var(--aq-sheet-ease) forwards!important`
- L653: `animation:aq-picker-fade .32s ease-out both`
- L654: `animation:aq-picker-up var(--aq-motion-sheet) var(--aq-sheet-ease) both`
- L655: `animation:aq-picker-scrim-out var(--aq-motion-sheet-out) ease forwards`
- L656: `animation:aq-picker-down var(--aq-motion-sheet-out) var(--aq-sheet-ease) forwards`
- L696: `animation:aq-fade-in var(--aq-motion-short) ease-out both`
- L744: `transition:background-color var(--aq-dur) var(--aq-ease)`
- L758: `transition:transform var(--aq-motion-base) var(--aq-enter)`
- L771: `transition:color var(--aq-dur-fast) var(--aq-ease)`
- L788: `transition:background-color var(--aq-dur-fast) var(--aq-ease)`
- L894: `transition:background-color var(--aq-motion-short) var(--aq-ease),color var(--aq-motion-short) var(--aq-ease),border-color var(--aq-motion-short) var(--aq-ease),transform var(--aq-press-out) var(--aq-press-spring)!important`
- L895: `transition:background-color var(--aq-dur) var(--aq-ease),color var(--aq-dur) var(--aq-ease),box-shadow var(--aq-dur) var(--aq-ease),transform var(--aq-press-out) var(--aq-press-spring)`
- L896: `transition:background-color var(--aq-dur) var(--aq-ease),color var(--aq-dur) var(--aq-ease),transform var(--aq-press-out) var(--aq-press-spring)`
- L897: `transition-duration:var(--aq-press-in)!important`
- L898: `transition-duration:var(--aq-press-in)`
- L899: `transition-duration:var(--aq-press-in)`
- L919: `animation:aq-fade-in var(--aq-motion-base) var(--aq-enter) both!important`
- L926: `animation:none!important`
- L935: `transition:box-shadow .28s var(--aq-enter)`
- L941: `@keyframes aq-land-fade`
- L942: `animation:aq-land-fade var(--aq-motion-long) var(--aq-enter) backwards`
- L943: `animation-delay:25ms`; `animation-delay:50ms`; `animation-delay:75ms`
- L944: `animation-delay:100ms`
- L947: `animation-delay:.08s`; `animation-delay:.105s`; `animation-delay:.13s`
- L948: `animation-delay:.155s`; `animation-delay:.18s`
- L950: `animation:none`
- L953: `@keyframes aq-faq-open`
- L954: `animation:aq-faq-open var(--aq-motion-long) var(--aq-enter) backwards`
- L957: `animation:none!important`
- L958: `transition:none`
- L971: `transition:transform var(--aq-press-out) var(--aq-press-spring)`
- L972: `transition-duration:var(--aq-press-in)`
- L977: `transition:transform var(--aq-press-out) var(--aq-press-spring)`
- L978: `transition-duration:var(--aq-press-in)`
- L983: `transition:color var(--aq-dur) var(--aq-ease)`
- L1023: `transition:background-color var(--aq-dur) var(--aq-ease),transform var(--aq-press-out) var(--aq-press-spring)`
- L1025: `transition-duration:var(--aq-press-in)`
- L1038: `transition:background-color var(--aq-dur) var(--aq-ease),transform var(--aq-press-out) var(--aq-press-spring)`
- L1039: `transition-duration:var(--aq-press-in)`
- L1109: `transition:background var(--aq-dur-fast,.15s) ease,transform var(--aq-press-out,.2s) var(--aq-ease-out,ease)`
- L1112: `transition-duration:var(--aq-press-in,.06s)`
- L1139: `transition:background var(--aq-dur-fast,.15s) ease,transform var(--aq-press-out,.2s) var(--aq-ease-out,ease)`
- L1141: `transition-duration:var(--aq-press-in,.06s)`
- L1181: `transition:transform var(--aq-press-out) var(--aq-press-spring),background-color var(--aq-dur) var(--aq-ease)`
- L1183: `transition-duration:var(--aq-press-in)`
- L1199: `animation:aq-land-fade .5s cubic-bezier(.22,.61,.36,1) both`
- L1209: `animation:none`
- L1228: `transition:transform var(--aq-press-out) var(--aq-press-spring)`
- L1229: `transition-duration:var(--aq-press-in)`
- L1231: `transition:color var(--aq-dur) var(--aq-ease)`
- L1266: `@keyframes aq-heart-pop`
- L1267: `@keyframes aq-cheer-glow`
- L1268: `@keyframes aq-leave`
- L1269: `@keyframes aq-saved-ring`
- L1272: `transition:background-color var(--aq-dur) var(--aq-ease)`
- L1287: `transition:transform var(--aq-dur) var(--aq-ease)`
- L1290: `animation-delay:40ms`; `animation-delay:80ms`
- L1291: `animation-delay:120ms`
- L1292: `animation:aq-soft-pop var(--aq-motion-base) var(--aq-enter) both`
- L1295: `transition:color var(--aq-dur) var(--aq-ease)`
- L1298: `animation:aq-heart-pop .5s var(--aq-enter)`
- L1299: `animation:aq-soft-pop var(--aq-motion-base) var(--aq-enter) both`
- L1301: `animation:aq-land-fade .5s cubic-bezier(.22,.61,.36,1) both,aq-cheer-glow 1.8s ease-out`
- L1302: `animation:aq-leave .26s var(--aq-enter) forwards`
- L1303: `transition:opacity var(--aq-dur) var(--aq-ease)`
- L1304: `animation:aq-soft-pop var(--aq-motion-base) var(--aq-enter) both`
- L1308: `animation:aq-shimmer 1.4s ease-in-out infinite`
- L1317: `animation:none`
- L1320: `transition:background-color var(--aq-dur) var(--aq-ease)`
- L1321: `transition:transform var(--aq-motion-long) var(--aq-enter),box-shadow var(--aq-motion-long) var(--aq-enter)`
- L1323: `transition:background-color var(--aq-dur) var(--aq-ease),color var(--aq-dur) var(--aq-ease)`
- L1325: `transition:grid-template-rows var(--aq-motion-long) var(--aq-enter)`
- L1329: `transition:opacity var(--aq-motion-long) var(--aq-ease)`
- L1331: `transition:transform var(--aq-motion-long) var(--aq-enter)`
- L1333: `transition:none`
- L1336: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L1337: `animation:aq-leave .26s var(--aq-enter) forwards`
- L1338: `animation:aq-saved-ring .9s ease-out .15s`
- L1339: `transition:background-color var(--aq-dur) var(--aq-ease),transform var(--aq-press-out) var(--aq-press-spring)`
- L1340: `animation:aq-heart-pop .45s var(--aq-enter)`
- L1342: `animation:none`
- L1347: `transition:opacity 1.8s ease-in`
- L1367: `transition:background-color var(--aq-dur) var(--aq-ease),color var(--aq-dur) var(--aq-ease),transform var(--aq-press-out) var(--aq-press-spring)`
- L1368: `transition-duration:var(--aq-press-in)`
- L1399: `transition:opacity var(--aq-dur) var(--aq-ease)`
- L1434: `animation:aq-roll-in var(--aq-motion-long) var(--aq-enter) both`
- L1435: `animation:aq-roll-out var(--aq-motion-base) var(--aq-exit,var(--aq-enter)) both`
- L1436: `@keyframes aq-roll-in`
- L1437: `@keyframes aq-roll-out`
- L1439: `animation:none`
- L1508: `transition:fill var(--aq-motion-short) var(--aq-enter)`
- L1554: `transition:none`
- L1688: `transition:width var(--aq-motion-long) var(--aq-enter)`
- L1700: `transition:transform var(--aq-motion-short) var(--aq-enter)`
- L1713: `animation:none`
- L1725: `transition:background-color var(--aq-motion-short) var(--aq-enter)`
- L1768: `animation:aq-grow-x var(--aq-motion-page) var(--aq-enter) var(--aq-motion-short) both`
- L1769: `animation:aq-badge-pop var(--aq-motion-base) var(--aq-pop) both`
- L1771: `animation:aq-pop var(--aq-motion-base) var(--aq-pop) var(--aq-motion-short) both`
- L1773: `transition:background-color var(--aq-motion-short) var(--aq-enter),transform var(--aq-press-out) var(--aq-press-spring)`
- L1774: `transition-duration:var(--aq-press-in)`
- L1775: `transition:color var(--aq-motion-short) var(--aq-enter)`
- L1778: `transition-duration:var(--aq-press-in)`
- L1780: `animation:aq-badge-pop var(--aq-motion-base) var(--aq-pop) var(--aq-motion-long) both`
- L1782: `animation:aq-pop var(--aq-motion-base) var(--aq-pop) both`
- L1786: `animation:none`
- L1803: `animation:aq-grow-y var(--aq-motion-page) var(--aq-enter) both`
- L1814: `animation:aq-grow-x var(--aq-motion-page) var(--aq-enter) both`
- L1820: `animation:none`

### src/styles/design.css

- L80: `transition:background-color var(--aq-motion-fast) var(--aq-motion-ease),color var(--aq-motion-fast) var(--aq-motion-ease),border-color var(--aq-motion-fast) var(--aq-motion-ease)!important`
- L92: `transition:background-color var(--aq-motion-fast) var(--aq-motion-ease),color var(--aq-motion-fast) var(--aq-motion-ease),border-color var(--aq-motion-fast) var(--aq-motion-ease)!important`
- L159: `transition:background-color .28s var(--aq-motion-ease)!important`
- L207: `transition:background-color var(--aq-motion-fast) var(--aq-motion-ease),color var(--aq-motion-fast) var(--aq-motion-ease),border-color var(--aq-motion-fast) var(--aq-motion-ease)!important`

### src/styles/desktop.css

- L130: `transition: background-color .2s ease`
- L170: `transition: width .5s var(--aq-e-ease)`

### src/styles/docs.css

- L141: `transition:transform .15s`
- L251: `transition:none`

### src/styles/enhance.css

- L16: `@keyframes aq-fade-up`
- L17: `@keyframes aq-fade-in`
- L18: `@keyframes aq-soft-pop`
- L19: `@keyframes aq-pop`
- L20: `@keyframes aq-shimmer`
- L21: `@keyframes aq-shake`
- L22: `@keyframes aq-spin`
- L23: `@keyframes aq-grow-y`
- L24: `@keyframes aq-grow-x`
- L25: `@keyframes aq-boot-bar`
- L26: `@keyframes aq-badge-pop`
- L27: `@keyframes aq-modal-out`
- L28: `@keyframes aq-overlay-out`
- L29: `@keyframes aq-nav-out`
- L32: `transition:top .2s var(--aq-e-ease)`
- L37: `animation:aq-fade-in .4s ease both`
- L40: `animation:aq-boot-bar 1s var(--aq-e-ease) infinite`
- L43: `transition:box-shadow .25s ease,background-color .25s ease`
- L45: `transition:background .2s,color .2s`
- L47: `transition:transform .25s var(--aq-e-spring)`
- L48: `animation:aq-soft-pop .3s var(--aq-e-ease) both`
- L64: `@keyframes aq-row-in`
- L65: `animation:aq-row-in var(--aq-motion-base) var(--aq-enter) both`
- L68: `transition:transform .25s var(--aq-e-ease),box-shadow .25s var(--aq-e-ease),background-color .2s ease!important`
- L74: `transition:transform var(--aq-motion-long) var(--aq-enter)`
- L75: `transition:transform .2s var(--aq-e-ease),color .2s`
- L76: `transition:transform .28s var(--aq-e-ease),box-shadow .28s var(--aq-e-ease),background-color .2s!important`
- L83: `transition:background-color .2s`
- L86: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L89: `transition:transform .25s var(--aq-e-ease)`
- L102: `animation:aq-grow-y .7s var(--aq-e-ease) both`; `animation-delay:var(--d,0s)`
- L157: `animation:aq-shimmer 1.4s ease-in-out infinite`
- L159: `animation:aq-fade-in .3s ease both`
- L168: `animation:aq-fade-in .25s ease .12s both`
- L177: `animation:aq-spin .7s linear infinite`
- L178: `transition:transform var(--aq-motion-short) var(--aq-enter)`
- L182: `animation:aq-overlay-out .18s ease forwards!important`
- L183: `animation:aq-modal-out .18s ease forwards!important`
- L185: `transition:background-color .18s ease!important`
- L196: `animation:aq-fade-up var(--aq-motion-base) var(--aq-enter) both`
- L197: `animation:aq-modal-out var(--aq-motion-short) var(--aq-exit) forwards`
- L203: `animation:aq-shake var(--aq-motion-long) ease-out`
- L204: `animation:aq-shake var(--aq-motion-long) ease-out`
- L205: `animation:aq-fade-up .25s ease both`
- L206: `animation:aq-fade-up .25s ease both`
- L208: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L223: `transition:width .35s var(--aq-e-ease),background-color .35s`
- L233: `animation:aq-fade-in .24s ease both`
- L235: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L240: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L244: `animation:aq-fade-in .3s ease both`
- L245: `animation:aq-spin .7s linear infinite`
- L250: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L251: `animation:aq-fade-up .3s var(--aq-e-ease) both`
- L252: `transition:transform .25s var(--aq-e-ease)!important`
- L255: `transition:border-color .2s,background-color .2s,transform .2s var(--aq-e-ease)`
- L260: `transition:transform .3s var(--aq-e-spring)`
- L262: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L266: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L269: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L270: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L273: `transition:background-color .2s,transform .2s var(--aq-e-ease)`
- L283: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) .08s both`
- L286: `transition:transform .3s var(--aq-e-spring)`
- L293: `animation:aq-grow-x .5s var(--aq-enter) both`
- L294: `animation-delay:.08s`
- L295: `animation-delay:.16s`
- L296: `animation-delay:.24s`
- L297: `transition:filter .2s`
- L302: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L305: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L309: `animation:none!important`
- L321: `animation:aq-fade-in .2s ease .15s both!important`
- L331: `@keyframes aq-ring-draw`
- L332: `@keyframes aq-tick-draw`
- L334: `animation:aq-pop var(--aq-motion-long) var(--aq-enter) both`
- L335: `animation:aq-ring-draw .5s var(--aq-enter) both`
- L336: `animation:aq-tick-draw .3s var(--aq-enter) .3s both`
- L340: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) .25s both`
- L391: `animation:none!important`
- L394: `@keyframes aq-check-pop`
- L396: `transition:width .5s var(--aq-e-ease)`
- L398: `transition:background-color .2s ease,box-shadow .2s ease`
- L399: `transition:background-color .25s ease,color .25s ease,opacity .25s ease,transform .12s ease`
- L401: `transition:background-color .2s ease,box-shadow .2s ease,transform .12s ease`
- L403: `transition:transform .2s var(--aq-e-ease)`
- L408: `animation:aq-fade-up var(--aq-motion-long) var(--aq-e-ease) both`
- L409: `animation:aq-fade-up var(--aq-motion-long) var(--aq-e-ease) .04s both`
- L411: `animation:none`
- L413: `animation:aq-fade-up var(--aq-motion-long) var(--aq-e-ease) .04s both`
- L415: `animation:aq-check-pop var(--aq-motion-base) var(--aq-enter)`
- L417: `animation:aq-fade-up .22s var(--aq-e-ease) both,aq-shake .38s ease .18s`
- L420: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) .18s both`
- L421: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) .22s both`
- L422: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) .3s both`
- L423: `animation:none`
- L424: `animation:aq-fade-in var(--aq-motion-long) ease .42s both`
- L425: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) .45s both`
- L440: `transition:background-color .2s!important`
- L453: `transition:background-color .2s,box-shadow .2s!important`
- L505: `animation:aq-fade-in .2s ease both`
- L509: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L522: `animation:none`
- L548: `animation:aq-sheet-up var(--aq-motion-long) var(--aq-enter) both`
- L550: `animation:aq-sheet-down .22s var(--aq-exit) forwards!important`
- L554: `@keyframes aq-sheet-up`
- L555: `@keyframes aq-sheet-down`
- L559: `transition:background-color .2s,transform .2s var(--aq-e-ease)`
- L569: `transition:transform .2s var(--aq-e-ease)`
- L583: `transition:background-color .25s`
- L585: `transition:background-color .25s,color .25s`
- L586: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L594: `transition:background-color .2s,color .2s,transform .15s`
- L598: `transition:background-color .2s,color .2s`
- L612: `animation:aq-fade-up .25s ease both`
- L614: `transition:width .3s var(--aq-e-ease)`
- L623: `transition:background-color .2s var(--aq-e-ease),color .2s var(--aq-e-ease)`
- L631: `transition:background-color .25s var(--aq-e-ease),color .25s var(--aq-e-ease)`
- L659: `animation:aq-fix-flash 2s var(--aq-e-ease) 1`
- L660: `@keyframes aq-fix-flash`
- L667: `transition:background-color .2s ease`
- L742: `transition:background-color .2s var(--aq-e-ease)`
- L770: `animation:aq-fade-up .3s cubic-bezier(.2,.8,.2,1) backwards`
- L772: `animation:aq-doc-draw .3s ease-out .12s forwards`
- L783: `@keyframes aq-doc-pop`
- L784: `@keyframes aq-doc-draw`
- L786: `animation:aq-paper-in var(--aq-motion-long) var(--aq-enter) .05s both`
- L787: `@keyframes aq-paper-in`
- L826: `animation:aq-sig-in .5s var(--aq-enter) .2s both`
- L828: `@keyframes aq-sig-in`
- L859: `animation:none!important`
- L860: `animation:none!important`
- L861: `animation:none!important`
- L871: `animation:aq-fade-up .3s var(--aq-e-ease) both`
- L894: `transition:color .2s var(--aq-e-ease)`
- L896: `transition:color .2s var(--aq-e-ease)`
- L898: `transition:transform .25s var(--aq-e-ease)`
- L910: `transition:color .2s var(--aq-e-ease)`
- L915: `transition:background-color .2s,color .2s`
- L930: `transition:color .2s var(--aq-e-ease)`
- L985: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L1016: `animation:aq-fade-up .3s var(--aq-e-ease) both`
- L1054: `animation:aq-fade-up .3s var(--aq-e-ease) both`
- L1066: `animation:none`
- L1067: `@keyframes aq-cadmin-pulse`
- L1068: `animation:none`
- L1111: `transition:none!important`
- L1126: `transition:background-color .18s ease,box-shadow .18s ease`
- L1157: `transition:background-color .18s ease,color .18s ease,transform .18s ease`
- L1171: `transition:background-color .18s ease,color .18s ease,transform .18s ease`
- L1175: `transition:filter .18s ease,opacity .18s ease`
- L1178: `transition:color .18s ease`
- L1188: `transition:none`
- L1209: `@keyframes aq-reveal`
- L1210: `animation:aq-reveal var(--aq-motion-long) var(--aq-enter) both`
- L1211: `animation:none`
- L1242: `transition-duration:var(--aq-press-in)!important`
- L1279: `animation:aq-fade-up .3s var(--aq-e-ease) both`
- L1281: `animation:none`
- L1305: `transition:background-color .2s ease,transform .2s var(--aq-e-ease)`
- L1326: `transition:transform .2s var(--aq-e-ease)`
- L1336: `transition:none`
- L1350: `transition:background-color .18s ease,color .18s ease,transform .15s var(--aq-e-ease)`
- L1357: `transition:background-color .18s ease,box-shadow .18s ease`
- L1360: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L1365: `transition:none`; `animation:none`
- L1377: `transition:background-color .18s ease,transform .15s var(--aq-e-ease)`
- L1427: `animation:aq-fade-up .28s ease both`
- L1430: `transition:background-color .15s ease,color .15s ease`
- L1433: `animation:none`
- L1447: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L1460: `animation:aq-grow-x .5s var(--aq-enter) both`
- L1462: `animation:aq-stage-fill .5s var(--aq-enter) both`
- L1463: `animation-delay:.08s`
- L1464: `animation-delay:.16s`
- L1465: `animation-delay:.24s`
- L1466: `@keyframes aq-stage-fill`
- L1481: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L1494: `animation:none`; `animation:none!important`
- L1518: `animation:aq-fade-up var(--aq-motion-long) var(--aq-enter) both`
- L1556: `animation:aq-soft-pop var(--aq-motion-long) var(--aq-enter) both`
- L1581: `animation:none`
- L1582: `animation:none`
- L1670: `transition:transform .35s var(--aq-spring),background-color var(--aq-dur)`
- L1735: `transition:transform .2s var(--aq-e-ease)`
- L1755: `transition:background-color .15s ease,transform .12s ease`

### src/styles/live.css

- L17: `transition:transform .2s`
- L26: `transition:color .2s,background .2s`
- L27: `transition:transform .35s var(--ease-out),box-shadow .35s var(--ease-out)`
- L77: `@keyframes bar-in`
- L88: `animation:none!important`; `transition:none!important`
- L129: `transition:background .2s`
- L134: `animation:nav-enter .28s var(--studio-ease) both`
- L135: `@keyframes nav-enter`
- L138: `transition:background .18s,color .18s`
- L146: `@keyframes studio-enter`
- L150: `transition:transform .32s var(--studio-ease),box-shadow .32s var(--studio-ease),background .32s`
- L168: `transition:width .5s var(--portal-ease)`
- L206: `transition:background .25s`
- L261: `animation:none!important`
- L335: `transition:background-color .2s ease`
- L344: `transition:transform .15s ease,box-shadow .15s ease`
- L382: `transition:background .16s`
- L383: `transition:transform .18s`
- L402: `animation:none!important`; `transition:none!important`
- L447: `transition:background .2s ease`
- L470: `transition:none`
- L478: `animation:none!important`
- L480: `transition:background-color var(--aq-motion-fast) var(--aq-motion-ease),color var(--aq-motion-fast) var(--aq-motion-ease),border-color var(--aq-motion-fast) var(--aq-motion-ease)!important`
- L483: `transition:transform .25s var(--aq-motion-ease)!important`
- L485: `transition:background-color .28s var(--aq-motion-ease)!important`
- L487: `transition:color var(--aq-motion-fast) var(--aq-motion-ease)`
- L489: `transition:background-color .22s var(--aq-motion-ease),border-color .22s var(--aq-motion-ease)`
- L490: `animation:aq-overlay-enter var(--aq-motion-base) ease-out both`
- L491: `animation:aq-dialog-enter var(--aq-motion-long) var(--aq-enter) both`
- L492: `@keyframes aq-overlay-enter`
- L493: `@keyframes aq-dialog-enter`
- L495: `animation:none!important`; `animation-duration:.7s`
- L499: `animation:none!important`
- L500: `transition:none!important`
- L551: `animation:none!important`; `transition:none!important`; `animation-duration:.01ms!important`; `animation-iteration-count:1!important`; `transition-duration:.01ms!important`
- L588: `transition:width .25s ease`
- L592: `transition:background .18s,box-shadow .18s`
- L599: `transition:transform .32s cubic-bezier(.32,.72,.28,1),width .32s cubic-bezier(.32,.72,.28,1),opacity .2s`
- L600: `transition:background .16s,color .16s,box-shadow .16s`
- L601: `transition:transform .16s ease,background .16s ease,box-shadow .16s ease`
- L605: `animation:aqFadeIn .16s ease`; `animation:aqDialogIn .22s cubic-bezier(.2,.8,.2,1)`; `@keyframes aqFadeIn`; `@keyframes aqDialogIn`
- L617: `animation:bar-in .6s var(--portal-ease) both!important`
- L620: `animation:studio-enter .35s var(--studio-ease) backwards`
- L622: `animation:none!important`
- L700: `transition:transform .2s`
- L702: `animation:fadeIn .2s ease`
- L703: `@keyframes fadeIn`

### src/styles/ui.css

- L30: `transition:background-color var(--aq-dur) var(--aq-ease),color var(--aq-dur) var(--aq-ease),transform var(--aq-dur-fast) var(--aq-ease),opacity var(--aq-dur)`
- L90: `animation:aq-spin .7s linear infinite`
- L92: `transition:transform var(--aq-dur) var(--aq-ease)`
- L107: `transition:background-color var(--aq-dur) var(--aq-ease),box-shadow var(--aq-dur) var(--aq-ease)`
- L113: `transition:color var(--aq-motion-short) var(--aq-ease)`
- L140: `transition:background-color var(--aq-dur)`
- L160: `transition:background-color var(--aq-dur-fast) var(--aq-ease),color var(--aq-dur-fast) var(--aq-ease)`
- L217: `transition:background-color var(--aq-dur) var(--aq-ease),color var(--aq-dur) var(--aq-ease),transform var(--aq-dur-fast) var(--aq-ease)`
- L236: `transition:transform var(--aq-dur-slow) var(--aq-glide),width var(--aq-dur-slow) var(--aq-glide),height var(--aq-dur-slow) var(--aq-glide),opacity var(--aq-dur)`
- L240: `transition:color var(--aq-dur) var(--aq-ease),transform var(--aq-dur-fast) var(--aq-ease)`
- L260: `transition:color var(--aq-dur) var(--aq-ease)`
- L273: `transition:transform var(--aq-dur-slow) var(--aq-glide),opacity var(--aq-dur)`
- L275: `@keyframes aq-panel-in`
- L276: `animation:aq-panel-in var(--aq-motion-long) var(--aq-enter) both`
- L285: `transition:background-color var(--aq-dur),color var(--aq-dur),transform var(--aq-dur) var(--aq-ease)`
- L287: `transition:transform var(--aq-dur) var(--aq-ease)`
- L311: `transition:background-color var(--aq-dur) var(--aq-ease)`
- L324: `transition:background-color var(--aq-dur) var(--aq-ease),transform var(--aq-dur-fast) var(--aq-ease),box-shadow var(--aq-dur) var(--aq-ease)`
- L350: `transition:background-color var(--aq-dur) var(--aq-ease),transform var(--aq-dur-fast) var(--aq-ease),box-shadow var(--aq-dur) var(--aq-ease)`
- L397: `transition:transform var(--aq-dur) var(--aq-spring),background-color var(--aq-dur)`
- L439: `animation:aq-fade-up .4s var(--aq-ease) both`
- L460: `animation:ui-meter-grow var(--aq-motion-long) var(--aq-enter) backwards`
- L464: `@keyframes ui-meter-grow`
- L465: `animation:none`
- L470: `transition:grid-template-rows var(--aq-motion-base) var(--aq-exit),margin-top var(--aq-motion-base) var(--aq-exit),opacity var(--aq-motion-short) var(--aq-exit)`
- L472: `transition-duration:var(--aq-motion-long),var(--aq-motion-long),var(--aq-motion-base)`; `transition-timing-function:var(--aq-enter)`
- L474: `transition:transform var(--aq-motion-long) var(--aq-enter)`
- L479: `transition:none`
- L508: `animation:aq-shimmer 1.4s ease-in-out infinite`
- L533: `transition:none`
- L534: `animation:none`
- L538: `animation:aq-fade-up var(--aq-motion-long) var(--aq-ease) both`
- L539: `animation:none`
- L568: `transition:transform var(--aq-dur) var(--aq-ease)`
- L648: `transition:background-color .15s ease`
- L655: `transition:background-color .15s ease`
- L658: `transition:background-color .15s ease,transform .12s ease`
- L664: `transition:background-color .15s ease,transform .12s ease`
- L666: `transition:background-color .2s ease,color .2s ease`
- L723: `transition:background-color var(--aq-dur) var(--aq-ease)`
- L743: `transition:background-color .2s ease,box-shadow .2s ease,transform .12s ease`
- L762: `transition:background-color .15s ease,box-shadow .15s ease`
- L764: `animation:aq-otp-caret 1s steps(1) infinite`
- L765: `animation:aq-check-pop .22s var(--aq-e-spring)`
- L766: `@keyframes aq-otp-caret`
- L767: `animation:none!important`; `animation:none`
- L789: `animation:none`
- L790: `@keyframes aq-verify-pulse`
- L791: `animation:none`
- L802: `@keyframes aq-picker-fade`
- L803: `@keyframes aq-picker-up`
- L805: `@keyframes aq-picker-pop`
- L807: `animation:aq-picker-fade var(--aq-motion-base) ease-out both`
- L809: `animation:aq-picker-up var(--aq-motion-long) var(--aq-enter) both`
- L816: `animation:aq-picker-pop .18s var(--aq-enter) both`
- L961: `animation:none!important`
- L1035: `transition:filter var(--aq-dur) var(--aq-ease)`
- L1058: `transition:background-color var(--aq-press-out) var(--aq-press-spring),color var(--aq-press-out) var(--aq-press-spring),opacity var(--aq-dur),transform var(--aq-press-out) var(--aq-press-spring)`
- L1059: `transition-duration:var(--aq-press-in)`
- L1060: `transition:background-color var(--aq-dur) var(--aq-ease),color var(--aq-dur) var(--aq-ease),transform var(--aq-press-out) var(--aq-press-spring)`
- L1061: `transition-duration:var(--aq-press-in)`
- L1062: `transition:background-color var(--aq-press-out) var(--aq-press-spring),transform var(--aq-press-out) var(--aq-press-spring),filter var(--aq-press-out) var(--aq-press-spring)`
- L1063: `transition-duration:var(--aq-press-in)`
- L1098: `@keyframes aq-picker-scrim-out`
- L1099: `@keyframes aq-picker-down`
- L1100: `@keyframes aq-picker-pop-out`
- L1103: `animation:aq-picker-scrim-out .22s ease forwards`
- L1104: `animation:aq-picker-down .22s cubic-bezier(.4,0,1,1) forwards`
- L1105: `animation:aq-picker-pop-out .15s cubic-bezier(.4,0,1,1) forwards`
- L1109: `@keyframes aq-panel-from-right`
- L1110: `@keyframes aq-panel-from-left`
- L1111: `animation:aq-panel-from-right .34s var(--aq-ease) both`
- L1112: `animation:aq-panel-from-left .34s var(--aq-ease) both`
- L1113: `animation:none`

### src/styles/wizard-motion.css

- L5: `@keyframes aq-wiz-invalid`
- L7: `@keyframes aq-wiz-step-in`
- L11: `animation:aq-wiz-step-in var(--aq-motion-base) var(--aq-enter) backwards`
- L12: `animation-delay:.03s`
- L13: `animation:none`
- L15: `animation:aq-wiz-step-in var(--aq-motion-base) var(--aq-enter) backwards`
- L17: `animation:aq-wiz-step-in .18s ease-out backwards`
- L18: `animation:aq-wiz-invalid 1.2s ease 1`
- L19: `animation:none`
- L23: `transition:background-color .3s ease!important`
- L27: `transition:background-color .2s ease,color .2s ease`
- L28: `transition:background-color .2s ease,color .2s ease`
- L29: `transition:width .3s var(--aq-e-ease)`
- L41: `animation:none`
- L42: `animation-duration:.32s`; `animation-timing-function:cubic-bezier(.22,1,.36,1)`
- L44: `animation-duration:.24s`
- L45: `animation-duration:.16s`
- L46: `animation-duration:.36s`
- L47: `animation:none`
- L49: `animation:none`
- L50: `animation-duration:.2s`
- L53: `animation:aq-fix-flash 1.8s var(--aq-e-ease) 1`
- L57: `animation:none!important`
- L58: `transition:none!important`

### src/styles/work.css

- L81: `transition:background-color .15s,color .15s`
- L122: `transition:background-color .15s`
- L157: `transition:box-shadow .2s var(--adm-ease)`
- L213: `transition:background-color .18s var(--adm-ease),transform .18s var(--adm-ease)`
- L309: `transition:opacity .2s var(--adm-ease)`
- L310: `transition:background-color .15s var(--adm-ease),box-shadow .15s var(--adm-ease)`
- L336: `transition:box-shadow .15s var(--adm-ease),opacity .15s var(--adm-ease),transform .15s var(--adm-ease)`
- L386: `animation:adm-kb-fade .18s ease-out`
- L387: `animation:adm-kb-slide .22s var(--adm-ease)`
- L416: `@keyframes adm-kb-slide`
- L417: `@keyframes adm-kb-fade`
- L461: `transition:none`
- L463: `animation:none`

### src/ui/CountUp.tsx

- L38: `if (p < 1) raf = requestAnimationFrame(tick);`
- L40: `raf = requestAnimationFrame(tick);`

### src/ui/Feedback.tsx

- L36: `{Array.from({ length: rows }, (_, i) => <span key={i} style={{ animationDelay: '${i * 70}ms' }} />)}`

### src/ui/Reveal.tsx

- L32: `if (Math.abs(dy) > 2) window.scrollBy({ top: dy, behavior: reduced() ? 'auto' : 'smooth' });`
- L58: `raf = requestAnimationFrame(() => { raf = requestAnimationFrame(() => setExpanded(true)); });`

