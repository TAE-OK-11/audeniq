-- 0044: AUDENIQ staff portal (crate::staff, /api/staff/*).
--
-- Staff are AUDENIQ employees, not org members: a staff role is granted by
-- the operator CLI (audeniq-admin staff grant) with the schema-owner login.
-- The API role can only read it, so no request can promote itself.
CREATE TABLE identity.staff_members (
 user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE RESTRICT,
 -- ADMIN: everything. REVIEWER: release review, documents, inquiries.
 -- OPERATOR: delivery staging approvals, DSP overview. SUPPORT: inquiries.
 role text NOT NULL CHECK (role IN ('ADMIN','REVIEWER','OPERATOR','SUPPORT')),
 status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','REVOKED')),
 granted_by text NOT NULL CHECK (length(btrim(granted_by)) BETWEEN 1 AND 120),
 granted_at timestamptz NOT NULL DEFAULT now(),
 revoked_at timestamptz NULL,
 CHECK ((status = 'REVOKED') = (revoked_at IS NOT NULL))
);

-- Second-person rule for staff: approving a release whose open checks
-- include a rights/money class or a BLOCKED finding needs a different staff
-- reviewer. The PASS overrides are written only on approval.
CREATE TABLE rights.staff_approvals (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL,
 revision_id uuid NOT NULL REFERENCES catalog.application_revisions ON DELETE RESTRICT,
 check_codes text[] NOT NULL CHECK (cardinality(check_codes) BETWEEN 1 AND 200),
 reason text NOT NULL CHECK (length(btrim(reason)) BETWEEN 1 AND 2000),
 requested_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','APPROVED','DECLINED')),
 decided_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 decided_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL DEFAULT now() + interval '7 days',
 CHECK ((status = 'PENDING') = (decided_at IS NULL)),
 CHECK (status <> 'APPROVED' OR decided_by <> requested_by)
);
CREATE INDEX staff_approvals_open ON rights.staff_approvals(status, created_at) WHERE status = 'PENDING';
CREATE UNIQUE INDEX staff_approvals_one_open ON rights.staff_approvals(revision_id) WHERE status = 'PENDING';

-- Reviewer notes the artist sees next to a correction (per check or for
-- the whole release when check_code is NULL). Append-only.
CREATE TABLE rights.review_notes (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL,
 revision_id uuid NOT NULL REFERENCES catalog.application_revisions ON DELETE RESTRICT,
 check_code text NULL CHECK (check_code IS NULL OR length(btrim(check_code)) BETWEEN 1 AND 100),
 decision text NOT NULL CHECK (decision IN ('APPROVE','REQUEST_CORRECTION','REJECT')),
 note text NOT NULL CHECK (length(note) <= 2000),
 author_user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX review_notes_revision ON rights.review_notes(org_id, revision_id, created_at);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON rights.review_notes
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- A staff rejection closes the application (config/states.json keeps the
-- same edge).
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_REVIEW','WITHDRAWN')
 ON CONFLICT DO NOTHING;

-- Tell the artist about a rejection like any other status change.
CREATE OR REPLACE FUNCTION portal.release_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text := coalesce(nullif(btrim(NEW.title), ''), '발매');
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.status LIKE '%\_CORRECTION' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '에 보완 요청이 있어요.',
   '발매 관리에서 ‘보완하기’를 누르면 고칠 곳으로 바로 이동해요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'SUBMITTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 신청이 접수됐어요.',
   '담당자 검토가 시작됐어요. 진행 상황은 발매 관리에서 볼 수 있어요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'ON_HOLD_RIGHTS' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '의 권리 확인이 필요해요.',
   '권리·보완 서류에서 요청된 증빙을 제출해 주세요.', '/rights');
 ELSIF NEW.status = 'READY_FOR_DELIVERY' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 검토가 끝났어요.',
   '플랫폼 배급이 준비됐어요. 발매일에 맞춰 공개돼요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'WITHDRAWN' AND OLD.status = 'STAGE2_REVIEW' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 신청이 반려됐어요.',
   '검토 의견은 발매 상세에서 볼 수 있어요. 궁금한 점은 문의로 남겨 주세요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'LIVE' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '이(가) 발매됐어요.',
   '플랫폼에 공개됐어요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'TAKEN_DOWN' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '이(가) 플랫폼에서 내려갔어요.',
   '자세한 내용은 문의로 확인해 주세요.', '/releases/' || NEW.id);
 END IF;
 RETURN NEW;
END $$;

-- Onboarding state is operator-owned (RLS, no runtime policy). Staff screens
-- read it through this narrow definer function: stage + missing evidence.
CREATE FUNCTION execution.partner_readiness(p_partner_id text)
RETURNS TABLE (stage text, gaps text[])
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT o.stage, ARRAY(SELECT g.requirement FROM execution.partner_onboarding_gaps(p_partner_id) g)
 FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id
$$;
REVOKE ALL ON FUNCTION execution.partner_readiness(text) FROM PUBLIC;
