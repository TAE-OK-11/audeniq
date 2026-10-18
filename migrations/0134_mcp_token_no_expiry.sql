-- AUDENIQ MCP: connection tokens without an end date. See docs/MCP.md.
--
-- The artist may make a Studio connection token that lasts until they
-- revoke it (or change the password / log out everywhere). Only access
-- tokens may have no expiry; refresh tokens always do.
ALTER TABLE identity.mcp_tokens ALTER COLUMN expires_at DROP NOT NULL;
ALTER TABLE identity.mcp_tokens ADD CONSTRAINT mcp_tokens_expiry_check
 CHECK(expires_at IS NOT NULL OR kind='ACCESS');
