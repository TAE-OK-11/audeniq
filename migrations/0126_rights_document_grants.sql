-- 0126: 권리 서류 승인 → 실제 권한.
--
-- Approving a rights proof only changed the document status; nothing told
-- delivery or settlement what was licensed. Staff now record, with the
-- approval, the grants the document proves (staff::review_document):
--   DISTRIBUTION — the rights holder (grantor) lets the release owner
--                  (party_id) distribute: territories, uses, period;
--   ROYALTY      — the rights holder (party_id) receives share_bps of the
--                  covered works' royalties: the settlement entitlement,
--                  kept apart from the distribution permission.
-- Each row names the work (release or track), the document it came from,
-- and its period. The 0020 trigger bumps the release's rights epoch.
ALTER TABLE rights.grant_atoms
 ADD COLUMN grant_class text NOT NULL DEFAULT 'DISTRIBUTION' CHECK (grant_class IN ('DISTRIBUTION','ROYALTY')),
 ADD COLUMN grantor_party_id uuid NULL,
 ADD COLUMN share_bps integer NULL CHECK (share_bps IS NULL OR share_bps BETWEEN 1 AND 10000),
 ADD COLUMN source_document_id uuid NULL REFERENCES portal.documents(id) ON DELETE RESTRICT,
 ADD CONSTRAINT grant_atoms_grantor_tenant FOREIGN KEY (org_id, grantor_party_id) REFERENCES identity.parties(org_id, id) ON DELETE RESTRICT,
 ADD CONSTRAINT grant_atoms_royalty_share CHECK ((grant_class = 'ROYALTY') = (share_bps IS NOT NULL));
CREATE INDEX grant_atoms_document ON rights.grant_atoms(source_document_id) WHERE source_document_id IS NOT NULL;

DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_api') THEN
  GRANT SELECT, INSERT ON rights.grant_atoms TO audeniq_api;
 END IF;
END $$;

-- A rights re-check started by staff (approved grants moved the epoch) is
-- not a correction request: the artist has nothing to fix.
CREATE OR REPLACE FUNCTION portal.release_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE
 t text := coalesce(nullif(btrim(NEW.title), ''), '발매');
 by_whom text := coalesce(current_setting('audeniq.withdraw_by', true), '');
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF coalesce(current_setting('audeniq.rights_recheck', true), '') = '1' THEN RETURN NEW; END IF;
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
