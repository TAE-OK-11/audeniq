-- 0078: 업무 Phase 2 — 통합 업무함 · SLA/마감 엔진 · 예외 처리 큐. 설계: docs/WORK_MANAGEMENT_PLAN.md
--
-- Nothing here copies the automation's failures. The exception queue reads
-- them where they already live (delivery jobs, reconciliation cases, Stage
-- checks, delivery staging, rights documents, unmatched report lines,
-- dead-lettered jobs); `work.exception_triage` keeps only what people add:
-- who handles it, the outcome, retries and the follow-up task. Only problems
-- with no source record (a settlement mismatch found by hand) are stored, in
-- `work.exception_reports`.

-- ---------------------------------------------------------------------------
-- Working calendars (business-day SLA). One Korean calendar now; more can be
-- added (another office, a partner's calendar) without code changes.
-- ---------------------------------------------------------------------------
CREATE TABLE work.calendars (
 id text PRIMARY KEY CHECK (id ~ '^[A-Z][A-Z0-9_]{1,39}$'),
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
 -- fixed offset from UTC; Korea has no daylight saving time
 utc_offset_minutes int NOT NULL CHECK (utc_offset_minutes BETWEEN -720 AND 840),
 work_start time NOT NULL,
 work_end time NOT NULL,
 -- ISO weekdays, 1 = Monday … 7 = Sunday
 workdays smallint[] NOT NULL CHECK (cardinality(workdays) BETWEEN 1 AND 7 AND workdays <@ ARRAY[1,2,3,4,5,6,7]::smallint[]),
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK (work_end > work_start)
);
INSERT INTO work.calendars(id,name,utc_offset_minutes,work_start,work_end,workdays)
VALUES ('KR_BUSINESS','한국 영업일 (평일 09–18시)',540,'09:00','18:00','{1,2,3,4,5}');

CREATE TABLE work.calendar_holidays (
 calendar_id text NOT NULL REFERENCES work.calendars ON DELETE RESTRICT,
 day date NOT NULL,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
 created_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 PRIMARY KEY (calendar_id, day)
);

-- ---------------------------------------------------------------------------
-- SLA policies: per work type (tasks) and per existing queue (release review,
-- second approval, document review, customer inquiry, exception), optionally
-- scoped to one entity (e.g. a DSP's own inquiry SLA). NULL calendar = 24h clock.
-- ---------------------------------------------------------------------------
CREATE TABLE work.sla_policies (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 work_type text NOT NULL CHECK (work_type IN (
   'GENERAL','URGENT_DELIVERY','RELEASE','RIGHTS','CONTRACT','ARTIST','DSP_INQUIRY','SETTLEMENT','SUPPORT','LEGAL',
   'RELEASE_REVIEW','SECOND_APPROVAL','DOCUMENT_REVIEW','SUPPORT_INQUIRY','EXCEPTION')),
 scope_kind text NULL CHECK (scope_kind IN ('dsp')),
 scope_id text NULL CHECK (scope_id IS NULL OR scope_id ~ '^[A-Za-z0-9-]{1,64}$'),
 target_minutes int NOT NULL CHECK (target_minutes BETWEEN 15 AND 200000),
 calendar_id text NULL REFERENCES work.calendars ON DELETE RESTRICT,
 warn_ratio real NOT NULL DEFAULT 0.25 CHECK (warn_ratio > 0 AND warn_ratio < 1),
 idle_alert_minutes int NOT NULL DEFAULT 240 CHECK (idle_alert_minutes BETWEEN 10 AND 200000),
 escalate_to text NOT NULL DEFAULT 'ADMIN' CHECK (escalate_to IN ('ADMIN','REVIEWER','OPERATOR','SUPPORT')),
 auto_raise_priority boolean NOT NULL DEFAULT true,
 enabled boolean NOT NULL DEFAULT true,
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((scope_kind IS NULL) = (scope_id IS NULL)),
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE UNIQUE INDEX sla_policies_active ON work.sla_policies(work_type, coalesce(scope_kind,''), coalesce(scope_id,'')) WHERE archived_at IS NULL;

-- 예: 긴급 송출 2시간(24시간 기준), 권리 검토 1영업일(9시간)
INSERT INTO work.sla_policies(work_type,target_minutes,calendar_id,idle_alert_minutes,escalate_to) VALUES
 ('URGENT_DELIVERY',120,NULL,30,'OPERATOR'),
 ('RIGHTS',540,'KR_BUSINESS',120,'REVIEWER'),
 ('RELEASE',1080,'KR_BUSINESS',240,'REVIEWER'),
 ('CONTRACT',1620,'KR_BUSINESS',540,'ADMIN'),
 ('ARTIST',2700,'KR_BUSINESS',1080,'ADMIN'),
 ('DSP_INQUIRY',540,'KR_BUSINESS',120,'OPERATOR'),
 ('SETTLEMENT',1620,'KR_BUSINESS',540,'ADMIN'),
 ('SUPPORT',540,'KR_BUSINESS',60,'SUPPORT'),
 ('LEGAL',2700,'KR_BUSINESS',540,'ADMIN'),
 ('GENERAL',2700,'KR_BUSINESS',1080,'ADMIN'),
 ('RELEASE_REVIEW',1080,'KR_BUSINESS',240,'REVIEWER'),
 ('SECOND_APPROVAL',540,'KR_BUSINESS',120,'REVIEWER'),
 ('DOCUMENT_REVIEW',540,'KR_BUSINESS',120,'REVIEWER'),
 ('SUPPORT_INQUIRY',540,'KR_BUSINESS',60,'SUPPORT'),
 ('EXCEPTION',240,'KR_BUSINESS',60,'OPERATOR');

-- Task SLA state (deadline from the policy at creation; escalation applied
-- once per level and recorded as a task event).
ALTER TABLE work.tasks
 ADD COLUMN sla_due_at timestamptz NULL,
 ADD COLUMN escalation_level smallint NOT NULL DEFAULT 0 CHECK (escalation_level BETWEEN 0 AND 2),
 ADD COLUMN priority_raised text NULL CHECK (priority_raised IN ('URGENT','HIGH','NORMAL','LOW'));
CREATE INDEX tasks_sla_due ON work.tasks(LEAST(due_at, sla_due_at)) WHERE archived_at IS NULL AND status NOT IN ('DONE','CANCELLED');

-- ---------------------------------------------------------------------------
-- Inbox: items are computed from the sources; only "read" is stored.
-- ---------------------------------------------------------------------------
CREATE TABLE work.inbox_reads (
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 item_id text NOT NULL CHECK (length(item_id) BETWEEN 1 AND 160),
 read_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (user_id, item_id)
);

-- ---------------------------------------------------------------------------
-- Exception queue
-- ---------------------------------------------------------------------------
-- Problems with no source record of their own (found by a person).
CREATE TABLE work.exception_reports (
 id uuid PRIMARY KEY,
 kind text NOT NULL CHECK (kind IN ('QC_FAILURE','METADATA_CONFLICT','MISSING_RIGHTS_DOCUMENT','DSP_REJECTION','DISTRIBUTION_FAILURE','SETTLEMENT_MISMATCH','AUTOMATION_FAILURE')),
 severity text NOT NULL CHECK (severity IN ('CRITICAL','HIGH','MEDIUM','LOW')),
 title text NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 300),
 detail text NOT NULL DEFAULT '' CHECK (length(detail) <= 4000),
 entity_kind text NULL CHECK (entity_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task')),
 entity_id text NULL CHECK (entity_id IS NULL OR entity_id ~ '^[A-Za-z0-9-]{1,64}$'),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK ((entity_kind IS NULL) = (entity_id IS NULL))
);

-- What people did about an exception, keyed by its source record.
CREATE TABLE work.exception_triage (
 source text NOT NULL CHECK (source IN ('check','staging','document','rights_hold','reconciliation','delivery_job','report','job','manual')),
 source_id text NOT NULL CHECK (length(source_id) BETWEEN 1 AND 120),
 assignee_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 -- NULL = open; RESOLVED/IGNORED hide it until the source record changes
 status text NULL CHECK (status IN ('ACKNOWLEDGED','RESOLVED','IGNORED')),
 resolution text NULL CHECK (resolution IS NULL OR length(resolution) <= 1000),
 resolved_at timestamptz NULL,
 resolved_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 retry_count int NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
 last_retry_at timestamptz NULL,
 last_retry_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 task_id uuid NULL REFERENCES work.tasks ON DELETE RESTRICT,
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (source, source_id),
 CHECK ((status IN ('RESOLVED','IGNORED')) = (resolved_at IS NOT NULL))
);

-- Staff (after the API checked the staff role and set app.staff) read delivery
-- results across workspaces for the queue and DSP failure rates, the same way
-- delivery_staging already allows it (0043).
CREATE POLICY delivery_jobs_staff_read ON execution.delivery_jobs FOR SELECT
 USING (current_setting('app.staff', true) = 'on');
CREATE POLICY delivery_attempts_staff_read ON execution.delivery_attempts FOR SELECT
 USING (current_setting('app.staff', true) = 'on');
CREATE POLICY ack_events_staff_read ON execution.ack_events FOR SELECT
 USING (current_setting('app.staff', true) = 'on');
CREATE POLICY reconciliation_cases_staff_read ON execution.reconciliation_cases FOR SELECT
 USING (current_setting('app.staff', true) = 'on');

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT SELECT, INSERT, UPDATE ON work.calendars, work.calendar_holidays, work.sla_policies, work.inbox_reads, work.exception_reports, work.exception_triage TO audeniq_api';
    EXECUTE 'GRANT DELETE ON work.calendar_holidays TO audeniq_api';
  END IF;
END
$$;
