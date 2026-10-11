-- 0092: 계약 범위 → 배급 권한 (감사 2번).
--
-- Staff store a structured distribution scope with the agreement terms
-- (agreement_terms.scope: territories / Worldwide, excluded territories,
-- end date, covered DSPs, UGC). Delivery reads it once the agreement is
-- APPROVED or SIGNED, from the worker, which has no portal grants: one
-- narrow SECURITY DEFINER read, like execution.agreement_signed (0048).
CREATE FUNCTION execution.agreement_scope(p_org uuid, p_release uuid)
RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT d.agreement_terms -> 'scope'
   FROM portal.documents d
  WHERE d.org_id = p_org AND d.release_id = p_release
    AND d.kind = 'AGREEMENT' AND d.status IN ('APPROVED','SIGNED')
  ORDER BY d.updated_at DESC
  LIMIT 1
$$;
REVOKE ALL ON FUNCTION execution.agreement_scope(uuid, uuid) FROM PUBLIC;
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
  GRANT EXECUTE ON FUNCTION execution.agreement_scope(uuid, uuid) TO audeniq_worker;
 END IF;
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_api') THEN
  GRANT EXECUTE ON FUNCTION execution.agreement_scope(uuid, uuid) TO audeniq_api;
 END IF;
END $$;
