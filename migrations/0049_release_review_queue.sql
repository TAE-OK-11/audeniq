-- 0049: the release application is decided in 발매 심사, not 서류 검토.
--
-- A release whose automatic checks all pass reaches READY_FOR_DELIVERY with
-- its AGREEMENT (the signed release application) still waiting on staff.
-- That agreement used to show up only in the document queue, so new release
-- requests never reached the release review queue. Staff now approve,
-- correct or reject the release there (staff::decide); the document queue
-- keeps only the rights proofs staff asked for.
--
-- - REJECT of a READY_FOR_DELIVERY release closes it (WITHDRAWN). Nothing
--   has been sent: delivery waits for the SIGNED agreement (0048).
-- - A rejected agreement becomes REJECTED and can never be signed.
-- - Notices: READY_FOR_DELIVERY no longer claims staff review is done;
--   a rejection from READY_FOR_DELIVERY is announced like one from review.
INSERT INTO operations.allowed_transitions VALUES ('application_pipeline_status','READY_FOR_DELIVERY','WITHDRAWN')
 ON CONFLICT DO NOTHING;

ALTER TABLE portal.documents DROP CONSTRAINT documents_status_check;
ALTER TABLE portal.documents ADD CONSTRAINT documents_status_check
 CHECK (status IN ('AWAITING_DOCUMENTS','REVIEW','PREPARED','APPROVED','NEEDS','SIGNED','REJECTED'));

CREATE OR REPLACE FUNCTION portal.release_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text := coalesce(nullif(btrim(NEW.title), ''), '발매');
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
 ELSIF NEW.status = 'WITHDRAWN' AND OLD.status IN ('STAGE2_REVIEW','READY_FOR_DELIVERY') THEN
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

CREATE OR REPLACE FUNCTION portal.document_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.kind = 'AGREEMENT' AND NEW.status = 'APPROVED' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '발매 심사가 끝났어요.',
   left(NEW.title, 150) || ' · 내용을 확인하고 서명해 주세요.', '/contracts');
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'AWAITING_DOCUMENTS' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '제출할 서류가 있어요.', left(NEW.title, 150), '/rights');
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'NEEDS' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '서류 보완 요청이 있어요.',
   left(coalesce(nullif(NEW.review_note, ''), NEW.title), 300), '/rights');
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'APPROVED' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '제출한 서류가 승인됐어요.', left(NEW.title, 150), '/rights');
 END IF;
 RETURN NEW;
END $$;
