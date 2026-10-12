-- 스태프(개인정보취급자) 접속 보호.
-- 개인정보의 안전성 확보조치 기준 제5조·제6조, NIST SP 800-63B AAL2, FTC Safeguards Rule:
--   * 30분 동안 요청이 없으면 스태프 세션을 끊는다 (last_seen_at).
--   * 외부에서 관리 화면에 들어올 때 비밀번호에 더해 TOTP(RFC 6238) 두 번째 인증을 요구한다.
--     세션은 두 번째 인증을 마친 시각(mfa_at)을 가진다.

ALTER TABLE identity.sessions
 ADD COLUMN last_seen_at timestamptz NOT NULL DEFAULT now(),
 ADD COLUMN mfa_at timestamptz NULL;

-- TOTP 비밀은 서버가 다시 계산해야 하므로 단방향이 아니라 개인정보 키링으로 봉인한다
-- (AAD: 열 + 사용자 id). enrolled_at이 비어 있으면 등록 중(첫 코드 확인 전)이다.
-- last_step: 마지막으로 받은 30초 구간 — 같은 코드를 다시 쓸 수 없다.
CREATE TABLE identity.staff_mfa (
 user_id uuid PRIMARY KEY REFERENCES identity.staff_members ON DELETE RESTRICT,
 totp_cipher bytea NOT NULL CHECK(octet_length(totp_cipher) BETWEEN 29 AND 256),
 enrolled_at timestamptz NULL,
 last_step bigint NULL CHECK(last_step > 0),
 updated_at timestamptz NOT NULL DEFAULT now()
);
