-- 0177: ask to change where and what a signed publishing contract covers.
--
-- Besides renewal and termination (0168/0173), the writer may ask to change the contract's territories and rights
-- (e.g. add Japan, drop sync). The request is kept on the contract until staff answer it: approved, the contract's
-- territories and rights become the requested ones; rejected, they stay, with a reason. Either way the artist is told.
ALTER TABLE portal.pub_contracts
 ADD COLUMN scope_territories text[] NULL,
 ADD COLUMN scope_rights text[] NULL,
 ADD COLUMN scope_requested_at timestamptz NULL,
 ADD CONSTRAINT pub_contract_scope_request CHECK (
   (scope_requested_at IS NULL) = (scope_territories IS NULL) AND (scope_territories IS NULL) = (scope_rights IS NULL));
ALTER TABLE portal.pub_contracts DROP CONSTRAINT pub_contracts_request_outcome_check;
ALTER TABLE portal.pub_contracts ADD CONSTRAINT pub_contracts_request_outcome_check
 CHECK (request_outcome IN ('renewed','renewal_rejected','ended','termination_rejected','scope_changed','scope_rejected'));

CREATE OR REPLACE FUNCTION portal.pub_contract_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE msg text;
BEGIN
 IF NEW.request_outcome IS NULL OR NEW.request_decided_at IS NOT DISTINCT FROM OLD.request_decided_at THEN RETURN NEW; END IF;
 msg := CASE NEW.request_outcome
   WHEN 'renewed' THEN '저작권 관리 계약이 연장됐어요.'
   WHEN 'renewal_rejected' THEN '저작권 관리 계약 연장 요청이 반려됐어요.'
   WHEN 'ended' THEN '저작권 관리 계약이 종료됐어요.'
   WHEN 'scope_changed' THEN '저작권 관리 범위가 바뀌었어요.'
   WHEN 'scope_rejected' THEN '저작권 관리 범위 변경 요청이 반려됐어요.'
   ELSE '저작권 관리 계약 종료 요청이 반려됐어요.' END;
 PERFORM portal.notify(NEW.org_id, 'RELEASE', msg,
   left(coalesce(NEW.request_note, CASE WHEN NEW.request_outcome = 'renewed' THEN '새 종료일은 ' || NEW.end_date::text || '이에요.'
     WHEN NEW.request_outcome = 'scope_changed' THEN '바뀐 관리 국가·권리는 계약서 화면에서 볼 수 있어요.' ELSE '계약서 화면에서 확인해 주세요.' END), 300),
   '/contracts/publishing/' || NEW.id);
 RETURN NEW;
END $$;
