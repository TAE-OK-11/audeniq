-- F2: submit idempotency key. The same key on the same release always resolves to
-- the revision created by the first submit; reusing a key with a changed body is
-- rejected so a client retry can never silently point at a different application.
ALTER TABLE catalog.application_revisions
    ADD COLUMN idempotency_key text NOT NULL DEFAULT '';
-- Backfill pre-key revisions with unique placeholders so the unique index builds
-- even on databases that already hold revisions.
UPDATE catalog.application_revisions
    SET idempotency_key = 'backfill:' || id::text
    WHERE idempotency_key = '';
CREATE UNIQUE INDEX application_revisions_idem_key
    ON catalog.application_revisions (release_id, idempotency_key);
