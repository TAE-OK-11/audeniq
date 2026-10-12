# Google·Apple 로그인

로그인·가입 화면에 "Google로 계속하기", "Apple로 계속하기" 버튼이 있다. 코드는 모두 들어가 있다. 아래 값을 넣고 API를 다시 시작하면 그 제공자의 버튼이 나타난다. 값이 빠진 제공자는 버튼이 숨겨지고 API는 `OAUTH_PROVIDER_NOT_CONFIGURED`를 돌려준다.

- 서버: `crates/core/src/oauth.rs`
- 화면: `web/studio/app/src/components/SocialSignIn.tsx`, `ConnectedAccounts.tsx`
- DB: 마이그레이션 0105

## 동작

1. **시작:** 화면이 `POST /api/auth/oauth/{google|apple}/start`를 부른다. 서버는 다음을 저장하고 제공자 주소를 돌려준다.
   - 한 번만 쓰는 state와 nonce: 해시로만 저장한다.
   - PKCE verifier: 10분 유효.
2. **돌아오기:** 제공자가 `https://studio.audeniq.com/api/auth/oauth/{provider}/callback`으로 돌아온다. Google은 GET, Apple은 form POST다.
3. **확인:** 서버는 다음을 순서대로 한다.
   - code를 제공자의 토큰 주소에서 교환한다. 이때 client secret을 쓰며, Apple은 .p8 키로 서명한 ES256 JWT를 매번 만든다.
   - ID 토큰을 검증한다: 제공자 공개키(JWKS, 1시간 캐시, 키가 바뀌면 다시 받음)로 서명을 확인하고, 발급자·대상·만료·nonce를 확인한다.
4. **결과:** 로그인 세션을 만들고 원래 화면으로 보낸다. 실패하면 `/login?oauth_error=코드`로 보낸다.

**계정 처리**

| 상황 | 처리 |
|---|---|
| 이미 연결된 Google·Apple 계정 (`sub`로 찾음) | 그 계정으로 로그인 |
| 처음 왔고, 확인된 이메일이며, 그 이메일의 계정이 없음 | 가입 화면에서 이용약관에 동의하고 시작한 경우에만 새 계정을 만든다. 로그인 화면에서 시작했다면 가입 화면으로 안내한다(`OAUTH_SIGNUP_REQUIRED`). |
| 처음 왔는데 같은 이메일의 계정이 이미 있음 | **자동으로 연결하지 않는다**(`OAUTH_EMAIL_IN_USE`). 비밀번호로 로그인한 뒤 계정 정보 > 로그인 > 연결된 계정에서 연결한다. 남의 이메일로 먼저 가입해 두거나 계정을 가로채는 공격을 막기 위해서다. |
| 이메일이 확인되지 않았거나 없음 | 거절한다(`OAUTH_EMAIL_REQUIRED`). |

**기타**

- **저장:** 연결 정보의 이메일은 개인정보 키링으로 봉인한다(#104). 제공자 사용자 id(`sub`)는 조회용이라 평문으로 둔다.
- **Google·Apple로만 가입한 계정:** 비밀번호가 없다. 비밀번호가 필요하면 "비밀번호 찾기"(SMS)로 정한다. 마지막 로그인 수단은 연결을 해제할 수 없다(`OAUTH_LAST_SIGN_IN_METHOD`).
- **스태프:** Google·Apple 로그인은 2단계 인증으로 치지 않으므로 관리 화면에서는 인증 앱 코드를 요구한다. 관리 화면에는 버튼이 없다(별도 도메인).
- **엣지:** Apple의 form POST는 다른 사이트에서 오는 요청이라, 엣지와 API의 Origin 검사에서 이 콜백 하나만 예외다. state와 code 교환으로 인증한다. 엣지는 콜백의 리다이렉트를 따라가지 않고 Set-Cookie와 함께 브라우저로 넘긴다.

## Google 설정

1. [Google Cloud Console](https://console.cloud.google.com/) > API 및 서비스 > **OAuth 동의 화면**에서 앱을 만든다.
   - 유형: 외부. 앱 이름은 AUDENIQ. 지원 이메일과 개인정보처리방침·약관 URL을 넣는다.
   - 범위: `openid`, `email`만 추가한다. 민감하지 않은 범위라 Google 검수가 필요 없다.
   - 게시 상태를 "프로덕션"으로 바꾼다. 테스트 상태에서는 등록한 테스트 사용자만 로그인할 수 있다.
2. **사용자 인증 정보** > OAuth 클라이언트 ID 만들기 > 웹 애플리케이션을 고른다.
   - 승인된 리디렉션 URI: `https://studio.audeniq.com/api/auth/oauth/google/callback`
   - 승인된 JavaScript 원본은 필요 없다.
3. 발급된 값을 넣는다.
   - `GOOGLE_OAUTH_CLIENT_ID`: `….apps.googleusercontent.com`
   - `GOOGLE_OAUTH_CLIENT_SECRET`: 시크릿 매니저에 둔다.

## Apple 설정

Apple Developer Program(유료) 계정이 필요하다.

1. Certificates, Identifiers & Profiles > **Identifiers** > App IDs에서 앱 ID를 하나 만들고 "Sign in with Apple"을 켠다. 기존 iOS 앱이 있으면 그것을 쓴다.
2. Identifiers > **Services IDs**를 만든다. 예: `com.audeniq.studio.signin`. 이 값이 `APPLE_OAUTH_CLIENT_ID`다.
   - "Sign in with Apple"을 켜고 Configure를 누른다. Primary App ID는 1번에서 만든 앱 ID다.
   - Domains: `studio.audeniq.com`
   - Return URLs: `https://studio.audeniq.com/api/auth/oauth/apple/callback`
3. **Keys**에서 새 키를 만들고 "Sign in with Apple"을 켠다(Primary App ID 연결).
   - `.p8` 파일은 한 번만 내려받을 수 있다.
   - 키 ID(10자)가 `APPLE_OAUTH_KEY_ID`다.
   - `.p8` 내용 전체가 `APPLE_OAUTH_PRIVATE_KEY`다. 한 줄로 넣을 때는 줄바꿈을 `\n`으로 쓴다. 시크릿 매니저에 둔다.
4. 오른쪽 위에 보이는 팀 ID(10자)가 `APPLE_OAUTH_TEAM_ID`다.
5. **이메일 릴레이:** "이메일 가리기"를 고른 사용자는 `…@privaterelay.appleid.com` 주소로 가입된다. 이 주소로 메일을 보내려면 Certificates, Identifiers & Profiles > Services > **Sign in with Apple for Email Communication**에 발신 도메인(SPF 통과)과 발신 주소를 등록해야 한다.

## 적용 확인

1. API를 다시 시작한다. `GET /api/auth/oauth/providers`가 `{"google":true,"apple":true}`를 돌려주면 된다.
2. 가입 화면에서 약관에 동의하고 "Google로 계속하기"를 누른다. Google 화면을 거쳐 Studio로 돌아오면 로그인된 상태여야 한다.
3. 계정 정보 > 로그인 > 연결된 계정에 연결 이메일이 보인다.
4. 로그아웃한 뒤 로그인 화면의 같은 버튼으로 다시 로그인한다.
5. 실패하면 주소창의 `oauth_error` 값과 API 로그(`oauth sign-in refused`, 코드만 남김)를 본다.

| 코드 | 흔한 원인 |
|---|---|
| `OAUTH_CODE_REJECTED` | 리디렉션 URI 불일치, 잘못된 client secret, Apple 키·팀 ID 오류 |
| `OAUTH_TOKEN_INVALID` | 서버 시계 오차, 잘못된 client ID |
| `OAUTH_PROVIDER_UNAVAILABLE` | API 서버의 외부 HTTPS 연결 문제(방화벽) |

API 서버는 `accounts.google.com`, `oauth2.googleapis.com`, `www.googleapis.com`, `appleid.apple.com`으로 HTTPS 연결이 가능해야 한다.

## 테스트

`crates/core/tests/social_login.rs`는 두 제공자를 흉내 내는 로컬 서버로 전체 흐름을 검증한다(`OAUTH_FIXTURE_BASE`, production에서는 무시). 이 서버는 RSA로 서명한 ID 토큰을 발급하고, PKCE와 Apple JWT client secret을 확인한다.
