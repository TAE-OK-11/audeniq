-- 0064: every job carries the release it works for (P1, docs/PIPELINE_ARCHITECTURE.md).
--
-- Tracing a release meant searching job payload JSON for revision, package
-- or delivery-job ids. Jobs now record release_id at insert, resolved by a
-- trigger so no enqueue site (and no payload, which takes part in the
-- idempotency comparison) has to change:
--   payload.release_id
--   -> pinned_revision_id / payload.revision_id  (application_revisions)
--   -> payload.package_id                          (packages -> canonical release)
--   -> payload.delivery_job_id                     (delivery job -> package)
-- Jobs not tied to a release (asset analysis before attachment, partner
-- inbox, outbox) keep NULL. The trigger runs as the inserting role; a row
-- its RLS hides also leaves NULL, never an error.
ALTER TABLE operations.jobs ADD COLUMN release_id uuid NULL;

CREATE FUNCTION operations.set_job_release_id() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
  uuid_re constant text := '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$';
  v text;
  rev uuid;
  pkg uuid;
BEGIN
  IF NEW.release_id IS NOT NULL THEN
    RETURN NEW;
  END IF;
  v := NEW.payload->>'release_id';
  IF v ~ uuid_re THEN
    NEW.release_id := v::uuid;
    RETURN NEW;
  END IF;
  rev := NEW.pinned_revision_id;
  IF rev IS NULL THEN
    v := NEW.payload->>'revision_id';
    IF v ~ uuid_re THEN rev := v::uuid; END IF;
  END IF;
  IF rev IS NOT NULL THEN
    SELECT r.release_id INTO NEW.release_id
      FROM catalog.application_revisions r WHERE r.id = rev;
    RETURN NEW;
  END IF;
  v := NEW.payload->>'package_id';
  IF v ~ uuid_re THEN
    pkg := v::uuid;
  ELSE
    v := NEW.payload->>'delivery_job_id';
    IF v ~ uuid_re THEN
      SELECT j.package_id INTO pkg FROM execution.delivery_jobs j WHERE j.id = v::uuid;
    END IF;
  END IF;
  IF pkg IS NOT NULL THEN
    SELECT cr.release_id INTO NEW.release_id
      FROM distribution.distribution_packages dp
      JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
     WHERE dp.id = pkg;
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER jobs_release_id
BEFORE INSERT ON operations.jobs
FOR EACH ROW EXECUTE FUNCTION operations.set_job_release_id();

-- Backfill existing rows with the same rules (the trigger fires on INSERT
-- only; this UPDATE touches no status/run_at, so no queue notification).
UPDATE operations.jobs j SET release_id = COALESCE(
  CASE WHEN j.payload->>'release_id' ~ '^[0-9a-fA-F-]{36}$' THEN (j.payload->>'release_id')::uuid END,
  (SELECT r.release_id FROM catalog.application_revisions r
    WHERE r.id = COALESCE(j.pinned_revision_id,
                          CASE WHEN j.payload->>'revision_id' ~ '^[0-9a-fA-F-]{36}$'
                               THEN (j.payload->>'revision_id')::uuid END)),
  (SELECT cr.release_id FROM distribution.distribution_packages dp
     JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
    WHERE dp.id = COALESCE(
            CASE WHEN j.payload->>'package_id' ~ '^[0-9a-fA-F-]{36}$' THEN (j.payload->>'package_id')::uuid END,
            (SELECT dj.package_id FROM execution.delivery_jobs dj
              WHERE j.payload->>'delivery_job_id' ~ '^[0-9a-fA-F-]{36}$'
                AND dj.id = (j.payload->>'delivery_job_id')::uuid))))
WHERE j.release_id IS NULL;

CREATE INDEX jobs_release ON operations.jobs(release_id, created_at)
  WHERE release_id IS NOT NULL;

-- Staff release timeline (GET /api/staff/releases/{id}/timeline): audit
-- events are looked up by the release's resources (release, packages,
-- delivery jobs, jobs, track assets), and upload-time analysis jobs by
-- asset. Both were sequential scans of append-only history.
CREATE INDEX audit_resource_time ON operations.audit_events(resource_id, occurred_at)
  WHERE resource_id IS NOT NULL;
CREATE INDEX jobs_asset_analyze ON operations.jobs((payload->>'asset_id'))
  WHERE kind = 'asset.analyze';
