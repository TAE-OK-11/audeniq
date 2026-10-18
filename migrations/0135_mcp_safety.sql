-- AUDENIQ MCP safety: retry-safe creates, activity notices, and an optional
-- final approval in the Studio for submissions made through an AI assistant.
-- See docs/MCP.md "Safety".

-- One create per (connection, tool, client request id): a client retrying
-- after a lost response gets the first result instead of a duplicate.
-- result_id is NULL while the first call is still running.
CREATE TABLE identity.mcp_idempotency (
 grant_id uuid NOT NULL REFERENCES identity.mcp_grants ON DELETE CASCADE,
 tool text NOT NULL CHECK(length(tool) BETWEEN 1 AND 64),
 request_key text NOT NULL CHECK(length(request_key) BETWEEN 8 AND 64),
 result_id uuid NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(grant_id, tool, request_key)
);
CREATE INDEX mcp_idempotency_age ON identity.mcp_idempotency(created_at);

-- Per-user AI settings. studio_approval: submissions made through an AI
-- assistant wait until the artist approves them in the Studio.
CREATE TABLE identity.mcp_user_settings (
 user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE CASCADE,
 studio_approval boolean NOT NULL DEFAULT false,
 updated_at timestamptz NOT NULL DEFAULT now()
);

-- Submissions waiting for that approval: exactly what the artist confirmed
-- in the chat (declarations, the release content hash), for 7 days.
CREATE TABLE identity.mcp_pending_submissions (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE CASCADE,
 release_id uuid NOT NULL,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE CASCADE,
 grant_id uuid NOT NULL REFERENCES identity.mcp_grants ON DELETE CASCADE,
 client_name text NOT NULL CHECK(length(client_name) BETWEEN 1 AND 200),
 declarations jsonb NOT NULL,
 scope_hash text NOT NULL CHECK(length(scope_hash) BETWEEN 1 AND 128),
 idempotency_key text NOT NULL UNIQUE CHECK(length(idempotency_key) BETWEEN 1 AND 200),
 status text NOT NULL DEFAULT 'PENDING' CHECK(status IN ('PENDING','APPROVED','REJECTED','STALE','EXPIRED')),
 revision_id uuid NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL,
 decided_at timestamptz NULL,
 FOREIGN KEY(org_id, release_id) REFERENCES catalog.releases(org_id, id) ON DELETE CASCADE
);
CREATE INDEX mcp_pending_user ON identity.mcp_pending_submissions(user_id) WHERE status='PENDING';

-- A notice to one user about what an AI assistant did for them (a new
-- connection, a submission, a payout request). The notification e-mail
-- trigger (0112) mails it like any other notice.
CREATE FUNCTION identity.mcp_notice(p_org uuid, p_user uuid, p_kind text, p_title text, p_detail text, p_link text)
RETURNS void LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog AS $$
  INSERT INTO portal.notifications(org_id, user_id, kind, title, detail, link)
  VALUES (p_org, p_user, p_kind, left(p_title, 200), left(p_detail, 1000), p_link);
$$;
REVOKE ALL ON FUNCTION identity.mcp_notice(uuid, uuid, text, text, text, text) FROM PUBLIC;
