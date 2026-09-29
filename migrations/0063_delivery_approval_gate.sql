-- 0063: staff approval gates the start of every send, in the database.
--
-- Before: E-0 read the APPROVED staging rows in one transaction and created
-- delivery jobs in a later one, and nothing re-checked approval before a
-- send. A staff HOLD that landed in between, or any time after a job was
-- queued, did not stop the job: a held release could still reach the DSP.
--
-- Now a delivery job can only be leased (the step right before the wire
-- send) while its route is approved. The rule is the one E-0 already used
-- to create jobs: among the ROUTABLE route decisions of (package, partner),
-- at least one is a non-registry route or a registry DSP whose staging row
-- is APPROVED and not content-blocked. A HOLD therefore parks queued jobs
-- (they stay QUEUED and are skipped); re-approving makes them leasable
-- again. The staging rows are read FOR SHARE, so a lease and a staff
-- decision (FOR UPDATE on the same row) serialize: a HOLD either lands
-- before the lease (no send) or after the send started.
--
-- Jobs with no recorded route decision (created before route decisions
-- existed) keep today's behaviour.
CREATE FUNCTION execution.delivery_gate_open(p_package uuid, p_partner text)
RETURNS boolean
LANGUAGE plpgsql
VOLATILE
SET search_path = pg_catalog
AS $$
DECLARE
  routed boolean;
BEGIN
  PERFORM 1 FROM distribution.delivery_staging s
   WHERE s.package_id = p_package
     AND s.dsp_id IN (SELECT d.dsp_id FROM execution.route_decisions d
                       WHERE d.package_id = p_package AND d.partner_id = p_partner
                         AND d.status = 'ROUTABLE')
   FOR SHARE;
  SELECT EXISTS (SELECT 1 FROM execution.route_decisions d
                  WHERE d.package_id = p_package AND d.partner_id = p_partner
                    AND d.status = 'ROUTABLE')
    INTO routed;
  IF NOT routed THEN
    RETURN true;
  END IF;
  RETURN EXISTS (
    SELECT 1 FROM execution.route_decisions d
     WHERE d.package_id = p_package AND d.partner_id = p_partner AND d.status = 'ROUTABLE'
       AND (NOT EXISTS (SELECT 1 FROM distribution.dsp_registry r WHERE r.dsp_id = d.dsp_id)
            OR EXISTS (SELECT 1 FROM distribution.delivery_staging s
                        WHERE s.package_id = d.package_id AND s.dsp_id = d.dsp_id
                          AND s.approval = 'APPROVED' AND s.readiness <> 'CONTENT_BLOCKED')));
END
$$;
REVOKE ALL ON FUNCTION execution.delivery_gate_open(uuid, text) FROM PUBLIC;
