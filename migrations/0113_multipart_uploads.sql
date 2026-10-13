-- Resumable uploads of large masters (crates/core/src/uploads.rs): an S3
-- multipart upload at the session's quarantine key. Parts are PUT through
-- per-part signed URLs; completion lists the stored parts, finishes the
-- upload and then runs the same checks as a single PUT.
ALTER TABLE catalog.upload_sessions
  ADD COLUMN multipart_upload_id text NULL
    CHECK (multipart_upload_id IS NULL OR length(multipart_upload_id) BETWEEN 1 AND 1024),
  ADD COLUMN part_size bigint NULL
    CHECK (part_size IS NULL OR part_size BETWEEN 5242880 AND 104857600),
  ADD CONSTRAINT upload_sessions_multipart_shape
    CHECK ((multipart_upload_id IS NULL) = (part_size IS NULL));
