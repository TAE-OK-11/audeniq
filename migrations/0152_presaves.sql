-- 0152: pre-saves (crates/core/src/presave.rs).
--
-- Before the release, a fan on the smart link signs in to Spotify, Deezer or
-- TIDAL and allows "add to library". At the release instant the server finds
-- the album on that platform (its reported id, else by UPC) and saves it to
-- the fan's library, once. The platform token is sealed (payout key ring,
-- field PresaveToken) and erased when the album is saved, the pre-save fails,
-- is cancelled (page gone) or expires (album not found within a week).
-- AUDENIQ never learns who the fan is: no account, no email, no profile.
CREATE TABLE portal.presave_starts (
 state_hash bytea PRIMARY KEY,
 link_id uuid NOT NULL REFERENCES portal.smart_links ON DELETE CASCADE,
 provider text NOT NULL CHECK (provider IN ('spotify','deezer','tidal')),
 code_verifier text NOT NULL CHECK (length(code_verifier) BETWEEN 43 AND 128),
 expires_at timestamptz NOT NULL
);

CREATE TABLE portal.presaves (
 id uuid PRIMARY KEY,
 link_id uuid NOT NULL REFERENCES portal.smart_links ON DELETE CASCADE,
 provider text NOT NULL CHECK (provider IN ('spotify','deezer','tidal')),
 token_cipher bytea NULL,
 status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','SAVED','FAILED','CANCELLED','EXPIRED')),
 attempts integer NOT NULL DEFAULT 0,
 next_attempt_at timestamptz NOT NULL DEFAULT now(),
 last_error text NULL CHECK (last_error IS NULL OR last_error ~ '^[A-Z_]{1,60}$'),
 created_at timestamptz NOT NULL DEFAULT now(),
 saved_at timestamptz NULL,
 -- Only a waiting pre-save keeps the token.
 CHECK ((status = 'PENDING') = (token_cipher IS NOT NULL))
);
CREATE INDEX presaves_pending ON portal.presaves(next_attempt_at) WHERE status = 'PENDING';
CREATE INDEX presaves_link ON portal.presaves(link_id, provider);
