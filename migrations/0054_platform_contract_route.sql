-- 0054: platform-level contract route for CONTRACTED partners.
--
-- Until now a CONTRACTED profile was sendable only through a per-org route
-- plan (distribution.route_plans + dsp_endpoints + contract revision). The
-- F1 schema locks those rows disabled (CHECK enabled=false,
-- integration_status='INTEGRATION_PENDING'), so no contracted DSP could ever
-- become routable — and a DSP contract is signed once by the distributor,
-- not once per artist organization.
--
-- A partner's own onboarding record is the distributor-level contract: it
-- already requires the registered DPID, endpoint, stored credential, an
-- XSD-valid test ERN, a parsed test ACK and the signed contract reference,
-- and only reaches stage LIVE when none of those is missing. This function
-- is the single verdict routing, Stage 2 eligibility and E-0 use. Moving
-- the stage back (e.g. to TECHNICAL) suspends the route immediately; the
-- delivery_enabled kill switch and staff approval still apply on top.
CREATE OR REPLACE FUNCTION execution.platform_contract_live(p_partner_id text)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT COALESCE((
    SELECT o.stage = 'LIVE'
       AND o.contract_signed_at IS NOT NULL
       AND o.credential_status = 'STORED'
       AND NOT EXISTS (SELECT 1 FROM execution.partner_onboarding_gaps(p_partner_id))
      FROM execution.partner_onboarding o
     WHERE o.partner_id = p_partner_id), false)
$$;
REVOKE ALL ON FUNCTION execution.platform_contract_live(text) FROM PUBLIC;

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT EXECUTE ON FUNCTION execution.platform_contract_live(text) TO audeniq_api';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
    EXECUTE 'GRANT EXECUTE ON FUNCTION execution.platform_contract_live(text) TO audeniq_worker';
  END IF;
END
$$;
