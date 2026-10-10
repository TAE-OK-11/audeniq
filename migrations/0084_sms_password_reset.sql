-- 0084: 휴대폰 번호 등록(문자 인증)과 비밀번호 재설정.
--
-- 휴대폰 번호는 서비스 비밀값을 섞은 해시와 뒷자리 4개만 둔다(원문 미보관).
-- 비밀번호 재설정은 이메일 + 등록한 휴대폰 번호가 맞을 때만 문자로 인증번호를 보내고,
-- 인증번호를 맞히면 15분짜리 일회용 재설정 토큰(해시만 보관)을 준다.
CREATE TABLE portal.account_phones (
 user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE RESTRICT,
 phone_hash text NOT NULL CHECK(phone_hash ~ '^[0-9a-f]{64}$'),
 phone_last4 text NOT NULL CHECK(phone_last4 ~ '^[0-9]{4}$'),
 verified_at timestamptz NOT NULL DEFAULT now(),
 row_version bigint NOT NULL DEFAULT 1
);
CREATE INDEX account_phones_hash ON portal.account_phones(phone_hash);

CREATE TABLE identity.sms_challenges (
 id uuid PRIMARY KEY,
 purpose text NOT NULL CHECK(purpose IN ('PHONE_REGISTER','PASSWORD_RESET')),
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 phone_hash text NOT NULL CHECK(phone_hash ~ '^[0-9a-f]{64}$'),
 phone_last4 text NOT NULL CHECK(phone_last4 ~ '^[0-9]{4}$'),
 code_hash bytea NOT NULL CHECK(octet_length(code_hash) = 32),
 attempts integer NOT NULL DEFAULT 0 CHECK(attempts >= 0),
 expires_at timestamptz NOT NULL,
 verified_at timestamptz NULL,
 reset_token_hash bytea NULL UNIQUE CHECK(reset_token_hash IS NULL OR octet_length(reset_token_hash) = 32),
 reset_expires_at timestamptz NULL,
 consumed_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK((reset_token_hash IS NULL) = (reset_expires_at IS NULL)),
 CHECK(reset_token_hash IS NULL OR (purpose = 'PASSWORD_RESET' AND verified_at IS NOT NULL))
);
CREATE INDEX sms_challenges_user ON identity.sms_challenges(user_id, purpose, created_at DESC);
