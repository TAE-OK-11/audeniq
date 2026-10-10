-- 0062: indexes for operations.check_results.
--
-- The table is append-only (immutable trigger) and gains ~20 rows per
-- audio asset per submission, but it had no index besides its primary key:
-- every lookup below was a sequential scan of the whole history, growing
-- toward the 15 s statement_timeout.
--
-- Per revision: Stage 1 write-back (revision + code + result hash), the
-- Stage 2 hold module, staff release detail and delivery staging (latest
-- row per check code: DISTINCT ON (check_code) ... created_at DESC).
CREATE INDEX check_results_revision
    ON operations.check_results(revision_id, check_code, created_at DESC);
-- Stage 1 change cache: bytes-unchanged assets reuse a prior result by
-- (check code, rule version, content-derived hash).
CREATE INDEX check_results_cache
    ON operations.check_results(check_code, rule_version, result_hash);
