# AUDENIQ STUDIO

Studio is served by the Rust backend through AWS CloudFront at `studio.audeniq.com`.
CI builds `app/` and installs `public/` with `deploy/studio-release.sh`; the backend
loads an atomic release snapshot. API calls use the same origin.

See [CDN caching and transition](../../deploy/cloudfront/cdn-caching-and-transition.md)
for cache policies, release checks and remaining live cleanup. `worker.ts` is a
legacy proxy retained for reference, not the production deployment path.
Its workers.dev and preview URLs are disabled in repository configuration.

## 공지·이벤트 올리기

1. ADMIN 계정으로 로그인한 뒤 https://studio.audeniq.com/admin/content 에 들어가요. `/admin` 메뉴의 `공지·이벤트`에서도 열 수 있어요. 기존 `/content-admin` 주소는 새 화면으로 이동해요.
2. `새 공지 쓰기` / `새 이벤트 쓰기` → 저장하면 스튜디오 공지사항·이벤트 화면에 바로 보여요.
   - 게시 시각을 미래로 두면 예약 게시, `중요 공지로 상단에 고정`은 상단 고정.
   - `내리기`는 화면에서만 숨기고 목록에는 남아요. 다시 열어 저장하면 재게시돼요.

## 서버 점검 알리기

`/admin/content` → `서버 점검` 탭 → `점검 일정 추가`에서 시작·예상 종료 시각(한국 시간)과 안내 문구를 넣어요.

- 시작 72시간 전부터 스튜디오 맨 위에 예고 띠가 보여요 (사용자가 닫을 수 있음).
- 시작 시각이 되면 스튜디오 전체가 점검 화면으로 바뀌고, 1분마다 상태를 다시 확인해요. `내리기`하거나 종료 시각이 지나면 자동으로 돌아와요.
- 관리 화면(`/admin/content`)과 로그인은 점검 중에도 열려요. 계정·권한 검증용 백엔드가 응답해야 게시할 수 있어요.
- 공지·이벤트·점검 정보는 백엔드 PostgreSQL에서 관리해요 (migration 0086).

## 긴급 점검

**방법 1 — 관리 화면 (평소)**: `/admin/content` → `서버 점검` 탭 → `긴급 점검 시작` → 안내 문구·예상 소요(30분·1시간·2시간·4시간·미정) → `지금 시작`.

- 모든 사용자에게 1분 안에(무언가를 누르면 바로) 점검 화면이 떠요. 쓰던 화면은 뒤에 그대로 남아 있어서, 점검이 끝나면 입력하던 내용을 이어서 쓸 수 있어요.
- 발매·심사 등 업무 API는 Worker가 `503 MAINTENANCE`로 막아요. 인증과 D1 공지·이벤트 관리는 유지돼요.
- 같은 패널에서 `30분 연장`, `점검 종료`를 누를 수 있어요. 종료하면 스튜디오가 바로 다시 열려요.

**방법 2 — 비상 스위치 (D1이나 관리 화면이 안 될 때)**:

```sh
cd web/studio
echo on | npx wrangler secret put MAINTENANCE_MODE          # 켜기 (바로 적용)
echo '복구 중이에요. 30분 안에 돌아올게요.' | npx wrangler secret put MAINTENANCE_MESSAGE   # 선택
echo 2026-09-27T03:00:00Z | npx wrangler secret put MAINTENANCE_UNTIL    # 선택: 예상 종료(UTC). 없으면 '종료 시각 미정'
npx wrangler secret delete MAINTENANCE_MODE                  # 끄기
```

처음 한 번 `npx wrangler d1 migrations apply audeniq-content --remote`로 `0002_maintenance.sql`, `0003_maintenance_emergency.sql`을 적용해 주세요. 비상 스위치는 D1 없이도 동작해요.

그 밖에 스튜디오는 없는 주소(404), 서버 장애(502·503·504·연결 실패), 화면 오류, 새 버전 배포, 오프라인, 쿠키·저장소 차단, 보안 연결 아님, 오래된 브라우저를 스스로 감지해 안내 화면이나 창을 띄워요.

API 규칙과 curl 예시는 `docs/STUDIO_DEPLOYMENT.md`를 참고하세요.

## 성능

- `/api/status`와 점검 중 API 차단 검사는 점검 일정 행을 인스턴스에서 10초 기억해 요청마다 D1을 읽지 않아요(판정은 항상 현재 시각으로).
- 공개 공지·이벤트(`GET /api/notices|events[/:id]`)는 데이터센터 캐시에 30초 둬요. 관리 화면에서 글을 바꾸면 해당 캐시를 바로 지워요(다른 데이터센터는 최대 30초 뒤 반영).
- 백엔드 프록시는 필요한 헤더만 전달하고(`x-audeniq-*` 위조 차단, `CF-Connecting-IP` → `x-audeniq-client-ip`), 본문을 읽지 않고 스트림으로 넘겨 압축된 응답이 다시 풀리거나 재압축되지 않아요.

## 확인

```sh
bun run check          # 설정·Worker 테스트
cargo test -p audeniq-core --lib studio_static   # 백엔드 화면 서빙 (SPA 폴백·캐시 헤더·_redirects)
```

배포 순서는 `docs/STUDIO_DEPLOYMENT.md`를 참고하세요. 별도 관리 토큰은 화면에서 필요하지 않으며, 비상 스크립트용 `CONTENT_ADMIN_TOKEN` 설정만 선택적으로 유지합니다.
