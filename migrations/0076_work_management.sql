-- 0076: 내부 업무(Work) 모듈 — 범용 업무(Task)·하위 업무, 공통 Entity 연결, 내부 메모·멘션·첨부.
-- API: crates/core/src/work (`/api/staff/work/*`). 설계: docs/WORK_MANAGEMENT_PLAN.md
--
-- Staff-only, internal data. Customers never read these tables: there is no
-- tenant API over them, and every call goes through the staff API (ACTIVE
-- identity.staff_members row, staff access log, audit of every mutation).
--
-- Nothing here copies data that already lives elsewhere. A link stores only
-- (entity_kind, entity_id); the title of a release, the name of an artist or
-- a DSP is read from its own table when shown. Nothing is deleted: tasks,
-- links, notes and attachments are archived (archived_at/by).
CREATE SCHEMA IF NOT EXISTS work;

-- ---------------------------------------------------------------------------
-- Tasks and subtasks
-- ---------------------------------------------------------------------------
CREATE SEQUENCE work.task_seq START 1001;
CREATE TABLE work.tasks (
 id uuid PRIMARY KEY,
 seq bigint NOT NULL UNIQUE DEFAULT nextval('work.task_seq'),
 -- one level of subtasks; the API refuses a parent that is itself a subtask
 parent_id uuid NULL REFERENCES work.tasks ON DELETE RESTRICT,
 title text NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 200),
 description text NOT NULL DEFAULT '' CHECK (length(description) <= 8000),
 work_type text NOT NULL CHECK (work_type IN ('GENERAL','URGENT_DELIVERY','RELEASE','RIGHTS','CONTRACT','ARTIST','DSP_INQUIRY','SETTLEMENT','SUPPORT','LEGAL')),
 status text NOT NULL DEFAULT 'TODO' CHECK (status IN ('TODO','IN_PROGRESS','IN_REVIEW','BLOCKED','DONE','CANCELLED')),
 priority text NOT NULL DEFAULT 'NORMAL' CHECK (priority IN ('URGENT','HIGH','NORMAL','LOW')),
 assignee_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 reviewer_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 due_at timestamptz NULL,
 -- the reviewer sent it back; cleared when review is requested again
 changes_requested boolean NOT NULL DEFAULT false,
 recurrence jsonb NULL CHECK (recurrence IS NULL OR (
   recurrence->>'freq' IN ('DAILY','WEEKLY','MONTHLY')
   AND (recurrence->>'interval') ~ '^[0-9]{1,2}$'
   AND (recurrence->>'interval')::int BETWEEN 1 AND 12)),
 recurred_from uuid NULL REFERENCES work.tasks ON DELETE RESTRICT,
 tags text[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 8),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 assigned_at timestamptz NULL,
 started_at timestamptz NULL,
 completed_at timestamptz NULL,
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 row_version int NOT NULL DEFAULT 1,
 CHECK (parent_id IS NULL OR parent_id <> id),
 CHECK ((status IN ('DONE','CANCELLED')) = (completed_at IS NOT NULL)),
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE INDEX tasks_open_assignee ON work.tasks(assignee_id) WHERE archived_at IS NULL AND status NOT IN ('DONE','CANCELLED');
CREATE INDEX tasks_open_reviewer ON work.tasks(reviewer_id) WHERE archived_at IS NULL AND status = 'IN_REVIEW';
CREATE INDEX tasks_parent ON work.tasks(parent_id) WHERE parent_id IS NOT NULL;
CREATE INDEX tasks_due ON work.tasks(due_at) WHERE archived_at IS NULL AND status NOT IN ('DONE','CANCELLED');
CREATE INDEX tasks_completed ON work.tasks(completed_at) WHERE completed_at IS NOT NULL;

-- What happened to a task, for the task screen. The authoritative trail is
-- operations.audit_events (work.* actions); this keeps the human detail.
CREATE TABLE work.task_events (
 id uuid PRIMARY KEY,
 task_id uuid NOT NULL REFERENCES work.tasks ON DELETE RESTRICT,
 actor_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 action text NOT NULL CHECK (action ~ '^[A-Z_]{2,40}$'),
 detail text NULL CHECK (detail IS NULL OR length(detail) <= 2000),
 at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX task_events_task ON work.task_events(task_id, at);
CREATE INDEX task_events_at ON work.task_events(at);

-- ---------------------------------------------------------------------------
-- Common entity links: a task (later: a CRM relationship) points at any
-- AUDENIQ entity by kind + id. Labels are resolved from the source tables.
-- ---------------------------------------------------------------------------
CREATE TABLE work.links (
 id uuid PRIMARY KEY,
 subject_kind text NOT NULL CHECK (subject_kind IN ('task')),
 subject_id uuid NOT NULL,
 entity_kind text NOT NULL CHECK (entity_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task')),
 entity_id text NOT NULL CHECK (entity_id ~ '^[A-Za-z0-9-]{1,64}$'),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE UNIQUE INDEX links_active ON work.links(subject_kind, subject_id, entity_kind, entity_id) WHERE archived_at IS NULL;
CREATE INDEX links_entity ON work.links(entity_kind, entity_id) WHERE archived_at IS NULL;

-- ---------------------------------------------------------------------------
-- Internal notes (@mentions, attachments, edit history, resolve, archive)
-- ---------------------------------------------------------------------------
CREATE TABLE work.notes (
 id uuid PRIMARY KEY,
 target_kind text NOT NULL CHECK (target_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task')),
 target_id text NOT NULL CHECK (target_id ~ '^[A-Za-z0-9-]{1,64}$'),
 author_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 body text NOT NULL DEFAULT '' CHECK (length(body) <= 4000),
 mentions uuid[] NOT NULL DEFAULT '{}' CHECK (cardinality(mentions) <= 20),
 resolved_at timestamptz NULL,
 resolved_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 edited_at timestamptz NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((resolved_at IS NULL) = (resolved_by IS NULL)),
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE INDEX notes_target ON work.notes(target_kind, target_id, created_at);
CREATE INDEX notes_mentions ON work.notes USING gin (mentions) WHERE archived_at IS NULL AND resolved_at IS NULL;

-- Previous versions of an edited note (the current text stays on work.notes).
CREATE TABLE work.note_revisions (
 id uuid PRIMARY KEY,
 note_id uuid NOT NULL REFERENCES work.notes ON DELETE RESTRICT,
 body text NOT NULL CHECK (length(body) <= 4000),
 mentions uuid[] NOT NULL DEFAULT '{}',
 edited_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 edited_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX note_revisions_note ON work.note_revisions(note_id, edited_at);

-- Note attachments live in the object store (same presigned upload, freeze
-- and antivirus path as studio uploads); only metadata is kept here.
CREATE TABLE work.attachments (
 id uuid PRIMARY KEY,
 note_id uuid NULL REFERENCES work.notes ON DELETE RESTRICT,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
 content_type text NOT NULL CHECK (length(content_type) BETWEEN 3 AND 120),
 size_bytes bigint NOT NULL CHECK (size_bytes BETWEEN 1 AND 20971520),
 upload_key text NOT NULL UNIQUE,
 nonce uuid NOT NULL,
 object_key text NOT NULL UNIQUE,
 state text NOT NULL DEFAULT 'ISSUED' CHECK (state IN ('ISSUED','READY')),
 sha256 text NULL CHECK (sha256 IS NULL OR sha256 ~ '^[0-9a-f]{64}$'),
 antivirus_status text NULL CHECK (antivirus_status IN ('SCANNED','NOT_SCANNED')),
 uploaded_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL,
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((state = 'READY') = (sha256 IS NOT NULL AND antivirus_status IS NOT NULL)),
 CHECK (note_id IS NULL OR state = 'READY'),
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE INDEX attachments_note ON work.attachments(note_id) WHERE note_id IS NOT NULL;

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT USAGE ON SCHEMA work TO audeniq_api';
    -- no DELETE: work data is archived, never removed
    EXECUTE 'GRANT SELECT, INSERT, UPDATE ON ALL TABLES IN SCHEMA work TO audeniq_api';
    EXECUTE 'GRANT USAGE, SELECT ON SEQUENCE work.task_seq TO audeniq_api';
  END IF;
END
$$;
