-- Negative source adjustments are evidence, not negative ledger entries.
ALTER TABLE finance.report_lines DROP CONSTRAINT report_lines_quantity_check;
ALTER TABLE finance.report_lines DROP CONSTRAINT report_lines_gross_amount_check;
ALTER TABLE finance.report_lines DROP CONSTRAINT report_lines_match_status_check;
ALTER TABLE finance.report_lines ADD CONSTRAINT report_lines_match_status_check
 CHECK(match_status IN ('AUTO','MANUAL','UNMATCHED','AMBIGUOUS'));
ALTER TABLE finance.royalty_reports DROP CONSTRAINT royalty_reports_status_check;
ALTER TABLE finance.royalty_reports ADD CONSTRAINT royalty_reports_status_check
 CHECK(status IN ('RECEIVED','NORMALIZED','PARTIALLY_MATCHED','MATCHED','POSTED'));
-- Correct the old matcher label without altering source evidence or POSTED books.
UPDATE finance.royalty_reports r SET status='PARTIALLY_MATCHED'
 WHERE r.status='MATCHED' AND EXISTS (
   SELECT 1 FROM finance.report_lines l WHERE l.org_id=r.org_id AND l.report_id=r.id
     AND l.match_status='UNMATCHED'
 );

CREATE TABLE finance.royalty_receipts (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 report_id uuid NOT NULL,
 bank_reference text NOT NULL CHECK(length(btrim(bank_reference)) BETWEEN 1 AND 200),
 amount numeric NOT NULL CHECK(amount > 0),
 currency char(3) NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 evidence jsonb NOT NULL CHECK(jsonb_typeof(evidence)='object' AND evidence<>'{}'::jsonb),
 recorded_by uuid NOT NULL,
 recorded_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(org_id,report_id) REFERENCES finance.royalty_reports(org_id,id) ON DELETE RESTRICT,
 UNIQUE(org_id,bank_reference)
);
CREATE INDEX royalty_receipts_report ON finance.royalty_receipts(org_id,report_id);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON finance.royalty_receipts
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

ALTER TABLE finance.payout_orders ADD CONSTRAINT payout_orders_org_id_id_key UNIQUE(org_id,id);
CREATE TABLE finance.payout_returns (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 payout_order_id uuid NOT NULL,
 bank_reference text NOT NULL CHECK(length(btrim(bank_reference)) BETWEEN 1 AND 200),
 returned_amount numeric NOT NULL CHECK(returned_amount > 0),
 refunded_fee numeric NOT NULL DEFAULT 0 CHECK(refunded_fee >= 0),
 currency char(3) NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 related_transaction_id uuid NOT NULL,
 evidence jsonb NOT NULL CHECK(jsonb_typeof(evidence)='object' AND evidence<>'{}'::jsonb),
 recorded_by uuid NOT NULL,
 recorded_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(org_id,bank_reference),
 FOREIGN KEY(org_id,payout_order_id) REFERENCES finance.payout_orders(org_id,id) ON DELETE RESTRICT,
 FOREIGN KEY(org_id,related_transaction_id) REFERENCES finance.ledger_transactions(org_id,id) ON DELETE RESTRICT
);
CREATE INDEX payout_returns_order ON finance.payout_returns(org_id,payout_order_id);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON finance.payout_returns
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
-- Owner CLI only. No additional runtime-role permissions are granted here.
