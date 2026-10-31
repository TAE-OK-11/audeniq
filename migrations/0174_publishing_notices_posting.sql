-- 0174: tell artists what staff did in publishing, and post publishing royalties to their balance.
--
-- Notices (portal.notify, as the work registration notice of 0167): a share change applied or rejected, a contract
-- renewal / termination answered, an Open A&R fix asked / reviewed / proposing, royalties arrived from a statement,
-- royalties credited to the balance.
--
-- Statements may be in another currency: amounts are converted to whole won at import with the staff-given rate
-- (kept on the statement). A confirmed statement is posted to the books from the staff console (owner-level login
-- audeniq_staff_ops, step-up): per org DEBIT ROYALTY_RECEIVED, CREDIT AUDENIQ_FEE and ROYALTY_PAYABLE; its rows become
-- paid. Posting is once (posted_at).
ALTER TABLE portal.pub_royalty_statements
 ADD COLUMN currency text NOT NULL DEFAULT 'KRW' CHECK (currency ~ '^[A-Z]{3}$'),
 ADD COLUMN fx_rate numeric(18,6) NULL CHECK (fx_rate > 0),
 ADD COLUMN posted_at timestamptz NULL,
 ADD COLUMN posted_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT;
-- 0173 let staff mark a statement paid without posting it; paid now means posted, so those go back to confirmed.
UPDATE portal.pub_royalties SET status='confirmed', paid_at=NULL WHERE status='paid'
 AND statement_id IN (SELECT id FROM portal.pub_royalty_statements WHERE status='paid');
UPDATE portal.pub_royalty_statements SET status='confirmed' WHERE status='paid';
ALTER TABLE portal.pub_royalty_statements
 ADD CONSTRAINT pub_statement_fx CHECK ((currency = 'KRW') = (fx_rate IS NULL)),
 ADD CONSTRAINT pub_statement_posted CHECK ((posted_at IS NULL) = (posted_by IS NULL)),
 ADD CONSTRAINT pub_statement_paid_posted CHECK ((status = 'paid') = (posted_at IS NOT NULL));

CREATE FUNCTION portal.pub_change_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text := '‘' || left(NEW.title, 80) || '’';
BEGIN
 IF NEW.change_outcome IS NULL OR NEW.change_outcome IS NOT DISTINCT FROM OLD.change_outcome AND NEW.change_decided_at IS NOT DISTINCT FROM OLD.change_decided_at THEN
  RETURN NEW;
 END IF;
 IF NEW.change_outcome = 'applied' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 지분 변경이 반영됐어요.', '지분이 바뀐 공동 작가에게는 다시 확인을 받아요.', '/works/' || NEW.id);
 ELSE
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 지분 변경이 반려됐어요.', left(coalesce(NEW.change_note, '곡 화면에서 사유를 확인해 주세요.'), 300), '/works/' || NEW.id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER pub_works_change_notice AFTER UPDATE OF change_outcome, change_decided_at ON portal.pub_works
 FOR EACH ROW EXECUTE FUNCTION portal.pub_change_notice();

CREATE FUNCTION portal.pub_contract_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE msg text;
BEGIN
 IF NEW.request_outcome IS NULL OR NEW.request_decided_at IS NOT DISTINCT FROM OLD.request_decided_at THEN RETURN NEW; END IF;
 msg := CASE NEW.request_outcome
   WHEN 'renewed' THEN '저작권 관리 계약이 연장됐어요.'
   WHEN 'renewal_rejected' THEN '저작권 관리 계약 연장 요청이 반려됐어요.'
   WHEN 'ended' THEN '저작권 관리 계약이 종료됐어요.'
   ELSE '저작권 관리 계약 종료 요청이 반려됐어요.' END;
 PERFORM portal.notify(NEW.org_id, 'RELEASE', msg,
   left(coalesce(NEW.request_note, CASE WHEN NEW.request_outcome = 'renewed' THEN '새 종료일은 ' || NEW.end_date::text || '이에요.' ELSE '계약서 화면에서 확인해 주세요.' END), 300),
   '/contracts/publishing/' || NEW.id);
 RETURN NEW;
END $$;
CREATE TRIGGER pub_contracts_request_notice AFTER UPDATE OF request_outcome, request_decided_at ON portal.pub_contracts
 FOR EACH ROW EXECUTE FUNCTION portal.pub_contract_notice();

CREATE FUNCTION portal.pub_ar_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text;
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status OR NEW.status NOT IN ('needs_fix','reviewed','proposing') THEN RETURN NEW; END IF;
 SELECT '‘' || left(w.title, 80) || '’' INTO t FROM portal.pub_works w WHERE w.id = NEW.work_id;
 IF NEW.status = 'needs_fix' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' A&R 보완 요청이 있어요.', left(coalesce(NEW.fix_reason, ''), 300), '/ar');
 ELSIF NEW.status = 'reviewed' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' A&R 심사가 끝났어요.', '승인된 곡은 지정된 관계자에게 소개돼요.', '/ar');
 ELSE
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '에 관심 있는 곳이 있어요.', '담당자가 제안 내용을 정리해 알려 드려요.', '/ar');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER pub_ar_status_notice AFTER UPDATE OF status ON portal.pub_ar_submissions
 FOR EACH ROW EXECUTE FUNCTION portal.pub_ar_notice();

-- Royalties: one notice per org per statement — when imported, and when credited to the balance.
CREATE FUNCTION portal.pub_royalty_import_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE r record;
BEGIN
 FOR r IN SELECT org_id, min(source) AS source, min(period) AS period, sum(net) AS net, bool_and(status = 'estimated') AS estimated
            FROM new_rows GROUP BY org_id, statement_id LOOP
  PERFORM portal.notify(r.org_id, 'SETTLEMENT',
    CASE WHEN r.estimated THEN '새 저작권료 예상 내역이 들어왔어요.' ELSE '새 저작권료가 확정됐어요.' END,
    r.source || ' · ' || r.period || ' 사용분 · ' || to_char(r.net, 'FM999,999,999,990') || '원', '/reports?type=publishing');
 END LOOP;
 RETURN NULL;
END $$;
CREATE TRIGGER pub_royalties_import_notice AFTER INSERT ON portal.pub_royalties
 REFERENCING NEW TABLE AS new_rows FOR EACH STATEMENT EXECUTE FUNCTION portal.pub_royalty_import_notice();

CREATE FUNCTION portal.pub_royalty_paid_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE r record;
BEGIN
 FOR r IN SELECT n.org_id, min(n.source) AS source, min(n.period) AS period, sum(n.net) AS net
            FROM new_rows n JOIN old_rows o ON o.id = n.id
           WHERE n.status = 'paid' AND o.status <> 'paid' GROUP BY n.org_id, n.statement_id LOOP
  PERFORM portal.notify(r.org_id, 'SETTLEMENT', '저작권료가 정산 잔액에 쌓였어요.',
    r.source || ' · ' || r.period || ' 사용분 · ' || to_char(r.net, 'FM999,999,999,990') || '원 — 배급 수익과 함께 지급을 요청할 수 있어요.', '/settlement');
 END LOOP;
 RETURN NULL;
END $$;
CREATE TRIGGER pub_royalties_paid_notice AFTER UPDATE ON portal.pub_royalties
 REFERENCING NEW TABLE AS new_rows OLD TABLE AS old_rows FOR EACH STATEMENT EXECUTE FUNCTION portal.pub_royalty_paid_notice();
