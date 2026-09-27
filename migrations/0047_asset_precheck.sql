-- 0047: audio QC runs once, right after upload, not again at submission.
--
-- Upload completion no longer downloads the master to hash it: it sniffs the
-- first bytes and queues `asset.analyze`. That worker job is the only full
-- download before delivery: it hashes the object while streaming it to disk
-- (records catalog.assets.sha256), runs every byte-dependent Stage 1 check
-- and stores the fingerprint. Its outcomes land here, keyed like
-- operations.check_results (check code + rule version + content hash), so
-- Stage 1 finds them as cache hits and never downloads the file again.
--
-- Content-addressed, not org data: the key is derived from the bytes' hash
-- and the rule version. Only the worker writes; results are immutable.
-- TECHNICAL_RETRY is never stored (it is not a result).
CREATE TABLE operations.asset_qc_results (
 check_code text NOT NULL,
 rule_version text NOT NULL,
 result_hash text NOT NULL CHECK(result_hash ~ '^[a-f0-9]{64}$'),
 status text NOT NULL CHECK(status IN ('PASS','CORRECTION_REQUIRED','REVIEW_REQUIRED','BLOCKED','NOT_APPLICABLE')),
 detail text,
 created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(check_code, rule_version, result_hash)
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON operations.asset_qc_results
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
