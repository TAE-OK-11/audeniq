-- 0076: 관리자 업무(Work) 모듈 — 범용 업무(Task) · SLA · 예외 처리 · 내부 메모 · 업무함 읽음 ·
-- 거래처(CRM Lite) · DSP 운영 정보. API: crates/core/src/work.rs (`/api/staff/work/*`).
--
-- Staff-only data. No tenant RLS here: every read and write goes through the
-- staff API, which checks an ACTIVE identity.staff_members row first and
-- audits mutations. Customers never see any of these tables.
--
-- The exception queue is fed by the automation itself: triggers on delivery
-- jobs (FAILED / DEAD_LETTER), partner acks (REJECTED), delivery staging
-- (CONTENT_BLOCKED) and rights-proof documents (NEEDS) open or bump an
-- exception, and the matching success clears it. Normal runs never reach a
-- person.
CREATE SCHEMA IF NOT EXISTS work;

-- ---------------------------------------------------------------------------
-- SLA policies — one per task type
-- ---------------------------------------------------------------------------
CREATE TABLE work.sla_policies (
 type text PRIMARY KEY CHECK (type IN ('URGENT_DELIVERY','RELEASE','RIGHTS','CONTRACT','DSP_INQUIRY','SETTLEMENT','LEGAL','SUPPORT','GENERAL')),
 target_minutes int NOT NULL CHECK (target_minutes BETWEEN 15 AND 100000),
 business_hours boolean NOT NULL DEFAULT true,
 warn_ratio real NOT NULL DEFAULT 0.25 CHECK (warn_ratio > 0 AND warn_ratio < 1),
 idle_alert_minutes int NOT NULL DEFAULT 240 CHECK (idle_alert_minutes BETWEEN 10 AND 100000),
 escalate_to text NOT NULL DEFAULT 'ADMIN' CHECK (escalate_to IN ('ADMIN','REVIEWER','OPERATOR','SUPPORT')),
 auto_raise_priority boolean NOT NULL DEFAULT true,
 enabled boolean NOT NULL DEFAULT true,
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now()
);
-- 예: 긴급 송출 2시간, 권리 검토 1영업일(9시간)
INSERT INTO work.sla_policies(type,target_minutes,business_hours,idle_alert_minutes,escalate_to) VALUES
 ('URGENT_DELIVERY',120,false,30,'OPERATOR'),
 ('RIGHTS',540,true,120,'REVIEWER'),
 ('RELEASE',1080,true,240,'REVIEWER'),
 ('DSP_INQUIRY',540,true,120,'OPERATOR'),
 ('SUPPORT',540,true,60,'SUPPORT'),
 ('CONTRACT',1620,true,540,'ADMIN'),
 ('SETTLEMENT',1620,true,540,'ADMIN'),
 ('LEGAL',2700,true,540,'ADMIN'),
 ('GENERAL',2700,true,1080,'ADMIN')
ON CONFLICT (type) DO NOTHING;

-- ---------------------------------------------------------------------------
-- Tasks
-- ---------------------------------------------------------------------------
CREATE SEQUENCE work.task_seq START 1001;
CREATE TABLE work.tasks (
 id uuid PRIMARY KEY,
 seq bigint NOT NULL UNIQUE DEFAULT nextval('work.task_seq'),
 title text NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 200),
 description text NOT NULL DEFAULT '' CHECK (length(description) <= 8000),
 type text NOT NULL REFERENCES work.sla_policies(type) ON DELETE RESTRICT,
 status text NOT NULL DEFAULT 'TODO' CHECK (status IN ('TODO','IN_PROGRESS','IN_REVIEW','BLOCKED','DONE','CANCELLED')),
 -- chosen priority; the effective one may be raised by SLA (priority_raised)
 priority text NOT NULL DEFAULT 'NORMAL' CHECK (priority IN ('URGENT','HIGH','NORMAL','LOW')),
 priority_raised text NULL CHECK (priority_raised IN ('URGENT','HIGH','NORMAL','LOW')),
 assignee_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 reviewer_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 due_at timestamptz NULL,
 sla_due_at timestamptz NULL,
 escalation_level smallint NOT NULL DEFAULT 0 CHECK (escalation_level BETWEEN 0 AND 2),
 changes_requested boolean NOT NULL DEFAULT false,
 recurrence jsonb NULL CHECK (recurrence IS NULL OR (
   recurrence->>'freq' IN ('DAILY','WEEKLY','MONTHLY')
   AND (recurrence->>'interval')::int BETWEEN 1 AND 12)),
 related jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(related) = 'array' AND jsonb_array_length(related) <= 20),
 tags text[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 8),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 assigned_at timestamptz NULL,
 started_at timestamptz NULL,
 completed_at timestamptz NULL,
 recurred_from uuid NULL REFERENCES work.tasks ON DELETE SET NULL,
 row_version int NOT NULL DEFAULT 1,
 CHECK (status NOT IN ('DONE','CANCELLED') OR completed_at IS NOT NULL)
);
CREATE INDEX tasks_open_assignee ON work.tasks(assignee_id) WHERE status NOT IN ('DONE','CANCELLED');
CREATE INDEX tasks_open_reviewer ON work.tasks(reviewer_id) WHERE status = 'IN_REVIEW';
CREATE INDEX tasks_open_due ON work.tasks(LEAST(due_at, sla_due_at)) WHERE status NOT IN ('DONE','CANCELLED');
CREATE INDEX tasks_related ON work.tasks USING gin (related jsonb_path_ops);
CREATE INDEX tasks_completed ON work.tasks(completed_at) WHERE completed_at IS NOT NULL;

CREATE TABLE work.task_events (
 id uuid PRIMARY KEY,
 task_id uuid NOT NULL REFERENCES work.tasks ON DELETE CASCADE,
 actor_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 action text NOT NULL CHECK (length(action) BETWEEN 1 AND 40),
 detail text NULL CHECK (detail IS NULL OR length(detail) <= 2000),
 at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX task_events_task ON work.task_events(task_id, at);
CREATE INDEX task_events_at ON work.task_events(at);

-- ---------------------------------------------------------------------------
-- Internal notes (@mention, attachments, resolved)
-- ---------------------------------------------------------------------------
CREATE TABLE work.notes (
 id uuid PRIMARY KEY,
 target_kind text NOT NULL CHECK (target_kind IN ('release','org','contact','dsp','inquiry','document','exception','payout','task')),
 target_id text NOT NULL CHECK (length(target_id) BETWEEN 1 AND 80),
 author_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 body text NOT NULL DEFAULT '' CHECK (length(body) <= 4000),
 mentions uuid[] NOT NULL DEFAULT '{}' CHECK (cardinality(mentions) <= 20),
 resolved boolean NOT NULL DEFAULT false,
 resolved_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 resolved_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK (resolved = (resolved_at IS NOT NULL))
);
CREATE INDEX notes_target ON work.notes(target_kind, target_id, created_at);
CREATE INDEX notes_mentions ON work.notes USING gin (mentions) WHERE NOT resolved;

CREATE TABLE work.note_attachments (
 id uuid PRIMARY KEY,
 note_id uuid NOT NULL REFERENCES work.notes ON DELETE CASCADE,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
 content_type text NOT NULL CHECK (length(content_type) BETWEEN 1 AND 120),
 data bytea NOT NULL CHECK (octet_length(data) BETWEEN 1 AND 2097152),
 sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX note_attachments_note ON work.note_attachments(note_id);

-- Attachment uploads in pieces: the edge forwards JSON bodies of at most
-- 64 KiB, so a file arrives as ordered base64 parts and becomes a note
-- attachment once its size and SHA-256 match. Unclaimed uploads expire.
CREATE TABLE work.uploads (
 id uuid PRIMARY KEY,
 owner_id uuid NOT NULL REFERENCES identity.users ON DELETE CASCADE,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
 content_type text NOT NULL CHECK (length(content_type) BETWEEN 1 AND 120),
 size int NOT NULL CHECK (size BETWEEN 1 AND 2097152),
 sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
 data bytea NOT NULL DEFAULT ''::bytea CHECK (octet_length(data) <= size),
 parts int NOT NULL DEFAULT 0,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX uploads_created ON work.uploads(created_at);

-- ---------------------------------------------------------------------------
-- Exception queue
-- ---------------------------------------------------------------------------
CREATE TABLE work.exceptions (
 id uuid PRIMARY KEY,
 kind text NOT NULL CHECK (kind IN ('QC','RIGHTS_DOCS','DSP_REJECTION','METADATA_CONFLICT','SETTLEMENT_MISMATCH','DELIVERY_FAILURE')),
 severity text NOT NULL CHECK (severity IN ('CRITICAL','HIGH','MEDIUM','LOW')),
 status text NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN','ASSIGNED','RESOLVED','IGNORED')),
 title text NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 300),
 detail text NOT NULL DEFAULT '' CHECK (length(detail) <= 4000),
 source text NOT NULL CHECK (length(btrim(source)) BETWEEN 1 AND 80),
 -- one row per underlying problem; a repeat bumps occurrences
 dedupe_key text NOT NULL UNIQUE CHECK (length(dedupe_key) BETWEEN 1 AND 300),
 related jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(related) = 'array'),
 assignee_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 task_id uuid NULL REFERENCES work.tasks ON DELETE SET NULL,
 occurrences int NOT NULL DEFAULT 1 CHECK (occurrences >= 1),
 first_seen timestamptz NOT NULL DEFAULT now(),
 last_seen timestamptz NOT NULL DEFAULT now(),
 resolved_at timestamptz NULL,
 resolved_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 resolution text NULL CHECK (resolution IS NULL OR length(resolution) <= 1000),
 CHECK ((status IN ('RESOLVED','IGNORED')) = (resolved_at IS NOT NULL))
);
CREATE INDEX exceptions_active ON work.exceptions(kind, last_seen DESC) WHERE status IN ('OPEN','ASSIGNED');

-- Open (or reopen and bump) an exception. Automation calls this; a resolved
-- problem that happens again comes back to the queue, an IGNORED one stays
-- quiet but still counts.
CREATE OR REPLACE FUNCTION work.report_exception(
  p_kind text, p_severity text, p_title text, p_detail text, p_source text, p_key text, p_related jsonb)
RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, work AS $$
DECLARE v uuid;
BEGIN
  INSERT INTO work.exceptions(id,kind,severity,title,detail,source,dedupe_key,related)
  VALUES (gen_random_uuid(), p_kind, p_severity, left(p_title,300), left(coalesce(p_detail,''),4000), p_source, left(p_key,300), coalesce(p_related,'[]'))
  ON CONFLICT (dedupe_key) DO UPDATE SET
    occurrences = work.exceptions.occurrences + 1,
    last_seen = now(),
    detail = EXCLUDED.detail,
    severity = EXCLUDED.severity,
    status = CASE WHEN work.exceptions.status = 'RESOLVED'
                  THEN CASE WHEN work.exceptions.assignee_id IS NULL THEN 'OPEN' ELSE 'ASSIGNED' END
                  ELSE work.exceptions.status END,
    resolved_at = CASE WHEN work.exceptions.status = 'RESOLVED' THEN NULL ELSE work.exceptions.resolved_at END,
    resolved_by = CASE WHEN work.exceptions.status = 'RESOLVED' THEN NULL ELSE work.exceptions.resolved_by END,
    resolution = CASE WHEN work.exceptions.status = 'RESOLVED' THEN NULL ELSE work.exceptions.resolution END
  RETURNING id INTO v;
  RETURN v;
END $$;

-- The automation fixed it (job delivered, document approved): close quietly.
CREATE OR REPLACE FUNCTION work.clear_exception(p_key text, p_resolution text)
RETURNS void LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, work AS $$
  UPDATE work.exceptions SET status='RESOLVED', resolved_at=now(), resolution=p_resolution
   WHERE dedupe_key = p_key AND status IN ('OPEN','ASSIGNED');
$$;

-- Release behind a distribution package (for links in the queue).
CREATE OR REPLACE FUNCTION work.package_release(p_package uuid)
RETURNS TABLE(release_id uuid, title text) LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
  SELECT r.id, r.title FROM distribution.distribution_packages p
    JOIN distribution.canonical_releases c ON c.id = p.canonical_release_id
    JOIN catalog.releases r ON r.id = c.release_id
   WHERE p.id = p_package
$$;

CREATE OR REPLACE FUNCTION work.dsp_name(p_code text)
RETURNS text LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
  SELECT coalesce((SELECT name FROM distribution.dsp_registry WHERE code = p_code), p_code)
$$;

CREATE OR REPLACE FUNCTION work.related_for(p_package uuid, p_dsp text)
RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
  SELECT coalesce((SELECT jsonb_agg(x) FROM (
    SELECT jsonb_build_object('kind','release','id',pr.release_id::text,'label',pr.title) AS x FROM work.package_release(p_package) pr
    UNION ALL
    SELECT jsonb_build_object('kind','dsp','id',d.code,'label',d.name) FROM distribution.dsp_registry d WHERE d.code = p_dsp
  ) s), '[]'::jsonb)
$$;

-- Delivery job failed for good → DELIVERY_FAILURE; delivered → cleared.
CREATE OR REPLACE FUNCTION work.on_delivery_job() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text;
BEGIN
  IF NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
  IF NEW.status IN ('FAILED','DEAD_LETTER') THEN
    SELECT pr.title INTO t FROM work.package_release(NEW.package_id) pr;
    PERFORM work.report_exception('DELIVERY_FAILURE',
      CASE WHEN NEW.status = 'DEAD_LETTER' THEN 'CRITICAL' ELSE 'HIGH' END,
      coalesce(t,'발매') || ' — ' || work.dsp_name(NEW.partner_id) || ' 전송 실패' || CASE WHEN NEW.attempts > 1 THEN ' (' || NEW.attempts || '회 시도)' ELSE '' END,
      coalesce(NEW.last_error,''), 'delivery-runner', 'job:' || NEW.id, work.related_for(NEW.package_id, NEW.partner_id));
  ELSIF NEW.status = 'DELIVERED' THEN
    PERFORM work.clear_exception('job:' || NEW.id, '자동 재전송 성공');
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER work_delivery_job_exception AFTER UPDATE OF status ON execution.delivery_jobs
 FOR EACH ROW EXECUTE FUNCTION work.on_delivery_job();

-- Partner ack REJECTED → DSP_REJECTION (one per job).
CREATE OR REPLACE FUNCTION work.on_ack_event() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE pkg uuid; t text;
BEGIN
  IF NEW.outcome <> 'REJECTED' THEN
    IF NEW.outcome IN ('ACCEPTED','LIVE') AND NEW.job_id IS NOT NULL THEN
      PERFORM work.clear_exception('ack:' || NEW.job_id, 'DSP가 다시 받았어요');
    END IF;
    RETURN NEW;
  END IF;
  SELECT j.package_id INTO pkg FROM execution.delivery_jobs j WHERE j.id = NEW.job_id;
  SELECT pr.title INTO t FROM work.package_release(pkg) pr;
  PERFORM work.report_exception('DSP_REJECTION', 'HIGH',
    coalesce(t,'발매') || ' — ' || work.dsp_name(NEW.partner_id) || ' 반려',
    'partner event ' || NEW.event_id, 'dsp-ack-ingest',
    'ack:' || coalesce(NEW.job_id::text, NEW.partner_id || ':' || NEW.event_id),
    work.related_for(pkg, NEW.partner_id));
  RETURN NEW;
END $$;
CREATE TRIGGER work_ack_event_exception AFTER INSERT ON execution.ack_events
 FOR EACH ROW EXECUTE FUNCTION work.on_ack_event();

-- Delivery staging blocked by content (metadata the DSP will not take).
CREATE OR REPLACE FUNCTION work.on_staging() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text; key text;
BEGIN
  key := 'staging:' || NEW.package_id || ':' || NEW.dsp_code;
  IF NEW.readiness = 'CONTENT_BLOCKED' AND (TG_OP = 'INSERT' OR OLD.readiness IS DISTINCT FROM NEW.readiness) THEN
    SELECT r.title INTO t FROM catalog.releases r WHERE r.id = NEW.release_id;
    PERFORM work.report_exception('METADATA_CONFLICT', 'MEDIUM',
      coalesce(t,'발매') || ' — ' || work.dsp_name(NEW.dsp_code) || ' 전송 전 검사에서 막힘',
      coalesce((SELECT string_agg(coalesce(c->>'message', c->>'code'), ' · ') FROM jsonb_array_elements(NEW.checks) c
                 WHERE c->>'severity' = 'BLOCKER' OR c->>'class' = 'CONTENT'), ''),
      'delivery-staging', key,
      jsonb_build_array(jsonb_build_object('kind','release','id',NEW.release_id::text,'label',coalesce(t,'발매')))
        || coalesce((SELECT jsonb_agg(jsonb_build_object('kind','dsp','id',d.code,'label',d.name)) FROM distribution.dsp_registry d WHERE d.code = NEW.dsp_code), '[]'));
  ELSIF TG_OP = 'UPDATE' AND OLD.readiness = 'CONTENT_BLOCKED' AND NEW.readiness <> 'CONTENT_BLOCKED' THEN
    PERFORM work.clear_exception(key, '보완 후 다시 검사해서 통과');
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER work_staging_exception AFTER INSERT OR UPDATE OF readiness ON distribution.delivery_staging
 FOR EACH ROW EXECUTE FUNCTION work.on_staging();

-- Rights proof sent back for more (NEEDS) → RIGHTS_DOCS; approved → cleared.
CREATE OR REPLACE FUNCTION work.on_document() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text; rel jsonb;
BEGIN
  IF NEW.kind <> 'RIGHTS_PROOF' OR NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
  IF NEW.status = 'NEEDS' THEN
    SELECT r.title INTO t FROM catalog.releases r WHERE r.id = NEW.release_id;
    rel := jsonb_build_array(jsonb_build_object('kind','document','id',NEW.id::text,'label',NEW.title));
    IF NEW.release_id IS NOT NULL THEN
      rel := jsonb_build_array(jsonb_build_object('kind','release','id',NEW.release_id::text,'label',coalesce(t,'발매'))) || rel;
    END IF;
    PERFORM work.report_exception('RIGHTS_DOCS', 'MEDIUM',
      coalesce(t || ' — ', '') || NEW.title || ' 보완 필요', NEW.review_note, 'rights-checker', 'doc:' || NEW.id, rel);
  ELSIF NEW.status IN ('APPROVED','SIGNED') THEN
    PERFORM work.clear_exception('doc:' || NEW.id, '서류 승인');
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER work_document_exception AFTER UPDATE OF status ON portal.documents
 FOR EACH ROW EXECUTE FUNCTION work.on_document();

-- ---------------------------------------------------------------------------
-- Inbox read marks (items are computed; only "read" is stored)
-- ---------------------------------------------------------------------------
CREATE TABLE work.inbox_reads (
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE CASCADE,
 item_id text NOT NULL CHECK (length(item_id) BETWEEN 1 AND 120),
 read_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (user_id, item_id)
);

-- ---------------------------------------------------------------------------
-- CRM Lite
-- ---------------------------------------------------------------------------
CREATE TABLE work.contacts (
 id uuid PRIMARY KEY,
 kind text NOT NULL CHECK (kind IN ('ARTIST','LABEL','DSP','PARTNER')),
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
 company text NOT NULL DEFAULT '' CHECK (length(company) <= 120),
 person text NOT NULL DEFAULT '' CHECK (length(person) <= 120),
 email text NOT NULL DEFAULT '' CHECK (length(email) <= 200),
 phone text NOT NULL DEFAULT '' CHECK (length(phone) <= 40),
 owner_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 last_contact_at timestamptz NULL,
 next_follow_up_at timestamptz NULL,
 contract_stage text NOT NULL DEFAULT 'NONE' CHECK (contract_stage IN ('NONE','NEGOTIATING','SENT','SIGNED','RENEWAL','ENDED')),
 tags text[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 8),
 org_id uuid NULL REFERENCES identity.orgs ON DELETE SET NULL,
 dsp_code text NULL REFERENCES distribution.dsp_registry(code) ON DELETE SET NULL,
 memo text NOT NULL DEFAULT '' CHECK (length(memo) <= 2000),
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX contacts_follow_up ON work.contacts(next_follow_up_at) WHERE next_follow_up_at IS NOT NULL;

CREATE TABLE work.contact_activities (
 id uuid PRIMARY KEY,
 contact_id uuid NOT NULL REFERENCES work.contacts ON DELETE CASCADE,
 kind text NOT NULL CHECK (kind IN ('CALL','EMAIL','MEETING','NOTE')),
 summary text NOT NULL CHECK (length(btrim(summary)) BETWEEN 1 AND 2000),
 by_user uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX contact_activities_contact ON work.contact_activities(contact_id, at DESC);

-- ---------------------------------------------------------------------------
-- DSP operations (manual fields; metrics come from execution.*)
-- ---------------------------------------------------------------------------
CREATE TABLE work.dsp_ops (
 code text PRIMARY KEY REFERENCES distribution.dsp_registry(code) ON DELETE CASCADE,
 owner_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 contract_stage text NOT NULL DEFAULT 'NONE' CHECK (contract_stage IN ('NONE','NEGOTIATING','SENT','SIGNED','RENEWAL','ENDED')),
 contract_renewal_at timestamptz NULL,
 sla_hours int NOT NULL DEFAULT 24 CHECK (sla_hours BETWEEN 1 AND 720),
 -- NULL = derive from recent delivery results
 health text NULL CHECK (health IN ('HEALTHY','DEGRADED','DOWN','ONBOARDING')),
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now()
);

-- Staff (after the API checked the staff role and set app.staff) may read
-- delivery results across orgs for DSP failure rates and approval times.
CREATE POLICY delivery_jobs_staff_read ON execution.delivery_jobs FOR SELECT
 USING (current_setting('app.staff', true) = 'on');
CREATE POLICY delivery_attempts_staff_read ON execution.delivery_attempts FOR SELECT
 USING (current_setting('app.staff', true) = 'on');
CREATE POLICY ack_events_staff_read ON execution.ack_events FOR SELECT
 USING (current_setting('app.staff', true) = 'on');

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT USAGE ON SCHEMA work TO audeniq_api';
    EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA work TO audeniq_api';
    EXECUTE 'GRANT USAGE, SELECT ON SEQUENCE work.task_seq TO audeniq_api';
    EXECUTE 'GRANT EXECUTE ON FUNCTION work.report_exception(text,text,text,text,text,text,jsonb) TO audeniq_api';
    EXECUTE 'GRANT EXECUTE ON FUNCTION work.clear_exception(text,text) TO audeniq_api';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_worker') THEN
    EXECUTE 'GRANT USAGE ON SCHEMA work TO audeniq_worker';
    EXECUTE 'GRANT EXECUTE ON FUNCTION work.report_exception(text,text,text,text,text,text,jsonb) TO audeniq_worker';
    EXECUTE 'GRANT EXECUTE ON FUNCTION work.clear_exception(text,text) TO audeniq_worker';
  END IF;
END
$$;
