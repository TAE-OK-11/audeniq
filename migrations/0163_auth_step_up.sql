-- Signup phone numbers are pending until the existing SMS ceremony succeeds.
ALTER TABLE portal.account_phones ALTER COLUMN verified_at DROP NOT NULL;
ALTER TABLE portal.account_phones ALTER COLUMN verified_at DROP DEFAULT;
ALTER TABLE identity.webauthn_challenges DROP CONSTRAINT webauthn_challenges_purpose_check;
ALTER TABLE identity.webauthn_challenges DROP CONSTRAINT webauthn_challenges_check;
ALTER TABLE identity.webauthn_challenges ADD CHECK (purpose IN ('REGISTER','LOGIN','STEP_UP'));
ALTER TABLE identity.webauthn_challenges ADD CHECK ((purpose IN ('REGISTER','STEP_UP')) = (user_id IS NOT NULL));
ALTER TABLE identity.webauthn_challenges ADD COLUMN session_hash bytea REFERENCES identity.sessions(token_hash) ON DELETE CASCADE;
ALTER TABLE identity.webauthn_challenges ADD CHECK ((purpose='STEP_UP') = (session_hash IS NOT NULL));
CREATE TABLE identity.passkey_proofs (
 token_hash bytea PRIMARY KEY CHECK(octet_length(token_hash)=32),
 session_hash bytea NOT NULL REFERENCES identity.sessions(token_hash) ON DELETE CASCADE,
 user_id uuid NOT NULL REFERENCES identity.users(id) ON DELETE CASCADE,
 expires_at timestamptz NOT NULL
);
CREATE INDEX passkey_proofs_expiry ON identity.passkey_proofs(expires_at);
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_api') THEN
  GRANT SELECT,INSERT,DELETE ON identity.passkey_proofs TO audeniq_api;
 END IF;
END $$;

ALTER TABLE identity.oauth_states ADD COLUMN session_hash bytea REFERENCES identity.sessions(token_hash) ON DELETE CASCADE;
