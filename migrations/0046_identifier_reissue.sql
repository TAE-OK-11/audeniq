-- 0046: replace VIRTUAL (test-range) codes once real ranges exist.
--
-- A release or track that received a VIRTUAL UPC/ISRC used to keep it
-- forever, so registering the company's real GS1 prefix / ISRC registrant
-- code never reached it. A VIRTUAL assignment may now be RETIRED (one way,
-- never deleted); the next Stage 3 run for that target issues a code from
-- the active REGISTERED range. Retired codes stay in the ledger and are
-- never handed out again (UNIQUE(kind, identifier) is unchanged). Codes the
-- artist supplied (EXISTING) or issued from a registered range never change.
ALTER TABLE distribution.identifier_assignments
 DROP CONSTRAINT identifier_assignments_status_check;
ALTER TABLE distribution.identifier_assignments
 ADD CONSTRAINT identifier_assignments_status_check CHECK(status IN ('ASSIGNED','RETIRED'));
ALTER TABLE distribution.identifier_assignments ADD COLUMN retired_at timestamptz;
ALTER TABLE distribution.identifier_assignments
 ADD CONSTRAINT identifier_assignments_retired_check
 CHECK((status = 'RETIRED') = (retired_at IS NOT NULL) AND (status = 'ASSIGNED' OR source = 'VIRTUAL'));

-- One live code per target; retired rows no longer occupy the slot.
DROP INDEX distribution.identifier_track_assignment;
DROP INDEX distribution.identifier_release_assignment;
CREATE UNIQUE INDEX identifier_track_assignment
 ON distribution.identifier_assignments(org_id,track_id) WHERE kind='ISRC' AND status='ASSIGNED';
CREATE UNIQUE INDEX identifier_release_assignment
 ON distribution.identifier_assignments(org_id,release_id) WHERE kind='UPC' AND status='ASSIGNED';

-- Replace blanket immutability with: no deletes; the only update is
-- VIRTUAL ASSIGNED -> RETIRED with every other column unchanged.
DROP TRIGGER immutable ON distribution.identifier_assignments;
CREATE FUNCTION distribution.guard_identifier_retire() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP = 'DELETE' THEN RAISE EXCEPTION 'identifier ledger rows are permanent' USING ERRCODE='55000'; END IF;
 IF OLD.status <> 'ASSIGNED' OR OLD.source <> 'VIRTUAL' OR NEW.status <> 'RETIRED'
    OR NEW.id <> OLD.id OR NEW.org_id <> OLD.org_id OR NEW.release_id <> OLD.release_id
    OR NEW.track_id IS DISTINCT FROM OLD.track_id OR NEW.revision_id <> OLD.revision_id
    OR NEW.kind <> OLD.kind OR NEW.identifier <> OLD.identifier OR NEW.source <> OLD.source
    OR NEW.created_at <> OLD.created_at THEN
  RAISE EXCEPTION 'only a VIRTUAL identifier can be retired' USING ERRCODE='55000';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER retire_only BEFORE UPDATE OR DELETE ON distribution.identifier_assignments
 FOR EACH ROW EXECUTE FUNCTION distribution.guard_identifier_retire();

-- A frozen package carrying VIRTUAL codes can go back to the artist for a
-- resubmission that picks up real codes (staff action, crate::staff).
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','READY_FOR_DELIVERY','STAGE3_CORRECTION')
 ON CONFLICT DO NOTHING;
