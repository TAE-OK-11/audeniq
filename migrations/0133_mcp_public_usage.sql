-- AUDENIQ MCP: daily limits for the public tools (notices, events, service
-- status, release requirements). See docs/MCP.md "Public tools".
--
-- Without an account a client may call them a few times a day per IP
-- address (AUDENIQ_MCP_PUBLIC_DAILY_ANONYMOUS, default 10) and is then asked
-- to connect; a signed-in user gets AUDENIQ_MCP_PUBLIC_DAILY_USER (default
-- 150) and then contacts support, who can raise that user's limit.

-- Calls per bucket (keyed HMAC of the IP or user, like auth_limits) per KST day.
CREATE TABLE identity.mcp_daily_usage (
 bucket_hash bytea NOT NULL CHECK(octet_length(bucket_hash)=32),
 day date NOT NULL,
 calls integer NOT NULL CHECK(calls >= 0),
 PRIMARY KEY(bucket_hash, day)
);
CREATE INDEX mcp_daily_usage_day ON identity.mcp_daily_usage(day);

-- A user's own daily limit, set by an operator after a support inquiry.
CREATE TABLE identity.mcp_usage_limits (
 user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE CASCADE,
 daily_limit integer NOT NULL CHECK(daily_limit BETWEEN 0 AND 100000),
 note text NULL CHECK(note IS NULL OR length(note) <= 500),
 set_by text NOT NULL CHECK(length(btrim(set_by)) BETWEEN 1 AND 200),
 updated_at timestamptz NOT NULL DEFAULT now()
);
