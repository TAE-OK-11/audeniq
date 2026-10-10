# 개발 단계 화면 서빙: Cloudflare CDN(Workers 없음) → 터널 → Linode 서버

AWS로 옮기기 전까지는 Workers를 쓰지 않고, 서버의 `web`(nginx) 서비스가 화면과 `/api`를 받아요.
Cloudflare는 프록시(주황 구름) CDN과 터널만 맡아요. AWS 구성(`deploy/aws`)은 나중에 그대로 쓸 수 있어요.

```
브라우저 ── Cloudflare CDN ── 터널(cloudflared) ── web:8080 (nginx) ─┬─ 화면: ./web/site/<app>
                                                                     └─ /api/*, /ready → api:8080
                                                                        + x-audeniq-service, x-audeniq-site, x-audeniq-client-ip
```

| 예전 Worker가 하던 일 | 지금 |
|---|---|
| 정적 파일, SPA 폴백, `_redirects`, 랜딩 라우팅 | nginx 설정 (`nginx.mjs`가 만든다) |
| 보안 헤더(`_headers`), Cache-Control | nginx `add_header` (`_headers`가 원본) |
| 공지·이벤트·서버 점검(D1), 점검 중 503, 비상 스위치 | 백엔드 + Postgres (`crates/core/src/site_content.rs`, 마이그레이션 0086) |
| 관리자 콘솔 경로 제한·Origin 확인(인증 보조) | 백엔드 (`crates/core/src/sites.rs`) |
| 설문 접수(D1, Turnstile) | 백엔드 (`crates/core/src/survey.rs`) |
| 서비스 비밀·사용자 IP(CF-Connecting-IP) 전달, 브라우저 헤더 정리 | nginx `proxy_set_header` |

`web`은 터널만 닿는 내부 네트워크(`backend`)에 있고 호스트 포트를 열지 않아요. 그래서 `CF-Connecting-IP`는 Cloudflare가 덮어쓴 값만 들어와요.

## 파일

| 파일 | 역할 |
|---|---|
| `nginx.mjs` | nginx 설정 템플릿 생성 (앱 목록은 `deploy/aws/apps.mjs`와 공용) |
| `publish.sh` | 빌드한 화면을 서버 `./web/site/<app>`에 올리고, 설정이 바뀌었으면 `./deploy.sh --web`으로 nginx만 다시 띄워요 |
| `test/web.test.mjs` | 설정 내용 확인 |

## 처음 한 번

1. **백엔드 배포**: `web` 서비스와 마이그레이션 0086이 포함된 버전을 배포하고 `grants.sql`을 다시 실행해요.
   - `production.env`에 `ADMIN_ORIGIN=https://admin.audeniq.com`, `SURVEY_ORIGIN=https://survey.audeniq.com`(설문을 쓰면 `TURNSTILE_*`도)을 넣어요.
2. **D1 데이터 옮기기**(필요하면): `node deploy/aws/migrate-d1.mjs > d1-import.sql` → `psql "$OWNER_DATABASE_URL" -v ON_ERROR_STOP=1 -f d1-import.sql`
3. **화면 올리기**: 각 앱을 빌드한 뒤 다음을 실행해요.
   ```sh
   DEPLOY_HOST=... DEPLOY_USER=... deploy/web/publish.sh studio admin survey landing
   ```
4. **터널 경로**: Cloudflare Zero Trust → Networks → Tunnels → (지금 터널) → Public hostnames에 추가해요.
   - 서비스는 모두 `http://web:8080`이에요.
   - 예: `studio.audeniq.com`, `admin.audeniq.com`, `survey.audeniq.com`, `audeniq.com`, `www.audeniq.com`
   - 같은 이름이 Worker 커스텀 도메인에 붙어 있으면 먼저 Worker에서 떼요(Workers → 해당 Worker → Domains).
   - 지금 백엔드용으로 쓰는 터널 경로(`api-origin.audeniq.com → http://api:8080`)는 그대로 둬요.
5. **Cloudflare 설정**: SSL/TLS → Edge Certificates에서 *Always Use HTTPS*를 켜요. nginx는 터널 안쪽에서 HTTP로 받아요.
6. **확인**: 로그인, 공지 읽기·쓰기, 관리자 점검 등록 → 스튜디오 점검 화면, 새로고침한 화면 경로, 설문 제출을 확인해요.

되돌리기는 4단계의 공개 호스트 이름을 지우고 Worker 커스텀 도메인을 다시 붙이면 돼요.

## CI

CircleCI Context `audeniq-production`에 `UI_ON_SERVER=true`를 넣으면, main의 frontend 작업이 Worker 배포 뒤 화면을 서버로도 올려요(`DEPLOY_*` 사용).
전환이 끝나면 CI의 `wrangler deploy`와 D1 단계, Cloudflare의 Workers·D1을 정리하면 돼요.

## 로컬 확인

nginx만 있으면 실제로 띄워 볼 수 있어요.
- 생성한 설정에서 `api:8080`을 로컬 스텁 주소로, `${EDGE_SERVICE_SECRET}`을 임의 값으로 바꿔요.
- `curl -H 'Host: studio.audeniq.com' http://127.0.0.1:<포트>/...`로 경로를 확인해요.
- 이번 변경은 SPA 폴백, 리다이렉트(상대 Location), 랜딩 허용 목록·www 이동, 설문 404 페이지, `/api` 헤더 덮어쓰기, gzip, 보안 헤더를 이 방법으로 확인했어요.

## 참고

- 비상 점검 스위치는 백엔드 환경변수 `MAINTENANCE_MODE=on`이에요(api 재시작). 평소에는 관리자 콘솔의 '서버 점검'을 써요.
- 해시 에셋은 `immutable`이라 Cloudflare가 캐시하고, HTML은 `no-cache`라 매번 확인해요. 배포 뒤 캐시를 지울 필요가 없어요.
- 업로드는 지금처럼 브라우저가 R2 presigned URL로 직접 올려서 서버를 거치지 않아요.
