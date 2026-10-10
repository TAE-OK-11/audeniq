-- 0085: 출금 정책·계좌 고정·지급 처리 (법률·운영 감사 2026-10-10 C01·C02·C04·C14·C15·C17).
--
-- - 출금 신청에 정책 버전·수수료·실수령액과 신청 시점의 수령 계좌(암호문 사본 + 마스킹 정보)를
--   고정한다. 계좌를 나중에 바꿔도 이미 신청한 지급은 신청 때 계좌로 간다.
-- - 신청 → 지급 주문 전환은 한 거래에서 이뤄지고(finance::order_payout_request) 주문은
--   신청 한 건에만 묶인다. 같은 은행 거래번호로 두 주문을 완료할 수 없다.
-- - 지급 완료(SETTLED)는 같은 거래에서 원장 차감 분개를 남긴다(related_transaction_id).
-- - 송금 실행 전 취소: 신청 상태는 API가, 주문(승인 대기·승인) 상태는 아래 함수가 처리한다.
ALTER TABLE portal.payout_requests
 ADD COLUMN fee_amount numeric NULL CHECK(fee_amount IS NULL OR fee_amount >= 0),
 ADD COLUMN net_amount numeric NULL CHECK(net_amount IS NULL OR net_amount > 0),
 ADD COLUMN policy_version text NULL CHECK(policy_version IS NULL OR length(policy_version) BETWEEN 1 AND 80),
 ADD COLUMN account_snapshot jsonb NULL,
 ADD COLUMN account_cipher bytea NULL CHECK(account_cipher IS NULL OR octet_length(account_cipher) BETWEEN 29 AND 128),
 ADD COLUMN account_key_version smallint NULL,
 ADD COLUMN cancelled_at timestamptz NULL,
 ADD CONSTRAINT payout_requests_net CHECK(net_amount IS NULL OR fee_amount IS NULL OR net_amount + fee_amount = amount);

ALTER TABLE finance.payout_orders
 ADD COLUMN payout_request_id uuid NULL UNIQUE REFERENCES portal.payout_requests ON DELETE RESTRICT,
 ADD COLUMN fee_amount numeric NULL CHECK(fee_amount IS NULL OR fee_amount >= 0),
 ADD COLUMN net_amount numeric NULL CHECK(net_amount IS NULL OR net_amount > 0),
 ADD COLUMN fee_policy_version text NULL,
 ADD COLUMN account_snapshot jsonb NULL,
 ADD COLUMN account_cipher bytea NULL CHECK(account_cipher IS NULL OR octet_length(account_cipher) BETWEEN 29 AND 128),
 ADD COLUMN account_key_version smallint NULL,
 ADD COLUMN cancelled_at timestamptz NULL,
 ADD CONSTRAINT payout_orders_net CHECK(net_amount IS NULL OR fee_amount IS NULL OR net_amount + fee_amount = amount);
CREATE UNIQUE INDEX payout_orders_bank_transaction
  ON finance.payout_orders(org_id, bank_transaction_id) WHERE bank_transaction_id IS NOT NULL;

-- 아티스트가 송금 실행 전(승인 대기·승인) 주문을 취소한다. API 역할에 주문 상태를 바꾸는
-- 일반 UPDATE 권한을 주지 않고, 이 전이 하나만 허용한다.
CREATE FUNCTION finance.cancel_unsent_payout(p_org uuid, p_request uuid) RETURNS text
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
  IF o.status NOT IN ('PENDING_APPROVAL', 'APPROVED') THEN RETURN 'NOT_CANCELLABLE'; END IF;
  UPDATE finance.payout_orders SET status = 'CANCELLED', cancelled_at = now() WHERE id = o.id;
  UPDATE portal.payout_requests SET status = 'CANCELLED', cancelled_at = now() WHERE id = r.id;
  RETURN 'CANCELLED';
END $$;
REVOKE ALL ON FUNCTION finance.cancel_unsent_payout(uuid, uuid) FROM PUBLIC;
