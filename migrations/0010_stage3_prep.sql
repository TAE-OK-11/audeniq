-- F4: Stage 3 prep artifacts, Muse portion (BLUEPRINT §6 3-A/3-D/3-F).
-- canonical_releases: byte-immutable snapshot of the approved metadata, tracks,
-- credits, rights scope, asset SHAs and policy values pinned from the Stage 2
-- verification package. One row per verification package; new revision or new
-- rights epoch means a new snapshot, never an update.
CREATE TABLE distribution.canonical_releases (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 revision_id uuid NOT NULL REFERENCES catalog.application_revisions ON DELETE RESTRICT,
 verification_package_id uuid NOT NULL REFERENCES distribution.verification_packages ON DELETE RESTRICT,
 canonical_hash text NOT NULL CHECK(canonical_hash ~ '^[a-f0-9]{64}$'),
 body jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(verification_package_id),
 UNIQUE(org_id,id)
);
CREATE INDEX canonical_releases_release ON distribution.canonical_releases(org_id,release_id);
-- distribution_packages: frozen submission packages. status is intentionally
-- open-ended ('PREPARED' today; Astra's route/ERN stages extend it) so the
-- route/ERN work lands without a schema change. Rows are immutable; a package
-- is identified by its canonical snapshot + content hash.
CREATE TABLE distribution.distribution_packages (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 canonical_release_id uuid NOT NULL REFERENCES distribution.canonical_releases ON DELETE RESTRICT,
 package_hash text NOT NULL CHECK(package_hash ~ '^[a-f0-9]{64}$'),
 body jsonb NOT NULL,
 status text NOT NULL DEFAULT 'PREPARED' CHECK(length(btrim(status))>0),
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(canonical_release_id),
 UNIQUE(org_id,id)
);
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['distribution.canonical_releases','distribution.distribution_packages'] LOOP
  EXECUTE format('CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON %s FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation()',t);
 END LOOP;
END $$;
-- Deliberately no runtime grants for the new tables (same stance as F2/F3):
-- deploy/grants.sql stays closed until the runtime-role hardening pass.
