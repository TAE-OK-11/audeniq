-- Independent reference recordings, including releases from other distributors.
-- Only the schema-owner operator imports/deactivates them. Runtime roles read
-- fingerprints and public attribution; they cannot supply their own references.
CREATE TABLE catalog.external_recordings (
 id uuid PRIMARY KEY,
 title text NOT NULL CHECK(length(title) BETWEEN 1 AND 300),
 artist text NOT NULL CHECK(length(artist) BETWEEN 1 AND 200),
 isrc text CHECK(isrc ~ '^[A-Z]{2}[A-Z0-9]{3}[0-9]{7}$'),
 source_url text NOT NULL CHECK(length(source_url) BETWEEN 1 AND 2000),
 permission_basis text NOT NULL CHECK(length(permission_basis) BETWEEN 1 AND 2000),
 source_sha256 text NOT NULL CHECK(source_sha256 ~ '^[a-f0-9]{64}$'),
 version smallint NOT NULL CHECK(version=2),
 frames integer NOT NULL CHECK(frames BETWEEN 128 AND 2000),
 hash bytea NOT NULL CHECK(octet_length(hash)=frames*4),
 duration_secs double precision NOT NULL CHECK(duration_secs>0),
 active boolean NOT NULL DEFAULT true,
 imported_by text NOT NULL CHECK(length(imported_by) BETWEEN 1 AND 200),
 imported_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(source_sha256,version)
);
CREATE INDEX external_recordings_scan ON catalog.external_recordings(version,id) WHERE active;
CREATE INDEX external_recordings_isrc ON catalog.external_recordings(isrc) WHERE active AND isrc IS NOT NULL;
CREATE TABLE catalog.external_recording_epoch (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 epoch bigint NOT NULL DEFAULT 0 CHECK(epoch>=0)
);
INSERT INTO catalog.external_recording_epoch(singleton) VALUES(true);
CREATE FUNCTION catalog.advance_external_recording_epoch() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM pg_catalog.pg_advisory_xact_lock(64481168068);
 UPDATE catalog.external_recording_epoch SET epoch=epoch+1 WHERE singleton;
 RETURN NEW;
END $$;
CREATE TRIGGER external_recording_epoch AFTER INSERT OR UPDATE ON catalog.external_recordings
 FOR EACH STATEMENT EXECUTE FUNCTION catalog.advance_external_recording_epoch();
-- Attribution and fingerprint evidence stay immutable; only deactivate/reactivate.
CREATE FUNCTION catalog.guard_external_recording() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' THEN
  RAISE EXCEPTION 'external recording evidence is immutable' USING ERRCODE='23514';
 END IF;
 IF (to_jsonb(OLD)-'active') IS DISTINCT FROM (to_jsonb(NEW)-'active') THEN
  RAISE EXCEPTION 'external recording evidence is immutable' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER external_recording_evidence BEFORE UPDATE OR DELETE ON catalog.external_recordings
 FOR EACH ROW EXECUTE FUNCTION catalog.guard_external_recording();
