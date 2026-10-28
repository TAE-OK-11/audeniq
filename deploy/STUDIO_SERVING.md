# Studio 서빙 구조: 백엔드가 화면을 서빙하고, 엣지는 프록시만

> Current CloudFront deployment and caching: [CDN end state](cloudfront/cdn-caching-and-transition.md).
> Worker/Tunnel migration commands below are historical. Do not reconnect Studio to Workers or create tunnel tokens for the CloudFront deployment.


1단계(지금)는 Cloudflare에서 이 구조로 바꿔 시험해요. 2단계(AWS 이주)에서는 엣지만 Cloudflare Worker에서
CloudFront(+ CloudFront Function)로 바뀌고, 백엔드·배포는 그대로예요. DNS와 R2(업로드 저장소)는 계속 Cloudflare예요.

```
1단계 (Cloudflare)
브라우저 ── studio.audeniq.com ── Worker audeniq-studio-proxy ── (터널) api-origin ── api :8080
                                   헤더만 다듬기                                     ├─ /api/*, /ready, /health, /.well-known/oauth-*  → API (boundary)
                                                                                    └─ 그 밖의 경로                                 → 화면 파일 (boundary 바깥)
2단계 (AWS)
브라우저 ── studio.audeniq.com ── CloudFront ── api-origin.audeniq.com:443 ── api (TLS 직접, 같은 화면 파일)
                                  edge.js Function: 헤더만                CloudFront IP 대역·서비스 비밀만 받음
                                  origin custom header: x-audeniq-service, x-audeniq-site
```

## 누가 무엇을 하나

| 일 | 예전 | 지금 | 코드 |
|---|---|---|---|
| 화면 파일 서빙, SPA 폴백, `_redirects`, `_headers`(보안·캐시 헤더), brotli | Workers Static Assets | 백엔드 api | `crates/core/src/studio_static.rs` |
| 브라우저가 보낸 `x-audeniq-*` 버리기 | Worker | Worker (→ CloudFront Function) | `web/shared/transport.ts` `backendHeaders` → `deploy/aws/functions/edge.js` |
| 실제 사용자 IP → `x-audeniq-client-ip` | Worker (`CF-Connecting-IP`) | Worker (→ `event.viewer.ip`) | 같음 |
| 서비스 식별 `x-audeniq-service`(비밀)·`x-audeniq-site` | Worker | Worker (→ origin custom header) | 같음 |
| HSTS | Worker | Worker (→ CloudFront 응답 헤더 정책) | `secureResponse` |
| 배포 | `wrangler deploy` | 서버로 업로드 + 심링크 교체 | `deploy/studio-release.sh`, `.github/workflows/studio-deploy.yml` |

백엔드 `/api` 라우트와 boundary(서비스 비밀·Origin 검사)는 바뀌지 않았어요. 화면 처리와 로그의 사용자 IP는
API 라우터의 모든 층 **바깥**에 덧붙인 층이에요(`studio_static::app`).

## 백엔드의 화면 서빙 (`STUDIO_DIR`)

- `STUDIO_DIR`(production: `/srv/studio/current`)가 있으면 켜져요. 비우면 지금처럼 API만이에요.
- 경로 나누기: `/api`, `/api/*`, `/ready`, `/health`, `/.well-known/oauth-*`는 API로. 나머지는 화면 파일이라 **서비스 비밀이 없어도 403이 아니에요**.
  `x-audeniq-site: admin`인 요청은 화면 파일을 주지 않고 API로 넘겨요(관리자 화면은 자기 Worker가 서빙).
- 순서: `_redirects`(정확한 경로, 쿼리 유지) → 있는 파일 → `/.well-known/*`는 JSON 404 → 확장자 있는 경로는 404(예전 배포의 스크립트가 HTML로 "로드"되지 않게) → 그 밖에는 `/index.html`(SPA 폴백).
- 캐시: `_headers`가 원본이에요. `index.html`과 SPA 폴백은 늘 `no-cache`, `/assets/*`는 `public, max-age=31536000, immutable`(`_headers`에 없어도 이 값).
- 압축: 빌드가 만든 `.br`을 `Accept-Encoding`에 맞춰 보내고 `Vary: Accept-Encoding`. ETag/`If-None-Match` → 304.
- 폴더는 메모리에 한 번 읽어 둬요(지금 빌드 약 5 MB). `current` 심링크가 다른 릴리스를 가리키면 다음 요청에서 다시 읽어요. 새 릴리스가 깨졌으면(`index.html` 없음 등) 이전 화면을 계속 줘요. 첫 배포 전에는 `503 STUDIO_NOT_DEPLOYED`.
- 로그: 모든 로그 줄에 `edge{client_ip=…}` 스팬이 붙어요(API의 `http_request` 줄 포함). 화면 요청은 `static_request` 줄을 남겨요.
  `x-audeniq-client-ip`는 서비스 비밀이 맞을 때만 기록해요(브라우저가 직접 보낸 값은 버려요).

## 배포: 서버가 한다

```
/opt/audeniq/studio/
  current -> releases/<커밋>      # api 컨테이너는 ./studio를 /srv/studio로 읽기 전용 마운트
  releases/<커밋>/                # 최근 5개 + 지금·직전은 늘 남긴다
  release-history.log
```

- main에 Studio가 바뀐 커밋이 들어오면 GitHub Actions **Studio deploy**(CircleCI를 쓰면 `studio` 잡)가 테스트·빌드 →
  `studio.tar.gz`를 서버에 올리고 `./studio-release.sh install <커밋> <파일>`을 실행해요. api 재시작은 없어요.
- 필요한 값은 백엔드 배포와 같아요: production 환경 Secrets `DEPLOY_HOST`, `DEPLOY_USER`, `DEPLOY_SSH_KEY`, `DEPLOY_KNOWN_HOSTS`(변수 `DEPLOY_PATH`, 기본 `/opt/audeniq`). 하나도 없으면 배포만 건너뛰고 빌드 결과(`studio-build`)를 남겨요.
- 서버에서 직접: `./studio-release.sh status`, `./studio-release.sh rollback`(직전 화면), `./studio-release.sh install <커밋> <tar.gz>`.
- 릴리스를 풀 때 링크·바깥 경로가 든 압축은 거절하고, `index.html`이 가리키는 `/assets/*`가 모두 있는지 확인한 뒤에만 `current`를 바꿔요.
- Workers 배포(wrangler)는 CI에서 뺐어요. 프록시 Worker는 바뀔 때만 손으로 올려요.

## 1단계 전환 순서 (Cloudflare)

1. **백엔드 배포**: 이 커밋의 이미지로 배포해요(`compose.production.yaml`에 `STUDIO_DIR`·`./studio` 마운트). 지금 Worker는 `/api`만 넘기므로 사용자에게 바뀌는 건 없어요.
2. **화면 올리기**: Actions → Studio deploy → Run workflow(main). 서버에서 `./studio-release.sh status`로 `current`를 확인해요.
3. **프록시 Worker 올리기** (새 이름이라 비밀을 따로 넣어요):
   ```sh
   cd web/studio
   npx --yes wrangler@4.149.0 deploy
   echo "<production.env의 EDGE_SERVICE_SECRET>" | npx --yes wrangler@4.149.0 secret put EDGE_SERVICE_SECRET
   # 백엔드 주소가 기본(https://api-origin.audeniq.com)과 다르면 BACKEND_URL 변수도
   ```
4. **시험**: `https://audeniq-studio-proxy.<계정>.workers.dev`에서 아래 '검증'을 해요. 여기서는 Origin이 달라 쓰기(로그인 등)는 백엔드가 막아요. 화면·GET API·헤더·IP만 봐요.
5. **전환**: Cloudflare 대시보드 → Workers → `audeniq-studio` → Domains에서 `studio.audeniq.com`을 빼고, `audeniq-studio-proxy`에 Custom Domain으로 붙여요.
6. **롤백**: 5단계를 거꾸로(도메인을 `audeniq-studio`로 되돌리기). 예전 Worker는 지우지 않고 그대로 둬요.
   - 예전 Worker의 화면은 마지막 `wrangler deploy` 때 것이에요. 전환 뒤 바뀐 화면은 롤백하면 빠져요.
   - 화면만 잘못됐으면 DNS를 건드리지 말고 서버에서 `./studio-release.sh rollback`.
7. **정리**(안정된 뒤): `audeniq-studio` Worker 삭제는 AWS 이주 때 함께.

## 검증

```sh
H=https://audeniq-studio-proxy.<계정>.workers.dev   # 전환 뒤에는 https://studio.audeniq.com
curl -sI $H/ | grep -iE '^(HTTP|cache-control|content-security-policy|strict-transport)'   # 200, no-cache, CSP, HSTS
curl -sI $H/releases/123 | grep -iE '^(HTTP|cache-control)'                                # SPA 폴백: 200, no-cache
curl -sI $H/assets/$(curl -s $H/ | grep -oE 'index-[A-Za-z0-9_-]+\.js' | head -1) | grep -i cache-control   # 1년 immutable
curl -sI $H/assets/없는파일.js | head -1                                                    # 404 (HTML 아님)
curl -sI $H/studio | grep -iE '^(HTTP|location)'                                            # 308 → /
curl -s  $H/api/status -H 'x-audeniq-service: forged' -H 'x-audeniq-client-ip: 1.2.3.4'     # 정상 응답 (위조 헤더는 버려짐)
```

- 가짜 `x-audeniq-*`가 백엔드까지 닿지 않는지: 위 마지막 요청 뒤 api 로그(`docker compose logs api | grep http_request | tail`)의
  `client_ip`가 `1.2.3.4`가 아니라 내 실제 IP인지 확인해요. 자동 테스트: `web/studio/test/worker.test.ts`.
- 실제 viewer IP: 화면 요청은 `static_request` 줄의 `client_ip`, API 요청은 `spans`의 `edge.client_ip`.
- 기존 기능: 로그인, 발매 목록, 공지, 점검 화면, 새 버전 안내(옛 스크립트 404 → 새로고침 안내)를 직접 확인해요.
- 자동 테스트: `cargo test -p audeniq-core --lib studio_static`, `cd web/studio && bun test test/`.

## 2단계(AWS CloudFront)

구현과 전환 순서는 [`aws/README.md`](aws/README.md)에 있어요. 이 구조에서 바뀐 것만:

- **Function은 헤더만**: `deploy/aws/functions/edge.js`(cloudfront-js-2.0)가 `backendHeaders`와 같은 허용 목록만 남기고
  `x-audeniq-client-ip = event.viewer.ip`를 넣어요. 응답에서는 Studio `/api`의 CORS 헤더만 맞춰요. 같은지는 `deploy/aws/test/aws.test.ts`가 맞춰 봐요.
- **비밀은 Function에 넣지 않기**: Function 코드는 콘솔·API로 읽혀요. `x-audeniq-service`와 `x-audeniq-site: studio`는 오리진 custom header로 붙여요.
- **SPA 폴백·리다이렉트는 백엔드**: 그래서 Studio S3 버킷과 S3용 SPA 함수가 없어졌어요. 오리진은 백엔드 하나예요.
- **behavior와 캐시**: `/assets/*`만 CachingOptimized(gzip·br 캐시 키, 오리진 요청 정책 없음 → 브라우저 헤더는 오리진에 닿지 않음).
  나머지 `/*`(화면·`/api`)는 CachingDisabled + AllViewerExceptHostHeader, 요청·응답 모두 `edge.js`.
- **헤더 원본은 백엔드**: CSP 등은 `_headers`로 백엔드가 붙여요. CloudFront 응답 헤더 정책에는 HSTS만 둬요.
- **ALB·터널 없음**: api가 `TLS_BIND`(컨테이너 8443 → 서버 443)에서 인증서로 직접 받아요(HTTP/1.1과 h2, CloudFront는 HTTP/1.1로 붙어요)(`crates/core/src/edge_tls.rs`). 인증서는 api가 Let's Encrypt에서 직접 받고 자동 갱신해요(`edge_acme.rs`).
  CloudFront 오리진 IP 대역이 아닌 연결은 TLS 전에 끊고, 서비스 비밀이 없는 요청은 화면 파일까지 403이에요.
  평문 `API_BIND`(8080)는 healthcheck와 전환 전 터널용이라 1단계 동작은 그대로예요.
- **엣지 중립**: 백엔드는 `CF-Connecting-IP`·`X-Forwarded-For` 같은 엣지 고유 헤더를 읽지 않고 `x-audeniq-client-ip`만 읽어요.
- **시간 제한**: CloudFront 오리진 응답 시간 60초(요청하면 180초까지). api는 요청을 120초에서 끊고, 업로드 완료·실시간 스트림은 예외예요.
- **배포는 그대로**: 화면은 계속 서버의 `studio/releases`로 가요. CloudFront 무효화는 필요 없어요(`/assets/*`는 이름에 해시, 나머지는 캐시하지 않음).
