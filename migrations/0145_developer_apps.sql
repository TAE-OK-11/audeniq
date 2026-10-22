-- Developer apps (crate::mcp::developer) and the REST API resource (crate::mcp::rest).
--
-- An AUDENIQ user registers an OAuth client in the Studio (kind DEV, owned by
-- them). It starts PRIVATE (only the owner can connect it), REVIEW after the
-- owner asks to open it, PUBLIC once an operator approves. Other kinds keep
-- PUBLIC (their own rules decide who connects).
--
-- Grants already record their resource (0131): {origin}/api/mcp for MCP
-- tokens, {origin}/api/v1 for API tokens; a token works only on its own.
ALTER TABLE identity.mcp_clients DROP CONSTRAINT mcp_clients_kind_check;
ALTER TABLE identity.mcp_clients ADD CONSTRAINT mcp_clients_kind_check
 CHECK(kind IN ('CIMD','DCR','ADMIN','TOKEN','DEV'));

ALTER TABLE identity.mcp_clients
 ADD COLUMN owner_user_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 ADD COLUMN visibility text NOT NULL DEFAULT 'PUBLIC'
  CHECK(visibility IN ('PRIVATE','REVIEW','PUBLIC')),
 ADD COLUMN description text NULL CHECK(description IS NULL OR length(description) <= 2000),
 ADD COLUMN review_requested_at timestamptz NULL,
 ADD COLUMN reviewed_at timestamptz NULL,
 ADD COLUMN review_note text NULL CHECK(review_note IS NULL OR length(review_note) <= 2000);

-- A developer app always has an owner; nothing else has one.
ALTER TABLE identity.mcp_clients ADD CONSTRAINT mcp_clients_dev_owner
 CHECK((kind = 'DEV') = (owner_user_id IS NOT NULL));

CREATE INDEX mcp_clients_owner ON identity.mcp_clients(owner_user_id)
 WHERE owner_user_id IS NOT NULL AND disabled_at IS NULL;
