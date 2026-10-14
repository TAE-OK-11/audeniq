-- 0089: 서류 알림이 해당 서류를 바로 연다 (/rights?doc=<id>, /contracts?doc=<id>).
--
-- A staff document request (RIGHTS_PROOF AWAITING_DOCUMENTS/NEEDS) used to
-- link to the /rights list only, and the release card's 보완하기 opened the
-- application wizard, so artists could not find where to attach the file.
-- The approved-agreement notice likewise opened only the list. Same trigger
-- as 0049; only the links now name the document so Studio opens it.
CREATE OR REPLACE FUNCTION portal.document_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF NEW.kind = 'AGREEMENT' AND NEW.status = 'APPROVED' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '발매 심사가 끝났어요.',
   left(NEW.title, 150) || ' · 내용을 확인하고 서명해 주세요.', '/contracts?doc=' || NEW.id::text);
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'AWAITING_DOCUMENTS' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '제출할 서류가 있어요.', left(NEW.title, 150), '/rights?doc=' || NEW.id::text);
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'NEEDS' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '서류 보완 요청이 있어요.',
   left(coalesce(nullif(NEW.review_note, ''), NEW.title), 300), '/rights?doc=' || NEW.id::text);
 ELSIF NEW.kind = 'RIGHTS_PROOF' AND NEW.status = 'APPROVED' THEN
  PERFORM portal.notify(NEW.org_id, 'DOCUMENT', '제출한 서류가 승인됐어요.', left(NEW.title, 150), '/rights');
 END IF;
 RETURN NEW;
END $$;
