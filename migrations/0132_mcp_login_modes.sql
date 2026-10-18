-- AUDENIQ MCP: more ways for an AI client to connect. See docs/MCP.md.
--
-- 1. Device authorization (RFC 8628): the assistant shows a link and a code,
--    the artist signs in to the Studio in a browser and approves there.
-- 2. Password grant for trusted agents only: clients an operator registered
--    (audeniq-admin mcp-client) whose credential form sends the e-mail and
--    password straight to AUDENIQ without the model seeing them. They
--    authenticate with a client secret.
-- 3. Connection tokens: a fixed bearer token the artist makes in the Studio
--    for every other agent (shown once, at most 90 days, revocable).

-- ADMIN: registered by an operator (confidential, client secret).
-- TOKEN: the built-in client that owns Studio-made connection tokens.
ALTER TABLE identity.mcp_clients DROP CONSTRAINT mcp_clients_kind_check;
ALTER TABLE identity.mcp_clients ADD CONSTRAINT mcp_clients_kind_check
 CHECK(kind IN ('CIMD','DCR','ADMIN','TOKEN'));
ALTER TABLE identity.mcp_clients ADD COLUMN password_grant boolean NOT NULL DEFAULT false;
ALTER TABLE identity.mcp_clients ADD CONSTRAINT mcp_clients_password_grant_check
 CHECK(NOT password_grant OR (kind='ADMIN' AND secret_hash IS NOT NULL));

-- How the artist approved the grant, and the name they gave a connection
-- token (shown in the Studio's connected AI apps list).
ALTER TABLE identity.mcp_grants ADD COLUMN method text NOT NULL DEFAULT 'CONSENT'
 CHECK(method IN ('CONSENT','DEVICE','PASSWORD','TOKEN'));
ALTER TABLE identity.mcp_grants ADD COLUMN label text NULL
 CHECK(label IS NULL OR length(btrim(label)) BETWEEN 1 AND 60);

-- Pending device authorizations (10 minutes). The device code goes to the
-- client, the short user code to the artist; both are stored as digests.
CREATE TABLE identity.mcp_device_codes (
 device_code_hash bytea PRIMARY KEY CHECK(octet_length(device_code_hash)=32),
 user_code_hash bytea NOT NULL UNIQUE CHECK(octet_length(user_code_hash)=32),
 client_pk uuid NOT NULL REFERENCES identity.mcp_clients ON DELETE CASCADE,
 scopes text[] NOT NULL CHECK(cardinality(scopes) BETWEEN 1 AND 8),
 expires_at timestamptz NOT NULL,
 polled_at timestamptz NULL,
 grant_id uuid NULL REFERENCES identity.mcp_grants ON DELETE CASCADE,
 denied_at timestamptz NULL,
 used_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK(grant_id IS NULL OR denied_at IS NULL)
);
CREATE INDEX mcp_device_codes_expiry ON identity.mcp_device_codes(expires_at);

INSERT INTO identity.mcp_clients(id,client_id,kind,client_name,redirect_uris,token_endpoint_auth_method)
VALUES('00000000-0000-4000-8000-00000000c0de','audeniq-connection-token','TOKEN','연결 토큰',ARRAY['https://invalid.audeniq.local/none'],'none');
