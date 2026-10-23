-- 0147: an AI connection may take back what it asked for while the artist has not answered
-- yet (crates/core/src/mcp/actions.rs, approvals.rs): an action request, a permission request
-- or a submission waiting for Studio approval. WITHDRAWN is final, like REJECTED.
ALTER TABLE identity.mcp_action_requests DROP CONSTRAINT mcp_action_requests_status_check;
ALTER TABLE identity.mcp_action_requests ADD CONSTRAINT mcp_action_requests_status_check
  CHECK(status IN ('PENDING','RUNNING','DONE','FAILED','REJECTED','EXPIRED','REVOKED','WITHDRAWN'));
ALTER TABLE identity.mcp_permission_requests DROP CONSTRAINT mcp_permission_requests_status_check;
ALTER TABLE identity.mcp_permission_requests ADD CONSTRAINT mcp_permission_requests_status_check
  CHECK(status IN ('PENDING','APPROVED','REJECTED','EXPIRED','REVOKED','WITHDRAWN'));
ALTER TABLE identity.mcp_pending_submissions DROP CONSTRAINT mcp_pending_submissions_status_check;
ALTER TABLE identity.mcp_pending_submissions ADD CONSTRAINT mcp_pending_submissions_status_check
  CHECK(status IN ('PENDING','APPROVED','REJECTED','STALE','EXPIRED','WITHDRAWN'));
