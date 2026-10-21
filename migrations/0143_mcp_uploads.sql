-- 0143: files sent or imported by an AI connection (crates/core/src/mcp/files.rs).
-- Upload completion of a long master can outlast a tool call, so it runs in the background
-- and records its outcome here for get_upload_status. The upload itself is an ordinary
-- catalog.upload_sessions row; source_host is the host an imported file came from.
CREATE TABLE identity.mcp_uploads (
 session_id uuid PRIMARY KEY REFERENCES catalog.upload_sessions ON DELETE CASCADE,
 grant_id uuid NOT NULL REFERENCES identity.mcp_grants ON DELETE CASCADE,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE CASCADE,
 source text NOT NULL CHECK(source IN ('PUT','URL')),
 source_host text NULL CHECK(length(source_host) <= 255),
 status text NOT NULL DEFAULT 'PROCESSING' CHECK(status IN ('PROCESSING','DONE','FAILED')),
 error_code text NULL CHECK(length(error_code) <= 100),
 created_at timestamptz NOT NULL DEFAULT now(),
 finished_at timestamptz NULL
);
CREATE INDEX mcp_uploads_grant ON identity.mcp_uploads(grant_id);
