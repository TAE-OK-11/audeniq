-- 0061: page the Stage 1 similarity scan.
--
-- Every new audio asset is compared against every stored fingerprint at
-- the current version, in its own org and (through the narrow SECURITY
-- DEFINER read of 0034) in all other orgs. Reading them in one query held
-- the whole platform's fingerprints in worker memory at once, inside one
-- transaction, and grows toward the 15 s statement_timeout with the
-- catalog. The worker now reads keyset pages ordered by asset_id.
--
-- Own org: (org_id, version, asset_id) serves the org-scoped page directly.
CREATE INDEX asset_fingerprints_org_page
    ON catalog.asset_fingerprints(org_id, version, asset_id);

-- Other orgs: same narrow read as fingerprints_outside_org (asset_id,
-- org_id, hash outside the caller's org at one version), one page after
-- p_after. EXECUTE is granted to the worker only (deploy/grants.sql).
CREATE OR REPLACE FUNCTION catalog.fingerprints_outside_org_page(
    p_org uuid, p_version smallint, p_after uuid, p_limit integer)
RETURNS TABLE(asset_id uuid, org_id uuid, hash bytea)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT f.asset_id, f.org_id, f.hash
    FROM catalog.asset_fingerprints f
   WHERE f.org_id <> p_org AND f.version = p_version AND f.asset_id > p_after
   ORDER BY f.asset_id
   LIMIT least(greatest(p_limit, 1), 5000)
$$;
REVOKE ALL ON FUNCTION catalog.fingerprints_outside_org_page(uuid, smallint, uuid, integer) FROM PUBLIC;
