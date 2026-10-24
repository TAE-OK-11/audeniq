-- 0153: artist pages get a handle the artist chooses (crates/core/src/artist_links.rs).
--
-- Public pages move to /link/artist/{handle} (and /link/release/{slug} for
-- smart links). A handle is 3–30 characters: lowercase letters, digits, '_'
-- and '-', starting and ending with a letter or digit. The random 8-character
-- slugs made before keep working (they fit the same rule). Handles are unique
-- across every org: one taken by someone else cannot be chosen.
ALTER TABLE portal.artist_links DROP CONSTRAINT IF EXISTS artist_links_slug_check;
ALTER TABLE portal.artist_links
  ADD CONSTRAINT artist_links_slug_check CHECK (slug ~ '^[a-z0-9][a-z0-9_-]{1,28}[a-z0-9]$');
