-- 0117: smart links (crates/core/src/smart_links.rs).
--
-- One public landing page per submitted release (/l/{slug}): cover, title,
-- artist and the platform links the artist pasted (hosts checked per
-- platform). Views and clicks are counted per day and platform; the page
-- hides itself when the release is withdrawn, on hold, taken down or
-- suspended over a rights claim.
CREATE TABLE portal.smart_links (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL UNIQUE REFERENCES catalog.releases ON DELETE RESTRICT,
 slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]{8}$'),
 enabled boolean NOT NULL DEFAULT true,
 links jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(links) = 'object'),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX smart_links_org ON portal.smart_links(org_id);

-- platform '_view' counts page views.
CREATE TABLE portal.smart_link_stats (
 link_id uuid NOT NULL REFERENCES portal.smart_links ON DELETE CASCADE,
 day date NOT NULL,
 platform text NOT NULL CHECK (platform ~ '^[a-z_]{1,20}$'),
 count integer NOT NULL DEFAULT 0 CHECK (count >= 0),
 PRIMARY KEY (link_id, day, platform)
);
