-- 0151: artist pages (crates/core/src/artist_links.rs).
--
-- One public page per artist (/a/{slug}): name, a short introduction, the
-- artist's own pages on each platform (catalog.artist_platform_pages) and
-- every release of the artist that has a visible smart link; each release
-- opens its smart link. Views are counted per day (once per visitor address
-- every 15 minutes) and shown to the org for the last 30 days.
-- The page also carries the artist's own links (social, tickets, shop: up to
-- ten https links with a label) and an optional contact address for the
-- press page (/a/{slug}/press).
--
-- Smart links can show the lyrics the release was submitted with
-- (catalog.tracks.lyrics), when the artist switches that on.
CREATE TABLE portal.artist_links (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 artist_id uuid NOT NULL,
 slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]{8}$'),
 enabled boolean NOT NULL DEFAULT true,
 bio text NOT NULL DEFAULT '' CHECK (length(bio) <= 1500),
 links jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(links) = 'array' AND jsonb_array_length(links) <= 10),
 contact text NOT NULL DEFAULT '' CHECK (length(contact) <= 254),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (org_id, artist_id),
 FOREIGN KEY (org_id, artist_id) REFERENCES catalog.artists(org_id, id) ON DELETE RESTRICT
);

CREATE TABLE portal.artist_link_stats (
 link_id uuid NOT NULL REFERENCES portal.artist_links ON DELETE CASCADE,
 day date NOT NULL,
 count integer NOT NULL DEFAULT 0 CHECK (count >= 0),
 PRIMARY KEY (link_id, day)
);

ALTER TABLE portal.smart_links ADD COLUMN show_lyrics boolean NOT NULL DEFAULT false;
