-- 개인정보 암호화: 계좌번호와 같은 KMS 보호 키링(payout_keys.rs)으로 AES-256-GCM 봉인한다.
-- 각 암호문의 AAD는 열 이름과 행의 주체 id를 묶고, 키 버전은 암호문 머리(7~8바이트)에 있다.
-- 값으로 찾아야 하는 이메일은 키 버전마다 다른 HMAC 블라인드 인덱스를 함께 둔다.
--
-- 기존 평문 행은 `audeniq-admin --operator NAME privacy encrypt-pii`가 100개씩 봉인한다.
-- 그 사이에도 API는 평문·암호문 행을 모두 읽고, 새로 쓰는 값은 항상 암호문이다.
-- 한 행의 한 값은 평문이나 암호문 중 하나만 갖는다.

-- 로그인 이메일
ALTER TABLE identity.users
 ALTER COLUMN email DROP NOT NULL,
 ADD COLUMN email_cipher bytea NULL CHECK(octet_length(email_cipher) BETWEEN 29 AND 1024),
 ADD COLUMN email_bidx text NULL CHECK(email_bidx ~ '^[0-9a-f]{64}$'),
 ADD CONSTRAINT users_email_one_form
  CHECK((email IS NULL) <> (email_cipher IS NULL) AND (email_cipher IS NULL) = (email_bidx IS NULL));
CREATE UNIQUE INDEX users_email_bidx ON identity.users(email_bidx);

-- 본인인증 실명·생년월일
ALTER TABLE portal.account_identities
 ALTER COLUMN real_name DROP NOT NULL,
 ALTER COLUMN birth_date DROP NOT NULL,
 ADD COLUMN real_name_cipher bytea NULL CHECK(octet_length(real_name_cipher) BETWEEN 29 AND 1024),
 ADD COLUMN birth_date_cipher bytea NULL CHECK(octet_length(birth_date_cipher) BETWEEN 29 AND 128),
 ADD CONSTRAINT account_identities_one_form
  CHECK((real_name IS NULL) <> (real_name_cipher IS NULL) AND (birth_date IS NULL) <> (birth_date_cipher IS NULL));

-- 정산 계좌 예금주
ALTER TABLE portal.payout_accounts
 ALTER COLUMN holder_name DROP NOT NULL,
 ADD COLUMN holder_cipher bytea NULL CHECK(octet_length(holder_cipher) BETWEEN 29 AND 1024),
 ADD CONSTRAINT payout_accounts_holder_one_form CHECK((holder_name IS NULL) <> (holder_cipher IS NULL));

-- 지급 요청·지시의 계좌 스냅샷: 예금주는 스냅샷 JSON에서 빼고 암호문 열에 둔다(AAD는 조직 id).
ALTER TABLE portal.payout_requests
 ADD COLUMN holder_cipher bytea NULL CHECK(octet_length(holder_cipher) BETWEEN 29 AND 1024);
ALTER TABLE finance.payout_orders
 ADD COLUMN holder_cipher bytea NULL CHECK(octet_length(holder_cipher) BETWEEN 29 AND 1024);

-- 공개 프로필 연락 이메일 (봉인 후 평문 열은 '')
ALTER TABLE portal.artist_profiles
 ADD COLUMN contact_email_cipher bytea NULL CHECK(octet_length(contact_email_cipher) BETWEEN 29 AND 1024),
 ADD CONSTRAINT artist_profiles_contact_one_form CHECK(contact_email = '' OR contact_email_cipher IS NULL);

-- 관계 관리(CRM) 담당자 이름·이메일·전화
ALTER TABLE work.relationship_contacts
 ALTER COLUMN name DROP NOT NULL,
 ADD COLUMN name_cipher bytea NULL CHECK(octet_length(name_cipher) BETWEEN 29 AND 1024),
 ADD COLUMN email_cipher bytea NULL CHECK(octet_length(email_cipher) BETWEEN 29 AND 1024),
 ADD COLUMN phone_cipher bytea NULL CHECK(octet_length(phone_cipher) BETWEEN 29 AND 256),
 ADD CONSTRAINT relationship_contacts_one_form
  CHECK((name IS NULL) <> (name_cipher IS NULL) AND (email IS NULL OR email_cipher IS NULL) AND (phone IS NULL OR phone_cipher IS NULL));

-- 납세자번호: 이전에는 계좌번호용 AAD(사용자 id)로 봉인했다. 새 값은 TIN 전용 AAD를 쓰고,
-- encrypt-pii가 이전 값을 다시 봉인한다.
ALTER TABLE portal.tax_residencies
 ADD COLUMN tin_field_aad boolean NOT NULL DEFAULT false;

-- 계좌 등록 알림 문구에 들어간 예금주 이름을 지운다 ("은행 · •••• 1234 · 예금주").
UPDATE portal.notifications
   SET detail = regexp_replace(detail, '^(.* · •••• [0-9]{2,4}) · .*$', '\1')
 WHERE kind = 'ACCOUNT' AND detail ~ '^.* · •••• [0-9]{2,4} · ';
