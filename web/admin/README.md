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
- `src/api/staff.ts` — 스태프 API, `src/api/mock.ts` — 체험 데이터(`VITE_MOCK=true`일 때만)
- `src/pages/` — 오늘의 업무 · 발매 심사 · 심사 상세 · 2차 승인 · 서류 · 문의 · 배급 · DSP · 지급 요청

## 권한
로그인은 스튜디오 계정과 같고, 스태프 역할은 `audeniq-admin staff grant EMAIL ROLE`로만 부여됩니다.
ADMIN(전부) · REVIEWER(심사·2차 승인·서류·문의) · OPERATOR(배급) · SUPPORT(문의). 역할에 없는 작업은 조회만 됩니다.

## 배포
`.github/workflows/admin-deploy.yml` — 모든 브랜치에서 타입 검사·빌드, `main`에서만 `wrangler deploy`.
처음 한 번: `npx wrangler secret put EDGE_SERVICE_SECRET`, 그리고 Cloudflare에서 도메인(예: admin.audeniq.com) 연결.

```sh
bun install
bun run dev:demo   # 체험 데이터로 화면 확인
bun run build      # dist/
```
