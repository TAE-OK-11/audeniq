-- 0056: per-DSP contract route — direct contract or Merlin.
--
-- Each DSP is licensed either under AUDENIQ's own (direct) contract with the
-- DSP or under the Merlin agreement (Merlin licenses independent labels'
-- catalogues to the DSPs it has deals with). Delivery is technically the
-- same — the DSP's ingestion endpoint, the partner config file — but the
-- contract evidence differs:
--   DIRECT : the DSP's own onboarding row carries the signed contract.
--   MERLIN : the Merlin membership agreement (onboarding row 'merlin')
--            stands in for the DSP contract; every technical requirement of
--            the DSP (DPID, endpoint, credential, test ERN/ACK) still applies.
-- MERLIN can only be chosen for a DSP with a Merlin deal (merlin_eligible,
-- seeded from the DSP registry; the Korean services have none).
--
-- The pre-launch lock (DSP_LIVE_TRANSMISSION) still blocks routing either way.
CREATE TABLE distribution.dsp_contract_routes (
 code text PRIMARY KEY REFERENCES distribution.dsp_registry(code) ON DELETE RESTRICT,
 route text NOT NULL DEFAULT 'DIRECT' CHECK (route IN ('DIRECT','MERLIN')),
 merlin_eligible boolean NOT NULL DEFAULT false,
 updated_by text NULL CHECK (updated_by IS NULL OR length(btrim(updated_by)) BETWEEN 1 AND 200),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK (route = 'DIRECT' OR merlin_eligible)
);

INSERT INTO distribution.dsp_contract_routes(code, merlin_eligible) VALUES
 ('D-1', false), ('D-2', false), ('D-3', false), ('D-4', false),
 ('D-5', true), ('D-6', true), ('D-7', true), ('D-8', true), ('D-9', true), ('D-10', true),
 ('D-11', false)
ON CONFLICT (code) DO NOTHING;

-- Contract evidence follows the chosen route.
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
    SELECT 'contract_signed',
           CASE WHEN (SELECT r.route FROM distribution.dsp_contract_routes r WHERE r.code = p_partner_id) = 'MERLIN'
                THEN 'no signed Merlin agreement on file (route MERLIN)'
                ELSE 'no signed contract on file' END
      WHERE CASE
              WHEN (SELECT r.route FROM distribution.dsp_contract_routes r WHERE r.code = p_partner_id) = 'MERLIN'
              THEN NOT EXISTS (
                     SELECT 1 FROM distribution.dsp_contract_routes r
                       JOIN execution.partner_onboarding m ON m.partner_id = 'merlin'
                      WHERE r.code = p_partner_id AND r.merlin_eligible
                        AND m.contract_signed_at IS NOT NULL)
              ELSE (SELECT o.contract_signed_at FROM execution.partner_onboarding o WHERE o.partner_id = p_partner_id) IS NULL
            END
$$;

-- The gaps already carry the route-aware contract requirement.
CREATE OR REPLACE FUNCTION execution.platform_contract_live(p_partner_id text)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT COALESCE((
    SELECT o.stage = 'LIVE'
       AND o.credential_status = 'STORED'
       AND NOT EXISTS (SELECT 1 FROM execution.partner_onboarding_gaps(p_partner_id))
      FROM execution.partner_onboarding o
     WHERE o.partner_id = p_partner_id), false)
$$;
REVOKE ALL ON FUNCTION execution.platform_contract_live(text) FROM PUBLIC;

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT SELECT, UPDATE (route, updated_by, updated_at) ON distribution.dsp_contract_routes TO audeniq_api';
    EXECUTE 'GRANT EXECUTE ON FUNCTION execution.platform_contract_live(text) TO audeniq_api';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
    EXECUTE 'GRANT EXECUTE ON FUNCTION execution.platform_contract_live(text) TO audeniq_worker';
  END IF;
END
$$;
