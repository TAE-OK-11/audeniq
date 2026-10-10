-- 0088: let the worker share-lock a release's rights epoch without UPDATE.
--
-- delivery_staging::auto_approve held the epoch row with
-- `SELECT ... FOR SHARE OF r,e` so a concurrent rights write (the 0020 bump
-- trigger) cannot advance the epoch between the check and the automatic
-- approval. PostgreSQL requires UPDATE on at least one column for any row
-- lock, and audeniq_worker deliberately holds only SELECT/INSERT on
-- rights.rights_epochs: every delivery.stage failed with 42501
-- (permission denied for table rights_epochs) and went DEAD_LETTER.
--
-- Granting UPDATE would let the worker move the counter directly, so the
-- lock goes through one narrow SECURITY DEFINER function instead (same
-- stance as execution.agreement_signed in 0048). It takes FOR SHARE on the
-- one row and returns its epoch; the lock lasts until the caller's
-- transaction ends. It never writes, so the counter stays bumpable only by
-- the 0020 trigger. VOLATILE: row locks are not allowed in STABLE functions.
CREATE FUNCTION rights.share_lock_epoch(p_org uuid, p_release uuid)
RETURNS bigint
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT e.epoch FROM rights.rights_epochs e
  WHERE e.org_id = p_org AND e.release_id = p_release
  FOR SHARE
$$;
REVOKE ALL ON FUNCTION rights.share_lock_epoch(uuid, uuid) FROM PUBLIC;
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
  GRANT EXECUTE ON FUNCTION rights.share_lock_epoch(uuid, uuid) TO audeniq_worker;
 END IF;
END $$;
