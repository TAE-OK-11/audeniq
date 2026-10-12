-- 패스키(WebAuthn) 로그인. 이메일을 입력한 뒤 비밀번호 대신 기기의 패스키(지문·얼굴·PIN)로 로그인한다.
-- 공개키만 저장한다(비밀키는 기기를 떠나지 않는다). 공개키는 비밀이 아니므로 암호화하지 않는다.
CREATE TABLE identity.passkeys (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 credential_id bytea NOT NULL UNIQUE CHECK(octet_length(credential_id) BETWEEN 1 AND 1023),
 -- X.509 SubjectPublicKeyInfo (DER)
 public_key bytea NOT NULL CHECK(octet_length(public_key) BETWEEN 32 AND 2048),
 -- COSE: -7 ES256, -8 EdDSA, -257 RS256
 alg integer NOT NULL CHECK(alg IN (-7,-8,-257)),
 sign_count bigint NOT NULL DEFAULT 0 CHECK(sign_count >= 0),
 backed_up boolean NOT NULL DEFAULT false,
 name text NOT NULL CHECK(length(btrim(name)) BETWEEN 1 AND 60),
 created_at timestamptz NOT NULL DEFAULT now(),
 last_used_at timestamptz NULL
);
CREATE INDEX passkeys_user ON identity.passkeys(user_id);

-- 등록·로그인 요청마다 한 번만 쓰는 challenge. 값은 해시로만 저장한다(단방향), 5분 유효.
CREATE TABLE identity.webauthn_challenges (
 id uuid PRIMARY KEY,
 challenge_hash bytea NOT NULL CHECK(octet_length(challenge_hash)=32),
 purpose text NOT NULL CHECK(purpose IN ('REGISTER','LOGIN')),
 user_id uuid NULL REFERENCES identity.users ON DELETE CASCADE,
 expires_at timestamptz NOT NULL,
 consumed_at timestamptz NULL,
 CHECK((purpose='REGISTER') = (user_id IS NOT NULL))
);
CREATE INDEX webauthn_challenges_expiry ON identity.webauthn_challenges(expires_at);
