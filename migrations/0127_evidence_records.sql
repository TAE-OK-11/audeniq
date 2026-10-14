-- 0127: 신청서·동의 증거 고정.
--
-- * The signed application kept only a hash the browser computed; the seal
--   (0108) took the application revision that existed at signing time,
--   which is the previous one when the artist signs before resubmitting.
--   The server now builds the original itself at signing (release fields,
--   tracks with their audio SHA-256, attached proofs, signer, terms
--   version), stores it and hashes it: `content` + `server_hash`, sealed
--   with the application. The browser's hash stays as `content_hash` for
--   comparison. `signer_identity` records the account that signed and how
--   its identity was verified (CI hash, never the name: 0102).
-- * Payout-account consents were checked only in the browser. They are now
--   append-only rows: which items, which document version, which user,
--   when, under which request, for which version of the account.
ALTER TABLE portal.release_applications
 ADD COLUMN content jsonb NULL CHECK (content IS NULL OR jsonb_typeof(content) = 'object'),
 ADD COLUMN server_hash text NULL CHECK (server_hash IS NULL OR server_hash ~ '^[0-9a-f]{64}$'),
 ADD COLUMN signer_identity jsonb NULL;

CREATE TABLE portal.consent_records (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 org_id uuid NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 context text NOT NULL CHECK (context IN ('PAYOUT_ACCOUNT')),
 items jsonb NOT NULL CHECK (jsonb_typeof(items) = 'array' AND jsonb_array_length(items) BETWEEN 1 AND 20),
 subject_hash text NULL CHECK (subject_hash IS NULL OR subject_hash ~ '^[0-9a-f]{64}$'),
 request_id uuid NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX consent_records_user ON portal.consent_records(user_id, created_at DESC);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON portal.consent_records
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_api') THEN
  GRANT SELECT, INSERT ON portal.consent_records TO audeniq_api;
 END IF;
END $$;
