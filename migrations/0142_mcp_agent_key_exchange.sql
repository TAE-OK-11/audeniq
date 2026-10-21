-- 0142: agent keys and work tokens (crates/core/src/mcp/login.rs, docs/MCP.md).
-- A Studio connection token is the agent's key: it can be used as a bearer as it is, or exchanged
-- (RFC 8693 token exchange at /api/oauth/token) for short work tokens limited to the scopes asked for.
-- A work token carries its own scopes (the intersection with the connection's applies) and can
-- never be exchanged again. Revoking the connection deletes the key and its work tokens.
ALTER TABLE identity.mcp_tokens
 ADD COLUMN scopes text[] NULL CHECK(scopes IS NULL OR cardinality(scopes) BETWEEN 1 AND 8),
 ADD COLUMN exchanged boolean NOT NULL DEFAULT false,
 ADD CONSTRAINT mcp_tokens_work_token_check
   CHECK(NOT exchanged OR (kind='ACCESS' AND expires_at IS NOT NULL AND scopes IS NOT NULL));
