CREATE SCHEMA identity;
CREATE SCHEMA catalog;
CREATE SCHEMA rights;
CREATE SCHEMA distribution;
CREATE SCHEMA finance;
CREATE SCHEMA operations;
CREATE TABLE identity.orgs (
 id uuid PRIMARY KEY, name text NOT NULL CHECK(length(name) BETWEEN 1 AND 200),
 kind text NOT NULL CHECK(kind IN ('PERSONAL','LABEL','COMPANY')), created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE identity.parties (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 kind text NOT NULL CHECK(kind IN ('PERSON','LEGAL_ENTITY')), display_name text NOT NULL,
 UNIQUE(org_id,id)
);
CREATE TABLE identity.users (
 id uuid PRIMARY KEY, email text NOT NULL UNIQUE CHECK(email=lower(email)), password_hash text NOT NULL,
 party_id uuid NOT NULL REFERENCES identity.parties ON DELETE RESTRICT,
 status text NOT NULL DEFAULT 'ACTIVE' CHECK(status IN ('ACTIVE','ON_HOLD','DISABLED')),
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE identity.memberships (
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 role text NOT NULL CHECK(role IN ('OWNER','EDITOR','VIEWER')),
 status text NOT NULL DEFAULT 'ACTIVE' CHECK(status IN ('ACTIVE','REVOKED')),
 PRIMARY KEY(org_id,user_id)
);
CREATE TABLE identity.sessions (
 token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 csrf_hash bytea NOT NULL CHECK(octet_length(csrf_hash)=32),
 expires_at timestamptz NOT NULL, revoked_at timestamptz, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user ON identity.sessions(user_id);
CREATE TABLE identity.auth_limits (
 bucket_hash bytea PRIMARY KEY, window_start timestamptz NOT NULL DEFAULT now(), attempts integer NOT NULL CHECK(attempts>=0)
);
-- A typed resource registry supplies a real composite FK for every ACL.
CREATE TABLE identity.resources (
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 id uuid NOT NULL, kind text NOT NULL CHECK(kind IN ('artist','label','release','asset')),
 PRIMARY KEY(org_id,id), UNIQUE(id), UNIQUE(org_id,id,kind)
);
CREATE TABLE identity.resource_acl (
 org_id uuid NOT NULL, resource_id uuid NOT NULL,
 principal_party_id uuid NOT NULL REFERENCES identity.parties ON DELETE RESTRICT,
 action text NOT NULL CHECK(action IN ('read','write')),
 starts_at timestamptz NOT NULL DEFAULT now(), ends_at timestamptz, revoked_at timestamptz,
 PRIMARY KEY(org_id,resource_id,principal_party_id,action),
 FOREIGN KEY(org_id,resource_id) REFERENCES identity.resources(org_id,id) ON DELETE RESTRICT,
 CHECK(ends_at IS NULL OR ends_at>starts_at)
);
CREATE TABLE identity.payees (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, party_id uuid NOT NULL,
 FOREIGN KEY(org_id,party_id) REFERENCES identity.parties(org_id,id) ON DELETE RESTRICT,
 UNIQUE(org_id,id)
);
CREATE TABLE catalog.labels (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, resource_kind text NOT NULL DEFAULT 'label' CHECK(resource_kind='label'),
 party_id uuid NOT NULL, name text NOT NULL CHECK(length(name) BETWEEN 1 AND 200),
 profile jsonb NOT NULL DEFAULT '{}', row_version bigint NOT NULL DEFAULT 0, archived_at timestamptz,
 FOREIGN KEY(org_id,id,resource_kind) REFERENCES identity.resources(org_id,id,kind) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,party_id) REFERENCES identity.parties(org_id,id) ON DELETE RESTRICT,
 UNIQUE(org_id,id)
);
CREATE TABLE catalog.artists (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, resource_kind text NOT NULL DEFAULT 'artist' CHECK(resource_kind='artist'),
 party_id uuid, label_id uuid, name text NOT NULL CHECK(length(name) BETWEEN 1 AND 200),
 profile jsonb NOT NULL DEFAULT '{}', row_version bigint NOT NULL DEFAULT 0, archived_at timestamptz,
 FOREIGN KEY(org_id,id,resource_kind) REFERENCES identity.resources(org_id,id,kind) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,party_id) REFERENCES identity.parties(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,label_id) REFERENCES catalog.labels(org_id,id) ON DELETE RESTRICT,
 UNIQUE(org_id,id)
);
CREATE TABLE catalog.releases (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, resource_kind text NOT NULL DEFAULT 'release' CHECK(resource_kind='release'),
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 300),
 release_type text NOT NULL CHECK(release_type IN ('SINGLE','EP','ALBUM')),
 status text NOT NULL DEFAULT 'DRAFT', draft jsonb NOT NULL DEFAULT '{}',
 current_revision_id uuid, row_version bigint NOT NULL DEFAULT 0, rights_epoch bigint NOT NULL DEFAULT 0 CHECK(rights_epoch>=0),
 archived_at timestamptz, created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(org_id,id,resource_kind) REFERENCES identity.resources(org_id,id,kind) ON DELETE RESTRICT,
 UNIQUE(org_id,id)
);
CREATE TABLE catalog.assets (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, resource_kind text NOT NULL DEFAULT 'asset' CHECK(resource_kind='asset'),
 kind text NOT NULL CHECK(kind IN ('AUDIO','IMAGE')), object_key text NOT NULL UNIQUE,
 size_bytes bigint NOT NULL CHECK(size_bytes>0), content_type text NOT NULL,
 state text NOT NULL DEFAULT 'UPLOADING' CHECK(state IN ('UPLOADING','REGISTERED','REJECTED')),
 qc_status text NOT NULL DEFAULT 'PENDING' CHECK(qc_status IN ('PENDING','PASS','BLOCKED')),
 sha256 text CHECK(sha256 ~ '^[a-f0-9]{64}$'), etag text,
 FOREIGN KEY(org_id,id,resource_kind) REFERENCES identity.resources(org_id,id,kind) ON DELETE RESTRICT,
 UNIQUE(org_id,id)
);
CREATE TABLE catalog.upload_sessions (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, asset_id uuid NOT NULL UNIQUE,
 expected_key text NOT NULL UNIQUE, nonce uuid NOT NULL UNIQUE,
 expected_bytes bigint NOT NULL CHECK(expected_bytes>0 AND expected_bytes<=536870912),
 content_type text NOT NULL, expires_at timestamptz NOT NULL,
 status text NOT NULL DEFAULT 'ISSUED' CHECK(status IN ('ISSUED','COMPLETED')),
 completed_at timestamptz,
 FOREIGN KEY(org_id,asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT,
 CHECK((status='COMPLETED')=(completed_at IS NOT NULL))
);
CREATE TABLE catalog.tracks (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, release_id uuid NOT NULL,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 300), disc_number integer NOT NULL CHECK(disc_number>0),
 track_number integer NOT NULL CHECK(track_number>0), artist_id uuid NOT NULL, asset_id uuid,
 isrc text CHECK(isrc ~ '^[A-Z]{2}[A-Z0-9]{3}[0-9]{7}$'),
 FOREIGN KEY(org_id,release_id) REFERENCES catalog.releases(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,artist_id) REFERENCES catalog.artists(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT,
 UNIQUE(release_id,disc_number,track_number), UNIQUE(org_id,id)
);
CREATE TABLE catalog.credits (
 track_id uuid NOT NULL, org_id uuid NOT NULL, party_id uuid NOT NULL, role text NOT NULL,
 PRIMARY KEY(track_id,party_id,role),
 FOREIGN KEY(org_id,track_id) REFERENCES catalog.tracks(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,party_id) REFERENCES identity.parties(org_id,id) ON DELETE RESTRICT
);
CREATE TABLE catalog.application_revisions (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, release_id uuid NOT NULL, revision integer NOT NULL CHECK(revision>0),
 body jsonb NOT NULL, body_hash text NOT NULL CHECK(body_hash ~ '^[a-f0-9]{64}$'),
 consent_package_hash text NOT NULL CHECK(consent_package_hash ~ '^[a-f0-9]{64}$'),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT, created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(org_id,release_id) REFERENCES catalog.releases(org_id,id) ON DELETE RESTRICT,
 UNIQUE(release_id,revision), UNIQUE(org_id,release_id,id), UNIQUE(org_id,id)
);
ALTER TABLE catalog.releases ADD CONSTRAINT current_revision_owner
 FOREIGN KEY(org_id,id,current_revision_id) REFERENCES catalog.application_revisions(org_id,release_id,id) ON DELETE RESTRICT;
CREATE TABLE catalog.consent_packages (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, revision_id uuid NOT NULL, body jsonb NOT NULL,
 package_hash text NOT NULL CHECK(package_hash ~ '^[a-f0-9]{64}$'), policy_version text NOT NULL,
 FOREIGN KEY(org_id,revision_id) REFERENCES catalog.application_revisions(org_id,id) ON DELETE RESTRICT,
 UNIQUE(org_id,id)
);
CREATE TABLE distribution.verification_packages (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, revision_id uuid NOT NULL,
 body jsonb NOT NULL, package_hash text NOT NULL CHECK(package_hash ~ '^[a-f0-9]{64}$'),
 rights_epoch bigint NOT NULL CHECK(rights_epoch>=0),
 FOREIGN KEY(org_id,revision_id) REFERENCES catalog.application_revisions(org_id,id) ON DELETE RESTRICT,
 UNIQUE(org_id,id)
);
CREATE TABLE distribution.release_snapshots (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, verification_id uuid NOT NULL,
 body jsonb NOT NULL, snapshot_hash text NOT NULL CHECK(snapshot_hash ~ '^[a-f0-9]{64}$'),
 FOREIGN KEY(org_id,verification_id) REFERENCES distribution.verification_packages(org_id,id) ON DELETE RESTRICT,
 UNIQUE(org_id,id)
);
CREATE TABLE operations.audit_events (
 id uuid PRIMARY KEY, actor_user_id uuid REFERENCES identity.users ON DELETE RESTRICT,
 actor_service text, org_id uuid REFERENCES identity.orgs ON DELETE RESTRICT,
 resource_id uuid, action text NOT NULL, reason_code text NOT NULL,
 request_id uuid NOT NULL, occurred_at timestamptz NOT NULL DEFAULT now(),
 CHECK(actor_user_id IS NOT NULL OR actor_service IS NOT NULL)
);
CREATE INDEX audit_org_time ON operations.audit_events(org_id,occurred_at DESC);
CREATE TABLE operations.jobs (
 id uuid PRIMARY KEY, queue text NOT NULL CHECK(queue IN ('interactive','qc','rights','distribution','finance')),
 kind text NOT NULL, payload jsonb NOT NULL, priority integer NOT NULL DEFAULT 0,
 run_at timestamptz NOT NULL DEFAULT now(), status text NOT NULL DEFAULT 'QUEUED'
 CHECK(status IN ('QUEUED','RUNNING','SUCCEEDED','DEAD_LETTER')),
 attempts integer NOT NULL DEFAULT 0 CHECK(attempts>=0), max_attempts integer NOT NULL DEFAULT 5 CHECK(max_attempts BETWEEN 1 AND 20),
 locked_by text, lock_token uuid, lease_until timestamptz,
 pinned_revision_id uuid REFERENCES catalog.application_revisions ON DELETE RESTRICT,
 idempotency_key text NOT NULL UNIQUE, last_error text, dead_lettered_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK((status='RUNNING')=(lock_token IS NOT NULL AND lease_until IS NOT NULL))
);
CREATE INDEX jobs_claim ON operations.jobs(queue,priority DESC,run_at) WHERE status IN ('QUEUED','RUNNING');
CREATE TABLE operations.outbox (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 aggregate_id uuid NOT NULL, event_type text NOT NULL, payload jsonb NOT NULL,
 idempotency_key text NOT NULL UNIQUE, created_at timestamptz NOT NULL DEFAULT now(), published_at timestamptz
);
CREATE TABLE operations.event_receipts (
 event_id uuid NOT NULL REFERENCES operations.outbox ON DELETE RESTRICT,
 consumer text NOT NULL, processed_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY(event_id,consumer)
);
CREATE TABLE operations.check_results (
 id uuid PRIMARY KEY, revision_id uuid NOT NULL REFERENCES catalog.application_revisions ON DELETE RESTRICT,
 check_code text NOT NULL, rule_version text NOT NULL, status text NOT NULL
 CHECK(status IN ('PASS','CORRECTION_REQUIRED','REVIEW_REQUIRED','BLOCKED','TECHNICAL_RETRY','NOT_APPLICABLE','STALE','UNKNOWN')),
 result_hash text NOT NULL CHECK(result_hash ~ '^[a-f0-9]{64}$'), created_at timestamptz NOT NULL DEFAULT now()
);
CREATE FUNCTION operations.reject_mutation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'immutable record' USING ERRCODE='23514'; END $$;
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['catalog.application_revisions','catalog.consent_packages','distribution.verification_packages','distribution.release_snapshots','operations.audit_events','operations.check_results','operations.event_receipts'] LOOP
 EXECUTE format('CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON %s FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation()',t);
 END LOOP;
END $$;
CREATE FUNCTION catalog.check_track_asset() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.asset_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM catalog.assets WHERE id=NEW.asset_id AND org_id=NEW.org_id AND state='REGISTERED' AND kind='AUDIO') THEN
 RAISE EXCEPTION 'unregistered audio asset' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER track_asset BEFORE INSERT OR UPDATE ON catalog.tracks FOR EACH ROW EXECUTE FUNCTION catalog.check_track_asset();
