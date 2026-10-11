-- 0094: 신청서·계약·동의 증거 고정 (감사 4번).
--
-- * The signed application kept only a hash the browser computed and no copy
--   of what was signed. The server now builds the original itself (release,
--   tracks, attached proofs with their SHA-256, signer, consents, terms
--   version), stores it, and hashes it: `content` + `server_hash`. The
--   browser's hash stays as `content_hash` for comparison. `signer_identity`
--   records the account that signed and its verified real name, if any.
-- * A signed agreement records the hash of exactly what was signed (body,
--   terms, ticked confirmations, signature) and the signer's identity.
-- * Signup and payout-account consents were checked only in the browser.
--   They are now append-only rows: which items, which document version,
--   which user, when, under which request.
ALTER TABLE portal.release_applications
 ADD COLUMN content jsonb NULL CHECK (content IS NULL OR jsonb_typeof(content) = 'object'),
 ADD COLUMN server_hash text NULL CHECK (server_hash IS NULL OR server_hash ~ '^[0-9a-f]{64}$'),
 ADD COLUMN signer_identity jsonb NULL;

ALTER TABLE portal.documents
 ADD COLUMN signed_content_hash text NULL CHECK (signed_content_hash IS NULL OR signed_content_hash ~ '^[0-9a-f]{64}$'),
 ADD COLUMN signer_identity jsonb NULL;

CREATE TABLE portal.consent_records (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 org_id uuid NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 context text NOT NULL CHECK (context IN ('SIGNUP','PAYOUT_ACCOUNT')),
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
