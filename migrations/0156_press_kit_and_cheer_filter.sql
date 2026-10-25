-- 0156: press kit (crates/core/src/artist_press.rs) and the cheers word
-- filter (crates/core/src/profanity.rs).
--
-- Press kit: what the artist page's press page (/link/artist/{handle}/press)
-- gives curators, journalists and promoters beyond the page itself — a
-- one-line pitch, a short bio to quote, facts (genre, where the artist is
-- based, label, debut year), highlights, quotes from press or curators, and
-- press releases the artist writes (optionally about one release), shown
-- once published.
CREATE TABLE portal.artist_press (
 link_id uuid PRIMARY KEY REFERENCES portal.artist_links ON DELETE CASCADE,
 tagline text NOT NULL DEFAULT '' CHECK (length(tagline) <= 120),
 bio_short text NOT NULL DEFAULT '' CHECK (length(bio_short) <= 400),
 genre text NOT NULL DEFAULT '' CHECK (length(genre) <= 60),
 based_in text NOT NULL DEFAULT '' CHECK (length(based_in) <= 60),
 label text NOT NULL DEFAULT '' CHECK (length(label) <= 80),
 debut text NOT NULL DEFAULT '' CHECK (debut = '' OR debut ~ '^(19|20)[0-9]{2}$'),
 highlights jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(highlights) = 'array' AND jsonb_array_length(highlights) <= 8),
 quotes jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(quotes) = 'array' AND jsonb_array_length(quotes) <= 6),
 updated_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE portal.artist_press_releases (
 id uuid PRIMARY KEY,
 link_id uuid NOT NULL REFERENCES portal.artist_links ON DELETE CASCADE,
 release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 title text NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 120),
 lead text NOT NULL DEFAULT '' CHECK (length(lead) <= 400),
 body text NOT NULL DEFAULT '' CHECK (length(body) <= 8000),
 published boolean NOT NULL DEFAULT false,
 published_at timestamptz NULL,
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK (published = (published_at IS NOT NULL))
);
CREATE INDEX artist_press_releases_link ON portal.artist_press_releases (link_id, created_at DESC);

-- Words the artist does not want in cheers on their page, on top of the
-- profanity filter every page has.
ALTER TABLE portal.artist_links
  ADD COLUMN cheer_blocklist text[] NOT NULL DEFAULT '{}' CHECK (cardinality(cheer_blocklist) <= 50);
