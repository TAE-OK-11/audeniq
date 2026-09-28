-- 0055: scope the delivery reconciler to orgs with open delivery work.
--
-- The 15-minute sweep authorized and scanned every organization (three
-- queries each), although only a handful ever have a delivery in flight;
-- with thousands of artist orgs that is thousands of transactions per run
-- for nothing. This narrow SECURITY DEFINER function (same owner-scan
-- policies as execution.partner_event_org, migration 0053) returns only the
-- org ids that have something the reconciler can act on: a queued/leased/
-- sending/parked job, or a delivered job whose live state is still open.
CREATE OR REPLACE FUNCTION execution.orgs_with_open_deliveries()
RETURNS SETOF uuid
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
  SELECT DISTINCT j.org_id
    FROM execution.delivery_jobs j
   WHERE j.status IN ('QUEUED','LEASED','SENDING','AWAITING_RECONCILIATION')
  UNION
  SELECT DISTINCT b.org_id
    FROM execution.live_bindings b
   WHERE b.live_status IN ('UNKNOWN','INGESTING','TAKEDOWN_REQUESTED')
$$;
REVOKE ALL ON FUNCTION execution.orgs_with_open_deliveries() FROM PUBLIC;

-- The live-state half of the scan above and reconcile's overdue query.
CREATE INDEX IF NOT EXISTS live_bindings_open
  ON execution.live_bindings(org_id, last_checked_at)
  WHERE live_status IN ('UNKNOWN','INGESTING','TAKEDOWN_REQUESTED');
-- Non-terminal delivery jobs (claim/repair/reconcile paths).
CREATE INDEX IF NOT EXISTS delivery_jobs_open
  ON execution.delivery_jobs(org_id, status, updated_at)
  WHERE status IN ('QUEUED','LEASED','SENDING','DELIVERED','AWAITING_RECONCILIATION');

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
    EXECUTE 'GRANT EXECUTE ON FUNCTION execution.orgs_with_open_deliveries() TO audeniq_worker';
  END IF;
END
$$;
