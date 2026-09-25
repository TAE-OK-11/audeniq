-- Sandbox round 2.5 (security): separation of duties for review overrides.
--
-- 1. Memberships need the invitee's consent. An OWNER used to be able to add
--    any account as an ACTIVE member (PUT /memberships) and then name it as
--    the "second approver" of a forced PASS. A new membership now starts as
--    INVITED and only the invitee's own session can accept it
--    (POST /api/orgs/{org}/memberships/accept).
--    accepted_at: when the member accepted. Pre-existing rows keep NULL and
--    are treated as legacy-accepted (they predate this rule); every row
--    created from now on gets a value (DEFAULT now() for OWNER rows written
--    at org creation, explicit timestamp on acceptance). INVITED rows carry
--    NULL until accepted and are never eligible for anything.
ALTER TABLE identity.memberships DROP CONSTRAINT IF EXISTS memberships_status_check;
ALTER TABLE identity.memberships
  ADD CONSTRAINT memberships_status_check CHECK(status IN ('INVITED','ACTIVE','REVOKED'));
ALTER TABLE identity.memberships ADD COLUMN accepted_at timestamptz;
ALTER TABLE identity.memberships ALTER COLUMN accepted_at SET DEFAULT now();
ALTER TABLE identity.memberships ADD COLUMN invited_by uuid REFERENCES identity.users ON DELETE RESTRICT;
ALTER TABLE identity.memberships ADD COLUMN invited_at timestamptz;

-- 2. Forced PASS that needs a second person is a two-step request: the
--    requester files it, a *different* eligible member approves it from
--    their own authenticated session (the approver's identity is the
--    session's, never an id supplied by the requester). The override row is
--    written only on approval, and review_overrides' own CHECK still forbids
--    actor = second approver. The requester may withdraw (DECLINED by
--    themselves); approval by the requester is refused in code. Requests
--    expire after 7 days.
CREATE TABLE rights.override_requests (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 revision_id uuid NOT NULL REFERENCES catalog.application_revisions ON DELETE RESTRICT,
 check_code text NOT NULL CHECK(length(btrim(check_code))>0),
 original_status text NOT NULL,
 proposed_status text NOT NULL
  CHECK(proposed_status IN ('PASS','CORRECTION_REQUIRED','REVIEW_REQUIRED','BLOCKED')),
 reason text NOT NULL CHECK(length(btrim(reason))>0),
 requested_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL DEFAULT now() + interval '7 days',
 status text NOT NULL DEFAULT 'PENDING' CHECK(status IN ('PENDING','APPROVED','DECLINED')),
 decided_by uuid REFERENCES identity.users ON DELETE RESTRICT,
 decided_at timestamptz,
 override_id uuid REFERENCES rights.review_overrides ON DELETE RESTRICT,
 CHECK((status='PENDING') = (decided_at IS NULL)),
 CHECK(status<>'APPROVED' OR override_id IS NOT NULL)
);
CREATE INDEX override_requests_revision ON rights.override_requests(org_id,revision_id,status);
-- Decided requests are history: no further mutation, no deletes.
CREATE FUNCTION rights.override_request_guard() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
BEGIN
  IF TG_OP = 'DELETE' OR OLD.status <> 'PENDING' THEN
    RAISE EXCEPTION 'override request % is immutable', OLD.id USING ERRCODE = '55000';
  END IF;
  IF NEW.id <> OLD.id OR NEW.org_id <> OLD.org_id OR NEW.revision_id <> OLD.revision_id
     OR NEW.check_code <> OLD.check_code OR NEW.proposed_status <> OLD.proposed_status
     OR NEW.reason <> OLD.reason OR NEW.requested_by <> OLD.requested_by
     OR NEW.created_at <> OLD.created_at OR NEW.expires_at <> OLD.expires_at THEN
    RAISE EXCEPTION 'override request % content is immutable', OLD.id USING ERRCODE = '55000';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON rights.override_requests
  FOR EACH ROW EXECUTE FUNCTION rights.override_request_guard();
