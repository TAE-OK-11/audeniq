-- Originals are never the registered download/processing object. This
-- evidence binds the inspected source and sanitized derivative by SHA-256.
CREATE TABLE catalog.asset_safety (
 asset_id uuid PRIMARY KEY,
 org_id uuid NOT NULL,
 source_key text NOT NULL CHECK(source_key LIKE 'quarantine/%'),
 source_sha256 text NOT NULL CHECK(source_sha256 ~ '^[a-f0-9]{64}$'),
 safe_key text NOT NULL CHECK(safe_key LIKE 'registered/%'),
 safe_sha256 text NOT NULL CHECK(safe_sha256 ~ '^[a-f0-9]{64}$'),
 rule_version text NOT NULL CHECK(rule_version='1'),
 inspected_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 FOREIGN KEY(org_id,asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON catalog.asset_safety
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- Runtime roles cannot register or rebind an asset without matching evidence.
-- Owners may seed test fixtures / migrate historical records explicitly.
CREATE FUNCTION catalog.require_asset_safety() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF current_user IN ('audeniq_api','audeniq_worker') AND NEW.state='REGISTERED'
    AND NOT EXISTS(SELECT 1 FROM catalog.asset_safety s WHERE s.asset_id=NEW.id
      AND s.org_id=NEW.org_id AND s.safe_key=NEW.object_key
      AND s.safe_sha256=NEW.sha256 AND s.rule_version='1') THEN
   RAISE EXCEPTION 'upload inspection required' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER safety_gate BEFORE INSERT OR UPDATE OF state,object_key,sha256
 ON catalog.assets FOR EACH ROW EXECUTE FUNCTION catalog.require_asset_safety();
