-- 0023: content controls — lyrics + parental advisory on tracks.
ALTER TABLE catalog.tracks
    ADD COLUMN lyrics TEXT,
    ADD COLUMN parental_advisory BOOLEAN NOT NULL DEFAULT FALSE;
