-- Sandbox round 2: fingerprint similarity was org-scoped, so re-encoded
-- copies of another account's released audio (MP3->WAV, AAC->FLAC, -6 dB,
-- upsampled, 16->24 bit) were all delivered. Stage 1 now also compares
-- against other organizations' fingerprints (REVIEW only, never a block).
--
-- asset_fingerprints stays FORCE RLS for every runtime role. The cross-org
-- read goes through one narrow SECURITY DEFINER function that returns only
-- (asset_id, org_id, hash) outside the caller's org at one algorithm
-- version; EXECUTE is granted to the worker only (deploy/grants.sql). The
-- extra SELECT policy matches only the table owner, i.e. this function (the
-- owner is the migration role, never a runtime login).
DROP POLICY IF EXISTS asset_fingerprints_owner_scan ON catalog.asset_fingerprints;
CREATE POLICY asset_fingerprints_owner_scan ON catalog.asset_fingerprints
  FOR SELECT
  USING (current_user = (SELECT pg_catalog.pg_get_userbyid(c.relowner)
                           FROM pg_catalog.pg_class c
                          WHERE c.oid = 'catalog.asset_fingerprints'::regclass));

CREATE OR REPLACE FUNCTION catalog.fingerprints_outside_org(p_org uuid, p_version smallint)
RETURNS TABLE(asset_id uuid, org_id uuid, hash bytea)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT f.asset_id, f.org_id, f.hash
    FROM catalog.asset_fingerprints f
   WHERE f.org_id <> p_org AND f.version = p_version
$$;
REVOKE ALL ON FUNCTION catalog.fingerprints_outside_org(uuid, smallint) FROM PUBLIC;
