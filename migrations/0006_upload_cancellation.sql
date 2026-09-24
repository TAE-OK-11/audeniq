ALTER TABLE catalog.upload_sessions DROP CONSTRAINT upload_sessions_status_check;
ALTER TABLE catalog.upload_sessions ADD CONSTRAINT upload_sessions_status_check CHECK(status IN ('ISSUED','COMPLETED','CANCELLED'));
-- Contract parties cannot change beneath an immutable contract revision.
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON rights.contracts FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
