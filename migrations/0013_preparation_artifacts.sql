-- F4: append-only preparation artifacts. The frozen package body keeps the
-- canonical snapshot immutable; the ERN hash, preflight report and route
-- plan land here keyed by package, exactly one row per frozen package.
-- Nothing here is ever updated or deleted: a re-preparation writes nothing
-- new (UNIQUE on package_id), a new snapshot freezes a new package first.
CREATE TABLE distribution.preparation_artifacts (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL,
 revision_id uuid NOT NULL,
 canonical_release_id uuid NOT NULL REFERENCES distribution.canonical_releases(id) ON DELETE RESTRICT,
 package_id uuid NOT NULL REFERENCES distribution.distribution_packages(id) ON DELETE RESTRICT,
 ern_sha256 text NOT NULL CHECK (ern_sha256 ~ '^[0-9a-f]{64}$'),
 preflight_report jsonb NOT NULL,
 route_plan jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (org_id, release_id, revision_id)
   REFERENCES catalog.application_revisions (org_id, release_id, id) ON DELETE RESTRICT,
 UNIQUE (package_id)
);
CREATE TRIGGER preparation_artifacts_immutable
 BEFORE UPDATE OR DELETE ON distribution.preparation_artifacts
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
