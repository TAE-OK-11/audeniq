-- 0149: an artist may take back a counter notice before staff decide (crates/core/src/infringement.rs
-- withdraw_counter). The claim is SUSPENDED again; the statement stays on record with this time.
ALTER TABLE portal.infringement_claims ADD COLUMN counter_withdrawn_at timestamptz NULL;
