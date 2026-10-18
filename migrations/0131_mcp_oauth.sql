-- AUDENIQ MCP: OAuth 2.1 authorization server for AI assistants (ChatGPT,
-- Claude, other MCP clients). See docs/MCP.md.
--
-- An AI client acts for one user in one workspace (org) with the scopes the
-- user approved on the consent screen. Tokens are opaque random values; only
-- their SHA-256 digests are stored (like browser sessions). A token never
-- satisfies MFA, never reaches payouts/contracts, and is revoked with the
-- user's other sessions (password change, log out everywhere).

-- Registered clients: Client ID Metadata Documents (the client_id is the
-- HTTPS document URL, re-fetched when stale) or Dynamic Client Registration
-- (RFC 7591, a random client_id; deprecated by MCP but still used).
CREATE TABLE identity.mcp_clients (
 id uuid PRIMARY KEY,
 client_id text NOT NULL UNIQUE CHECK(length(client_id) BETWEEN 8 AND 512),
 kind text NOT NULL CHECK(kind IN ('CIMD','DCR')),
 client_name text NOT NULL CHECK(length(btrim(client_name)) BETWEEN 1 AND 200),
 client_uri text NULL CHECK(client_uri IS NULL OR length(client_uri) <= 512),
 redirect_uris text[] NOT NULL CHECK(cardinality(redirect_uris) BETWEEN 1 AND 10),
 token_endpoint_auth_method text NOT NULL
  CHECK(token_endpoint_auth_method IN ('none','client_secret_post','client_secret_basic')),
 secret_hash bytea NULL CHECK(secret_hash IS NULL OR octet_length(secret_hash)=32),
 CHECK((token_endpoint_auth_method='none') = (secret_hash IS NULL)),
 fetched_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 disabled_at timestamptz NULL
);

-- What a user approved for a client: one workspace, a scope set, the MCP
-- resource. Revoking a grant kills all of its tokens.
CREATE TABLE identity.mcp_grants (
 id uuid PRIMARY KEY,
 client_pk uuid NOT NULL REFERENCES identity.mcp_clients ON DELETE RESTRICT,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 scopes text[] NOT NULL CHECK(cardinality(scopes) BETWEEN 1 AND 8),
 resource text NOT NULL CHECK(length(resource) BETWEEN 1 AND 512),
 created_at timestamptz NOT NULL DEFAULT now(),
 last_used_at timestamptz NULL,
 revoked_at timestamptz NULL,
 revoke_reason text NULL CHECK(revoke_reason IS NULL OR length(revoke_reason) <= 60)
);
CREATE INDEX mcp_grants_user ON identity.mcp_grants(user_id) WHERE revoked_at IS NULL;

-- One-time authorization codes (10 minutes, PKCE S256 only).
CREATE TABLE identity.mcp_codes (
 code_hash bytea PRIMARY KEY CHECK(octet_length(code_hash)=32),
 grant_id uuid NOT NULL REFERENCES identity.mcp_grants ON DELETE CASCADE,
 redirect_uri text NOT NULL CHECK(length(redirect_uri) <= 512),
 code_challenge text NOT NULL CHECK(length(code_challenge) = 43),
 expires_at timestamptz NOT NULL,
 used_at timestamptz NULL
);

-- Access tokens (1 hour) and rotating refresh tokens (30 days). A refresh
-- token used twice means it leaked: the whole grant is revoked.
CREATE TABLE identity.mcp_tokens (
 token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
 grant_id uuid NOT NULL REFERENCES identity.mcp_grants ON DELETE CASCADE,
 kind text NOT NULL CHECK(kind IN ('ACCESS','REFRESH')),
 expires_at timestamptz NOT NULL,
 used_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX mcp_tokens_grant ON identity.mcp_tokens(grant_id);
