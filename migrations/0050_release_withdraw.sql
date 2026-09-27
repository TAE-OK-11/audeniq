-- 0050: artists can cancel (withdraw) their own release application.
--
-- - Cancelling is allowed while nothing can have been sent: correction
--   states, staff review, rights hold and READY_FOR_DELIVERY before the
--   agreement is signed (delivery waits for the signature, 0048). Running
--   pipeline states are refused by the API ("잠시 후 다시").
-- - The agreement of a cancelled release becomes CANCELLED (never signable).
-- - Three artist cancellations per organisation per calendar month (KST)
--   are counted from `release.withdrawn` audit rows; beyond that the artist
--   opens an inquiry and staff cancel it (not counted).
-- - The API marks who withdrew with the transaction-local setting
--   `audeniq.withdraw_by` (ARTIST | STAFF) so the notice reads right.
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE1_CORRECTION','WITHDRAWN')
 ON CONFLICT DO NOTHING;
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE2_CORRECTION','WITHDRAWN')
 ON CONFLICT DO NOTHING;
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','STAGE3_CORRECTION','WITHDRAWN')
 ON CONFLICT DO NOTHING;
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','ON_HOLD_RIGHTS','WITHDRAWN')
 ON CONFLICT DO NOTHING;

ALTER TABLE portal.documents DROP CONSTRAINT documents_status_check;
ALTER TABLE portal.documents ADD CONSTRAINT documents_status_check
 CHECK (status IN ('AWAITING_DOCUMENTS','REVIEW','PREPARED','APPROVED','NEEDS','SIGNED','REJECTED','CANCELLED'));

CREATE OR REPLACE FUNCTION portal.release_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE
 t text := coalesce(nullif(btrim(NEW.title), ''), '발매');
 by_whom text := coalesce(current_setting('audeniq.withdraw_by', true), '');
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.status LIKE '%\_CORRECTION' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '에 보완 요청이 있어요.',
   '발매 관리에서 ‘보완하기’를 누르면 고칠 곳으로 바로 이동해요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'SUBMITTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 신청이 접수됐어요.',
   '담당자 검토가 시작됐어요. 진행 상황은 발매 관리에서 볼 수 있어요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'ON_HOLD_RIGHTS' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '의 권리 확인이 필요해요.',
   '권리·보완 서류에서 요청된 증빙을 제출해 주세요.', '/rights');
 ELSIF NEW.status = 'READY_FOR_DELIVERY' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 자동 검사를 통과했어요.',
   '담당자 심사가 끝나면 계약서 서명 안내를 드려요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'WITHDRAWN' AND by_whom = 'ARTIST' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 신청을 취소했어요.',
   '취소한 발매는 다시 접수할 수 없어요. 새 발매로 신청해 주세요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'WITHDRAWN' AND by_whom = 'STAFF' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 취소 요청이 처리됐어요.',
   '요청하신 대로 발매 신청을 취소했어요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'WITHDRAWN' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 신청이 반려됐어요.',
   '검토 의견은 발매 상세에서 볼 수 있어요. 궁금한 점은 문의로 남겨 주세요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'LIVE' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '이(가) 발매됐어요.',
   '플랫폼에 공개됐어요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'TAKEN_DOWN' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '이(가) 플랫폼에서 내려갔어요.',
   '자세한 내용은 문의로 확인해 주세요.', '/releases/' || NEW.id);
 END IF;
 RETURN NEW;
END $$;
