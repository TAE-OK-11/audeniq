# AUDENIQ STUDIO — React + TypeScript 앱

`web/studio/public/connected/`로 빌드되는 아티스트 포털입니다.

| 도구 | 버전 |
|---|---|
| React / React DOM | 19.3 |
| 라우터 | 자체 경량 해시 라우터 `src/lib/router.tsx` (react-router 호환 API) |
| Vite / @vitejs/plugin-react | 8.3 / 6.1 |
| TypeScript | 7.0 (네이티브 `tsc`) |
| Vitest + jsdom | 5.0 / 30 |
| Node | 22.12 이상 |

```sh
bun install
bun run dev      # http://localhost:5173/connected/
bun run check    # 타입 검사
bun run test     # 단위 테스트
bun run build    # ../public/connected 로 출력
```

## 데이터 모드
- 기본(`VITE_MOCK` 미설정): 브라우저 저장소(`localStorage`, 키 `aq.studio.v2.*`)에 발매·서류·정산·문의가 저장돼 새로고침 후에도 유지됩니다. 아티스트 정보 화면 하단에서 초기화할 수 있습니다.
- `VITE_MOCK=false VITE_API_BASE=https://api.example bun run build`: `api/client.ts`가 실제 API(`/api/auth/*`, `/api/orgs/{org}/releases*`)를 호출합니다. 서명·정산·문의 등은 아직 로컬 저장소 기반입니다.

## 구조
- `api/` — API 클라이언트, 목 서버(`mock.ts`), 인증 컨텍스트
- `lib/` — 날짜(`date.ts`, 사파리 호환 파싱), 영속 스토어(`store.ts`), 저장소 래퍼, 카탈로그 상수
- `store/` — 공유 상태(프로필, 수령 정보, 알림, 서류, 정산, 문의)
- `components/` — 모달(퇴장 애니메이션·포커스 트랩), 확인 대화상자, 토스트 스택, 스켈레톤, 오류 경계
- `styles/` — `design.css`·`live.css`(기존 디자인) 위에 `enhance.css`(모션·보강) 레이어

## 성능·경량화
- **라우터**: react-router(48KB min / 15KB gzip) 대신 이 앱이 쓰는 API만 같은 이름으로 구현한 `lib/router.tsx`(약 2KB). 되돌리려면 import 경로를 `'react-router'`로 바꾸고 패키지를 설치하면 됩니다.
- **CSS 정리**: 빌드 때만 `build/purge-css.ts`가 소스 문자열에 없는 클래스·ID 규칙과 안 쓰는 `@keyframes`를 제거하고, 주석·공백만 지우는 안전한 압축을 합니다(183KB → 154KB, gzip 36KB → 30KB). Lightning CSS 압축기는 같은 선택자가 뒤에 다시 나오면 앞 규칙의 `!important`를 잘못 지우는 문제가 있어 `cssMinify: false`로 꺼 두었습니다. 다시 켜지 마세요. 클래스 이름을 문자열 조합으로 만든다면 `is-${x}`처럼 접두사를 문자열 안에 남겨 주세요.
- **청크 프리페치**: 시작 시 현재 경로 청크를 인증 확인과 병렬로 받고, 유휴 시간에 홈·발매 목록·상세를, 메뉴 호버 시 해당 화면을 미리 받습니다(데이터 절약 모드·2G에서는 생략).
- **캐시**: `/connected/assets/*`(해시 파일)는 1년 immutable, `index.html`은 no-cache, 고정 이름 파일은 `/connected/static/`.
- **배포 후 청크 404**: 동적 import/프리로드 실패 시 한 번만 자동 새로고침합니다.
