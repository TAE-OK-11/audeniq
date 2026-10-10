-- 0083: 계정 정보 — 본인인증(실명)과 해외 납세 의무. 스튜디오 '계정 정보' 화면이
-- 브라우저에만 두던 값을 서버에 둔다 (docs/API.md "Portal" → Account).
--
-- 본인인증은 권리 서류 서명과 같은 인증 기관 연동(signing::Provider)을 쓴다.
-- 연계정보(CI)는 서비스 비밀값으로 섞은 해시만, 생년월일은 성인 여부 판단용으로만 둔다.
-- 같은 인증 거래로 두 계정을 인증할 수 없다.
CREATE TABLE portal.account_identities (
 user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE RESTRICT,
 real_name text NOT NULL CHECK(length(btrim(real_name)) BETWEEN 1 AND 120),
 birth_date date NOT NULL,
 provider text NOT NULL CHECK(length(provider) BETWEEN 1 AND 40),
 method text NOT NULL CHECK(length(method) BETWEEN 1 AND 40),
 transaction_id text NOT NULL CHECK(length(transaction_id) BETWEEN 1 AND 200),
 ci_hash text NOT NULL CHECK(ci_hash ~ '^[0-9a-f]{64}$'),
 verified_at timestamptz NOT NULL DEFAULT now(),
 row_version bigint NOT NULL DEFAULT 0,
 UNIQUE(provider, transaction_id)
);

-- 해외 납세 의무 (수익 지급 시 원천징수·신고 판단). 납세자번호(TIN)는 수령 계좌와 같은
-- AES-256-GCM 키(PAYOUT_ACCOUNT_KEY)로 봉인하고 뒷자리만 돌려준다.
CREATE TABLE portal.tax_residencies (
 user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE RESTRICT,
 foreign_tax boolean NOT NULL,
 country text NULL CHECK(country ~ '^[A-Z]{2}$' OR country = 'OTHER'),
 tin_cipher bytea NULL CHECK(octet_length(tin_cipher) BETWEEN 29 AND 128),
 tin_last4 text NULL CHECK(tin_last4 ~ '^[A-Za-z0-9]{1,4}$'),
 key_version smallint NULL,
 us_person boolean NOT NULL DEFAULT false,
 row_version bigint NOT NULL DEFAULT 0,
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK(foreign_tax OR (country IS NULL AND tin_cipher IS NULL AND NOT us_person)),
 CHECK(NOT foreign_tax OR country IS NOT NULL),
 CHECK(NOT us_person OR country = 'US'),
 CHECK((tin_cipher IS NULL) = (tin_last4 IS NULL) AND (tin_cipher IS NULL) = (key_version IS NULL))
);
