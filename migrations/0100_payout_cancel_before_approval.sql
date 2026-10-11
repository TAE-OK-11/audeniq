-- 0100: 승인된 지급은 아티스트가 취소할 수 없다 (백엔드 점검).
--
-- An APPROVED order is the operator's go-ahead to transfer; the bank
-- transaction id that marks it SUBMITTED only exists after the transfer. An
-- artist cancelling in that window cancelled an order whose money was
-- already on its way: the ledger debit was never posted and the amount
-- became withdrawable again (a double payout). Artists may now cancel only
-- before approval; after that, staff cancel (finance::cancel_payout_order).
CREATE OR REPLACE FUNCTION finance.cancel_unsent_payout(p_org uuid, p_request uuid) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE r record; o record;
BEGIN
  SELECT id, status, payout_order_id INTO r FROM portal.payout_requests
   WHERE org_id = p_org AND id = p_request FOR UPDATE;
  IF NOT FOUND THEN RETURN 'NOT_FOUND'; END IF;
  IF r.status = 'CANCELLED' THEN RETURN 'CANCELLED'; END IF;
  IF r.status = 'REQUESTED' THEN
    UPDATE portal.payout_requests SET status = 'CANCELLED', cancelled_at = now() WHERE id = r.id;
    RETURN 'CANCELLED';
  END IF;
  IF r.status <> 'ORDERED' OR r.payout_order_id IS NULL THEN RETURN 'NOT_CANCELLABLE'; END IF;
  SELECT id, status INTO o FROM finance.payout_orders WHERE org_id = p_org AND id = r.payout_order_id FOR UPDATE;
  IF o.status <> 'PENDING_APPROVAL' THEN RETURN 'NOT_CANCELLABLE'; END IF;
  UPDATE finance.payout_orders SET status = 'CANCELLED', cancelled_at = now() WHERE id = o.id;
  UPDATE portal.payout_requests SET status = 'CANCELLED', cancelled_at = now() WHERE id = r.id;
  RETURN 'CANCELLED';
END $$;
REVOKE ALL ON FUNCTION finance.cancel_unsent_payout(uuid, uuid) FROM PUBLIC;
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_api') THEN
  GRANT EXECUTE ON FUNCTION finance.cancel_unsent_payout(uuid, uuid) TO audeniq_api;
 END IF;
END $$;
