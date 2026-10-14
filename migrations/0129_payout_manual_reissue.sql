-- 0129: 수동 지급·재지급.
--
-- * Manual payout: staff who transferred outside a bank API settle the order
--   only with the transfer reference and evidence of the transfer
--   (이체확인증, recorded like a payout return's evidence, 0106). The person
--   who approved the order does not record the transfer.
-- * Reissue: a returned or failed order can be reissued (reissue_of) to the
--   payout account registered now, through approval again. One open order
--   per payout request at a time; each order is reissued at most once.
ALTER TABLE finance.payout_orders
 ADD COLUMN manual_evidence jsonb NULL CHECK (manual_evidence IS NULL OR (jsonb_typeof(manual_evidence)='object' AND manual_evidence<>'{}'::jsonb)),
 ADD COLUMN reissue_of uuid NULL REFERENCES finance.payout_orders(id) ON DELETE RESTRICT,
 ADD CONSTRAINT payout_orders_reissue_once UNIQUE (reissue_of);
ALTER TABLE finance.payout_orders DROP CONSTRAINT payout_orders_payout_request_id_key;
CREATE UNIQUE INDEX payout_orders_request_open ON finance.payout_orders(payout_request_id)
 WHERE payout_request_id IS NOT NULL AND status NOT IN ('RETURNED','FAILED','CANCELLED');
