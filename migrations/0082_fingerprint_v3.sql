-- 0082: fingerprint algorithm v3 (AUDENIQ engine resampler).
--
-- v3 keeps v2's 3 x 30 s head/middle/tail coverage and sub-fingerprint
-- layout, but its 11025 Hz mono PCM comes from the audeniq-qc engine
-- (resampler native-sinc64-v1) instead of FFmpeg. The two resamplers are not
-- interchangeable (BER up to ~0.4 on tonal material), so rows of different
-- versions are never compared: every reader filters by FINGERPRINT_VERSION.
--
-- An asset may now hold one row per version, so the v3 row is written next
-- to the existing v2 row (kept for rollback) instead of being skipped by the
-- old one-row-per-asset key. Assets fingerprinted before v3 are compared
-- again once `audeniq-admin fingerprint backfill` has queued their v3 rows.
ALTER TABLE catalog.asset_fingerprints
    DROP CONSTRAINT asset_fingerprints_version_check;
ALTER TABLE catalog.asset_fingerprints
    ADD CONSTRAINT asset_fingerprints_version_check CHECK (version IN (1, 2, 3));
ALTER TABLE catalog.asset_fingerprints
    DROP CONSTRAINT asset_fingerprints_pkey;
ALTER TABLE catalog.asset_fingerprints
    ADD CONSTRAINT asset_fingerprints_pkey PRIMARY KEY (asset_id, version);
COMMENT ON COLUMN catalog.asset_fingerprints.version IS
    'Fingerprint algorithm version: 1 = first-600s contiguous coverage, 2 = 3x30s head/middle/tail segments (FFmpeg resampler), 3 = same segments from the AUDENIQ engine resampler. Never mix versions in a comparison.';

-- External references are re-imported from their source files at v3; the
-- original files are not retained, so v2 references stay until replaced.
ALTER TABLE catalog.external_recordings
    DROP CONSTRAINT external_recordings_version_check;
ALTER TABLE catalog.external_recordings
    ADD CONSTRAINT external_recordings_version_check CHECK (version IN (2, 3));
