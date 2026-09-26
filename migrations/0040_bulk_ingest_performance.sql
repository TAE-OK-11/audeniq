-- 0040: indexes and lookups for bulk ingestion (hundreds to thousands of
-- tracks per label per day).
--
-- Every statement below replaces a sequential scan on a hot path whose cost
-- grew with the size of the whole catalog, not the size of the request:
--
-- * Stage 1 asset QC cache: `check_results WHERE check_code AND rule_version
--   AND result_hash` ran once per check per asset (about 20 per track). The
--   result hash already encodes code + rule version + input, so an index on
--   it alone is selective.
-- * Stage 1/3 idempotent check writes: `WHERE revision_id AND check_code AND
--   result_hash`, and submission status `WHERE revision_id`.
-- * Stage 2 catalog match: cross-org ISRC, UPC and asset SHA-256 lookups.
-- * Stage 2 duplicate application check: `WHERE org_id AND body_hash`.
-- * track -> asset joins (Stage 2 SHA match, preflight).
CREATE INDEX IF NOT EXISTS check_results_result_hash
  ON operations.check_results(result_hash, created_at DESC);
CREATE INDEX IF NOT EXISTS check_results_revision
  ON operations.check_results(revision_id, check_code, result_hash);
CREATE INDEX IF NOT EXISTS tracks_isrc
  ON catalog.tracks(isrc) WHERE isrc IS NOT NULL;
CREATE INDEX IF NOT EXISTS tracks_org_asset
  ON catalog.tracks(org_id, asset_id) WHERE asset_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS releases_upc
  ON catalog.releases(upc) WHERE upc IS NOT NULL;
CREATE INDEX IF NOT EXISTS assets_sha256
  ON catalog.assets(sha256) WHERE sha256 IS NOT NULL;
CREATE INDEX IF NOT EXISTS application_revisions_body_hash
  ON catalog.application_revisions(org_id, body_hash);

-- Rate-limit buckets older than their 15-minute window are dead weight; the
-- API housekeeping task deletes them by window_start.
CREATE INDEX IF NOT EXISTS auth_limits_window
  ON identity.auth_limits(window_start);

-- Fingerprint similarity used to load and compare every fingerprint in the
-- platform for every new track (O(catalog) per track, O(n^2) for a bulk
-- ingest). A v2 fingerprint covers head/middle/tail windows whose positions
-- depend on the track duration, so two long recordings only align when
-- their durations agree to well under a second. Recording the measured
-- track duration lets Stage 1 compare against the few candidates that can
-- possibly match. Rows without a duration (written before this migration
-- and not backfilled) are always compared, so recall never drops.
ALTER TABLE catalog.asset_fingerprints
  ADD COLUMN IF NOT EXISTS track_duration_secs double precision
  CHECK (track_duration_secs IS NULL OR track_duration_secs > 0);
UPDATE catalog.asset_fingerprints f
   SET track_duration_secs = a.duration_secs
  FROM catalog.assets a
 WHERE a.id = f.asset_id AND f.track_duration_secs IS NULL AND a.duration_secs IS NOT NULL;
CREATE INDEX IF NOT EXISTS asset_fingerprints_org_duration
  ON catalog.asset_fingerprints(org_id, version, track_duration_secs);
CREATE INDEX IF NOT EXISTS asset_fingerprints_duration
  ON catalog.asset_fingerprints(version, track_duration_secs);

-- Same narrow cross-org read as catalog.fingerprints_outside_org, limited to
-- candidates that can match: duration within [p_lo, p_hi], short tracks
-- (<= p_short, a snippet can match the head of a longer track) and rows
-- whose duration is unknown.
CREATE OR REPLACE FUNCTION catalog.fingerprints_outside_org_near(
  p_org uuid, p_version smallint, p_lo double precision, p_hi double precision,
  p_short double precision)
RETURNS TABLE(asset_id uuid, org_id uuid, hash bytea)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT f.asset_id, f.org_id, f.hash
    FROM catalog.asset_fingerprints f
   WHERE f.org_id <> p_org AND f.version = p_version
     AND (f.track_duration_secs IS NULL
          OR f.track_duration_secs BETWEEN p_lo AND p_hi
          OR f.track_duration_secs <= p_short)
$$;
REVOKE ALL ON FUNCTION catalog.fingerprints_outside_org_near(uuid, smallint, double precision, double precision, double precision) FROM PUBLIC;

-- Studio balance/statement reads sum the org's ROYALTY_PAYABLE entries on
-- every page load; with per-line royalty postings that is the whole ledger.
-- Covering index keeps it an index-only scan.
CREATE INDEX IF NOT EXISTS ledger_entries_org_account
  ON finance.ledger_entries(org_id, account, currency) INCLUDE (side, amount, transaction_id);
