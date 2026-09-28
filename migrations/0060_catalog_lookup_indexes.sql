-- 0060: indexes for the cross-org catalog lookups.
--
-- Stage 2 (2-C.1/2-C.2) and Stage 1's UPC check look releases, tracks and
-- assets up by identifier or content hash across every org. Without these
-- each lookup was a sequential scan of the whole catalog, growing with every
-- release ever registered. Partial: most drafts have no identifier yet.
CREATE INDEX tracks_isrc ON catalog.tracks(isrc) WHERE isrc IS NOT NULL;
CREATE INDEX releases_upc ON catalog.releases(upc) WHERE upc IS NOT NULL;
CREATE INDEX assets_sha256 ON catalog.assets(sha256) WHERE sha256 IS NOT NULL;
-- assets -> tracks join of the duplicate-audio check (2-C.2).
CREATE INDEX tracks_asset ON catalog.tracks(org_id, asset_id) WHERE asset_id IS NOT NULL;
