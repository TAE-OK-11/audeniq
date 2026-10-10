-- 0028: partner onboarding gate fixes (F6 groundwork).
--
-- 1. partner_onboarding_gaps: missing onboarding row must report ALL gaps,
--    not none. The old version used NOT (subquery) which is NOT NULL = NULL
--    (falsy) when no row exists — a missing row silently passed the gate.
--    Fixed with COALESCE to treat "no row" as "nothing evidenced".
--
-- 2. MockDSP seed: remove fake test_ern_validated_at / test_ack_parsed_at.
--    The 0027 seed set now() claiming the test suite exercised interop,
--    but the seed runs at migration time, not test time. Evidence must be
--    recorded by the operator functions (record_test_ern_validated /
--    record_test_ack_parsed) when the tests actually run, never invented
--    by a seed.
--
-- 3. guard_delivery_enabled: only applies to CONTRACTED partners. MOCK
--    partners (MockDSP) are eligible via delivery_enabled alone per the
--    activation_kind model (0022); the onboarding gate is the commercial
--    LIVE gate, not a test-partner gate.

CREATE OR REPLACE FUNCTION execution.partner_onboarding_gaps(p_partner_id text)
RETURNS TABLE (requirement text, detail text)
LANGUAGE sql STABLE AS $$
    SELECT 'dpid_registered'::text, 'recipient DPID not registered'::text
      WHERE NOT COALESCE((SELECT o.dpid_registered FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id), false)
    UNION ALL
    SELECT 'endpoint_url', 'delivery endpoint not registered'
      WHERE COALESCE((SELECT o.endpoint_url FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id), '') = ''
    UNION ALL
    SELECT 'credential_status', 'credential not stored (metadata only; secret lives in Secure Vault)'
      WHERE COALESCE((SELECT o.credential_status FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id), 'MISSING') IS DISTINCT FROM 'STORED'
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

-- The trigger now only gates CONTRACTED partners. MOCK partners bypass
-- the commercial onboarding gate by design (0022 activation_kind).
CREATE OR REPLACE FUNCTION execution.guard_delivery_enabled()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    gaps integer;
    kind text;
BEGIN
    IF NEW.delivery_enabled AND NOT OLD.delivery_enabled THEN
        SELECT activation_kind INTO kind FROM execution.adapter_profiles WHERE partner_id = NEW.partner_id;
        IF kind = 'CONTRACTED' THEN
            SELECT count(*) INTO gaps FROM execution.partner_onboarding_gaps(NEW.partner_id);
            IF gaps > 0 THEN
                RAISE EXCEPTION 'partner % not ready for live delivery: % requirement(s) missing (see execution.partner_onboarding_gaps)',
                    NEW.partner_id, gaps USING ERRCODE = '23514';
            END IF;
        END IF;
    END IF;
    RETURN NEW;
END $$;

-- Remove fake interop evidence from the MockDSP seed. The operator
-- functions record real timestamps when the test suite actually
-- generates a test ERN and parses a test ACK.
UPDATE execution.partner_onboarding
   SET test_ern_validated_at = NULL,
       test_ack_parsed_at = NULL,
       updated_at = now()
 WHERE partner_id = 'mockdsp';
