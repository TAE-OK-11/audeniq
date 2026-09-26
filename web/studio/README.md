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

API 규칙과 curl 예시는 `docs/STUDIO_DEPLOYMENT.md`를 참고하세요.

## 확인

```sh
bun run check          # 설정·Worker 테스트
npx wrangler dev       # 로컬 D1로 실행 (.dev.vars에 CONTENT_ADMIN_TOKEN=...)
```
