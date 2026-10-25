-- Length and frame size of an uploaded music video, read by the server from
-- the file's MP4/MOV header when the upload completes (crates/core/src/mp4.rs),
-- so the artist does not type them. NULL: the file does not say.
ALTER TABLE portal.addon_files
  ADD COLUMN duration_ms bigint NULL CHECK (duration_ms > 0),
  ADD COLUMN width integer NULL CHECK (width > 0),
  ADD COLUMN height integer NULL CHECK (height > 0);
