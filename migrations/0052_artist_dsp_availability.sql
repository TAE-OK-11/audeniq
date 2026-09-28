-- The API may inspect contracted route readiness for the current organization.
-- It cannot create or activate routes or contracts.
GRANT SELECT ON execution.route_coverage TO audeniq_api;
GRANT SELECT (org_id,dsp_id,route_kind,enabled,endpoint_id,contract_id,contract_revision_id) ON distribution.route_plans TO audeniq_api;
GRANT SELECT (org_id,dsp_id,id,integration_status) ON distribution.dsp_endpoints TO audeniq_api;
GRANT SELECT (id,org_id,contract_id,policy_version) ON rights.contract_revisions TO audeniq_api;
