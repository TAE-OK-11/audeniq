-- F5: the operations dispatcher fans delivery work out onto a dedicated
-- queue so wire-call leases and retries tune independently from the
-- distribution (prep) worker pool.
ALTER TABLE operations.jobs DROP CONSTRAINT jobs_queue_check;
ALTER TABLE operations.jobs ADD CONSTRAINT jobs_queue_check
  CHECK (queue IN ('interactive','qc','rights','distribution','finance','delivery'));
