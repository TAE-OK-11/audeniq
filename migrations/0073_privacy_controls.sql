CREATE SCHEMA privacy;
CREATE TABLE privacy.staff_access_logs (
 id uuid PRIMARY KEY, actor_user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 request_id uuid NOT NULL, source_ip inet, route text NOT NULL, method text NOT NULL,
 resource_id uuid, occurred_at timestamptz NOT NULL DEFAULT now(),
 retain_until timestamptz NOT NULL DEFAULT now()+interval '2 years',
 UNIQUE(actor_user_id,request_id,route), CHECK(retain_until>=occurred_at+interval '1 year')
);
CREATE INDEX staff_access_logs_time ON privacy.staff_access_logs(occurred_at);
REVOKE ALL ON SCHEMA privacy FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA privacy FROM PUBLIC;

-- Runtime roles cannot edit or delete access records. Even maintenance cannot
-- shorten retention or truncate the table; expired records are deleted by owner.
CREATE TRIGGER immutable BEFORE UPDATE ON privacy.staff_access_logs
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
CREATE FUNCTION privacy.guard_log_delete() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF OLD.retain_until>now() THEN RAISE EXCEPTION 'access log retention not elapsed' USING ERRCODE='23514'; END IF;
 RETURN OLD;
END $$;
CREATE TRIGGER retention BEFORE DELETE ON privacy.staff_access_logs
 FOR EACH ROW EXECUTE FUNCTION privacy.guard_log_delete();
CREATE TRIGGER no_truncate BEFORE TRUNCATE ON privacy.staff_access_logs
 FOR EACH STATEMENT EXECUTE FUNCTION operations.reject_mutation();
ALTER TABLE portal.payout_accounts ADD CONSTRAINT payout_key_version_positive CHECK(key_version>0);
CREATE INDEX sessions_expiry ON identity.sessions(expires_at);
CREATE INDEX auth_limits_expiry ON identity.auth_limits(window_start);
