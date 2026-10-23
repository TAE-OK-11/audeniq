-- 0148: the MCP marks one connection's overdue permission requests EXPIRED when it reads them
-- (crates/core/src/mcp/approvals.rs), instead of sweeping the whole table on every call.
CREATE INDEX mcp_permission_requests_grant_pending ON identity.mcp_permission_requests(grant_id) WHERE status='PENDING';
