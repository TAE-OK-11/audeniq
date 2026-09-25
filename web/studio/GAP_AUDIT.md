# AUDENIQ STUDIO — React 포털 vs 라이브 기능 격차 감사

- 감사일: 2026-09-25
- 라이브 기준: `/tmp/studio_live.html` (studio.audeniq.com 실제 HTML+JS, 323KB)
- React 기준: `/home/hatch/audeniq-f2/web/studio/app/src/`
- 원칙: 코드는 수정하지 않고 격차만 기록

---

## 페이지별 상태 요약

| 페이지 | React 파일 | 상태 | 비고 |
|---|---|---|---|
| 홈 | Dashboard.tsx | ⚠️ 부분적 | hero+그리드만, 3개 섹션 누락 |
| 새로운 발매 (마법사) | Upload.tsx | ⚠️ 부분적 | 단계 타이틀은 일치, 입력 필드 대거 누락 |
| 발매·곡 관리 | Releases.tsx | ✅ 대체로 완료 | 경미한 차이 |
| 발매 상세 | ReleaseDetail.tsx | ⚠️ 부분적 | delivery 탭 내용 불일치, 수정/삭제 없음 |
| 음악 리포트 | Reports.tsx | ⚠️ 부분적 | CSV 입출력·데이터 테이블 없음 |
| 정산·지급 | Settlement.tsx | ⚠️ 부분적 | 지급 요청/정산 기록 모달 없음 |
| 계약서 (배급신청) | Contracts.tsx | ⚠️ 부분적 | 라이브 현행은 카드 그리드+서명 플로우 |
| 권리·보완 서류 | Rights.tsx | ⚠️ 부분적 | 카드 그리드·접수 모달 없음 |
| 문의·알림 | Support.tsx | ⚠️ 부분적 | 작성/상세 모달 없음 |
| 아티스트·정산 정보 | Profile.tsx | ⚠️ 부분적 | 수령 정보 다단계 설정 없음 |

---

## 1. 홈 (Dashboard.tsx) — ⚠️ 부분적

라이브 `renderHome()`이 렌더하는 3개 섹션이 React에 없음.

- ❌ **내 발매** (`#homeReleases`): 최근 발매 4개 (`updatedAt` 순, `releaseRow`). 빈 상태 시 `empty-note` "아직 등록한 발매가 없어요. … 발매 등록하기 ↗"
- ❌ **확인할 작업** (`#homeTasks`): 조건부 작업 카드
  - 보완이 필요한 발매 N건 → catalog로 이동
  - 작성 중인 발매 N건 → catalog로 이동
  - 아티스트 정보 등록 (프로필명 없을 때) → profile로 이동
  - 읽지 않은 알림 N건 → support로 이동
  - 빈 상태: "지금 확인할 작업이 없어요."
- ❌ **이번 달 음악 리포트** (`#homeReport`): `money(sum) · 이번 달 수익 집계` + `makeChart()` 막대그래프 + "플랫폼 보고서를 기준으로 집계한 수익을 확인해 보세요." + "리포트 보기 ↗" 링크
- ⚠️ 인사말: 라이브는 `#greeting` = `{profile.name}님의 작업실` (프로필 연동). React는 "서린님의 작업실" 하드코딩.

## 2. 새로운 발매 마법사 (Upload.tsx) — ⚠️ 부분적

단계 타이틀(`wizardTitles`) 6개 문구는 라이브와 일치. 단계별 입력 필드 격차:

### Step 0 · 발매 정보
라이브 `wizardHTML()` step 0 필드: 아티스트명* / 발매 제목* / 발매 유형(select: 싱글·EP·정규 앨범·컴필레이션) / 주요 언어(select: 한국어·영어·일본어·기타) / 장르(`genreField()`: 30개 프리셋 + "기타 · 직접 입력" 시 텍스트 입력 전환) / 레이블·발매사 표기 / 앨범 소개(textarea 1500자)
- ❌ React 누락: 발매 유형, 주요 언어, 장르 직접입력, 레이블 표기, 앨범 소개
- ⚠️ React 장르: 7개 옵션 단순 select (라이브 30개와 다름)

### Step 1 · 트랙 등록
라이브 `trackEditor(t,i)` 필드: 곡 제목* / 버전·부제 / ISRC / 곡 길이 자동 표시(`track-duration-label`) / 작곡* / 작사 / 편곡 / 실연자·피처링 / 음원 파일*(WAV·FLAC·AIFF·MP3) / Explicit 체크 + `aq-explicit-note` 안내 / 트랙 삭제 버튼
- ❌ React: 곡 제목 input만 있음. 나머지 전부 누락

### Step 2 · 커버아트
라이브: 파일 선택 + `cover-preview` 미리보기 + "커버 삭제" 버튼
- ⚠️ React: 파일명 표시만. 미리보기·삭제 없음

### Step 3 · 배급 설정
라이브: 발매 예정일* / 최초 발매일(재발매인 경우) / UPC·EAN / 전 세계 배급 체크 / 플랫폼 모두 선택 스위치(`aq-switch`) / 플랫폼 직접 선택 details (11개: 멜론·지니·FLO·벅스·Spotify·Apple Music·YouTube Music·Amazon Music·TIDAL·Deezer·Qobuz)
- ❌ React 누락: 최초 발매일, UPC/EAN, 전 세계 배급, 전체선택 스위치
- ⚠️ React 플랫폼 6개 (벅스·Amazon·TIDAL·Deezer·Qobuz 없음)

### Step 4 · 권리 확인
라이브: 음원(마스터) 권리자* / ℗ 음반제작자 권리 표기* / © 아트워크·앨범 권리 표기* + 필수 확인 5개 체크박스(마스터 권한·저작물 허락·커버아트 권한·샘플링/피처링 허락·정보 정확성) + notice
- ❌ React: 단일 동의 체크박스만. 권리자 3개 필드·5개 항목 누락

### Step 5 · 최종 확인
- ⚠️ 항목 내용은 유사하나 라이브는 `review-section` 7개 항목(발매 정보·트랙·커버아트·발매일·배급 대상·권리자·권리 확인) + "'접수하기'를 누르면 신청 내용과 권리 확인서가 생성돼요…" notice. React는 surface/data-list 구조로 다름

### 검증·제출
- ❌ 라이브 `validateStep()`: 단계별 상세 검증 + 오류 필드 포커스 이동. React는 3개 단계만 부분 검증
- ❌ 접수 후 라이브 `ensureReleaseDocuments()`: "AUDENIQ 디지털 음원 배급 신청·계약서" 자동 생성. React는 성공 메시지만

## 3. 발매·곡 관리 (Releases.tsx) — ✅ 대체로 완료

- ✅ 탭(발매 목록/전체 트랙), toolbar 검색+상태 select, filterrail, 결과 카운트, 빈 상태 구조 일치
- ⚠️ 트랙 검색: 라이브는 곡명·아티스트·ISRC, React는 곡명·ISRC (아티스트는 '테스트 레이블' 하드코딩)
- ⚠️ 상태 체계: 라이브 6개(`draft/ready/needs/review/scheduled/live`), React는 mock 4개 상태를 매핑 (동작은 유사)

## 4. 발매 상세 (ReleaseDetail.tsx) — ⚠️ 부분적

### overview 탭
라이브: `split` 레이아웃 — `dl.information`(아티스트·발매 유형·발매 예정일·UPC/EAN·장르·레이블) + 앨범 소개 + `studio-album-aside`(커버아트 정보·진행 상태 chip)
- ❌ React: 단순 data-list 5개 항목 (발매명·아티스트·발매일·트랙 수·등록일). 발매 유형/UPC/장르/레이블/앨범 소개/aside 누락

### tracks 탭
라이브: 트랙 번호 아이콘, 버전 표기, `ISRC · 작곡 · 작사`, 음원 상태 행 + 상태 chip(파일명 등록/파일 없음) + notice(음원 원본 재첨부 안내)
- ⚠️ React: 제목·ISRC·길이만. 작곡/작사/음원 상태/notice 누락

### delivery 탭 — ❌ 내용 불일치
라이브: 배급 설정(희망 배급 지역·발매 예정일·플랫폼 `tag-row`·권리 확인 마스터/℗/©) + 계약·증빙 surface("문서 관리 ↗" → contracts 이동)
React: 플랫폼 목록 + "준비 중" chip (라이브에 없는 화면)

### history 탭
라이브: 실제 `r.history` 변경 기록 리스트
- ❌ React: 하드코딩된 가짜 2개 항목

### 상단 액션
- ❌ 수정하기/계속 작성 (→ 마법사 편집 모드), 삭제 (확인 모달), 발매 상태 변경 demo 모달 — 전부 없음

## 5. 음악 리포트 (Reports.tsx) — ⚠️ 부분적

- ⚠️ 발매 필터: 라이브는 `db.releases`에서 동적 생성. React 하드코딩 3개
- ⚠️ stat-card 라벨: 라이브 = 집계 수익·재생 수·플랫폼. React = 총 재생·총 수익·정산 예정 (다름)
- ⚠️ 차트: 라이브 `makeChart()` = 월별 버킷 + `report-axis` 시작월~종월 라벨 + `empty-graph` 빈 상태. React = 하드코딩 막대, 축 라벨 없음
- ❌ 플랫폼별 상세: 라이브는 `data-table` (기간·플랫폼·발매/곡·재생·수익). React는 data-list 요약 행
- ❌ CSV 내보내기: 라이브 `download()` 실제 동작. React 버튼 무동작
- ❌ CSV 가져오기: 라이브 `reportFile` + `importHelp` 모달(형식 안내·빈 양식 받기) + 2MB 검증 + `csvParse()`. React 없음

## 6. 정산·지급 (Settlement.tsx) — ⚠️ 부분적

- ⚠️ stat-card: 라이브 = 기록한 정산액·요청 전 잔액·지급 요청 기록 합계. React = 확정 정산액·지급 완료·지급 가능 (라벨·의미 다름)
- ⚠️ notice 문구 다름 (라이브: "정산 내역과 지급 요청 기록을 확인해 주세요. 실제 송금은 지급 서비스 연결 후 진행돼요.")
- ❌ 정산 행 삭제: 라이브 `data-delete-statement` (confirm). React 없음
- ❌ 정산 내역 기록 모달: 라이브 `newStatement` → 정산 기간(월)·플랫폼·금액·메모 폼. React 없음
- ❌ 수익 지급 요청 모달: 라이브 `openPayout()` — 수령 정보 미등록 시 `openPaymentSetup()` 유도, 잔액 확인, 전액 버튼, 수령 계좌 표시(은행 로고), 메모, doc-connection 안내. React는 버튼 클릭 시 상태 텍스트만 변경
- ⚠️ 지급 상태 chip: 라이브 '전송 전', React '지급 완료' (의미 다름)

## 7. 계약서 — 배급신청 (Contracts.tsx) — ⚠️ 부분적

> 주의: 라이브에서 `renderContracts`가 하단 스크립트에 의해 **오버라이드**됨:
> `renderContracts=function(){contractTab='agreements';$('#contractList').innerHTML=aqDocumentCards('agreements');renderRights()};`
> 즉 라이브 현행 계약서 페이지는 탭이 없고, 카드 그리드 + 권리 섹션이 함께 표시됨.

- ❌ 구조: React = 탭(계약서/권리 증빙) + `doc-row` 리스트 + 구 `openDocument` 모달. 라이브 현행 = `studio-doc-path` + `aq-doc-grid` 카드 + `renderRights()` 섹션
- ❌ 문서 카드: 라이브 `aq-doc-card` (상태 tone `is-needs/is-approved/is-review`, 상태 pill, 버튼 문구 "확인하고 서명"/"계약서 보기"/"보완하기"/"자세히 보기"). React는 구 리스트 행
- ❌ 문서 상세 모달(현행): `aq-document-snapshot`, 확인 및 동의 기록(consent history), `processTimeline`, 직접 서명 입력 기록(`aq-sign-record`: 서명 이미지·서명자·시간), 보완 요청 notice+reviewNote, 요청된 서류 첨부 input, 서명 절차 확인 버튼(`aqSignIntent` → 서명 패드), doc-actions(확인 및 저장/검토 요청/원본 열기). React 모달은 구 버전 (서명 패드·timeline·서류 첨부 없음)
- ❌ 배급신청서 자동 생성: `ensureReleaseDocuments()`가 발매 접수 시 "AUDENIQ 디지털 음원 배급 신청·계약서"(4개 섹션) 자동 생성. React는 하드코딩 mock 문서
- ✅ 배급신청서 본문 4개 섹션 텍스트는 React에 이식됨

## 8. 권리·보완 서류 (Rights.tsx) — ⚠️ 부분적

- ✅ 요약 카운트 3개 (제출할 서류·검토 중·보완 요청) 구조 일치
- ❌ 리스트: 라이브 `aqDocumentCards('rights')` 카드 그리드. React는 단순 track-row
- ❌ 권리 서류 접수 모달: 라이브 `openRequiredDocForm()` — 관련 발매 select, 서류 종류 6종(마스터·커버아트·피처링/실연자·작사작곡/커버곡·샘플·기타), 서류 이름, 증빙 원본 첨부. React "권리 서류 제출 ↗" 버튼은 메시지 표시만

## 9. 문의·알림 (Support.tsx) — ⚠️ 부분적

- ⚠️ 문의 행: 라이브 `ticket-row` (✉ 아이콘, 문의 유형·날짜·관련 발매, "열기 ↗"). React는 상태 chip(답변 완료/대기 — 라이브에 없는 개념)
- ❌ 문의 상세 모달: 라이브는 본문 + "문의 기록 삭제". React 없음
- ❌ 새 문의 작성 모달: 라이브 — 문의 유형 5종(발매·심사/수정·테이크다운/정산·지급/계약·권리/계정·기타), 관련 발매 select, 제목, 내용, 저장. React는 메시지 표시만
- ⚠️ 알림: 라이브 `studio-notice-list` (버튼형, kind 심볼 ₩/♫/•, 읽지 않음 dot, 클릭 → 상세 모달 + 읽음 처리). React는 track-row + NEW chip, 클릭 동작 없음
- ✅ 알림 필터(전체/읽지 않음), 모두 읽음 — 동작 유사

## 10. 아티스트·정산 정보 (Profile.tsx) — ⚠️ 부분적

- ✅ 아티스트 프로필 폼 (활동명*·연락 이메일·소개·활동 국가·아바타) 유사
- ❌ 수령 정보: 라이브 `renderPaymentSetup()` 다단계 모달 —
  수령인 유형(개인·개인사업자·법인) → 금융기관 선택(은행·저축은행·증권사 탭, 기관별 SVG 로고 `aqInstitutionLogo`) → 계좌번호·예금주.
  `aq-payment-summary`는 등록 완료 시 은행 로고+`은행 · •••• last4`+수령인·유형 표시.
  React는 은행/계좌번호 단순 input
- ⚠️ 작업 정보 초기화: 라이브 `demo-settings` 실제 데이터 리셋. React 버튼은 `saved=false`만 변경

## 11. 공통 시스템

- ❌ 전역 토스트: 라이브 `toast()` (하단 알림). React는 페이지별 inline notice만, 전역 토스트 없음
- ⚠️ 모달: 라이브 전역 `modal()/closeModal()` (ESC·배경 클릭 닫기, 포커스 복원). React는 Contracts에만 로컬 모달
- ⚠️ 빈 상태: 라이브 `emptyPage()/empty-note/empty-graph` 패턴. React는 일부 페이지만 구현
- ✅ 라우팅: 라이브 `data-nav` SPA ↔ React `react-router` 동등

---

## 우선순위 (사용자 눈에 보이는 순서)

1. **홈 3개 섹션** — 내 발매 / 확인할 작업 / 이번 달 음악 리포트 (홈 진입 즉시 노출)
2. **마법사 Step 1 트랙 에디터** — 곡 크레딧·음원 파일·Explicit 등 핵심 입력 화면
3. **마법사 Step 0/3/4 빠진 필드** — 발매 유형·언어·장르 직접입력·레이블·앨범 소개 / 최초 발매일·UPC·전 세계 배급·11개 플랫폼 / 권리자 3종·필수 확인 5항목
4. **발매 상세 delivery 탭 정정 + 수정/삭제 버튼** — 현재 delivery 탭은 라이브에 없는 내용
5. **계약서 카드 그리드 + 서명 플로우** — 배급신청 핵심 (서명 패드·동의 기록·타임라인)
6. **정산 지급 요청 모달 + 정산 내역 기록 모달** — 정산 페이지 핵심 액션
7. **리포트 CSV 내보내기/가져오기 + 데이터 테이블** — 테이블 구조·축 라벨 포함
8. **프로필 수령 정보 다단계 설정** — 기관 선택·로고·유형 구분
9. **문의 작성 모달 + 알림 상세 모달** — 작성·삭제·읽음 처리
10. **전역 토스트/모달 시스템** — 전 페이지 일관된 피드백
