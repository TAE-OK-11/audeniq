-- 0098: 발매 후 중단·계약 종료·유통사 이전 (감사 P8).
--
-- A delivered release could only be taken down by an operator calling the
-- worker job by hand: no request, no authority check, no approval, no record
-- of which platform confirmed it. Now:
--   request (artist owner, or staff on a rights holder's / authority's claim)
--   → authority check, stored with the request (`authority`)
--   → staff approval by someone other than the requester
--   → one task per platform that has the release (`release_action_tasks`),
--     each a takedown job (a transfer's run at the agreed switch date)
--   → completion evidence per platform: the platform's takedown confirmation,
--     or staff evidence (note + optional file) for platforms without a
--     takedown feed; the action completes when every task is confirmed.
-- Whether a platform actually removed the release is outside this system;
-- the task only records what the platform or the operator reported.
CREATE TABLE portal.release_actions (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 kind text NOT NULL CHECK (kind IN ('TAKEDOWN','TERMINATION','TRANSFER')),
 source text NOT NULL CHECK (source IN ('ARTIST','STAFF')),
 status text NOT NULL DEFAULT 'REQUESTED'
   CHECK (status IN ('REQUESTED','IN_PROGRESS','COMPLETED','REJECTED','CANCELLED')),
 reason text NOT NULL CHECK (length(btrim(reason)) BETWEEN 1 AND 2000),
 -- TAKEDOWN may name platforms (partner ids); NULL = every platform.
 partners text[] NULL,
 -- TRANSFER: the new distributor and the day it takes over (takedowns run then).
 transfer_to text NULL CHECK (transfer_to IS NULL OR length(btrim(transfer_to)) BETWEEN 1 AND 200),
 effective_date date NULL,
 authority jsonb NOT NULL CHECK (jsonb_typeof(authority) = 'object'),
 requested_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 decided_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 decided_at timestamptz NULL,
 decision_note text NULL CHECK (decision_note IS NULL OR length(decision_note) <= 2000),
 completed_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK (kind <> 'TRANSFER' OR (transfer_to IS NOT NULL AND effective_date IS NOT NULL)),
 CHECK (kind = 'TAKEDOWN' OR partners IS NULL),
 CHECK (decided_by IS NULL OR decided_by <> requested_by)
);
-- One open action per release at a time.
CREATE UNIQUE INDEX release_actions_open ON portal.release_actions(release_id)
 WHERE status IN ('REQUESTED','IN_PROGRESS');
CREATE INDEX release_actions_org ON portal.release_actions(org_id, created_at DESC);
CREATE INDEX release_actions_status ON portal.release_actions(status, created_at);

CREATE TABLE portal.release_action_tasks (
 id uuid PRIMARY KEY,
 action_id uuid NOT NULL REFERENCES portal.release_actions ON DELETE RESTRICT,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 package_id uuid NOT NULL REFERENCES distribution.distribution_packages(id) ON DELETE RESTRICT,
 partner_id text NOT NULL,
 -- QUEUED: takedown job waiting (or scheduled); SENT: the platform took the
 -- request; MANUAL_REQUIRED: no takedown feed, the operator must do it in the
 -- platform's own channel; FAILED: refused / unclear; CONFIRMED: removed.
 status text NOT NULL DEFAULT 'QUEUED'
   CHECK (status IN ('QUEUED','SENT','MANUAL_REQUIRED','FAILED','CONFIRMED')),
 job_id uuid NULL,
 last_error text NULL,
 evidence_note text NULL CHECK (evidence_note IS NULL OR length(evidence_note) <= 1000),
 evidence_asset_id uuid NULL,
 evidence_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 confirmed_via text NULL CHECK (confirmed_via IS NULL OR confirmed_via IN ('PARTNER','STAFF_EVIDENCE')),
 confirmed_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (action_id, partner_id),
 CHECK ((status = 'CONFIRMED') = (confirmed_at IS NOT NULL))
);
CREATE INDEX release_action_tasks_binding ON portal.release_action_tasks(package_id, partner_id) WHERE status <> 'CONFIRMED';

-- The worker reports a takedown job's outcome through this one function
-- (it has no access to the portal schema).
CREATE FUNCTION execution.release_action_task_outcome(p_task uuid, p_status text, p_error text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF p_status NOT IN ('SENT','MANUAL_REQUIRED','FAILED') THEN
  RAISE EXCEPTION 'invalid task outcome %', p_status USING ERRCODE = '22023';
 END IF;
 UPDATE portal.release_action_tasks
    SET status = p_status, last_error = left(p_error, 500), updated_at = now()
  WHERE id = p_task AND status IN ('QUEUED','SENT','FAILED','MANUAL_REQUIRED');
END $$;
REVOKE ALL ON FUNCTION execution.release_action_task_outcome(uuid, text, text) FROM PUBLIC;

-- Platform state drives the tasks: a takedown request taken → SENT; the
-- release removed → CONFIRMED (by the platform, or by staff evidence when
-- the operator recorded it); every task confirmed → the action COMPLETED.
CREATE FUNCTION portal.release_action_progress() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.live_status IS NOT DISTINCT FROM OLD.live_status THEN RETURN NEW; END IF;
 IF NEW.live_status = 'TAKEDOWN_REQUESTED' THEN
  UPDATE portal.release_action_tasks SET status = 'SENT', updated_at = now()
   WHERE package_id = NEW.package_id AND partner_id = NEW.partner_id AND status = 'QUEUED';
 ELSIF NEW.live_status = 'TAKEN_DOWN' THEN
  UPDATE portal.release_action_tasks
     SET status = 'CONFIRMED', confirmed_at = now(), updated_at = now(),
         confirmed_via = CASE WHEN evidence_by IS NOT NULL THEN 'STAFF_EVIDENCE' ELSE 'PARTNER' END
   WHERE package_id = NEW.package_id AND partner_id = NEW.partner_id AND status <> 'CONFIRMED';
  UPDATE portal.release_actions a SET status = 'COMPLETED', completed_at = now(), updated_at = now()
   WHERE a.status = 'IN_PROGRESS'
     AND EXISTS (SELECT 1 FROM portal.release_action_tasks t
                  WHERE t.action_id = a.id AND t.package_id = NEW.package_id AND t.partner_id = NEW.partner_id)
     AND NOT EXISTS (SELECT 1 FROM portal.release_action_tasks t
                      WHERE t.action_id = a.id AND t.status <> 'CONFIRMED');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER release_action_progress AFTER INSERT OR UPDATE OF live_status ON execution.live_bindings
 FOR EACH ROW EXECUTE FUNCTION portal.release_action_progress();

CREATE FUNCTION portal.release_action_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text; what text;
BEGIN
 IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 SELECT coalesce(nullif(btrim(title), ''), '발매') INTO t FROM catalog.releases WHERE org_id = NEW.org_id AND id = NEW.release_id;
 what := CASE NEW.kind WHEN 'TAKEDOWN' THEN '배급 중단' WHEN 'TERMINATION' THEN '계약 종료' ELSE '유통사 이전' END;
 IF NEW.status = 'IN_PROGRESS' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' ' || what || ' 요청이 승인됐어요.',
   CASE WHEN NEW.kind = 'TRANSFER' THEN to_char(NEW.effective_date, 'YYYY-MM-DD') || '에 플랫폼별로 내려요.'
        ELSE '플랫폼별로 내리는 중이에요. 진행 상황은 발매 상세에서 볼 수 있어요.' END,
   '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'REJECTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' ' || what || ' 요청이 반려됐어요.',
   left(coalesce(NEW.decision_note, ''), 900), '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'COMPLETED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' ' || what || '이(가) 끝났어요.',
   '모든 플랫폼에서 내려간 것을 확인했어요.', '/releases/' || NEW.release_id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER release_action_notice AFTER UPDATE OF status ON portal.release_actions
 FOR EACH ROW EXECUTE FUNCTION portal.release_action_notice();

DO $$ BEGIN
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_api') THEN
  GRANT SELECT, INSERT, UPDATE ON portal.release_actions, portal.release_action_tasks TO audeniq_api;
 END IF;
 IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
  GRANT EXECUTE ON FUNCTION execution.release_action_task_outcome(uuid, text, text) TO audeniq_worker;
 END IF;
END $$;
