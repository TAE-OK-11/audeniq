-- 0136: 직원이 현장·고객센터에서 신분증을 보고 하는 실명인증 (crates/core/src/identity_desk.rs).
--
-- 인증 기관 거래가 없으므로 연계정보(CI) 해시가 없다: provider='OFFLINE'인 행만 ci_hash가 비어 있다.
-- 누가·언제·어디서·어떤 서류로 확인했는지는 portal.offline_identity_checks에 쌓는다(고치거나 지우지 않는다).
-- 신분증 번호(주민등록번호 등)는 받지도 저장하지도 않는다.
ALTER TABLE portal.account_identities
 ALTER COLUMN ci_hash DROP NOT NULL,
 ADD CONSTRAINT account_identities_ci_unless_offline CHECK((provider = 'OFFLINE') = (ci_hash IS NULL));

CREATE TABLE portal.offline_identity_checks (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 staff_user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 channel text NOT NULL CHECK(channel IN ('IN_PERSON','CUSTOMER_CENTER')),
 document_type text NOT NULL CHECK(document_type IN ('RESIDENT_CARD','DRIVER_LICENSE','PASSPORT','ALIEN_REGISTRATION','DISABILITY_CARD','VETERAN_CARD')),
 -- 이미 있던 직원 실명인증을 관리자가 바로잡은 경우
 correction boolean NOT NULL DEFAULT false,
 note text NOT NULL DEFAULT '' CHECK(length(note) <= 500),
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX offline_identity_checks_user ON portal.offline_identity_checks(user_id, created_at DESC);

-- 직원이 실명을 확인했다고 본인에게 알린다 (개인 작업 공간 먼저). API는 알림 테이블에 직접 쓰지 않는다.
CREATE FUNCTION portal.identity_desk_notice(p_user uuid, p_title text, p_detail text)
RETURNS void LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog AS $$
  INSERT INTO portal.notifications(org_id, user_id, kind, title, detail, link)
  SELECT m.org_id, p_user, 'ACCOUNT', left(p_title, 200), left(p_detail, 1000), '/profile'
    FROM identity.memberships m JOIN identity.orgs o ON o.id = m.org_id
   WHERE m.user_id = p_user AND m.status = 'ACTIVE'
   ORDER BY (o.kind <> 'PERSONAL'), o.created_at
   LIMIT 1;
$$;
REVOKE ALL ON FUNCTION portal.identity_desk_notice(uuid, text, text) FROM PUBLIC;
