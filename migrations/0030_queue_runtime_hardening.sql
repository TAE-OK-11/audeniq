-- Wake workers on committed queue changes instead of forcing every idle
-- worker to poll PostgreSQL continuously. Notifications are only a latency
-- optimization; workers retain a bounded polling fallback for recovery.
CREATE OR REPLACE FUNCTION operations.notify_job_available()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF NEW.status = 'QUEUED'
     AND (TG_OP = 'INSERT' OR OLD.status IS DISTINCT FROM NEW.status
          OR OLD.run_at IS DISTINCT FROM NEW.run_at) THEN
    PERFORM pg_notify('audeniq_jobs', NEW.queue);
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER jobs_notify_available
AFTER INSERT OR UPDATE OF status, run_at ON operations.jobs
FOR EACH ROW EXECUTE FUNCTION operations.notify_job_available();

-- Separate ready-work and expired-lease access paths. The old mixed partial
-- index made the hot claim query scan RUNNING rows and did not include the
-- deterministic id tiebreaker.
DROP INDEX IF EXISTS operations.jobs_claim;
CREATE INDEX jobs_ready_claim
  ON operations.jobs(queue, priority DESC, run_at, id)
  WHERE status = 'QUEUED';
CREATE INDEX jobs_expired_lease
  ON operations.jobs(queue, lease_until)
  WHERE status = 'RUNNING';
