-- 0097: 지급 반송·대사·수동 지급 (감사 7번).
--
-- * A transfer the bank returns after it settled could not be recorded
--   (RETURNED was only allowed before settlement) and the ledger kept the
--   payout as paid. A post-settlement return now reverses the payout posting
--   (returned_transaction_id), so the amount is withdrawable again.
-- * Bank notices are recorded once each (payout_bank_events): a repeated
--   notice is a no-op and never posts or notifies twice; a notice that
--   contradicts a final state is refused.
-- * Manual payout: staff who transferred outside a bank API settle the order
--   only with the transfer reference and an uploaded evidence file (이체확인증).
-- * Reissue: a returned or failed order can be reissued (reissue_of) to the
--   payout account registered now, through approval again. One open order per
--   payout request at a time.
-- Real bank integration (IBK / Hana) waits for the contract and keys.
ALTER TABLE finance.payout_orders
 ADD COLUMN manual boolean NOT NULL DEFAULT false,
 ADD COLUMN evidence_asset_id uuid NULL,
 ADD COLUMN reissue_of uuid NULL REFERENCES finance.payout_orders(id) ON DELETE RESTRICT,
 ADD COLUMN returned_transaction_id uuid NULL REFERENCES finance.ledger_transactions ON DELETE RESTRICT,
 ADD COLUMN returned_at timestamptz NULL,
 ADD CONSTRAINT payout_orders_manual_evidence CHECK (NOT manual OR status NOT IN ('SETTLED') OR evidence_asset_id IS NOT NULL);
ALTER TABLE finance.payout_orders DROP CONSTRAINT payout_orders_payout_request_id_key;
CREATE UNIQUE INDEX payout_orders_request_open ON finance.payout_orders(payout_request_id)
 WHERE payout_request_id IS NOT NULL AND status NOT IN ('RETURNED','FAILED','CANCELLED');

CREATE TABLE finance.payout_bank_events (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 order_id uuid NOT NULL REFERENCES finance.payout_orders(id) ON DELETE RESTRICT,
 outcome text NOT NULL CHECK (outcome IN ('SETTLED','FAILED','RETURNED','SUBMITTED_UNKNOWN')),
 bank_transaction_id text NOT NULL DEFAULT '',
 payload jsonb NOT NULL DEFAULT '{}',
 recorded_by uuid NULL,
 received_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (order_id, outcome, bank_transaction_id)
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON finance.payout_bank_events
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
  GRANT SELECT, INSERT ON finance.payout_bank_events TO audeniq_worker;
 END IF;
END $$;

-- A return after the money arrived reads differently from a failed transfer.
CREATE OR REPLACE FUNCTION portal.payout_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.status = 'SETTLED' THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', '수익 지급이 완료됐어요.',
   to_char(NEW.amount, 'FM999,999,999,990') || ' ' || NEW.currency || ' · 등록한 계좌로 보냈어요.', '/settlement');
 ELSIF NEW.status = 'RETURNED' AND OLD.status = 'SETTLED' THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', '보낸 수익이 은행에서 반송됐어요.',
   to_char(NEW.amount, 'FM999,999,999,990') || ' ' || NEW.currency || ' · 잔액으로 돌아왔어요. 수령 계좌를 확인해 주시면 다시 보내 드려요.', '/settlement');
 ELSIF NEW.status IN ('FAILED','RETURNED') THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', '수익 지급이 완료되지 않았어요.',
   '수령 계좌 정보를 확인한 뒤 다시 요청해 주세요.', '/settlement');
 END IF;
 RETURN NEW;
END $$;
