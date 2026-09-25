-- 0015 execution already added the delivery schema; F5 also needs the frozen
-- ERN XML bytes persisted at preparation time. The hash alone (ern_sha256)
-- cannot be turned back into the transfer document, and regenerating it at
-- send time would re-run preparation logic outside its boundary. The bytes
-- are stored once, immutably, next to the hash; E-2 verifies the hash before
-- anything goes on the wire.
ALTER TABLE distribution.preparation_artifacts
    ADD COLUMN ern_xml TEXT;
