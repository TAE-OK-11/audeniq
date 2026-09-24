-- F4 Astra: record supplied identifiers only. No pool, reservation or issuance API.
CREATE FUNCTION distribution.valid_upc_a(v text) RETURNS boolean
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
 SELECT CASE WHEN v !~ '^[0-9]{12}$' OR v='000000000000' THEN false ELSE
   (SELECT sum(substr(v,i,1)::integer * CASE WHEN i%2=1 THEN 3 ELSE 1 END)%10=0
    FROM generate_series(1,12) AS i)
 END
$$;

CREATE TABLE distribution.identifier_assignments (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL,
 track_id uuid,
 revision_id uuid NOT NULL,
 kind text NOT NULL CHECK(kind IN ('ISRC','UPC')),
 identifier text NOT NULL,
 source text NOT NULL DEFAULT 'EXISTING' CHECK(source='EXISTING'),
 status text NOT NULL DEFAULT 'ASSIGNED' CHECK(status='ASSIGNED'),
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(org_id,release_id,revision_id)
   REFERENCES catalog.application_revisions(org_id,release_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,track_id) REFERENCES catalog.tracks(org_id,id) ON DELETE RESTRICT,
 CHECK((kind='ISRC' AND track_id IS NOT NULL AND identifier ~ '^[A-Z]{2}[A-Z0-9]{3}[0-9]{7}$')
    OR (kind='UPC' AND track_id IS NULL AND distribution.valid_upc_a(identifier))),
 UNIQUE(kind,identifier),
 UNIQUE(org_id,id)
);
CREATE UNIQUE INDEX identifier_track_assignment
 ON distribution.identifier_assignments(org_id,track_id) WHERE kind='ISRC';
CREATE UNIQUE INDEX identifier_release_assignment
 ON distribution.identifier_assignments(org_id,release_id) WHERE kind='UPC';

CREATE FUNCTION distribution.guard_identifier_track() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.track_id IS NOT NULL AND NOT EXISTS (
  SELECT 1 FROM catalog.tracks
  WHERE org_id=NEW.org_id AND release_id=NEW.release_id AND id=NEW.track_id
 ) THEN RAISE EXCEPTION 'identifier track release mismatch' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER identifier_track BEFORE INSERT ON distribution.identifier_assignments
 FOR EACH ROW EXECUTE FUNCTION distribution.guard_identifier_track();
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON distribution.identifier_assignments
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
ALTER TABLE distribution.identifier_assignments ENABLE ROW LEVEL SECURITY;
ALTER TABLE distribution.identifier_assignments FORCE ROW LEVEL SECURITY;
CREATE POLICY identifier_org_scope ON distribution.identifier_assignments
 USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid)
 WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid);
-- No runtime grants. Transaction owner must authorize the org and SET LOCAL
-- app.org_id before use. Cross-target/catalog reuse requires separate review;
-- this ledger cannot silently transfer or manufacture an identifier.
