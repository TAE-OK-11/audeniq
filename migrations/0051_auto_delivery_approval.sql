-- 0051: delivery runs by itself after the final release approval.
--
-- Staff decide a release once (발매 심사). When its agreement is APPROVED
-- (or SIGNED), every staged DSP package that is not content-blocked is
-- approved by the system, including packages staged or re-staged later.
-- Delivery still waits for the artist's signature (0048), so the flow is:
-- staff final approval -> artist signs -> E-0 sends. A staff HOLD stays held.
DO $$
DECLARE c text;
BEGIN
 SELECT conname INTO c FROM pg_constraint
  WHERE conrelid = 'distribution.delivery_staging'::regclass AND contype = 'c'
    AND pg_get_constraintdef(oid) LIKE '%approval_by IS NOT NULL%';
 IF c IS NOT NULL THEN
  EXECUTE format('ALTER TABLE distribution.delivery_staging DROP CONSTRAINT %I', c);
 END IF;
END $$;
-- approval_by NULL on an APPROVED row = approved by the system.
ALTER TABLE distribution.delivery_staging ADD CONSTRAINT delivery_staging_approved_not_blocked
 CHECK (approval <> 'APPROVED' OR readiness <> 'CONTENT_BLOCKED');

CREATE FUNCTION distribution.release_finally_approved(p_org uuid, p_release uuid)
RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT EXISTS (
  SELECT 1 FROM portal.documents d
   WHERE d.org_id = p_org AND d.release_id = p_release
     AND d.kind = 'AGREEMENT' AND d.status IN ('APPROVED','SIGNED'))
$$;
REVOKE ALL ON FUNCTION distribution.release_finally_approved(uuid, uuid) FROM PUBLIC;

CREATE FUNCTION distribution.auto_approve_staging() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF NEW.approval = 'PENDING' AND NEW.readiness <> 'CONTENT_BLOCKED'
    AND distribution.release_finally_approved(NEW.org_id, NEW.release_id) THEN
  NEW.approval := 'APPROVED';
  NEW.approval_by := NULL;
  NEW.approval_note := '발매 최종 승인에 따른 자동 승인';
  NEW.approval_at := now();
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER delivery_staging_auto_approve BEFORE INSERT OR UPDATE ON distribution.delivery_staging
 FOR EACH ROW EXECUTE FUNCTION distribution.auto_approve_staging();

-- Packages already staged when staff approve: touch them so the rule applies.
CREATE FUNCTION portal.agreement_approves_staging() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 UPDATE distribution.delivery_staging s SET approval = 'PENDING'
   FROM catalog.releases r
  WHERE r.id = s.release_id AND r.org_id = s.org_id AND r.current_revision_id = s.revision_id
    AND s.org_id = NEW.org_id AND s.release_id = NEW.release_id
    AND s.approval = 'PENDING' AND s.readiness <> 'CONTENT_BLOCKED';
 RETURN NEW;
END $$;
CREATE TRIGGER documents_agreement_approves_staging AFTER UPDATE OF status ON portal.documents
 FOR EACH ROW WHEN (NEW.kind = 'AGREEMENT' AND NEW.status IN ('APPROVED','SIGNED') AND OLD.status IS DISTINCT FROM NEW.status)
 EXECUTE FUNCTION portal.agreement_approves_staging();

-- Releases approved before this migration.
UPDATE distribution.delivery_staging s SET approval = 'PENDING'
  FROM catalog.releases r
 WHERE r.id = s.release_id AND r.org_id = s.org_id AND r.current_revision_id = s.revision_id
   AND s.approval = 'PENDING' AND s.readiness <> 'CONTENT_BLOCKED';
