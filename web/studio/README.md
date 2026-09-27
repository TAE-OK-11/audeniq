# AUDENIQ STUDIO (Workers 배포)

studio.audeniq.com을 서빙하는 Worker예요. React 빌드(`public/`, `app/`에서 `bun run build`)와 공지·이벤트 D1(`audeniq-content`)을 함께 제공합니다. noindex는 검색 제외일 뿐 접근제어가 아닙니다.

## 처음 한 번

```sh
cd web/studio
npx wrangler d1 migrations apply audeniq-content --remote   # 공지·이벤트 테이블 (다시 실행해도 안전)
openssl rand -hex 32                                      # 나온 값을 복사
npx wrangler secret put CONTENT_ADMIN_TOKEN               # 붙여넣기 (32자 이상)
npx wrangler deploy
```

## 공지·이벤트 올리기

1. https://studio.audeniq.com/content-admin 에 들어가 관리자 토큰(`CONTENT_ADMIN_TOKEN`)을 입력해요.
2. `새 공지 쓰기` / `새 이벤트 쓰기` → 저장하면 스튜디오 공지사항·이벤트 화면에 바로 보여요.
   - 게시 시각을 미래로 두면 예약 게시, `중요 공지로 상단에 고정`은 상단 고정.
   - `내리기`는 화면에서만 숨기고 목록에는 남아요. 다시 열어 저장하면 재게시돼요.

## 서버 점검 알리기

`/content-admin` → `서버 점검` 탭 → `점검 일정 추가`에서 시작·예상 종료 시각(한국 시간)과 안내 문구를 넣어요.

- 시작 72시간 전부터 스튜디오 맨 위에 예고 띠가 보여요 (사용자가 닫을 수 있음).
- 시작 시각이 되면 스튜디오 전체가 점검 화면으로 바뀌고, 1분마다 상태를 다시 확인해요. `내리기`하거나 종료 시각이 지나면 자동으로 돌아와요.
- 관리 화면(`/content-admin`)은 점검 중에도 열려요.
- 처음 한 번 `npx wrangler d1 migrations apply audeniq-content --remote`로 `0002_maintenance.sql`을 적용해 주세요. 적용 전에는 점검 기능만 꺼진 채로 동작해요.

그 밖에 스튜디오는 없는 주소(404), 서버 장애(502·503·504·연결 실패), 화면 오류, 새 버전 배포, 오프라인, 쿠키·저장소 차단, 보안 연결 아님, 오래된 브라우저를 스스로 감지해 안내 화면이나 창을 띄워요.

API 규칙과 curl 예시는 `docs/STUDIO_DEPLOYMENT.md`를 참고하세요.

## 확인

```sh
bun run check          # 설정·Worker 테스트
npx wrangler dev       # 로컬 D1로 실행 (.dev.vars에 CONTENT_ADMIN_TOKEN=...)
```
