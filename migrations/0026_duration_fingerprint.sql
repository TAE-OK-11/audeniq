-- 0026: measured audio duration on assets + audio fingerprint store.
--
-- catalog.assets.duration_secs: ffprobe-measured audio duration, persisted
-- by Stage 1 (submission.analyze_asset). The DDEX ERN 3.8.2 schema requires
-- SoundRecording/Duration, so the ERN builder fails closed
-- (DDEX_DURATION_UNKNOWN) when it is missing. Only filled when unknown;
-- the submit path never overwrites a known value.
ALTER TABLE catalog.assets
    ADD COLUMN duration_secs DOUBLE PRECISION
    CHECK (duration_secs IS NULL OR duration_secs > 0);

-- catalog.asset_fingerprints: perceptual audio fingerprints (v1 = Philips
-- robust-hash style sub-fingerprints) for similarity detection. SHA-256
-- only proves byte identity; the fingerprint catches the same recording
-- re-encoded, trimmed, or re-mastered. One row per audio asset; computed
-- at Stage 1 submit time from the analyzer temp file.
CREATE TABLE catalog.asset_fingerprints (
    asset_id uuid PRIMARY KEY REFERENCES catalog.assets(id) ON DELETE CASCADE,
    org_id uuid NOT NULL,
    version smallint NOT NULL DEFAULT 1 CHECK (version = 1),
    frames integer NOT NULL CHECK (frames > 0),
    duration_secs double precision NOT NULL CHECK (duration_secs > 0),
    -- 4 bytes big-endian u32 per frame, in frame order.
    hash bytea NOT NULL CHECK (octet_length(hash) = frames * 4),
    created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE catalog.asset_fingerprints ENABLE ROW LEVEL SECURITY;
ALTER TABLE catalog.asset_fingerprints FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS asset_fingerprints_org_scope ON catalog.asset_fingerprints;
CREATE POLICY asset_fingerprints_org_scope ON catalog.asset_fingerprints
  USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid)
  WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid);
