-- 0065 is reserved by the job-policy PR. Original metadata must survive
-- lossless normalization, which intentionally strips container metadata.
CREATE TABLE catalog.asset_provenance (
 asset_id uuid PRIMARY KEY,
 org_id uuid NOT NULL,
 source_sha256 text NOT NULL CHECK (source_sha256 ~ '^[a-f0-9]{64}$'),
 master_sha256 text NOT NULL CHECK (master_sha256 ~ '^[a-f0-9]{64}$'),
 rule_version text NOT NULL,
 body jsonb NOT NULL CHECK (jsonb_typeof(body)='object'),
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (org_id,asset_id) REFERENCES catalog.assets(org_id,id) ON DELETE RESTRICT
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON catalog.asset_provenance
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- A narrow allowlist; unknown warnings and informational checks need staff.
CREATE FUNCTION distribution.automatic_checks_clear(checks jsonb)
RETURNS boolean LANGUAGE sql IMMUTABLE STRICT SET search_path=pg_catalog AS $$
 SELECT jsonb_typeof(checks)='array' AND NOT EXISTS (
   SELECT 1 FROM jsonb_array_elements(checks) c
    WHERE NOT COALESCE(c->>'severity'='INFO'
          AND c->>'class'='CONTENT'
          AND c->>'code'='DSP_AUDIO_SERVED_DOWNSAMPLED',false))
$$;
ALTER TABLE distribution.delivery_staging ADD COLUMN approval_rule_version text;
DO $$ DECLARE name text; BEGIN
 SELECT conname INTO STRICT name FROM pg_constraint
  WHERE conrelid='distribution.delivery_staging'::regclass AND contype='c'
    AND pg_get_constraintdef(oid) LIKE '%approval_by IS NOT NULL%';
 EXECUTE format('ALTER TABLE distribution.delivery_staging DROP CONSTRAINT %I',name);
END $$;
ALTER TABLE distribution.delivery_staging ADD CONSTRAINT delivery_approval_provenance CHECK (
 (approval_rule_version IS NULL OR approval_rule_version='1') AND
 (approval_rule_version IS NULL OR approval='APPROVED') AND
 (approval <> 'APPROVED' OR
   (readiness <> 'CONTENT_BLOCKED' AND approval_by IS NOT NULL AND approval_rule_version IS NULL) OR
   COALESCE((approval_by IS NULL AND approval_rule_version='1' AND approval_at IS NOT NULL
    AND readiness='READY' AND route_status='ROUTABLE' AND NOT ern_is_preview
    AND distribution.automatic_checks_clear(checks)),false))
);
