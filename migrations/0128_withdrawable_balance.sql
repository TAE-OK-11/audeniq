-- 0128: 보류 금액·출금 잔액 계산 통합 (감사 6번).
--
-- The request, the staff order, the approval and the bank submission each
-- checked a different balance (or none: approval and submission never
-- re-checked holds or the balance). One definition now serves all of them:
--   payable (ROYALTY_PAYABLE) − held (finance.held_payable, 0124)
--   − every other open payout request and order.
-- `p_self` excludes the row being decided (a request, or an order and the
-- request it came from), so it is not counted against itself.
CREATE FUNCTION finance.withdrawable(p_org uuid, p_self uuid DEFAULT NULL)
RETURNS numeric
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT COALESCE((SELECT SUM(CASE WHEN e.side='CREDIT' THEN e.amount ELSE -e.amount END)
                    FROM finance.ledger_entries e
                   WHERE e.org_id=p_org AND e.account='ROYALTY_PAYABLE' AND e.currency='KRW'),0)
      - finance.held_payable(p_org)
      - COALESCE((SELECT SUM(r.amount) FROM portal.payout_requests r
                   WHERE r.org_id=p_org AND r.status='REQUESTED' AND r.currency='KRW'
                     AND r.id IS DISTINCT FROM p_self
                     AND NOT EXISTS (SELECT 1 FROM finance.payout_orders o WHERE o.org_id=p_org AND o.id=p_self AND o.payout_request_id=r.id)),0)
      - COALESCE((SELECT SUM(o.amount) FROM finance.payout_orders o
                   WHERE o.org_id=p_org AND o.currency='KRW'
                     AND o.status IN ('PENDING_APPROVAL','APPROVED','SUBMITTED','SUBMITTED_UNKNOWN')
                     AND (p_self IS NULL OR (o.id <> p_self AND o.payout_request_id IS DISTINCT FROM p_self))),0)
$$;
REVOKE ALL ON FUNCTION finance.withdrawable(uuid, uuid) FROM PUBLIC;
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_api') THEN
  GRANT EXECUTE ON FUNCTION finance.withdrawable(uuid, uuid) TO audeniq_api;
 END IF;
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
  GRANT EXECUTE ON FUNCTION finance.withdrawable(uuid, uuid) TO audeniq_worker;
 END IF;
END $$;
