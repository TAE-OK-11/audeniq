-- 0053: real partner delivery (F6 wiring, contract-ready).
--
-- 1. Onboarding endpoints may be file drops: DDEX deliveries are SFTP or
--    S3-bucket drops far more often than HTTPS. Plain http/ftp stay refused.
-- 2. execution.ack_events: one row per partner event id. The old dedupe
--    looked the id up inside delivery_attempts.response (a scan per ACK),
--    and a LIVE event overwrote the ACCEPTED event id stored there, so a
--    redelivered ACCEPTED was applied twice.
-- 3. execution.partner_inbox: signed partner webhooks land here from the
--    API (which holds no execution write grants) and the worker applies
--    them (`delivery.ack` job).
-- 4. execution.partner_event_org(): the worker resolves which org a partner
--    event belongs to without app.org_id (webhooks carry partner ids only).
--    Narrow SECURITY DEFINER function returning one uuid, same pattern as
--    catalog.fingerprints_outside_org (0034).
-- 5. Indexes for the ACK/poll correlation paths.

ALTER TABLE execution.partner_onboarding
  DROP CONSTRAINT IF EXISTS partner_onboarding_endpoint_url_check;
ALTER TABLE execution.partner_onboarding
  ADD CONSTRAINT partner_onboarding_endpoint_url_check
  CHECK (endpoint_url IS NULL OR endpoint_url ~ '^(https|sftp|s3)://[^[:space:]]+$');

ALTER TABLE execution.partner_onboarding
  DROP CONSTRAINT IF EXISTS partner_onboarding_credential_kind_check;
ALTER TABLE execution.partner_onboarding
  ADD CONSTRAINT partner_onboarding_credential_kind_check
  CHECK (credential_kind IN ('oauth2','api_key','mtls','sftp_key','s3_key','hmac'));

CREATE TABLE execution.ack_events (
 partner_id text NOT NULL CHECK (length(btrim(partner_id)) > 0),
 event_id text NOT NULL CHECK (length(event_id) BETWEEN 1 AND 300),
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 outcome text NOT NULL CHECK (outcome IN ('ACCEPTED','REJECTED','LIVE','TAKEN_DOWN')),
 job_id uuid NULL REFERENCES execution.delivery_jobs(id) ON DELETE RESTRICT,
 applied boolean NOT NULL,
 received_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (partner_id, event_id)
);
ALTER TABLE execution.ack_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE execution.ack_events FORCE ROW LEVEL SECURITY;
CREATE POLICY ack_events_org_scope ON execution.ack_events
 USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid)
 WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid);
CREATE TRIGGER ack_events_no_delete
 BEFORE DELETE ON execution.ack_events
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- Platform-level (partner, not tenant) queue: no RLS, grants restrict it.
-- The payload is kept as received for audit; the worker records the result.
CREATE TABLE execution.partner_inbox (
 id uuid PRIMARY KEY,
 partner_id text NOT NULL CHECK (length(btrim(partner_id)) BETWEEN 1 AND 64),
 payload bytea NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 1048576),
 payload_sha256 text NOT NULL CHECK (payload_sha256 ~ '^[0-9a-f]{64}$'),
 content_type text NULL,
 received_at timestamptz NOT NULL DEFAULT now(),
 processed_at timestamptz NULL,
 result text NULL CHECK (result IS NULL OR length(result) <= 500),
 UNIQUE (partner_id, payload_sha256)
);
CREATE INDEX partner_inbox_pending ON execution.partner_inbox(received_at)
 WHERE processed_at IS NULL;

-- The definer function reads across orgs as the table owner; FORCE RLS
-- applies to the owner too, so these SELECT policies admit exactly the
-- owner (the migration role, never a runtime login).
CREATE POLICY delivery_attempts_owner_scan ON execution.delivery_attempts FOR SELECT
 USING (current_user = (SELECT pg_catalog.pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class c
                         WHERE c.oid = 'execution.delivery_attempts'::regclass));
CREATE POLICY delivery_jobs_owner_scan ON execution.delivery_jobs FOR SELECT
 USING (current_user = (SELECT pg_catalog.pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class c
                         WHERE c.oid = 'execution.delivery_jobs'::regclass));
CREATE POLICY live_bindings_owner_scan ON execution.live_bindings FOR SELECT
 USING (current_user = (SELECT pg_catalog.pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class c
                         WHERE c.oid = 'execution.live_bindings'::regclass));

-- Org of a partner event: by our/partner message id first, then by the
-- partner's release id. NULL when nothing matches (event is UNMATCHED).
CREATE OR REPLACE FUNCTION execution.partner_event_org(p_partner text, p_message_id text, p_release_id text)
RETURNS uuid
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT org_id FROM (
    SELECT j.org_id, 1 AS rank
      FROM execution.delivery_attempts a
      JOIN execution.delivery_jobs j ON j.id = a.job_id
     WHERE p_message_id IS NOT NULL AND j.partner_id = p_partner AND a.partner_message_id = p_message_id
    UNION ALL
    SELECT b.org_id, 2
      FROM execution.live_bindings b
     WHERE p_release_id IS NOT NULL AND b.partner_id = p_partner AND b.partner_release_id = p_release_id
  ) m
  ORDER BY rank
  LIMIT 1
$$;
REVOKE ALL ON FUNCTION execution.partner_event_org(text, text, text) FROM PUBLIC;

CREATE INDEX IF NOT EXISTS delivery_attempts_partner_message
 ON execution.delivery_attempts(partner_message_id) WHERE partner_message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS live_bindings_partner_release
 ON execution.live_bindings(partner_id, partner_release_id) WHERE partner_release_id IS NOT NULL;

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT INSERT ON execution.partner_inbox TO audeniq_api';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
    EXECUTE 'GRANT SELECT,UPDATE ON execution.partner_inbox TO audeniq_worker';
    EXECUTE 'GRANT SELECT,INSERT ON execution.ack_events TO audeniq_worker';
    EXECUTE 'GRANT EXECUTE ON FUNCTION execution.partner_event_org(text, text, text) TO audeniq_worker';
  END IF;
END
$$;
