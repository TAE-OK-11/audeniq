-- 0159: 최고 관리자(OWNER)가 관리자 화면에서 정산금을 직접 넣는다.
--
-- OWNER is granted only with the CLI (`audeniq-admin staff grant EMAIL OWNER`,
-- docs/STAFF_LEVELS.md). The console operation `balance.credit`
-- (crate::staff_ops) calls this function on the staff operations login; the
-- function re-checks in the database that the actor is an active OWNER, so
-- no lower level can post money even through that login.
--
-- One balanced posting, never an edit:
--   MANUAL_CREDIT debit / ROYALTY_PAYABLE credit
-- under the same per-org lock as payouts and add-on charges. The
-- description is what the artist sees in the settlement statement; the
-- internal reason and the operator stay in source_ref and the audit log.
CREATE FUNCTION finance.staff_credit_balance(p_actor uuid, p_org uuid, p_amount numeric, p_title text, p_reason text)
RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE tx uuid;
BEGIN
  IF NOT EXISTS (SELECT 1 FROM identity.staff_members s JOIN identity.users u ON u.id = s.user_id
                  WHERE s.user_id = p_actor AND s.role = 'OWNER' AND s.status = 'ACTIVE' AND u.status = 'ACTIVE') THEN
    RAISE EXCEPTION 'STAFF_OWNER_REQUIRED' USING ERRCODE = '42501';
  END IF;
  IF p_amount IS NULL OR p_amount <= 0 OR p_amount > 10000000 OR p_amount <> trunc(p_amount) THEN
    RAISE EXCEPTION 'BALANCE_AMOUNT_INVALID' USING ERRCODE = '22023';
  END IF;
  IF length(btrim(coalesce(p_title, ''))) NOT BETWEEN 1 AND 100
     OR length(btrim(coalesce(p_reason, ''))) NOT BETWEEN 1 AND 500 THEN
    RAISE EXCEPTION 'BALANCE_REASON_INVALID' USING ERRCODE = '22023';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM identity.orgs WHERE id = p_org) THEN
    RAISE EXCEPTION 'ORG_NOT_FOUND' USING ERRCODE = 'P0002';
  END IF;
  PERFORM pg_advisory_xact_lock(hashtextextended(p_org::text, 7));
  tx := gen_random_uuid();
  INSERT INTO finance.ledger_transactions(id, org_id, transaction_code, currency, source_ref, description, created_by)
    VALUES (tx, p_org, 'MANUAL_CREDIT', 'KRW',
            jsonb_build_object('staff_credit', true, 'reason', btrim(p_reason)), btrim(p_title), p_actor);
  INSERT INTO finance.ledger_entries(id, org_id, transaction_id, account, side, amount, currency) VALUES
    (gen_random_uuid(), p_org, tx, 'MANUAL_CREDIT', 'DEBIT', p_amount, 'KRW'),
    (gen_random_uuid(), p_org, tx, 'ROYALTY_PAYABLE', 'CREDIT', p_amount, 'KRW');
  PERFORM portal.notify(p_org, 'SETTLEMENT', '정산금이 들어왔어요.',
    left(btrim(p_title) || ' · ' || to_char(p_amount, 'FM999,999,999') || '원이 정산금에 더해졌어요.', 300), '/settlement');
  RETURN tx;
END $$;
REVOKE ALL ON FUNCTION finance.staff_credit_balance(uuid, uuid, numeric, text, text) FROM PUBLIC;
DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_staff_ops') THEN
  GRANT EXECUTE ON FUNCTION finance.staff_credit_balance(uuid, uuid, numeric, text, text) TO audeniq_staff_ops;
 END IF;
END $$;
