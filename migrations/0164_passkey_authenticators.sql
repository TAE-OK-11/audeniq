-- Optional display metadata: never an attestation trust or access-control signal.
-- Historic credentials have no retained AAGUID; do not guess from nicknames.
ALTER TABLE identity.passkeys
 ADD COLUMN aaguid uuid NULL,
 ADD COLUMN authenticator_name text NULL
   CHECK(authenticator_name IS NULL OR length(authenticator_name) BETWEEN 1 AND 60);
