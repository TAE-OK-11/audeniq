# 화면(UI)은 AWS CloudFront + S3, `/api`는 Cloudflare Workers

```
브라우저 ── studio.audeniq.com ── CloudFront ─┬─ /*                → S3 (빌드 결과, 뷰어 함수가 SPA 폴백·리다이렉트)
                                              └─ /api/*, /ready     → studio-api.audeniq.com (Cloudflare Worker)
                                                                       ├─ D1 콘텐츠(공지·이벤트·점검)
                                                                       └─ Tunnel → 백엔드
```

| 앱 | 화면 주소 | 빌드 결과 | Worker로 가는 경로 |
|---|---|---|---|
| studio | studio.audeniq.com | `web/studio/public` | `/api/*`, `/ready` |
| admin | admin.audeniq.com | `web/admin/dist` | `/api/*` |
| survey | survey.audeniq.com | `web/survey/public` | `/api/*`, `/health` |
| landing | audeniq.com, www.audeniq.com | `web/landing/public` | 없음 (전부 S3) |

브라우저 입장에서는 화면과 `/api`가 계속 **같은 주소**예요. 그래서 세션 쿠키(`__Host-`, SameSite=Strict),
CSRF, CSP `connect-src 'self'`, 프론트 코드는 바꿀 필요가 없어요.

## 파일

| 파일 | 역할 |
|---|---|
| `apps.mjs` | 앱 목록 (주소, 빌드 폴더, `_headers`, Worker 경로) |
| `template.mjs` | CloudFormation 템플릿 생성. `_headers`의 `/*` 보안 헤더를 응답 헤더 정책으로, 뷰어 함수를 그대로 넣는다 |
| `functions/*.js` | CloudFront Functions. Workers의 SPA 폴백, `_redirects`, 랜딩 라우팅(`web/landing/src/worker.js`)을 대신한다 |
| `deploy-stack.sh` | 스택 `audeniq-<app>-ui` 생성·갱신 (S3, OAC, CloudFront, 함수, 헤더 정책) |
| `publish.mjs` | 빌드 결과를 S3에 올리고 무효화. `Cache-Control`은 `_headers` 규칙대로, `.br`·`_headers`·`_redirects`는 제외 |
| `ci-publish.sh` | CI용. `AWS_UI_ROLE_ARN`이 없으면 건너뛴다 |

`_headers`가 계속 헤더와 캐시 규칙의 원본이에요. CSP를 바꾸면 `deploy-stack.sh`를 다시 실행하세요.
화면 배포만으로는 CSP가 바뀌지 않아요.

## Worker 설정 (`shared/transport.js`)

| 이름 | 종류 | 뜻 |
|---|---|---|
| `PUBLIC_ORIGIN` | var | 브라우저가 보는 주소 (예: `https://studio.audeniq.com`). Origin 검사와 Turnstile hostname 비교 기준이에요. Worker가 `studio-api.*` 주소로 요청을 받기 때문에 필요해요 |
| `EDGE_ORIGIN_SECRET` | secret | CloudFront가 `x-audeniq-edge` 헤더로 붙이는 32자 이상 값. 이 값이 맞는 요청에서만 `CloudFront-Viewer-Address`를 사용자 IP로 믿어요. 맞지 않으면 지금처럼 `CF-Connecting-IP`를 써요 |
| `EDGE_ORIGIN_ONLY` | var | `true`면 CloudFront를 거치지 않은 직접 호출을 403으로 막아요. 전환이 끝난 뒤에 켜세요 |

`PUBLIC_ORIGIN`과 `EDGE_ORIGIN_SECRET`은 지금 Worker가 직접 도메인을 받는 상태에서 미리 넣어도 동작이 그대로예요.
그래서 전환 중에 공백이 생기지 않아요.

## 처음 한 번: 전환 순서 (앱마다)

예시는 studio예요. admin·survey도 같고, landing은 2·3·6단계가 없어요.

1. **인증서**: us-east-1 ACM에서 `studio.audeniq.com`(또는 `*.audeniq.com` + `audeniq.com`) 인증서를 발급해요. DNS 검증 레코드는 Cloudflare DNS에 넣어요.
2. **Worker용 API 주소**: Cloudflare 대시보드 → Workers → `audeniq-studio` → Domains에서 커스텀 도메인 `studio-api.audeniq.com`을 추가해요.
   - 이 주소에는 Bot Fight Mode나 챌린지가 걸리지 않게 하세요. CloudFront 요청이 봇으로 막힐 수 있어요.
3. **Worker 설정**(지금 바로 넣어도 안전해요):
   ```sh
   cd web/studio
   SECRET=$(openssl rand -hex 32); echo "$SECRET"   # 4번에서 같은 값 사용
   echo "$SECRET" | npx wrangler secret put EDGE_ORIGIN_SECRET
   # wrangler.jsonc "vars"에 "PUBLIC_ORIGIN": "https://studio.audeniq.com" 을 추가하고 배포
   ```
4. **스택 만들기**:
   ```sh
   AWS_UI_CERT_ARN=arn:aws:acm:us-east-1:...:certificate/... \
   API_ORIGIN_DOMAIN=studio-api.audeniq.com EDGE_ORIGIN_SECRET="$SECRET" \
     deploy/aws/deploy-stack.sh studio
   ```
5. **화면 올리기**: 빌드한 뒤 `node deploy/aws/publish.mjs studio`를 실행해요. 출력된 `DistributionDomainName`(`dxxxx.cloudfront.net`)으로 화면이 뜨는지 확인해요.
   - 이 주소에서는 Origin이 달라서 로그인 같은 쓰기 요청은 403이 정상이에요.
6. **DNS 전환**: Worker의 `studio.audeniq.com` 커스텀 도메인을 제거해요. 그다음 Cloudflare DNS에 `studio CNAME dxxxx.cloudfront.net`을 **DNS only(회색 구름)**로 추가해요.
   - 주황 구름(프록시)으로 두면 Cloudflare → CloudFront → Cloudflare로 이중 경유가 돼요.
7. **확인**: 로그인, 공지 읽기·쓰기, 점검 화면, 새로고침 시 SPA 경로를 확인해요. 백엔드 로그의 클라이언트 IP가 실제 사용자 IP인지도 확인하세요.
8. **마무리**: Worker `vars`에 `"EDGE_ORIGIN_ONLY": "true"`를 넣어요. 그다음 `wrangler.jsonc`의 `assets` 블록을 빼면 Worker는 API만 담당하게 돼요.
   - landing은 Worker(`audeniq-web`) 배포 단계를 CI에서 지우고 Worker를 삭제해요.

롤백은 6단계를 되돌리면 돼요(DNS를 지우고 Worker 커스텀 도메인을 다시 연결). `assets`를 빼기 전까지는 Worker가 화면도 그대로 서빙해요.

## CI

- **CircleCI**: Context `audeniq-production`에 `AWS_UI_ROLE_ARN`(과 선택으로 `AWS_UI_REGION`)을 넣어요. 역할의 신뢰 정책은 CircleCI OIDC(`oidc.circleci.com/org/<ORG_ID>`)를 허용해야 해요.
- **GitHub Actions**: 저장소 Variables에 `AWS_UI_ROLE_ARN`, `AWS_UI_REGION`을 넣어요. 신뢰 정책은 `token.actions.githubusercontent.com`, `repo:tae-ok-11/audeniq:ref:refs/heads/main`을 허용해요.

값이 없으면 화면 배포 단계는 건너뛰고 Worker 배포만 해요. CI 역할에 필요한 권한(스택 생성은 사람이 해요):

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

- 압축: 빌드의 `.br` 사이드카는 올리지 않아요. CloudFront가 gzip/br로 압축해요(`Compress: true`).
- 지난 배포의 해시 에셋은 S3에서 지우지 않아요. 열려 있던 화면이 예전 청크를 계속 받을 수 있게 하려는 거예요. 필요하면 S3 수명 주기 규칙으로 정리하세요.
- 설문 404 페이지는 CloudFront 오류 응답(배포 전체)이라서, Worker의 404 JSON도 `404.html`로 바뀌어요. 403·405는 그대로예요.
- HTTP 접속: 화면은 HTTPS로 리다이렉트하고, `/api`는 `https-only`(HTTP는 403)예요.
- 비밀 교체: 새 값을 Worker secret과 스택 파라미터에 넣는 사이에는 비밀이 안 맞는 요청이 `CF-Connecting-IP`로 처리돼요. `EDGE_ORIGIN_ONLY`를 켠 상태라면 잠시 끄고 교체하세요.
- 검증: `node --test deploy/aws/test/aws.test.mjs`
