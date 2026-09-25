-- 0027: partner onboarding model (F6 groundwork, contract-free).
--
-- Everything a future DSP / Merlin / LIMBO contract needs *around* the
-- signature itself can be built and tested now: party identification,
-- endpoint + credential metadata, and interop proof (XSD-valid test
-- messages, ACK parsing). The contract signature is tracked but cannot be
-- completed without a real counterparty — the readiness gate reports it
-- as missing instead of inventing it.
--
-- No secrets live here: credential_status/kind are metadata only. Actual
-- credentials go to the platform Secure Vault, never this table.

CREATE TABLE execution.partner_onboarding (
    partner_id text PRIMARY KEY REFERENCES execution.adapter_profiles(partner_id) ON DELETE CASCADE,
    stage text NOT NULL DEFAULT 'INTAKE'
        CHECK (stage IN ('INTAKE','TECHNICAL','COMMERCIAL','READY','LIVE')),
    -- Party identification (DDEX).
    dpid_registered boolean NOT NULL DEFAULT false,
    -- Technical transport. NULL = not registered yet.
    endpoint_url text CHECK (endpoint_url IS NULL OR endpoint_url ~ '^https://'),
    endpoint_health text NOT NULL DEFAULT 'UNKNOWN'
        CHECK (endpoint_health IN ('UNKNOWN','HEALTHY','UNHEALTHY')),
    credential_kind text CHECK (credential_kind IN ('oauth2','api_key','mtls','sftp_key')),
    credential_status text NOT NULL DEFAULT 'MISSING'
        CHECK (credential_status IN ('MISSING','STORED','EXPIRED','REVOKED')),
    -- Interop proof. Both are producible without any contract:
    -- test_ern_validated_at = our XSD-valid test message generated;
    -- test_ack_parsed_at   = a partner-style ACK successfully parsed.
    test_ern_validated_at timestamptz,
    test_ack_parsed_at timestamptz,
    -- Commercial. Stays NULL until a real contract exists; the readiness
    -- gate treats a missing signature as a blocker, never as assumed.
    contract_signed_at timestamptz,
    contract_ref text,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (contract_signed_at IS NULL OR contract_ref IS NOT NULL),
    CHECK (credential_kind IS NULL OR credential_status <> 'MISSING')
);
ALTER TABLE execution.partner_onboarding ENABLE ROW LEVEL SECURITY;
-- No FORCE: the table owner (platform operator) bypasses RLS; runtime
-- roles hold no grants on this table at all, and RLS with no policy
-- denies them even if a grant were added later. Partner onboarding is
-- operator-owned state, never tenant state.

-- Readiness gate: what is still missing before a partner may go live.
-- Returns one row per missing requirement; empty set = ready.
CREATE OR REPLACE FUNCTION execution.partner_onboarding_gaps(p_partner_id text)
RETURNS TABLE (requirement text, detail text)
LANGUAGE sql STABLE AS $$
    SELECT 'dpid_registered'::text, 'recipient DPID not registered'::text
      WHERE NOT (SELECT o.dpid_registered FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id)
    UNION ALL
    SELECT 'endpoint_url', 'delivery endpoint not registered'
      WHERE (SELECT o.endpoint_url FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id) IS NULL
    UNION ALL
    SELECT 'credential_status', 'credential not stored (metadata only; secret lives in Secure Vault)'
      WHERE (SELECT o.credential_status FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id) IS DISTINCT FROM 'STORED'
    UNION ALL
    SELECT 'test_ern_validated', 'no XSD-valid test ERN generated for this partner'
      WHERE (SELECT o.test_ern_validated_at FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id) IS NULL
    UNION ALL
    SELECT 'test_ack_parsed', 'no partner-style ACK successfully parsed'
      WHERE (SELECT o.test_ack_parsed_at FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id) IS NULL
    UNION ALL
    SELECT 'contract_signed', 'no signed contract on file'
      WHERE (SELECT o.contract_signed_at FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id) IS NULL
$$;

-- Guard: delivery_enabled may only flip false->true when the onboarding
-- gate is empty. The flip itself stays operator-only (grants), this just
-- makes "live" mean "all requirements evidenced".
CREATE OR REPLACE FUNCTION execution.guard_delivery_enabled()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    gaps integer;
BEGIN
    IF NEW.delivery_enabled AND NOT OLD.delivery_enabled THEN
        SELECT count(*) INTO gaps FROM execution.partner_onboarding_gaps(NEW.partner_id);
        IF gaps > 0 THEN
            RAISE EXCEPTION 'partner % not ready for live delivery: % requirement(s) missing (see execution.partner_onboarding_gaps)',
                NEW.partner_id, gaps USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END $$;
DROP TRIGGER IF EXISTS trg_guard_delivery_enabled ON execution.adapter_profiles;
CREATE TRIGGER trg_guard_delivery_enabled
    BEFORE UPDATE OF delivery_enabled ON execution.adapter_profiles
    FOR EACH ROW EXECUTE FUNCTION execution.guard_delivery_enabled();

-- Seed: MockDSP is the local test partner. Its technical interop is
-- exercised by the test suite itself, so the technical steps are marked
-- complete; commercial stays empty (no contract, honestly).
INSERT INTO execution.partner_onboarding
  (partner_id, stage, dpid_registered, endpoint_url, endpoint_health,
   credential_kind, credential_status, test_ern_validated_at, test_ack_parsed_at)
VALUES
  ('mockdsp', 'TECHNICAL', true, 'https://mockdsp.local/ddex', 'HEALTHY',
   'api_key', 'STORED', now(), now())
ON CONFLICT (partner_id) DO NOTHING;
