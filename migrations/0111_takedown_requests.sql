-- 0111: artist takedown requests for releases already sent to platforms.
--
-- Withdrawal (0050) only covers applications nothing was sent for. Once a
-- platform has the release, the artist asks for a takedown here with a
-- reason; staff with the delivery duty approve (a takedown message to every
-- platform the latest package reached, `delivery.takedown` jobs) or decline
-- with a note. Platforms that take no takedown message are confirmed by
-- staff by hand (`delivery.mark_taken_down`). Each step notifies the org.
CREATE TABLE portal.takedown_requests (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 package_id uuid NOT NULL,
 requested_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 reason text NOT NULL CHECK (length(btrim(reason)) BETWEEN 1 AND 1000),
 status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','APPROVED','DECLINED','CANCELLED')),
 decided_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 decided_at timestamptz NULL,
 decision_note text NOT NULL DEFAULT '' CHECK (length(decision_note) <= 1000),
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK ((status = 'PENDING') = (decided_at IS NULL))
);
CREATE UNIQUE INDEX takedown_requests_one_open ON portal.takedown_requests(release_id) WHERE status = 'PENDING';
CREATE INDEX takedown_requests_org ON portal.takedown_requests(org_id, created_at DESC);
CREATE INDEX takedown_requests_open ON portal.takedown_requests(created_at) WHERE status = 'PENDING';

CREATE FUNCTION portal.takedown_request_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text;
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 SELECT coalesce(nullif(btrim(title), ''), '발매') INTO t FROM catalog.releases WHERE id = NEW.release_id;
 IF NEW.status = 'PENDING' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 중단 요청이 접수됐어요.',
   '담당자가 확인한 뒤 각 플랫폼에 중단을 요청해요.', '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'APPROVED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 중단을 플랫폼에 요청했어요.',
   '플랫폼마다 내려가는 데 며칠 걸릴 수 있어요. 진행 상황은 발매 관리에서 볼 수 있어요.', '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'DECLINED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 발매 중단 요청이 반려됐어요.',
   left(coalesce(nullif(NEW.decision_note, ''), '자세한 내용은 문의로 확인해 주세요.'), 300), '/releases/' || NEW.release_id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER takedown_requests_notice AFTER INSERT OR UPDATE OF status ON portal.takedown_requests
 FOR EACH ROW EXECUTE FUNCTION portal.takedown_request_notice();
