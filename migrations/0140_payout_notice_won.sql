-- 0140: 지급 완료 알림의 금액을 통화 코드 대신 사람이 읽는 표기로 ("24,406 KRW" → "24,406원").
CREATE OR REPLACE FUNCTION portal.payout_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.status = 'SETTLED' THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', '수익 지급이 완료됐어요.',
   CASE WHEN NEW.currency = 'KRW'
        THEN to_char(NEW.amount, 'FM999,999,999,990') || '원을 등록한 계좌로 보냈어요.'
        ELSE to_char(NEW.amount, 'FM999,999,999,990.00') || ' ' || NEW.currency || '를 등록한 계좌로 보냈어요.' END,
   '/settlement');
 ELSIF NEW.status IN ('FAILED','RETURNED') THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', '수익 지급이 완료되지 않았어요.',
   '수령 계좌 정보를 확인한 뒤 다시 요청해 주세요.', '/settlement');
 END IF;
 RETURN NEW;
END $$;
