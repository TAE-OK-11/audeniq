-- 0091: 발매 후 수정 — 고친 제목·크레딧 등이 실제 전송 데이터에 반영되게 (감사 1번).
--
-- A delivered release could not be edited (catalog writes are limited to the
-- correction statuses) and execution::update_release rebuilt the ERN from
-- the ORIGINAL package, so nothing an artist fixed after release ever
-- reached a DSP. Now:
--   * the owner reopens a delivered release for editing
--     (READY_FOR_DELIVERY → STAGE3_CORRECTION, allowed since 0046); the
--     signed agreement stays SIGNED, identifiers stay with their targets;
--   * the resubmitted revision goes through Stage 1–3 again and staging
--     never auto-approves it: staff approve each DSP (재심사);
--   * the delivery job for a DSP that already has this release records the
--     job it supersedes and sends an ERN *Update* built from the new package,
--     threaded to the original message, through the adapter's update call.
ALTER TABLE execution.delivery_jobs
 ADD COLUMN supersedes_job_id uuid NULL REFERENCES execution.delivery_jobs(id) ON DELETE RESTRICT;

-- Self-opened edits are not staff correction requests: say what happens next.
CREATE OR REPLACE FUNCTION portal.release_status_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE
 t text := coalesce(nullif(btrim(NEW.title), ''), '발매');
 by_whom text := coalesce(current_setting('audeniq.withdraw_by', true), '');
 revising boolean := coalesce(current_setting('audeniq.post_release_edit', true), '') = '1';
 signed boolean := EXISTS (SELECT 1 FROM portal.documents d WHERE d.org_id = NEW.org_id AND d.release_id = NEW.id
                            AND d.kind = 'AGREEMENT' AND d.status = 'SIGNED');
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 IF revising AND NEW.status = 'STAGE3_CORRECTION' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 정보를 수정할 수 있어요.',
   '고친 뒤 다시 접수하면 검사와 담당자 확인을 거쳐 플랫폼에 수정 내용을 보내요.', '/releases/' || NEW.id);
 ELSIF NEW.status = 'READY_FOR_DELIVERY' AND signed THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 수정 내용이 자동 검사를 통과했어요.',
   '담당자가 확인하면 플랫폼에 수정 내용을 보내요.', '/releases/' || NEW.id);
 ELSIF NEW.status LIKE '%\_CORRECTION' THEN
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

-- The final-approval trigger (0051/0066) approves every staged row once the
-- agreement is APPROVED or SIGNED. A post-release revision keeps the signed
-- agreement, so that would send the Update without the staff re-review:
-- skip rows whose release was already delivered from another package.
-- Staff approve those per DSP (approval_by set; the trigger never touches
-- explicit decisions). delivery_jobs is FORCE RLS: scope the read to the
-- row's org and restore the caller's setting.
CREATE OR REPLACE FUNCTION distribution.auto_approve_staging() RETURNS trigger
 LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
DECLARE
 v_prev text := current_setting('app.org_id', true);
 v_revision boolean;
BEGIN
 IF NEW.approval='PENDING' AND NEW.readiness <> 'CONTENT_BLOCKED'
    AND distribution.release_finally_approved(NEW.org_id,NEW.release_id) THEN
  PERFORM set_config('app.org_id', NEW.org_id::text, true);
  v_revision := EXISTS (
   SELECT 1 FROM execution.delivery_jobs j
     JOIN distribution.distribution_packages dp ON dp.id = j.package_id
     JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
    WHERE j.org_id = NEW.org_id AND cr.release_id = NEW.release_id
      AND j.status = 'DELIVERED' AND j.package_id <> NEW.package_id);
  PERFORM set_config('app.org_id', coalesce(v_prev, ''), true);
  IF NOT v_revision THEN
   NEW.approval := 'APPROVED';
   NEW.approval_by := NULL;
   NEW.approval_rule_version := 'STAFF_FINAL';
   NEW.approval_note := '발매 최종 승인에 따른 자동 승인';
   NEW.approval_at := now();
  END IF;
 END IF;
 RETURN NEW;
END $$;
