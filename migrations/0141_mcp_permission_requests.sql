-- 0141: an AI connection asks the artist for more permissions (crates/core/src/mcp/approvals.rs).
-- The assistant calls request_permission; the artist opens the approval link in the Studio and
-- approves or rejects. Approved scopes are added to that connection's grant (identity.mcp_grants)
-- and stay until it is revoked; its existing tokens get them at once.
CREATE TABLE identity.mcp_permission_requests (
 id uuid PRIMARY KEY,
 grant_id uuid NOT NULL REFERENCES identity.mcp_grants ON DELETE CASCADE,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE CASCADE,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE CASCADE,
 client_name text NOT NULL CHECK(length(client_name) BETWEEN 1 AND 200),
 scopes text[] NOT NULL CHECK(cardinality(scopes) BETWEEN 1 AND 8),
 reason text NOT NULL DEFAULT '' CHECK(length(reason) <= 500),
 status text NOT NULL DEFAULT 'PENDING' CHECK(status IN ('PENDING','APPROVED','REJECTED','EXPIRED','REVOKED')),
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL,
 decided_at timestamptz NULL
);
CREATE INDEX mcp_permission_requests_user ON identity.mcp_permission_requests(user_id) WHERE status='PENDING';
CREATE INDEX mcp_permission_requests_grant ON identity.mcp_permission_requests(grant_id);
