# AUDENIQ ADMIN — 관리자 콘솔 (React + TypeScript, Cloudflare Workers)

스태프가 발매 심사(승인 · 보완 요청 · 거절), 2차 승인, 서류 검토, 문의 답변, 배급 승인을 처리하는 콘솔입니다.
백엔드 `/api/staff/*`(crates/core/src/staff.rs, docs/API.md "Staff portal")를 그대로 씁니다.

## 디자인
버튼 · 입력창 · 카드 · 모달 · 토스트는 AUDENIQ STUDIO와 같습니다. `src/styles/design.css`, `live.css`, `enhance.css`와
`src/components/{Modal,Confirm,Toast}.tsx`는 `web/studio/app/src`에서 그대로 가져온 복사본이고, 관리자 레이아웃만
`src/styles/admin.css`(`adm-` 접두사)에 있습니다. 스튜디오 디자인을 바꾸면 이 파일들도 같이 맞춰 주세요.

## 구조
- `worker.js` — 정적 에셋 + `/api/auth/{login,logout,csrf}`, `/api/me`, `/api/staff/*`만 백엔드로 전달.
  상태 변경 요청은 관리자 주소에서 온 것만 받고, 백엔드에는 `BACKEND_APP_ORIGIN`(스튜디오 APP_ORIGIN)으로 전달합니다.
- `/api/content/*` — 스튜디오와 같은 D1 공지·이벤트·점검 데이터를 사용하며, 백엔드의 ADMIN 세션·CSRF 검증 후 읽거나 변경합니다. `/admin/content`에서 코드나 별도 토큰 없이 바로 게시할 수 있습니다.
- `src/api/staff.ts` — 스태프 API, `src/api/mock.ts` — 체험 데이터(`VITE_MOCK=true`일 때만)
- `src/pages/` — 오늘의 업무 · 발매 심사 · 심사 상세 · 2차 승인 · 서류 · 문의 · 배급 · DSP · 지급 요청

## 권한
로그인은 스튜디오 계정과 같고, 스태프 역할은 `audeniq-admin staff grant EMAIL ROLE`로만 부여됩니다.
ADMIN(전부) · REVIEWER(심사·2차 승인·서류·문의) · OPERATOR(배급) · SUPPORT(문의). 역할에 없는 작업은 조회만 됩니다.
공지·이벤트 관리는 ADMIN만 접근합니다. 서명된 전자 권리 문서는 서류 화면에서 원문·서명·문서 해시를 확인한 뒤 기존 검토 절차로 승인하거나 보완 요청합니다.

## 배포
`.github/workflows/admin-deploy.yml` — 모든 브랜치에서 타입 검사·빌드, `main`에서만 `wrangler deploy`.
처음 한 번: `npx wrangler secret put EDGE_SERVICE_SECRET`, 그리고 Cloudflare에서 도메인(예: admin.audeniq.com) 연결.
콘텐츠 관리 배포 전에는 core API의 `/api/staff/content-access`와 PostgreSQL `0069` 마이그레이션을 먼저 반영합니다. `CONTENT_DB`는 스튜디오가 사용하는 기존 D1 데이터베이스에 연결합니다.

```sh
bun install
bun run dev:demo   # 체험 데이터로 화면 확인
bun run build      # dist/
```

심사 화면은 서버의 `review_context.allowed_actions`와 `requires_second_approval`을 사용한다.
같은 검사 코드라도 여러 트랙의 결과를 모두 보여 주며, 시스템 원결과와 담당자 결정 후 상태를 구분한다.
현재 제출본의 플랫폼별 배급 준비/승인 상태와 `/releases/{id}/timeline`의 검사·작업·전송·응답 이력을 표시한다.
진행 중인 심사는 화면이 보일 때 15초마다 갱신하고, 결정 입력 중에는 자동 갱신을 멈춘다.
`bun run test`의 화면 회귀 테스트는 Admin deploy CI에서 타입 검사/빌드와 함께 실행된다.
