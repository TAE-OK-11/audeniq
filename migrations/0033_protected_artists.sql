-- Sandbox round 2: artist impersonation is a hard block. A managed list of
-- protected artist names (with aliases and signature phrases) is refused in
-- names, titles, profiles and credits unless the organization holds an
-- explicit, unrevoked exception (e.g. the artist's verified label).
-- Management is an operator task (SQL / seed); see docs/PROTECTED_ARTISTS.md.
CREATE TABLE catalog.protected_artists (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 name text NOT NULL UNIQUE CHECK(length(btrim(name)) BETWEEN 2 AND 200),
 note text,
 active boolean NOT NULL DEFAULT true,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE catalog.protected_artist_aliases (
 protected_artist_id uuid NOT NULL REFERENCES catalog.protected_artists ON DELETE CASCADE,
 alias text NOT NULL CHECK(length(btrim(alias)) BETWEEN 2 AND 200),
 kind text NOT NULL DEFAULT 'NAME' CHECK(kind IN ('NAME','PHRASE')),
 PRIMARY KEY(protected_artist_id, alias)
);
CREATE TABLE catalog.protected_artist_exceptions (
 protected_artist_id uuid NOT NULL REFERENCES catalog.protected_artists ON DELETE CASCADE,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE CASCADE,
 reason text NOT NULL CHECK(length(btrim(reason)) > 0),
 granted_by text NOT NULL CHECK(length(btrim(granted_by)) > 0),
 granted_at timestamptz NOT NULL DEFAULT now(),
 revoked_at timestamptz,
 PRIMARY KEY(protected_artist_id, org_id)
);
WITH p AS (
 INSERT INTO catalog.protected_artists(name, note)
 VALUES ('Taylor Swift', 'Seed entry: impersonation reached MockDSP in the 2026-09 sandbox test')
 RETURNING id
)
INSERT INTO catalog.protected_artist_aliases(protected_artist_id, alias, kind)
SELECT p.id, v.alias, v.kind FROM p, (VALUES
 ('Taylor Alison Swift','NAME'),
 ('Taylor''s Version','PHRASE')
) AS v(alias, kind);
