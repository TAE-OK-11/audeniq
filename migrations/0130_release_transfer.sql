-- 0130: 유통사 이전 (crates/core/src/contract_terms.rs).
--
-- Moving a release to another distributor is a scheduled end of its
-- distribution here: on the switch date (at least 30 days ahead, like a
-- contract end) every platform gets a takedown and final settlement follows,
-- exactly as for a contract end. The term names the new distributor, and
-- the owner gets the handover data (UPC, ISRCs, each platform's own id) for
-- the new distributor to deliver the same codes.
ALTER TABLE portal.release_terms DROP CONSTRAINT release_terms_reason_check;
ALTER TABLE portal.release_terms
 ADD CONSTRAINT release_terms_reason_check
  CHECK (reason IN ('CONTRACT_END','LICENSE_EXPIRY','TERMINATION','ORG_CONTRACT_END','TRANSFER')),
 ADD COLUMN transfer_to text NULL CHECK (transfer_to IS NULL OR length(btrim(transfer_to)) BETWEEN 1 AND 200),
 ADD CONSTRAINT release_terms_transfer_names_distributor CHECK (reason <> 'TRANSFER' OR transfer_to IS NOT NULL);

-- The scheduled notice names the move.
CREATE OR REPLACE FUNCTION portal.release_term_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text; link text;
BEGIN
 IF NEW.reason = 'ORG_CONTRACT_END' AND NEW.status IN ('SCHEDULED','ENDED') THEN RETURN NEW; END IF;
 SELECT coalesce(nullif(btrim(title), ''), '발매') INTO t FROM catalog.releases WHERE id = NEW.release_id;
 link := '/releases/' || NEW.release_id;
 IF NEW.reason = 'TRANSFER' AND NEW.status = 'SCHEDULED' AND (TG_OP = 'INSERT' OR OLD.status <> 'SCHEDULED' OR OLD.end_date <> NEW.end_date) THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 유통사 이전이 ' || NEW.end_date || '로 정해졌어요.',
   left(NEW.transfer_to, 100) || '(으)로 옮겨요. 그날 모든 플랫폼에서 내려가요. 이전 자료(UPC·ISRC·플랫폼 ID)는 발매 상세에서 받을 수 있어요.', link);
 ELSIF NEW.status = 'SCHEDULED' AND (TG_OP = 'INSERT' OR OLD.status <> 'SCHEDULED' OR OLD.end_date <> NEW.end_date) THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 배급 종료가 ' || NEW.end_date || '로 정해졌어요.',
   CASE WHEN NEW.set_by_staff THEN left(coalesce(nullif(NEW.note, ''), '담당자가 정했어요.'), 300)
        ELSE '그날 모든 플랫폼에서 내려가요. 그 전까지는 발매 상세에서 취소할 수 있어요.' END, link);
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'SCHEDULED' AND OLD.warned_at IS NULL AND NEW.warned_at IS NOT NULL THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 배급이 ' || NEW.end_date || '에 끝나요.',
   '그날 모든 플랫폼에서 내려가요. 계속 배급하려면 발매 상세에서 종료를 취소해 주세요.', link);
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'ENDED' AND OLD.status <> 'ENDED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 배급이 끝났어요.',
   '모든 플랫폼에 내려 달라고 요청했어요. 남은 수익은 ' || NEW.final_settlement_due || '까지 최종 정산해요.', link);
 ELSIF TG_OP = 'UPDATE' AND NEW.status = 'FINAL_SETTLED' AND OLD.status <> 'FINAL_SETTLED' THEN
  PERFORM portal.notify(NEW.org_id, 'SETTLEMENT', t || ' 최종 정산을 마쳤어요.',
   left(coalesce(nullif(NEW.final_note, ''), '남은 금액은 정산·지급에서 확인해 주세요.'), 300), '/settlement');
 END IF;
 RETURN NEW;
END $$;
