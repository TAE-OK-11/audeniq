-- Sandbox finding P1-4: a release sent back to STAGE1_CORRECTION must be
-- editable (replace audio, fix credits) so the artist can resubmit. Submitted
-- content is frozen in catalog.application_revisions, so live track/credit
-- edits in correction never alter what was reviewed; resubmission creates a
-- new revision. Every other non-draft status stays locked.
CREATE OR REPLACE FUNCTION catalog.guard_draft_track() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE parent_status text; parent_archived timestamptz;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'archive tracks instead' USING ERRCODE='23514'; END IF;
 IF TG_OP='UPDATE' AND (NEW.org_id<>OLD.org_id OR NEW.release_id<>OLD.release_id OR NEW.id<>OLD.id OR OLD.archived_at IS NOT NULL) THEN
  RAISE EXCEPTION 'track identity and archived records are fixed' USING ERRCODE='23514';
 END IF;
 SELECT status,archived_at INTO parent_status,parent_archived FROM catalog.releases WHERE org_id=NEW.org_id AND id=NEW.release_id FOR UPDATE;
 IF parent_status IS NULL OR parent_status NOT IN ('DRAFT','STAGE1_CORRECTION') OR parent_archived IS NOT NULL THEN
  RAISE EXCEPTION 'track requires active draft' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION catalog.guard_draft_credit() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE track_uuid uuid; org_uuid uuid; parent_status text; parent_archived timestamptz; track_archived timestamptz;
BEGIN
 IF TG_OP='DELETE' THEN track_uuid:=OLD.track_id; org_uuid:=OLD.org_id;
 ELSE track_uuid:=NEW.track_id; org_uuid:=NEW.org_id; END IF;
 IF TG_OP='UPDATE' AND (NEW.org_id<>OLD.org_id OR NEW.track_id<>OLD.track_id) THEN
  RAISE EXCEPTION 'credit ownership is fixed' USING ERRCODE='23514';
 END IF;
 SELECT r.status,r.archived_at,t.archived_at INTO parent_status,parent_archived,track_archived
 FROM catalog.tracks t JOIN catalog.releases r ON r.org_id=t.org_id AND r.id=t.release_id
 WHERE t.org_id=org_uuid AND t.id=track_uuid FOR UPDATE OF r;
 IF parent_status IS NULL OR parent_status NOT IN ('DRAFT','STAGE1_CORRECTION') OR parent_archived IS NOT NULL OR track_archived IS NOT NULL THEN
  RAISE EXCEPTION 'credit requires active draft track' USING ERRCODE='23514';
 END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 RETURN NEW;
END $$;
