# AUDENIQ STUDIO 배포 준비 체크리스트

작성: 2026-09-26 | 대상: `audeniq-studio` Cloudflare Worker

## ✅ 완료

### 보안
- [x] 하드코딩된 시크릿 없음 (소스·빌드 산출물 스캔)
- [x] `dangerouslySetInnerHTML` 1건 — `esc()` 이스케이프 처리됨 (BankLogo)
- [x] 주민등록번호: 메모리 state에만 존재, 저장·로그·전송 없음
- [x] localStorage(`aq.studio.v2.*`): 목 모드 데이터만 저장. 계좌번호는 마스킹(뒤 4자리)만, 첨부 원본(File)은 저장하지 않음
- [x] 의존성(2026-09-26 최신): React 19.3.0 / React Router 8.4.0 / Vite 8.3.1 / TypeScript 7.0.2 / Vitest 5.0.2 — 알려진 CVE 없음
  - React RSC 계열 CVE(CVE-2025-55182 등): 서버 컴포넌트 미사용으로 해당 없음
  - Vite CVE: dev-server 전용, 6.4.3은 패치 버전
- [x] CSP 강화 (`public/_headers`): `/connected/*`에 strict CSP 적용
  - `script-src 'self'` (인라인 스크립트 없음 확인)
  - `style-src 'self' 'unsafe-inline'` (React 인라인 스타일 때문)
  - `frame-src blob:` (첨부 PDF 미리보기)
- [x] 보안 헤더: nosniff, Referrer-Policy, Permissions-Policy, COOP, X-Robots-Tag
- [x] `robots` noindex/nofollow (스테이징 단계)

### 압축
- [x] Cloudflare 엣지 자동 압축 확인: zstd / gzip / brotli 모두 동작
- [x] 별도 빌드 설정 불필요 (Workers Static Assets + CF 엣지 압축)

### 빌드
- [x] CSS syntax warning 수정 (깨진 주석 `not boxed page frames. */`)
- [x] `bun run check` + `bun run build` 통과
- [x] MOCK 모드 환경변수화 (`VITE_MOCK`, `VITE_API_BASE`) — 실제로 `api/client.ts`가 읽도록 연결
- [x] `bun run test` (Vitest 12건) + Playwright E2E 15개 시나리오 통과

## ⚠️ 실제 운영 전환 전 필수

### 1. 백엔드 API 연동 (최우선)
현재 `VITE_MOCK` 기본값 `true` — 목 데이터로 동작, mock 로그인은 자격증명 미검증.
실제 배포 시:
```bash
VITE_MOCK=false VITE_API_BASE=https://api.audeniq.example bun run build
```
- [ ] 실제 백엔드 API 엔드포인트 구현 (`/login`, `/signup`, `/releases` 등)
- [ ] 세션/토큰 인증 방식으로 전환 (현재 mock은 localStorage 플래그)
- [ ] 계정 찾기 API 구현 (현재 "준비 중")
- [ ] 파일 업로드 실구현 (현재 파일명만 저장)
- [ ] 전자서명 무결성 검증 (현재 클라이언트 전용)

### 2. 운영 설정
- [ ] `noindex` 제거 (정식 런칭 시)
- [ ] 커스텀 도메인 연결 + HTTPS 확인
- [ ] 에러 모니터링 (Sentry 등) 연동
- [ ] 접근 로그·감사 로그 정책 수립

### 3. 데이터·법무
- [ ] 개인정보처리방침·이용약관 페이지
- [ ] 주민등록번호 수집 시 법적 근거 재확인 (현재 수집만 하고 미사용)
- [ ] 전자서명 법적 효력 요건 검토

## 참고
- 배포: `/home/hatch/audeniq-f2/web/studio`에서 `~/.config/audeniq/cf-deploy.sh deploy`
- URL: https://audeniq-studio.audeniq-official.workers.dev/connected/
