-- F2: Stage 1 Validation Package (immutable, mirrors verification_packages).
CREATE TABLE distribution.validation_packages (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 revision_id uuid NOT NULL REFERENCES catalog.application_revisions ON DELETE RESTRICT,
 body jsonb NOT NULL,
 package_hash text NOT NULL CHECK(package_hash ~ '^[a-f0-9]{64}$'),
 rule_version text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,id), UNIQUE(org_id,revision_id)
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON distribution.validation_packages
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
-- F2: open the application pipeline. F1 closed every status change
-- ("pipeline execution gated until F2"); the allowed_transitions table is now
-- the real enforcement for stage progression.
CREATE OR REPLACE FUNCTION catalog.guard_pipeline() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.status<>OLD.status THEN
  IF NOT EXISTS(SELECT 1 FROM operations.allowed_transitions WHERE axis='application_pipeline_status' AND old_status=OLD.status AND new_status=NEW.status) THEN
   RAISE EXCEPTION 'forbidden pipeline transition' USING ERRCODE='23514'; END IF;
 END IF;
 IF NEW.row_version<>OLD.row_version+1 THEN RAISE EXCEPTION 'row version must increment' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
-- F2: human-readable check detail (inputs summary / cache_hit). NULL-safe for F1 rows.
ALTER TABLE operations.check_results ADD COLUMN detail text;
-- F2: consent is captured pre-submit, before any revision exists. The canonical
-- link is application_revisions.consent_package_hash = consent_packages.package_hash.
ALTER TABLE catalog.consent_packages ALTER COLUMN revision_id DROP NOT NULL;
