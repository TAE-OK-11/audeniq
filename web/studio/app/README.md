# AUDENIQ STUDIO — React + TypeScript 앱

`web/studio/public/connected/`로 빌드되는 아티스트 포털입니다.

| 도구 | 버전 |
|---|---|
| React / React DOM | 19.3 |
| React Router | 8.4 (`react-router` 단일 패키지) |
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
