-- Google·Apple 로그인 (OpenID Connect, authorization code + PKCE).
-- 계정 연결: 제공자의 사용자 식별자(sub)로 찾는다. 연결 정보의 이메일은 개인정보 키링으로 봉인한다.
CREATE TABLE identity.oauth_identities (
 id uuid PRIMARY KEY,
 provider text NOT NULL CHECK(provider IN ('google','apple')),
 subject text NOT NULL CHECK(length(subject) BETWEEN 1 AND 255),
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 email_cipher bytea NULL CHECK(email_cipher IS NULL OR octet_length(email_cipher) BETWEEN 29 AND 1024),
 created_at timestamptz NOT NULL DEFAULT now(),
 last_used_at timestamptz NULL,
 UNIQUE(provider, subject),
 UNIQUE(user_id, provider)
);

-- 로그인 시작마다 한 번만 쓰는 state. state·nonce는 해시로만 저장한다(단방향).
-- code_verifier(PKCE)는 토큰 교환 때 원문이 필요하다: 10분 유효, 한 번 쓰면 끝이며 혼자서는 쓸모가 없다.
CREATE TABLE identity.oauth_states (
 id uuid PRIMARY KEY,
 state_hash bytea NOT NULL UNIQUE CHECK(octet_length(state_hash)=32),
 provider text NOT NULL CHECK(provider IN ('google','apple')),
 purpose text NOT NULL CHECK(purpose IN ('LOGIN','LINK')),
 user_id uuid NULL REFERENCES identity.users ON DELETE CASCADE,
 nonce_hash bytea NOT NULL CHECK(octet_length(nonce_hash)=32),
 code_verifier text NOT NULL CHECK(length(code_verifier) BETWEEN 43 AND 128),
 -- 가입 화면에서 필수 약관에 동의하고 시작했을 때만 새 계정을 만든다.
 allow_signup boolean NOT NULL DEFAULT false,
 return_to text NOT NULL DEFAULT '/' CHECK(return_to ~ '^/[^/\\]' OR return_to = '/'),
 expires_at timestamptz NOT NULL,
 consumed_at timestamptz NULL,
 CHECK((purpose='LINK') = (user_id IS NOT NULL))
);
CREATE INDEX oauth_states_expiry ON identity.oauth_states(expires_at);
