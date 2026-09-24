-- Offline structural lineage only. No route can be activated by this migration.
CREATE TABLE rights.contracts (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 grantor_party_id uuid NOT NULL, grantee_party_id uuid NOT NULL,
 FOREIGN KEY(org_id,grantor_party_id) REFERENCES identity.parties(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,grantee_party_id) REFERENCES identity.parties(org_id,id) ON DELETE RESTRICT,
 CHECK(grantor_party_id<>grantee_party_id), UNIQUE(org_id,id)
);
CREATE TABLE rights.contract_revisions (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, contract_id uuid NOT NULL,
 revision integer NOT NULL CHECK(revision>0), document_asset_id uuid NOT NULL,
 document_hash text NOT NULL CHECK(document_hash ~ '^[a-f0-9]{64}$'),
 policy_version text NOT NULL CHECK(length(btrim(policy_version))>0),
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(org_id,contract_id) REFERENCES rights.contracts(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,document_asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT,
 UNIQUE(contract_id,revision), UNIQUE(org_id,contract_id,id)
);
CREATE TABLE distribution.dsp_endpoints (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 dsp_id uuid NOT NULL, profile_version text NOT NULL CHECK(length(btrim(profile_version))>0),
 adapter_version text NOT NULL CHECK(length(btrim(adapter_version))>0),
 integration_status text NOT NULL DEFAULT 'INTEGRATION_PENDING' CHECK(integration_status='INTEGRATION_PENDING'),
 UNIQUE(org_id,dsp_id,id)
);
CREATE TABLE distribution.route_plans (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, dsp_id uuid NOT NULL,
 route_kind text NOT NULL CHECK(route_kind IN ('DIRECT','MERLIN','LIMBO')),
 contract_id uuid NOT NULL, contract_revision_id uuid NOT NULL, endpoint_id uuid NOT NULL,
 fee_schedule_id uuid NOT NULL, enabled boolean NOT NULL DEFAULT false CHECK(enabled=false),
 FOREIGN KEY(org_id,contract_id,contract_revision_id) REFERENCES rights.contract_revisions(org_id,contract_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,dsp_id,endpoint_id) REFERENCES distribution.dsp_endpoints(org_id,dsp_id,id) ON DELETE RESTRICT,
 UNIQUE(org_id,id,dsp_id)
);
CREATE TABLE distribution.packages (
 id uuid PRIMARY KEY, org_id uuid NOT NULL, snapshot_id uuid NOT NULL,
 route_id uuid NOT NULL, dsp_id uuid NOT NULL,
 operation text NOT NULL CHECK(operation IN ('NEW_RELEASE','UPDATE','TAKEDOWN','MIGRATION')),
 adapter_version text NOT NULL CHECK(length(btrim(adapter_version))>0),
 profile_version text NOT NULL CHECK(length(btrim(profile_version))>0),
 package_hash text NOT NULL CHECK(package_hash ~ '^[a-f0-9]{64}$'),
 manifest_hash text NOT NULL CHECK(manifest_hash ~ '^[a-f0-9]{64}$'),
 immutable_bytes_ref uuid NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(org_id,snapshot_id) REFERENCES distribution.release_snapshots(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,route_id,dsp_id) REFERENCES distribution.route_plans(org_id,id,dsp_id) ON DELETE RESTRICT,
 UNIQUE(snapshot_id,dsp_id,adapter_version,profile_version,package_hash), UNIQUE(org_id,id)
);
-- Route and endpoint configuration are versioned records too, so a package cannot silently change meaning.
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['rights.contract_revisions','distribution.dsp_endpoints','distribution.route_plans','distribution.packages'] LOOP
  EXECUTE format('CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON %s FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation()',t);
 END LOOP;
END $$;
CREATE INDEX packages_org_snapshot ON distribution.packages(org_id,snapshot_id);
-- Deliberately no runtime grants: API and worker cannot approve, create or execute these artifacts.
CREATE FUNCTION distribution.guard_package_profile() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NOT EXISTS (
  SELECT 1 FROM distribution.route_plans r JOIN distribution.dsp_endpoints e
   ON e.org_id=r.org_id AND e.dsp_id=r.dsp_id AND e.id=r.endpoint_id
  WHERE r.org_id=NEW.org_id AND r.id=NEW.route_id AND r.dsp_id=NEW.dsp_id
   AND e.adapter_version=NEW.adapter_version AND e.profile_version=NEW.profile_version
 ) THEN RAISE EXCEPTION 'package route profile mismatch' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER package_profile BEFORE INSERT ON distribution.packages FOR EACH ROW EXECUTE FUNCTION distribution.guard_package_profile();
