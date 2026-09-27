-- 0048: delivery waits for the signed distribution agreement.
--
-- READY_FOR_DELIVERY used to fan out to every routable partner right away,
-- so MockDSP (no staging row, no staff approval) went LIVE before staff
-- reviewed the agreement or the artist signed it, and Studio showed the
-- release as released. E-0 (execution::enqueue_delivery_jobs) now sends
-- nothing until the release's AGREEMENT document is SIGNED; signing re-runs
-- E-0. Signing itself needs staff APPROVED + the artist's read confirmation.
--
-- The worker has no portal grants: it asks through one narrow SECURITY
-- DEFINER function in a schema it already uses.
CREATE FUNCTION execution.agreement_signed(p_org uuid, p_release uuid)
RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT EXISTS (
  SELECT 1 FROM portal.documents d
   WHERE d.org_id = p_org AND d.release_id = p_release
     AND d.kind = 'AGREEMENT' AND d.status = 'SIGNED')
$$;
REVOKE ALL ON FUNCTION execution.agreement_signed(uuid, uuid) FROM PUBLIC;
