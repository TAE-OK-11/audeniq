-- 0154: artist page news, fan cheers (crates/core/src/artist_community.rs).
--
-- The artist posts notices and events on their page (/link/artist/{handle}):
-- a title, text, for an event its date and place, an optional https link
-- (tickets) and a pin to keep it on top. Up to 100 posts per page.
--
-- Fans leave cheers on the page without an account: a nickname and a short
-- message. The artist can hide or delete each one, mark it with a heart and
-- switch cheers off for the page (existing cheers are then not shown).
CREATE TABLE portal.artist_posts (
 id uuid PRIMARY KEY,
 link_id uuid NOT NULL REFERENCES portal.artist_links ON DELETE CASCADE,
 kind text NOT NULL CHECK (kind IN ('NOTICE', 'EVENT')),
 title text NOT NULL CHECK (length(title) BETWEEN 1 AND 80),
 body text NOT NULL DEFAULT '' CHECK (length(body) <= 2000),
 starts_at timestamptz,
 place text NOT NULL DEFAULT '' CHECK (length(place) <= 80),
 url text NOT NULL DEFAULT '' CHECK (length(url) <= 500),
 pinned boolean NOT NULL DEFAULT false,
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK (kind <> 'EVENT' OR starts_at IS NOT NULL)
);
CREATE INDEX artist_posts_link ON portal.artist_posts (link_id, created_at DESC);

ALTER TABLE portal.artist_links ADD COLUMN cheers_enabled boolean NOT NULL DEFAULT true;

CREATE TABLE portal.artist_cheers (
 id uuid PRIMARY KEY,
 link_id uuid NOT NULL REFERENCES portal.artist_links ON DELETE CASCADE,
 nickname text NOT NULL CHECK (length(nickname) BETWEEN 1 AND 20),
 message text NOT NULL CHECK (length(message) BETWEEN 1 AND 300),
 hidden boolean NOT NULL DEFAULT false,
 heart boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX artist_cheers_link ON portal.artist_cheers (link_id, created_at DESC, id DESC);
