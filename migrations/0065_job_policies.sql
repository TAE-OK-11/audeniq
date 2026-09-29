-- 0065: per-kind job policy (P2, docs/PIPELINE_ARCHITECTURE.md).
--
-- Every job kind had the same contract: 5 attempts, backoff 5·2ⁿ s capped at
-- an hour, and no execution time limit (the worker's heartbeat renews the
-- lease for as long as a handler runs, so a network call that hangs held a
-- worker slot forever). Now each kind can have its own row:
--   max_attempts        copied onto the job at insert (trigger below)
--   backoff_base/max    retry delay = min(max, base·2^attempts) with ±20% jitter,
--                       so a shared outage does not retry 500 jobs in lockstep
--   timeout_secs        the worker cancels the handler and retries the job.
--                       Only for kinds whose work is cancellable async I/O
--                       (partner HTTP/SFTP, DB). CPU work (QC, fingerprints,
--                       FLAC) runs ffmpeg in threads a cancel cannot stop;
--                       those analyzers kill ffmpeg at their own deadline.
-- Kinds without a row keep the old defaults. Rows can be tuned in place.
CREATE TABLE operations.job_policies (
 kind text PRIMARY KEY CHECK (kind ~ '^[a-z0-9_.]{1,100}$'),
 max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 20),
 backoff_base_secs integer NOT NULL CHECK (backoff_base_secs BETWEEN 1 AND 3600),
 backoff_max_secs integer NOT NULL CHECK (backoff_max_secs BETWEEN 1 AND 86400),
 timeout_secs integer NULL CHECK (timeout_secs IS NULL OR timeout_secs BETWEEN 10 AND 86400),
 note text NOT NULL DEFAULT '' CHECK (length(note) <= 500),
 CHECK (backoff_max_secs >= backoff_base_secs)
);

INSERT INTO operations.job_policies(kind, max_attempts, backoff_base_secs, backoff_max_secs, timeout_secs, note) VALUES
 ('asset.analyze',      5,  5, 3600, NULL, 'CPU (ffmpeg); analyzer deadlines kill ffmpeg'),
 ('stage1',             5,  5, 3600, NULL, 'CPU (ffmpeg); analyzer deadlines kill ffmpeg'),
 ('stage2',             5,  5, 3600, NULL, 'fingerprint comparison may run in blocking threads'),
 ('prepare_release',    5,  5, 3600, NULL, 'renders packages in blocking threads'),
 ('delivery.stage',     5,  5, 3600, NULL, 'renders ERN in blocking threads'),
 ('delivery.enqueue',   5,  5, 3600, 300,  'DB + route decisions'),
 ('delivery.send',      5, 30, 3600, 1800, 'partner outages: slower first retry; large uploads'),
 ('delivery.poll',      5, 30, 3600, 120,  'partner status call'),
 ('delivery.ack',       5, 10, 3600, 120,  'applies a partner notification'),
 ('delivery.mark_live', 5, 10, 3600, 120,  'DB'),
 ('delivery.reconcile', 5, 30, 3600, 600,  'partner inquiries for unknown sends'),
 ('delivery.takedown',  5, 30, 3600, 600,  'partner takedown call'),
 ('outbox.record',      5,  2,  300, 60,   'DB only: retry fast');

-- max_attempts from the policy at insert. SECURITY DEFINER: every role that
-- enqueues (api, worker, definer functions) gets the policy without its own
-- grant on the table; it only reads the one row.
CREATE FUNCTION operations.apply_job_policy() RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
BEGIN
  SELECT p.max_attempts INTO NEW.max_attempts
    FROM operations.job_policies p WHERE p.kind = NEW.kind;
  IF NEW.max_attempts IS NULL THEN
    NEW.max_attempts := 5;
  END IF;
  RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION operations.apply_job_policy() FROM PUBLIC;
CREATE TRIGGER jobs_policy
BEFORE INSERT ON operations.jobs
FOR EACH ROW EXECUTE FUNCTION operations.apply_job_policy();

-- Staff requeue a dead-lettered job after fixing its cause.
ALTER TABLE operations.jobs ADD COLUMN retried_by uuid NULL, ADD COLUMN retried_at timestamptz NULL;
