-- 0109: incremental Stage 1 similarity scan.
--
-- AUDIO_SIMILAR_TO_EXISTING compares a track with every stored fingerprint
-- (own org and, through a narrow SECURITY DEFINER read, other orgs): about
-- 0.1 ms of CPU per stored fingerprint, so a resubmission of an unchanged
-- track paid the whole catalog again. The worker now remembers, per asset
-- and fingerprint, how far it has compared (`scanned_through`, created_at
-- of the newest fingerprint covered, minus a safety margin for late
-- commits) and the matches it found, and on the next run compares only
-- fingerprints added since. Previous matches are re-checked to still
-- exist; the stored list is capped, and a capped list that lost a member
-- triggers a full rescan, so the answer stays exactly that of a full scan.
--
-- Worker-only (deploy/grants.sql). Holds other orgs' asset ids among the
-- matches, so it is never readable by the API.
CREATE TABLE operations.fingerprint_scans (
 asset_id uuid NOT NULL,
 version smallint NOT NULL,
 fingerprint_sha256 text NOT NULL CHECK (fingerprint_sha256 ~ '^[a-f0-9]{64}$'),
 scanned_through timestamptz NOT NULL,
 hits jsonb NOT NULL CHECK (jsonb_typeof(hits) = 'array'),
 truncated boolean NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (asset_id, version)
);

-- Pages ordered by (created_at, asset_id): an incremental run reads only
-- the fingerprints added after its watermark.
CREATE INDEX asset_fingerprints_org_created
    ON catalog.asset_fingerprints(org_id, version, created_at, asset_id);
CREATE INDEX asset_fingerprints_version_created
    ON catalog.asset_fingerprints(version, created_at, asset_id);

-- Other orgs, created after p_since, one keyset page after
-- (p_after_at, p_after_id). Same narrow columns as
-- fingerprints_outside_org_page (0061); EXECUTE to the worker only.
CREATE OR REPLACE FUNCTION catalog.fingerprints_outside_org_since(
    p_org uuid, p_version smallint, p_since timestamptz,
    p_after_at timestamptz, p_after_id uuid, p_limit integer)
RETURNS TABLE(asset_id uuid, org_id uuid, created_at timestamptz, hash bytea)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT f.asset_id, f.org_id, f.created_at, f.hash
    FROM catalog.asset_fingerprints f
   WHERE f.version = p_version AND f.org_id <> p_org
     AND f.created_at > p_since
     AND (f.created_at, f.asset_id) > (p_after_at, p_after_id)
   ORDER BY f.created_at, f.asset_id
   LIMIT least(greatest(p_limit, 1), 5000)
$$;
REVOKE ALL ON FUNCTION catalog.fingerprints_outside_org_since(uuid, smallint, timestamptz, timestamptz, uuid, integer) FROM PUBLIC;

-- Which of the given assets (matches the worker found before, in any org)
-- still have a fingerprint at this version: ids in, ids out, no content.
CREATE OR REPLACE FUNCTION catalog.fingerprints_still_present(
    p_version smallint, p_ids uuid[])
RETURNS TABLE(asset_id uuid)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT f.asset_id FROM catalog.asset_fingerprints f
   WHERE f.version = p_version AND f.asset_id = ANY(p_ids[1:64])
$$;
REVOKE ALL ON FUNCTION catalog.fingerprints_still_present(smallint, uuid[]) FROM PUBLIC;
