-- 0095: 서명 계약 → 정산 배분율 (감사 5번).
--
-- Signing the distribution agreement now writes the release's commercial
-- split snapshots (append-only, effective from the signature): AUDENIQ's fee
-- from the signed terms, the royalty holders' shares (rights grants, 0093) of
-- the artist's portion, the owner the rest. A signed promotional rate gets
-- its own window. Settlement reads the snapshot effective at the revenue date
-- (finance::effective_split / settlement_split::split_for).
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_api') THEN
  GRANT SELECT, INSERT ON finance.commercial_split_snapshots TO audeniq_api;
 END IF;
END $$;
