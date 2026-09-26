-- 0038: allow fingerprint algorithm v2 (segmented coverage).
--
-- v1 covered the first 600 s of the track contiguously; v2 covers three
-- 30 s segments (head, middle, tail) tapped out of the single QC decode
-- pass. Both versions share the sub-fingerprint layout and the BER
-- comparison, but v1 and v2 rows must never be compared against each
-- other: the readers filter by version (see FINGERPRINT_VERSION).
ALTER TABLE catalog.asset_fingerprints
    DROP CONSTRAINT asset_fingerprints_version_check;
ALTER TABLE catalog.asset_fingerprints
    ADD CONSTRAINT asset_fingerprints_version_check CHECK (version IN (1, 2));
COMMENT ON COLUMN catalog.asset_fingerprints.version IS
    'Fingerprint algorithm version: 1 = first-600s contiguous coverage, 2 = 3x30s head/middle/tail segments. Never mix versions in a comparison.';
