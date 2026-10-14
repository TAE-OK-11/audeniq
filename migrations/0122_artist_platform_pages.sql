-- 0122: artist platform pages (crates/core/src/artist_pages.rs).
--
-- Each artist's own page on Spotify, Apple Music, Melon, ... is recorded
-- once and reused: from the application (existing page links), from the
-- artist after a first release created a new page, or by staff. Later
-- applications for the same artist are prefilled with it, and deliveries to
-- that platform name the page (DDEX DisplayArtist/PartyId) so the release
-- lands on the right artist, never on a same-name stranger.
-- A first release that creates new pages leaves a PENDING row per platform
-- until the links are registered.
CREATE TABLE catalog.artist_platform_pages (
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 artist_id uuid NOT NULL,
 platform text NOT NULL CHECK (platform IN ('spotify','apple','melon','genie','flo','bugs','vibe','youtube')),
 url text NULL CHECK (url IS NULL OR (url ~ '^https://' AND length(url) <= 500)),
 external_id text NULL CHECK (external_id IS NULL OR length(external_id) <= 200),
 status text NOT NULL CHECK (status IN ('PENDING','LINKED')),
 source text NOT NULL CHECK (source IN ('APPLICATION','ARTIST','STAFF')),
 first_release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (org_id, artist_id, platform),
 FOREIGN KEY (org_id, artist_id) REFERENCES catalog.artists(org_id, id) ON DELETE RESTRICT,
 CHECK ((status = 'LINKED') = (url IS NOT NULL))
);
