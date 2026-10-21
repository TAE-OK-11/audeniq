-- 0144: actions an AI connection asks the artist to approve once (crates/core/src/mcp/actions.rs).
-- Withdrawals, takedowns, revenue splits, release and contract end dates, counter notices,
-- member invites and payouts above the assistant limit. The assistant files the request with
-- what it wants done (payload) and a summary the artist reads; the artist approves or rejects
-- it in the Studio, and approval runs the action with the artist's own session.
CREATE TABLE identity.mcp_action_requests (
 id uuid PRIMARY KEY,
 grant_id uuid NOT NULL REFERENCES identity.mcp_grants ON DELETE CASCADE,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE CASCADE,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE CASCADE,
 client_name text NOT NULL CHECK(length(client_name) BETWEEN 1 AND 200),
 action text NOT NULL CHECK(action IN ('WITHDRAW','TAKEDOWN','SPLITS','RELEASE_END','CONTRACT_END','COUNTER_NOTICE','MEMBER_INVITE','PAYOUT')),
 release_id uuid NULL,
 payload jsonb NOT NULL,
 summary jsonb NOT NULL,
 status text NOT NULL DEFAULT 'PENDING' CHECK(status IN ('PENDING','RUNNING','DONE','FAILED','REJECTED','EXPIRED','REVOKED')),
 result jsonb NULL,
 error_code text NULL CHECK(length(error_code) <= 100),
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL,
 decided_at timestamptz NULL
);
CREATE INDEX mcp_action_requests_user ON identity.mcp_action_requests(user_id) WHERE status='PENDING';
CREATE INDEX mcp_action_requests_grant ON identity.mcp_action_requests(grant_id);
