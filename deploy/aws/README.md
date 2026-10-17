# (나중) 전부 AWS로: CloudFront + S3 + 백엔드(ALB)

> 지금(개발 단계)은 Cloudflare Workers가 화면을 서빙하고 `/api`를 터널로 백엔드에 넘겨요(공지·점검 로직은 백엔드, 설문은 베타 전까지 D1 Worker 그대로).
> 이 문서는 AWS로 옮길 때 쓰는 구성이에요. 공지·점검을 백엔드로 옮긴 부분은 두 구성이 같아요.

Cloudflare Workers, D1, Tunnel이 하던 일을 전부 AWS로 옮겨요.

```
브라우저 ── studio.audeniq.com ── CloudFront ─┬─ /*            → S3 (화면, 뷰어 함수: SPA 폴백·리다이렉트)
                                              └─ /api/*, /ready → ALB → 백엔드 api (Postgres)
                                                  + x-audeniq-service(서비스 비밀), x-audeniq-site(studio|admin)
```

| 예전 (Cloudflare) | 지금 (AWS) | 코드 |
|---|---|---|
| Workers Static Assets | S3 + CloudFront | `deploy/aws/` |
| SPA 폴백, `_redirects`, 랜딩 라우팅 | CloudFront Functions | `functions/*.js` |
| 공지·이벤트·서버 점검 (D1) | 백엔드 + Postgres `content` 스키마 | `crates/core/src/site_content.rs`, 마이그레이션 0086 |
| 점검 중 API 차단(503 MAINTENANCE), 비상 스위치 | 백엔드 미들웨어, 환경변수 `MAINTENANCE_MODE` | `site_content::gate` |
| 관리자 콘솔 경로 제한·Origin 확인 (인증 보조) | 백엔드 미들웨어 | `crates/core/src/sites.rs` |
| 서비스 비밀 헤더, 사용자 IP 전달 | CloudFront origin custom header + API 뷰어 함수 | `template.ts`, `functions/api.js` |
| Cloudflare Tunnel | ALB (인터넷용, HTTPS) → 서버 8080 | `deploy/compose.production.yaml` |

R2(업로드 저장소), DNS는 이번에 바꾸지 않아요. 설문(survey.audeniq.com)은 베타 전까지 D1 Worker 그대로라 이 구성에 없어요.

| 앱 | 화면 주소 | 빌드 결과 | 백엔드로 가는 경로 |
|---|---|---|---|
| studio | studio.audeniq.com | `web/studio/public` | `/api/*`, `/ready` |
| admin | admin.audeniq.com | `web/admin/dist` | `/api/*` (관리자 경로만 열림) |
| landing | audeniq.com, www.audeniq.com | `web/landing/public` | 없음 |

브라우저 입장에서는 화면과 `/api`가 계속 **같은 주소**예요. 그래서 프론트 코드, 세션 쿠키, CSRF, CSP는 그대로예요.

## 보안 경계

- 백엔드는 모든 요청에서 `x-audeniq-service`(= `EDGE_SERVICE_SECRET`)를 확인해요(기존 `boundary`). 이 값은 CloudFront가 origin custom header로 붙여요. 브라우저가 같은 이름을 보내도 CloudFront가 덮어써요.
- API 뷰어 함수가 브라우저가 보낸 `x-audeniq-*` 헤더를 지우고, `x-audeniq-client-ip`에 실제 사용자 IP(`event.viewer.ip`)를 넣어요. 로그인 시도 제한과 스태프 접근 기록이 이 값을 써요.
- `x-audeniq-site=admin`이면 관리자 콘솔 경로(로그인, `/api/me`, `/api/staff/*`, 공지·점검)만 열려요. 쓰기는 `ADMIN_ORIGIN`에서 온 것만 받고, 같은 사이트의 스튜디오 화면이 보낸 쓰기도 막아요.
- ALB는 인터넷에 열려 있지만 서비스 비밀 없이는 403이에요. 보안 그룹을 CloudFront 관리형 prefix list(`com.amazonaws.global.cloudfront.origin-facing`)로 좁히면 더 좋아요.

## 파일

| 파일 | 역할 |
|---|---|
| `apps.ts` | 앱 목록 (주소, 빌드 폴더, `_headers`, 백엔드 경로, 사이트 이름) |
| `template.ts` | CloudFormation 템플릿 생성 (S3 + OAC, CloudFront, 뷰어 함수 2개, 응답 헤더 정책) |
| `deploy-stack.sh` | 스택 `audeniq-<app>-ui` 생성·갱신 |
| `publish.ts` / `ci-publish.sh` | 빌드 결과를 S3에 올리고 무효화 (`Cache-Control`은 `_headers` 규칙대로) |
| `migrate-d1.ts` | D1 데이터 → Postgres 가져오기 SQL 생성 |

## 전환 순서

1. **백엔드 배포**(마이그레이션 0086 포함)와 `deploy/grants.sql` 재실행. `production.env`에 `ADMIN_ORIGIN`을 넣어요.
   - 이 단계는 지금 구조(Workers + Tunnel)에 영향이 없어요. 새 경로를 아직 아무도 부르지 않아요.
2. **데이터 옮기기**(1차): 관리 화면에서 글을 잠시 쓰지 않는 시간에 실행해요.
   ```sh
   CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=... node --experimental-strip-types deploy/aws/migrate-d1.ts > d1-import.sql
   psql "$OWNER_DATABASE_URL" -v ON_ERROR_STOP=1 -f d1-import.sql
   ```
3. **ALB**: 서버가 있는 VPC에 인터넷용 ALB를 만들어요.
   - HTTPS 443 리스너에 `api-origin.audeniq.com` ACM 인증서(ALB 리전)를 붙여요.
   - 대상 그룹은 인스턴스 `8080`(HTTP), 헬스체크 `/health`. 단, `/health`도 서비스 비밀이 필요해서 성공 코드를 `200,403`으로 두세요.
   - `compose.production.yaml`의 `api`에 `ports: ["<서버 사설 IP>:8080:8080"]`을 추가하고 재배포해요. 서버 보안 그룹은 ALB 보안 그룹에서 오는 8080만 허용해요.
   - DNS: `api-origin.audeniq.com`을 ALB로 바꿔요(Cloudflare DNS only). 지금 이 이름을 Tunnel이 쓰고 있다면 `api-aws.audeniq.com`처럼 새 이름을 쓰세요.
4. **인증서**: us-east-1 ACM에서 화면 주소(`studio.audeniq.com` 등, 또는 `*.audeniq.com` + `audeniq.com`) 인증서를 발급해요.
5. **스택 만들기**(앱마다):
   ```sh
   AWS_UI_CERT_ARN=arn:aws:acm:us-east-1:...:certificate/... \
   API_ORIGIN_DOMAIN=api-origin.audeniq.com EDGE_SERVICE_SECRET="<production.env 값>" \
     deploy/aws/deploy-stack.sh studio        # admin, landing(API 값 불필요)
   ```
6. **화면 올리기**: 빌드한 뒤 `node --experimental-strip-types deploy/aws/publish.ts studio`를 실행해요. 출력된 `dxxxx.cloudfront.net`에서 화면과 `/api/status`가 뜨는지 확인해요.
7. **DNS 전환**(앱마다): Worker의 커스텀 도메인을 제거하고, `studio CNAME dxxxx.cloudfront.net`을 **DNS only**로 추가해요.
   - 전환 직전에 2단계를 한 번 더 실행해서 마지막 글까지 옮겨요. 이미 있는 id는 건너뛰어요.
8. **확인**: 로그인, 공지 읽기·쓰기, 관리자 점검 등록 → 스튜디오 점검 화면, 백엔드 로그의 사용자 IP를 확인해요.
9. **정리**:
   - `compose.production.yaml`의 `tunnel` 서비스와 `tunnel-token`을 지워요.
   - CI의 `wrangler deploy` 단계(`.circleci/config.yml` frontend, `.github/workflows/studio-deploy.yml`, `admin-deploy.yml`)와 D1 마이그레이션 단계를 지워요.
   - Cloudflare의 Workers(`audeniq-studio`, `audeniq-admin`, `audeniq-web`)와 D1 `audeniq-content`를 삭제해요.
   - studio·admin·landing의 `worker.ts`·`wrangler.jsonc`, `web/studio/migrations`, `crates/edge`를 저장소에서 지워요.

롤백: 7단계 전까지는 아무것도 바뀌지 않아요. 7단계 뒤에는 DNS를 되돌리면 Workers가 다시 받아요. 다만 그사이 백엔드에 쓴 공지·점검은 D1에 없어요.

## CI

- **CircleCI**: Context `audeniq-production`에 `AWS_UI_ROLE_ARN`(CircleCI OIDC로 맡을 역할)과 선택으로 `AWS_UI_REGION`(기본 `ap-northeast-2`)을 넣어요.
- **GitHub Actions**: 저장소 Variables에 같은 이름으로 넣어요. 신뢰 정책은 `repo:tae-ok-11/audeniq:ref:refs/heads/main`이에요.

값이 없으면 화면 S3 배포는 건너뛰어요. CI 역할에 필요한 권한(스택 생성은 사람이 해요):

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

- `_headers`가 계속 보안 헤더와 캐시 규칙의 원본이에요. CSP를 바꾸면 `deploy-stack.sh`를 다시 실행하세요.
- 비상 점검 스위치는 이제 Worker secret이 아니라 백엔드 환경변수예요. `production.env`에 `MAINTENANCE_MODE=on`을 넣고 api를 재시작해요.
  - 평소에는 관리자 콘솔의 '서버 점검'으로 등록해요. 10초 안에 반영돼요.
- 공개 공지·이벤트는 `Cache-Control: public, max-age=30`이에요. CloudFront `/api`는 캐시하지 않아서 브라우저만 30초 캐시해요.
- 업로드는 지금처럼 브라우저가 저장소(R2) presigned URL로 직접 올려서 CloudFront를 거치지 않아요.
- 검증: `node --experimental-strip-types --test deploy/aws/test/aws.test.ts`, `cargo test -p audeniq-core --test site_content`
