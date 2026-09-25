-- F5: distribution execution (MockDSP + durable attempts).
--
-- External sends are never inside a PostgreSQL transaction: local state
-- (delivery jobs, attempts, live bindings, reconciliation cases) is one
-- transaction; the wire call is correlated by attempt id + idempotency key.
-- The DSP is assumed NOT to support idempotent receives, so the worker
-- deduplicates on its own idempotency_key before calling send.
--
-- SENT_UNKNOWN is never auto-retried (BLUEPRINT 16.4): a timeout/unknown
-- outcome parks the job in AWAITING_RECONCILIATION and opens a
-- reconciliation case for a human or an explicit inquire to resolve.
-- Re-sending the same attempt is forbidden; only a new attempt with a new
-- idempotency key may go out, and only after explicit reconciliation.
CREATE SCHEMA execution;

-- One delivery job per (org, frozen package, partner). Lifecycle:
-- QUEUED -> LEASED -> SENDING -> DELIVERED | FAILED | AWAITING_RECONCILIATION
-- -> DEAD_LETTER. Live state is tracked separately in live_bindings.
CREATE TABLE execution.delivery_jobs (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 package_id uuid NOT NULL REFERENCES distribution.distribution_packages(id) ON DELETE RESTRICT,
 partner_id text NOT NULL CHECK (length(btrim(partner_id)) > 0),
 status text NOT NULL DEFAULT 'QUEUED'
   CHECK (status IN ('QUEUED','LEASED','SENDING','DELIVERED','FAILED','AWAITING_RECONCILIATION','DEAD_LETTER')),
 locked_by text NULL,
 lock_token uuid NULL,
 lease_until timestamptz NULL,
 attempts int NOT NULL DEFAULT 0 CHECK (attempts >= 0),
 max_attempts int NOT NULL DEFAULT 5 CHECK (max_attempts BETWEEN 1 AND 25),
 last_error text NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (org_id, package_id, partner_id)
);
CREATE INDEX delivery_jobs_claim ON execution.delivery_jobs(status, created_at)
 WHERE status = 'QUEUED';
ALTER TABLE execution.delivery_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE execution.delivery_jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY delivery_jobs_org_scope ON execution.delivery_jobs
 USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid)
 WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid);

-- Wire attempts: exactly one row per send call; the UNIQUE idempotency_key
-- is the duplicate-send guard. The row is a small state machine, not
-- append-only: outcome moves IN_FLIGHT -> terminal, and TIMEOUT/UNKNOWN may
-- later resolve to ACCEPTED/REJECTED through an explicit inquiry (never a
-- re-send). The guard below freezes the wire facts (key, request hash,
-- attempt number) while allowing the lifecycle columns to move forward;
-- terminal ACCEPTED/REJECTED outcomes can never be rewritten.
CREATE TABLE execution.delivery_attempts (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 job_id uuid NOT NULL REFERENCES execution.delivery_jobs(id) ON DELETE RESTRICT,
 attempt_no int NOT NULL CHECK (attempt_no >= 1),
 idempotency_key text NOT NULL,
 request_sha256 text NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
 outcome text NOT NULL
   CHECK (outcome IN ('IN_FLIGHT','ACCEPTED','REJECTED','TIMEOUT','UNKNOWN')),
 partner_message_id text NULL,
 response jsonb NOT NULL DEFAULT '{}',
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (idempotency_key),
 UNIQUE (job_id, attempt_no)
);
CREATE INDEX delivery_attempts_job ON execution.delivery_attempts(job_id, attempt_no);
ALTER TABLE execution.delivery_attempts ENABLE ROW LEVEL SECURITY;
ALTER TABLE execution.delivery_attempts FORCE ROW LEVEL SECURITY;
CREATE POLICY delivery_attempts_org_scope ON execution.delivery_attempts
 USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid)
 WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid);
CREATE OR REPLACE FUNCTION execution.guard_attempt_mutation() RETURNS trigger AS $$
BEGIN
  IF NEW.id IS DISTINCT FROM OLD.id
     OR NEW.org_id IS DISTINCT FROM OLD.org_id
     OR NEW.job_id IS DISTINCT FROM OLD.job_id
     OR NEW.attempt_no IS DISTINCT FROM OLD.attempt_no
     OR NEW.idempotency_key IS DISTINCT FROM OLD.idempotency_key
     OR NEW.request_sha256 IS DISTINCT FROM OLD.request_sha256
     OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
    RAISE EXCEPTION 'immutable attempt fact';
  END IF;
  IF OLD.outcome IN ('ACCEPTED','REJECTED')
     AND NEW.outcome IS DISTINCT FROM OLD.outcome THEN
    RAISE EXCEPTION 'attempt outcome is final: %', OLD.outcome;
  END IF;
  IF OLD.outcome = 'IN_FLIGHT' AND NEW.outcome NOT IN
     ('IN_FLIGHT','ACCEPTED','REJECTED','TIMEOUT','UNKNOWN') THEN
    RAISE EXCEPTION 'invalid attempt transition: % -> %', OLD.outcome, NEW.outcome;
  END IF;
  IF OLD.outcome IN ('TIMEOUT','UNKNOWN') AND NEW.outcome NOT IN
     ('TIMEOUT','UNKNOWN','ACCEPTED','REJECTED') THEN
    RAISE EXCEPTION 'invalid attempt transition: % -> %', OLD.outcome, NEW.outcome;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER delivery_attempts_guard
 BEFORE UPDATE ON execution.delivery_attempts
 FOR EACH ROW EXECUTE FUNCTION execution.guard_attempt_mutation();
-- Attempt rows are wire evidence: never deleted, only resolved forward.
CREATE TRIGGER delivery_attempts_no_delete
 BEFORE DELETE ON execution.delivery_attempts
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- Partner-side lifecycle, updated by webhook ACKs, status polls or manual
-- evidence. Kept separate from the delivery job: DELIVERED only means the
-- partner accepted the message, not that the release is live.
CREATE TABLE execution.live_bindings (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 package_id uuid NOT NULL REFERENCES distribution.distribution_packages(id) ON DELETE RESTRICT,
 partner_id text NOT NULL CHECK (length(btrim(partner_id)) > 0),
 live_status text NOT NULL DEFAULT 'UNKNOWN'
   CHECK (live_status IN ('UNKNOWN','INGESTING','LIVE','TAKEDOWN_REQUESTED','TAKEN_DOWN')),
 partner_release_id text NULL,
 last_checked_at timestamptz NULL,
 updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (org_id, package_id, partner_id)
);
ALTER TABLE execution.live_bindings ENABLE ROW LEVEL SECURITY;
ALTER TABLE execution.live_bindings FORCE ROW LEVEL SECURITY;
CREATE POLICY live_bindings_org_scope ON execution.live_bindings
 USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid)
 WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid);

-- Human-in-the-loop queue for outcomes automation must not resolve alone:
-- missing ACKs past deadline, SENT_UNKNOWN attempts, overdue live status.
CREATE TABLE execution.reconciliation_cases (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 job_id uuid NOT NULL REFERENCES execution.delivery_jobs(id) ON DELETE RESTRICT,
 kind text NOT NULL
   CHECK (kind IN ('MISSING_ACK','SENT_UNKNOWN','OVERDUE_LIVE','PARTNER_REJECTED')),
 status text NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN','RESOLVED')),
 detail jsonb NOT NULL DEFAULT '{}',
 created_at timestamptz NOT NULL DEFAULT now(),
 resolved_at timestamptz NULL,
 UNIQUE (job_id, kind, status)
);
ALTER TABLE execution.reconciliation_cases ENABLE ROW LEVEL SECURITY;
ALTER TABLE execution.reconciliation_cases FORCE ROW LEVEL SECURITY;
CREATE POLICY reconciliation_cases_org_scope ON execution.reconciliation_cases
 USING (org_id = nullif(current_setting('app.org_id',true),'')::uuid)
 WITH CHECK (org_id = nullif(current_setting('app.org_id',true),'')::uuid);

-- Partner integration catalog. Capabilities are only ever enabled from the
-- partner's real documentation (BLUEPRINT 16.3); the mock enables all of
-- them. delivery_enabled is the F5 commercial gate: the F4 route plan keeps
-- delivery_enabled=false, so a job is only claimed when the partner profile
-- explicitly allows delivery. No profile row = no send, ever.
CREATE TABLE execution.adapter_profiles (
 partner_id text PRIMARY KEY CHECK (length(btrim(partner_id)) > 0),
 display_name text NOT NULL,
 profile_version text NOT NULL,
 -- Internal DSP identity from the route plan's approved scope. E-0 only
 -- enqueues a delivery job when a route item's dsp_id maps to a profile
 -- with delivery_enabled=true. No profile row = no send, ever.
 dsp_id uuid NULL UNIQUE,
 capabilities jsonb NOT NULL DEFAULT '{}',
 delivery_enabled boolean NOT NULL DEFAULT false,
 transport text NOT NULL DEFAULT 'mock',
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO execution.adapter_profiles
  (partner_id, display_name, profile_version, capabilities, delivery_enabled, transport)
VALUES
  ('mockdsp', 'MockDSP (local test partner)', '1',
   '{"validate_package":true,"prepare_transfer":true,"send_or_publish":true,
     "inquire_submission":true,"parse_ack":true,"get_release_status":true,
     "update_release":true,"takedown":true,"receive_royalty_report":false}',
   true, 'mock')
ON CONFLICT (partner_id) DO NOTHING;
