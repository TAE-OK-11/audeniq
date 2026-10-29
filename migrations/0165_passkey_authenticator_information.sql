-- Display hints only: never evidence of attestation trust, MFA or authorization.
-- Browser transports use TEXT[]; NULL means unreported, '{}' means reported empty.
-- Existing credentials stay NULL; do not infer metadata from older hints.
ALTER TABLE identity.passkeys
 ADD COLUMN authenticator_attachment text NULL
   CHECK (authenticator_attachment IN ('platform', 'cross-platform')),
 ADD COLUMN transports text[] NULL
   CHECK (transports <@ ARRAY['usb', 'nfc', 'ble', 'hybrid', 'internal']::text[]
          AND array_position(transports, NULL) IS NULL),
 ADD COLUMN backup_eligible boolean NULL;
