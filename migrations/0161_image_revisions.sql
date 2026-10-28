-- Retain only images actually published through a public image alias.
-- A scope is the artist-link or smart-link UUID, never a client-supplied org.
-- The alias remains uncached; its revision URL survives later photo/cover edits.
CREATE TABLE portal.image_revisions (
 kind text NOT NULL CHECK (kind IN ('artist', 'cover')),
 scope_id uuid NOT NULL,
 org_id uuid NOT NULL,
 asset_id uuid NOT NULL,
 sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
 published_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (kind, scope_id, sha256),
 FOREIGN KEY (org_id, asset_id) REFERENCES catalog.assets(org_id, id) ON DELETE RESTRICT
);

-- Code-ready derivative registry, intentionally empty until a trusted sandboxed
-- encoder is available. Never overwrite the sanitized source or its safety row.
-- Encode and verify before publishing; object keys and records are write-once.
CREATE TABLE catalog.image_variants (
 org_id uuid NOT NULL,
 asset_id uuid NOT NULL,
 source_sha256 text NOT NULL CHECK (source_sha256 ~ '^[0-9a-f]{64}$'),
 content_type text NOT NULL CHECK (content_type IN ('image/avif', 'image/webp')),
 object_key text NOT NULL UNIQUE,
 sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
 size_bytes bigint NOT NULL CHECK (size_bytes > 0 AND size_bytes <= 20971520),
 PRIMARY KEY (org_id, asset_id, source_sha256, content_type),
 FOREIGN KEY (org_id, asset_id) REFERENCES catalog.assets(org_id, id) ON DELETE RESTRICT
);

CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON portal.image_revisions
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON catalog.image_variants
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
