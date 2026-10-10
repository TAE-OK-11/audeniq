-- Normalization remains mandatory while cloud KMS and antivirus are opt-in.
-- Keep prior actually-scanned receipts valid; never label skipped AV as scanned.
ALTER TABLE catalog.asset_safety ADD COLUMN antivirus_status text NOT NULL
 DEFAULT 'SCANNED' CHECK(antivirus_status IN ('SCANNED','SKIPPED'));
ALTER TABLE catalog.inline_file_safety ADD COLUMN antivirus_status text NOT NULL
 DEFAULT 'SCANNED' CHECK(antivirus_status IN ('SCANNED','SKIPPED'));
ALTER TABLE catalog.inline_file_safety DROP CONSTRAINT inline_file_safety_pkey;
ALTER TABLE catalog.inline_file_safety ADD PRIMARY KEY(source_sha256,antivirus_status);
