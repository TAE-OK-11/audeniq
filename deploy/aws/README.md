# 전부 AWS로: CloudFront → 백엔드(443 직접) + S3

> 지금(개발 단계)은 Cloudflare Worker가 프록시하고 터널로 백엔드에 넘겨요. 이 문서는 CDN을 AWS CloudFront로 옮기는 구성이에요.
> 설문(survey.audeniq.com)은 베타 전까지 D1 Worker 그대로라 이 구성에 없어요. R2(업로드 저장소)와 DNS는 Cloudflare 그대로예요.

ALB도 터널도 없어요. 백엔드 api가 공인 인증서를 직접 들고 서버 443에서 CloudFront의 요청만 받아요.
브라우저 ↔ CloudFront는 h2·h3, CloudFront ↔ 백엔드는 HTTP/1.1이에요(CloudFront 제약). 백엔드는 h2도 ALPN으로 받아요.

```
브라우저 ── studio.audeniq.com ── CloudFront ──────────────── origin.audeniq.com:443 ── api (TLS 직접, 컨테이너 8443)
                                  │ edge.js (viewer-request/response)     ├─ CloudFront 오리진 IP 대역만 연결 허용 (그 밖은 TLS 전에 끊음)
                                  │ origin custom header:                  ├─ 서비스 비밀 없는 요청은 화면 파일까지 403
                                  │   x-audeniq-service(비밀), x-audeniq-site └─ 화면(SPA 폴백·_headers·_redirects)·/api 모두 api가
                                  └ /assets/* 만 캐시
브라우저 ── admin.audeniq.com ── CloudFront ─┬─ /*      → S3 (admin.js: SPA 폴백)
                                            └─ /api/*  → 위와 같은 백엔드 (edge.js)
브라우저 ── audeniq.com ──────── CloudFront ── S3 (landing.js: 랜딩 라우팅)
```

| 예전 (Cloudflare) | 지금 (AWS) | 코드 |
|---|---|---|
| 프록시 Worker의 헤더 다듬기 (`backendHeaders`: 필요한 헤더만, `x-audeniq-*` 버림, 사용자 IP) | CloudFront Function `edge.js` | `functions/edge.js` (Worker 규칙과 같은지 `test/aws.test.ts`가 맞춰 봐요) |
| 서비스 비밀·사이트 헤더 (Worker secret) | CloudFront origin custom header | `template.ts` |
| HSTS (`secureResponse`) | CloudFront 응답 헤더 정책 | `template.ts` |
| Studio `/api` CORS, 관리자 `/api` no-store | `edge.js` viewer-response | `functions/edge.js` |
| Studio 화면 서빙 | 백엔드 api 그대로 (`STUDIO_DIR`) | `crates/core/src/studio_static.rs`, [`../STUDIO_SERVING.md`](../STUDIO_SERVING.md) |
| 관리자·랜딩 화면 (Workers Static Assets, SPA 폴백, 랜딩 라우팅) | S3 + CloudFront Function | `functions/admin.js`, `functions/landing.js` |
| Cloudflare Tunnel (TLS는 Cloudflare) | api 내장 HTTPS, 서버 443 | `crates/core/src/edge_tls.rs`, `deploy/compose.production.yaml` |
| 인증서 (Cloudflare가 관리) | api가 Let's Encrypt에서 직접 받고 자동 갱신 (TLS-ALPN-01, 443 하나로) | `crates/core/src/edge_acme.rs` (대안: `deploy/tls-cert.sh` → `./tls`) |
| 공지·이벤트·서버 점검 (D1) | 백엔드 + Postgres `content` 스키마 | `crates/core/src/site_content.rs`, 마이그레이션 0086 |

| 앱 | 화면 주소 | 화면 | 백엔드로 가는 경로 |
|---|---|---|---|
| studio | studio.audeniq.com | 백엔드 (`deploy/studio-release.sh`로 서버에 배포) | 전부 (`/assets/*`만 CloudFront 캐시) |
| admin | admin.audeniq.com | S3 `web/admin/dist` | `/api/*` (관리자 경로만 열림) |
| landing | audeniq.com, www.audeniq.com | S3 `web/landing/public` | 없음 |

브라우저 입장에서는 화면과 `/api`가 계속 **같은 주소**예요. 그래서 프론트 코드, 세션 쿠키, CSRF, CSP는 그대로예요.

## 보안 경계: AWS(CloudFront)가 아니면 거절

백엔드 HTTPS 리스너(`TLS_BIND`)는 세 겹으로 막아요.

1. **연결 출처**: AWS가 공개하는 `CLOUDFRONT_ORIGIN_FACING` IP 대역에서 온 TCP 연결만 받아요. 그 밖의 연결은 TLS 핸드셰이크 전에 끊어요(로그: `HTTPS connection refused: not a CloudFront origin address`, 1분에 한 줄).
   - 대역은 api가 `https://ip-ranges.amazonaws.com/ip-ranges.json`을 6시간마다 받아 바꿔요(실패하면 10분 뒤 다시, 그동안은 이전 목록). 처음 뜰 때는 저장소 스냅샷 `config/cloudfront-origin-facing.json`(갱신: `python3 scripts/cloudfront-ranges.py > config/cloudfront-origin-facing.json`)을 써요.
   - production에서는 `TLS_ALLOWED_SOURCES=cloudfront`만 돼요(`any`면 시작하지 않아요).
   - 예외 하나: api가 Let's Encrypt 인증서를 받는 몇 초 동안만, ALPN `acme-tls/1` + 그 도메인 SNI인 검증 연결에 검증용 인증서로 답하고 바로 닫아요(요청은 받지 않아요, 로그: `ACME TLS-ALPN-01 challenge answered`). 그 밖의 연결은 그때도 끊어요.
2. **서비스 비밀**: 이 리스너로 온 요청은 화면 파일까지 모두 `x-audeniq-service`(= `EDGE_SERVICE_SECRET`)가 맞아야 해요. 다른 AWS 계정이 자기 CloudFront로 이 서버를 오리진으로 걸어도(IP는 CloudFront라 1을 통과) 403이에요. 비밀은 CloudFront origin custom header로만 붙고, 브라우저가 같은 이름을 보내면 `edge.js`가 지우고 CloudFront가 덮어써요.
3. **헤더 정리**: `edge.js`가 프록시 Worker와 같은 허용 목록만 남겨요. 사용자 IP는 `event.viewer.ip` → `x-audeniq-client-ip`(백엔드는 서비스 비밀이 맞을 때만 믿어요). `x-audeniq-site=admin`이면 관리자 경로만 열리는 규칙(`crates/core/src/sites.rs`)은 그대로예요.

방화벽(서버 보안 그룹): Let's Encrypt 자동 발급(`TLS_ACME_DOMAIN`)을 쓰면 **443은 인터넷 전체에 열어 둬요**. Let's Encrypt 검증은 여러 곳에서 오고 주소가 공개돼 있지 않아서, 거르기는 위 1을 api가 해요.
파일 인증서(`tls-cert.sh`)만 쓴다면 443을 CloudFront 관리형 prefix list `com.amazonaws.global.cloudfront.origin-facing`로 좁혀도 돼요(AWS 서버일 때).
평문 8080은 컨테이너 안 healthcheck와 (전환 전까지) 터널용이고, 서버 밖으로 열지 않아요.

## 인증서: api 내장 Let's Encrypt

CloudFront는 오리진 인증서를 공인 CA로 검증하고, 이름이 `ApiOriginDomain`과 맞아야 해요(자체 서명·Cloudflare Origin CA는 안 돼요).
`production.env`에 `TLS_ACME_DOMAIN=origin.audeniq.com`을 넣으면 api가 알아서 해요(`crates/core/src/edge_acme.rs`).

- **발급**: 처음 뜰 때 저장된 인증서가 없으면 Let's Encrypt에 주문하고 **TLS-ALPN-01**로 검증받아요. 서버 443만 있으면 돼요(80 포트·DNS API 토큰·certbot·cron 없음). 보통 몇 초, 그동안 HTTPS 핸드셰이크만 실패하고 api(평문·healthcheck)는 그대로 떠요.
- **갱신**: 12시간마다 보고 유효 기간의 마지막 1/3에 들어서면(90일 인증서면 만료 30일 전) 새로 받아요. 재시작 없이 다음 연결부터 새 인증서예요. 실패하면 5분부터 두 배씩(최대 6시간) 다시 하고, 그동안은 지금 인증서를 계속 써요.
- **보관**: compose 볼륨 `api_tls`(`/var/lib/audeniq-tls/letsencrypt/<도메인>/`, 계정 키 `account.json`). 재시작·재배포해도 다시 받지 않아요. 볼륨을 지우면 새로 받아요(Let's Encrypt 주당 발급 한도: 같은 이름 5번).
- **처음 시험**: `TLS_ACME_DIRECTORY=staging`이면 발급 한도가 넉넉한 시험 서버(신뢰되지 않는 인증서, CloudFront는 거절)예요. 로그에 `Let's Encrypt certificate issued`가 나오면 `production`으로 바꿔요(따로 보관돼서 섞이지 않아요).
- **로그**: `Let's Encrypt certificate loaded|issued`(not_after 포함), 실패는 `Let's Encrypt certificate not issued; keeping the current one`(이유·다음 시도까지 초).
- **조건**: `origin.audeniq.com`의 DNS(A)가 이 서버를 가리키고(DNS only), 서버 443이 인터넷에 열려 있어야 해요.

다른 인증서를 쓰려면 `TLS_ACME_DOMAIN`을 비우고 `./tls`에 PEM을 둬요: `./tls-cert.sh issue <주소> <메일>`(Let's Encrypt DNS-01, Cloudflare DNS 토큰, cron으로 `renew`) 또는 `./tls-cert.sh install fullchain.pem privkey.pem`(ACM 내보내기 인증서 등). api가 5분마다 다시 읽어요.

## 파일

| 파일 | 역할 |
|---|---|
| `apps.ts` | 앱 목록 (주소, 오리진 종류 `backend`/`s3`, 빌드 폴더, `_headers`, 백엔드 경로, 사이트 이름, CORS) |
| `template.ts` | CloudFormation 템플릿 생성 (CloudFront, 함수, 응답 헤더 정책, S3 앱은 S3 + OAC) |
| `functions/edge.js` | 백엔드로 가는 요청·응답 (헤더 허용 목록, 사용자 IP, CORS·no-store) |
| `functions/admin.js`, `functions/landing.js` | S3 화면 라우팅 (SPA 폴백, 랜딩 리다이렉트·404) |
| `deploy-stack.sh` | 스택 `audeniq-<app>-ui` 생성·갱신 |
| `publish.ts` / `ci-publish.sh` | admin·landing 빌드 결과를 S3에 올리고 무효화 (studio는 건너뜀) |
| `migrate-d1.ts` | D1 데이터 → Postgres 가져오기 SQL 생성 |
| `crates/core/src/edge_tls.rs`, `edge_acme.rs` | 백엔드 HTTPS 리스너(CloudFront 대역·서비스 비밀), 내장 Let's Encrypt |
| `../tls-cert.sh` | (대안) 바깥에서 받는 인증서 발급·갱신·설치 |
| `../compose.tls.yaml` | HTTPS를 켰을 때만 붙는 compose 파일 (api만 서버 443) |

## 전환 순서

1. **백엔드 배포**: 이 커밋의 이미지로 배포해요. `TLS_BIND`가 비어 있어서 아직 아무것도 바뀌지 않아요(터널 그대로).
   - 아직 안 했다면 마이그레이션 0086 이후의 `deploy/grants.sql` 재실행, `production.env`에 `ADMIN_ORIGIN`.
2. **오리진 주소**: Cloudflare DNS에 `origin.audeniq.com A <서버 공인 IP>`를 **DNS only**(회색 구름)로 만들어요. 터널이 쓰는 `api-origin.audeniq.com`은 건드리지 않아요.
3. **방화벽**: 서버 방화벽·보안 그룹에서 TCP 443을 인터넷에 열어요(Let's Encrypt 검증. CloudFront가 아닌 연결은 api가 끊어요).
4. **HTTPS 켜기**: `production.env`에 아래를 넣고 `./update.sh --force`(같은 버전을 다시 배포)로 api를 다시 올려요.
   ```sh
   TLS_BIND=0.0.0.0:8443
   TLS_ACME_DOMAIN=origin.audeniq.com
   TLS_ACME_EMAIL=ops@audeniq.com
   TLS_ACME_DIRECTORY=staging      # 처음 한 번 시험. issued 로그를 보면 production으로 바꾸고 다시 ./update.sh --force
   ```
   `deploy.sh`가 `TLS_BIND`를 보고 `compose.tls.yaml`(서버 443 → 컨테이너 8443)을 붙여요.
   - 로그 확인(`docker compose -p audeniq-production logs api | grep -E 'HTTPS listener|Let.s Encrypt|IP ranges'`): `HTTPS listener (TLS_BIND)`, `Let's Encrypt certificate issued`, `CloudFront origin-facing IP ranges updated`.
   - 바깥에서 `curl -v https://origin.audeniq.com/` → 연결이 끊겨야 정상(CloudFront가 아님).
5. **데이터 옮기기**(D1 공지·점검, 1차): 관리 화면에서 글을 잠시 쓰지 않는 시간에 실행해요.
   ```sh
   CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=... node --experimental-strip-types deploy/aws/migrate-d1.ts > d1-import.sql
   psql "$OWNER_DATABASE_URL" -v ON_ERROR_STOP=1 -f d1-import.sql
   ```
6. **화면 주소 인증서**: us-east-1 ACM에서 `studio.audeniq.com` 등(또는 `*.audeniq.com` + `audeniq.com`)을 발급해요.
7. **스택 만들기**(앱마다):
   ```sh
   AWS_UI_CERT_ARN=arn:aws:acm:us-east-1:...:certificate/... \
   API_ORIGIN_DOMAIN=origin.audeniq.com EDGE_SERVICE_SECRET="<production.env 값>" \
     deploy/aws/deploy-stack.sh studio        # admin도 같은 값, landing은 API 값 불필요
   ```
   - 예전에 S3로 만든 studio 스택이 있으면 같은 명령으로 갱신돼요. 버킷은 `Retain`이라 남으니 확인 뒤 손으로 지워요.
8. **화면 올리기**: studio는 이미 서버에 있어요(`./studio-release.sh status`). admin·landing은 빌드한 뒤 `node --experimental-strip-types deploy/aws/publish.ts admin`.
   출력된 `dxxxx.cloudfront.net`에서 아래 '검증'을 해요(다른 주소라 로그인 같은 쓰기는 Origin 검사로 막혀요. 화면·GET API·헤더·IP만 봐요).
9. **DNS 전환**(앱마다): Worker의 커스텀 도메인을 빼고 `studio CNAME dxxxx.cloudfront.net`을 **DNS only**로 넣어요.
   - 전환 직전에 5단계를 한 번 더 실행해서 마지막 글까지 옮겨요. 이미 있는 id는 건너뛰어요.
10. **확인**: 로그인, 공지 읽기·쓰기, 관리자 점검 등록 → 스튜디오 점검 화면, 백엔드 로그의 사용자 IP.
11. **정리**(안정된 뒤):
    - `compose.production.yaml`의 `tunnel` 서비스와 `tunnel-token`, Cloudflare Tunnel과 `api-origin` DNS를 지워요. 그 뒤 `API_BIND`는 `127.0.0.1:8080`(healthcheck만)으로 좁혀도 돼요.
    - Cloudflare의 Workers(`audeniq-studio`, `audeniq-studio-proxy`, `audeniq-admin`, `audeniq-web`)와 D1 `audeniq-content`를 지워요.
    - studio·admin·landing의 `worker.ts`·`wrangler.jsonc`, `web/studio/migrations`, `crates/edge`를 저장소에서 지워요.

롤백: 9단계 전까지는 사용자에게 바뀌는 게 없어요. 9단계 뒤에는 DNS를 되돌리면 Worker가 다시 받아요(터널은 정리 전까지 그대로 돌아요).
HTTPS만 끄려면 `TLS_BIND=`로 비우고 api를 다시 올려요.

## 검증

```sh
H=https://dxxxx.cloudfront.net        # 전환 뒤에는 https://studio.audeniq.com
curl -sI $H/ | grep -iE '^(HTTP|cache-control|content-security-policy|strict-transport)'   # 200, no-cache, CSP(백엔드), HSTS(CloudFront)
curl -sI $H/releases/123 | grep -iE '^(HTTP|cache-control)'                                # SPA 폴백: 200, no-cache
curl -sI $H/assets/$(curl -s $H/ | grep -oE 'index-[A-Za-z0-9_-]+\.js' | head -1) | grep -iE 'cache-control|x-cache'   # 1년 immutable, 두 번째부터 Hit
curl -s  $H/api/status -H 'x-audeniq-service: forged' -H 'x-audeniq-client-ip: 1.2.3.4'     # 정상 응답 (위조 헤더는 버려짐)
curl -sv https://origin.audeniq.com/ 2>&1 | tail -3                                           # 연결 끊김 (CloudFront가 아님)
```

- api 로그(`docker compose ... logs api | grep -E 'static_request|http_request' | tail`)의 `client_ip`가 `1.2.3.4`가 아니라 내 IP인지 확인해요.
- 자동 테스트: `node --experimental-strip-types --test deploy/aws/test/aws.test.ts`, `cargo test -p audeniq-core --lib edge_tls`.

## CI

- **CircleCI**: Context `audeniq-production`에 `AWS_UI_ROLE_ARN`(CircleCI OIDC로 맡을 역할)과 선택으로 `AWS_UI_REGION`(기본 `ap-northeast-2`)을 넣어요.
- **GitHub Actions**: 저장소 Variables에 같은 이름으로 넣어요. 신뢰 정책은 `repo:tae-ok-11/audeniq:ref:refs/heads/main`이에요.
- Studio 화면은 S3가 아니라 지금처럼 **Studio deploy** 워크플로가 서버로 올려요. CloudFront 무효화도 필요 없어요(`/assets/*`는 이름에 해시, 나머지는 캐시하지 않음).

값이 없으면 admin·landing 화면 S3 배포는 건너뛰어요. CI 역할에 필요한 권한(스택 생성은 사람이 해요):

```json
{
  "Version": "2012-10-17",
  "Statement": [
    { "Effect": "Allow", "Action": "cloudformation:DescribeStacks",
      "Resource": "arn:aws:cloudformation:*:*:stack/audeniq-*-ui/*" },
    { "Effect": "Allow", "Action": ["s3:ListBucket", "s3:PutObject"],
      "Resource": ["arn:aws:s3:::audeniq-*-ui-bucket*", "arn:aws:s3:::audeniq-*-ui-bucket*/*"] },
    { "Effect": "Allow", "Action": "cloudfront:CreateInvalidation", "Resource": "*" }
  ]
}
```

## 참고

- `_headers`가 계속 보안 헤더와 캐시 규칙의 원본이에요. Studio는 백엔드가 읽어서 배포만 하면 되고, admin·landing은 CSP를 바꾸면 `deploy-stack.sh`를 다시 실행하세요.
- CloudFront 오리진 응답 시간은 60초예요. api는 요청을 120초에서 끊고(업로드 완료·실시간 스트림 제외), 실시간 알림은 20초마다 keep-alive라 끊기지 않아요. 60초를 넘는 요청이 생기면 AWS에 한도 상향(최대 180초)을 요청하고 `template.ts`의 `OriginReadTimeout`을 올려요.
- 비상 점검 스위치는 이제 Worker secret이 아니라 백엔드 환경변수예요. `production.env`에 `MAINTENANCE_MODE=on`을 넣고 api를 재시작해요.
  - 평소에는 관리자 콘솔의 '서버 점검'으로 등록해요. 10초 안에 반영돼요.
  - 로그인이나 관리자 콘솔에 들어갈 수 없을 때는 서버에서 CLI로 비상 업무를 해요. 점검과 긴급 공지만 다루고, 배급 같은 평소 업무는 콘솔에서 해요. DB에 직접 쓰고, 도는 api는 10초 안에 반영해요. 스키마 소유자 `DATABASE_URL`이 필요해요.
    - 지금 상태와 목록: `audeniq-admin maintenance status`, `audeniq-admin maintenance list`
    - 긴급 점검 시작: `audeniq-admin --operator 이름 maintenance start [--minutes 60] [--title 제목] [--message 안내]`. `--minutes`를 빼면 종료 시각 미정이에요.
    - 연장·종료(해제): `audeniq-admin --operator 이름 maintenance extend --minutes 30`, `audeniq-admin --operator 이름 maintenance stop`
    - 점검 예약: `audeniq-admin --operator 이름 maintenance schedule --from "2026-10-24 02:00" --to "2026-10-24 04:00" [--announce-from 시각] [--title 제목] [--message 안내]`. 시각은 한국 시간이고 UTC `…Z`도 받아요. 예고 배너는 `--announce-from`부터(기본은 지금) 보여요.
    - 예약 취소: `audeniq-admin --operator 이름 maintenance cancel ID`. 진행 중인 점검은 `stop`으로 끝내요.
    - 긴급 공지: `audeniq-admin --operator 이름 notice post --title 제목 --body 내용 [--pin] [--at 시각]`(긴 글은 `--body-file 파일`), `notice list`, `notice remove ID`. 공지는 점검 중에도 보여요.
    - 관리자 콘솔과 같은 기록이라 콘솔에서 이어서 고칠 수 있어요. 바꿀 때마다 감사 기록(`audeniq-admin:이름`)이 남아요. `MAINTENANCE_MODE` 스위치로 켠 점검은 CLI로 끌 수 없어요. 환경변수를 지우고 api를 재시작해요.
- 공개 공지·이벤트는 `Cache-Control: public, max-age=30`이에요. CloudFront `/api`는 캐시하지 않아서 브라우저만 30초 캐시해요.
- 업로드는 지금처럼 브라우저가 저장소(R2) presigned URL로 직접 올려서 CloudFront를 거치지 않아요.
- 검증: `node --experimental-strip-types --test deploy/aws/test/aws.test.ts`, `cargo test -p audeniq-core --lib edge_tls`, `cargo test -p audeniq-core --test site_content`
