# AUDENIQ STUDIO — React + TypeScript 앱

`web/studio/public/`(사이트 루트)로 빌드되는 아티스트 포털입니다.

| 도구 | 버전 |
|---|---|
| React / React DOM | 19.3 |
| 라우터 | 자체 경량 경로 라우터 `src/lib/router.tsx` (History API, react-router 호환 API). `/login`처럼 실제 경로를 쓰고, 예전 `/#/login` 주소는 자동으로 바뀐다. 정적 호스팅은 SPA 폴백(없는 경로 → `index.html`)이 필요하다 |
| Vite / @vitejs/plugin-react | 8.3 / 6.1 |
| TypeScript | 7.0 (네이티브 `tsc`) |
| Vitest + jsdom | 5.0 / 30 |
| Node | 22.12 이상 |

```sh
bun install
bun run dev      # http://localhost:5173/
bun run check    # 타입 검사
bun run test     # 단위 테스트
bun run build    # 체험(목) 빌드 → ../public
bun run build:edge  # 서버 연결 빌드 → ../edge-dist (엣지 Worker가 루트에서 서빙)
EDGE_SERVICE_SECRET=... bun run dev:api  # 로컬 API(127.0.0.1:8080)와 연결해 개발
```

## 데이터 모드
- 기본(`VITE_MOCK` 미설정): 브라우저 저장소(`localStorage`, 키 `aq.studio.v2.*`)에 발매·서류·정산·문의가 저장돼 새로고침 후에도 유지됩니다. 아티스트 정보 화면 하단에서 초기화할 수 있습니다.
- 서버 연결(`--mode edge` 또는 `VITE_MOCK=false`): `api/remote.ts`가 같은 출처의 `/api/*`를 호출하고, 엣지 Worker(`crates/edge`)가 Workers VPC → Cloudflare Tunnel → 메인 서버로 전달합니다. 로그인·회원가입, 발매 임시 저장·수정·삭제, 트랙 동기화, 음원·커버 R2 직접 업로드, 사전 점검·동의·접수·서명 신청서, 아티스트 정보, 수령 계좌, 문의(담당자 답변), 알림, 계약서 확인·서명, 권리 증빙 제출, 정산(원장)·지급 요청, 리포트가 모두 서버를 씁니다(`api/portal.ts`, `store/portalSync.ts`). 공지·이벤트는 엣지 Worker가 D1에서 서빙합니다. 실서버 모드의 스토어는 로그인 후 서버에서 채우고 브라우저 저장소에 남기지 않습니다. 배포 흐름은 `docs/STUDIO_DEPLOYMENT.md` 참고.
- 서버 모드 제약: 비밀번호 12자 이상, 음원 WAV·FLAC(최대 512MB), 커버 JPG·PNG(최대 20MB), 미성년 발매 접수는 서버의 법정대리인 절차가 준비될 때까지 보류.

## 구조
- `api/` — API 진입점(`client.ts`), 서버 어댑터(`remote.ts`, `http.ts`), 목 서버(`mock.ts`), 오류 문구(`errors.ts`), 인증 컨텍스트
- `lib/` — 날짜(`date.ts`, 사파리 호환 파싱), 영속 스토어(`store.ts`), 저장소 래퍼, 카탈로그 상수
- `lib/dsp.ts` — 실제 배급 기준 검사: 발매 유형(곡 수·길이), 제목·아티스트 표기(피처링·버전·이모지·홍보 문구), UPC 체크 숫자, ISRC 형식·중복, ℗/© 표기, 기존 아티스트 프로필 주소, 커버 3000~6000px 정사각형, 발매일 준비 기간(일반 14일·신속 3일)
- `lib/audioSpec.ts` — WAV/FLAC 머리를 읽어 44.1kHz·16bit 이상, 2채널 이하, PCM 여부를 업로드 전에 확인
- `lib/application.ts` + `pages/Application.tsx` — 마지막 단계 서명으로 배급 신청서 발급(번호·SHA-256 문서 확인 코드), `/releases/{id}/application`에서 정식 서류로 보기·인쇄·PDF 저장
- `lib/corrections.ts` — 보완 요청 코드(서버 검사 `check_code` 포함) → 신청서 단계·입력칸 매핑. 발매 목록·상세의 ‘보완하기’는 `/upload?edit={id}&fix={code}`로 해당 칸에 바로 이동
- `store/` — 공유 상태(프로필, 수령 정보, 알림, 서류, 정산, 문의)
- `components/` — 모달(퇴장 애니메이션·포커스 트랩), 확인 대화상자, 토스트 스택, 스켈레톤, 오류 경계
- `styles/` — `design.css`·`live.css`(기존 디자인) 위에 `enhance.css`(모션·보강) 레이어

## 성능·경량화
- **라우터**: react-router(48KB min / 15KB gzip) 대신 이 앱이 쓰는 API만 같은 이름으로 구현한 `lib/router.tsx`(약 2KB). 되돌리려면 import 경로를 `'react-router'`로 바꾸고 패키지를 설치하면 됩니다.
- **CSS 정리**: 빌드 때만 `build/purge-css.ts`가 소스 문자열에 없는 클래스·ID 규칙과 안 쓰는 `@keyframes`를 제거하고, 주석·공백만 지우는 안전한 압축을 합니다(183KB → 154KB, gzip 36KB → 30KB). Lightning CSS 압축기는 같은 선택자가 뒤에 다시 나오면 앞 규칙의 `!important`를 잘못 지우는 문제가 있어 `cssMinify: false`로 꺼 두었습니다. 다시 켜지 마세요. 클래스 이름을 문자열 조합으로 만든다면 `is-${x}`처럼 접두사를 문자열 안에 남겨 주세요.
- **청크 프리페치**: 시작 시 현재 경로 청크를 인증 확인과 병렬로 받고, 유휴 시간에 홈·발매 목록·상세를, 메뉴 호버 시 해당 화면을 미리 받습니다(데이터 절약 모드·2G에서는 생략).
- **캐시**: `/assets/*`(해시 파일)는 1년 immutable, `index.html`은 no-cache, 고정 이름 파일은 `/static/`.
- **배포 후 청크 404**: 동적 import/프리로드 실패 시 한 번만 자동 새로고침합니다.
