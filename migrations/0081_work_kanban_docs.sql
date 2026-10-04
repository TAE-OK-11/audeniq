-- 0081: 업무 — 칸반 보드(보드·열·카드 위치·자동 카드) + Docs(블록 문서·권한·버전·템플릿)
-- 설계: docs/WORK_MANAGEMENT_PLAN.md (Kanban · Docs)
--
-- Kanban cards are the existing tasks (work.tasks): they already carry title,
-- description, priority, assignee, creator, due date, tags, type, entity links
-- (work.links), comments (work.notes), activity (work.task_events) and files.
-- This adds only where a task sits (board, column, position) and where it came
-- from (source + an idempotency key for cards created from system events).
--
-- Docs reuse work.notes for comments (now with replies and anchors),
-- work.links for related entities / cards, and work.attachments for images
-- and files (object store + scan; downloads follow the document's ACL).

-- ===========================================================================
-- Kanban
-- ===========================================================================
CREATE TABLE work.boards (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 -- system boards have a stable key (automation rules, seeds)
 key text NULL UNIQUE CHECK (key IS NULL OR key ~ '^[a-z][a-z0-9_]{1,39}$'),
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
 description text NOT NULL DEFAULT '' CHECK (length(description) <= 500),
 -- PERSONAL: only its owner · TEAM: everyone reads, its role (and ADMIN) edits · OPERATIONS: all staff
 kind text NOT NULL CHECK (kind IN ('PERSONAL','TEAM','OPERATIONS')),
 owner_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 team_role text NULL CHECK (team_role IN ('ADMIN','REVIEWER','OPERATOR','SUPPORT')),
 position double precision NOT NULL DEFAULT 0,
 created_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 row_version int NOT NULL DEFAULT 1,
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((kind = 'PERSONAL') = (owner_id IS NOT NULL)),
 CHECK ((kind = 'TEAM') = (team_role IS NOT NULL)),
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE INDEX boards_owner ON work.boards(owner_id) WHERE archived_at IS NULL;

CREATE TABLE work.board_columns (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 board_id uuid NOT NULL REFERENCES work.boards ON DELETE RESTRICT,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 40),
 -- the task status a card in this column has (inbox, SLA, review rules keep working)
 status text NOT NULL CHECK (status IN ('TODO','IN_PROGRESS','IN_REVIEW','BLOCKED','DONE','CANCELLED')),
 position double precision NOT NULL,
 wip_limit int NULL CHECK (wip_limit IS NULL OR wip_limit BETWEEN 1 AND 999),
 created_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 UNIQUE (board_id, id),
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE UNIQUE INDEX board_columns_name ON work.board_columns(board_id, lower(btrim(name))) WHERE archived_at IS NULL;
CREATE INDEX board_columns_board ON work.board_columns(board_id, position) WHERE archived_at IS NULL;

-- Cards created by an AUDENIQ event have no human creator (NULL = system; `source` says which).
ALTER TABLE work.tasks ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE work.links ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE work.tasks
 ADD COLUMN board_id uuid NULL,
 ADD COLUMN column_id uuid NULL,
 ADD COLUMN position double precision NULL,
 -- who created the card: a person, or an AUDENIQ event
 ADD COLUMN source text NOT NULL DEFAULT 'manual' CHECK (source IN ('manual','system','qc','distribution','support','rights','finance','sla')),
 -- idempotency key of an automatic card (one card per event)
 ADD COLUMN source_key text NULL CHECK (source_key IS NULL OR length(source_key) BETWEEN 1 AND 200),
 ADD CONSTRAINT tasks_board_column FOREIGN KEY (board_id, column_id) REFERENCES work.board_columns(board_id, id),
 ADD CONSTRAINT tasks_board_placed CHECK ((board_id IS NULL) = (column_id IS NULL) AND (column_id IS NULL) = (position IS NULL));
CREATE UNIQUE INDEX tasks_source_key ON work.tasks(source_key) WHERE source_key IS NOT NULL;
CREATE INDEX tasks_board ON work.tasks(board_id, column_id, position) WHERE archived_at IS NULL;
CREATE INDEX tasks_tags ON work.tasks USING gin (tags) WHERE archived_at IS NULL;

-- Which board/column receives cards for an AUDENIQ event (one board per trigger).
CREATE TABLE work.board_rules (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 board_id uuid NOT NULL,
 column_id uuid NOT NULL,
 trigger text NOT NULL CHECK (trigger IN (
   'QC_FAILURE','METADATA_CONFLICT','MISSING_RIGHTS_DOCUMENT','DSP_REJECTION','DISTRIBUTION_FAILURE',
   'SETTLEMENT_MISMATCH','AUTOMATION_FAILURE','SUPPORT_INQUIRY','APPROVAL_REQUIRED','RELEASE_REVIEW_OVERDUE')),
 enabled boolean NOT NULL DEFAULT true,
 -- only events detected after this moment become cards (no backfill flood)
 active_since timestamptz NOT NULL DEFAULT now(),
 created_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (board_id, column_id) REFERENCES work.board_columns(board_id, id)
);
CREATE UNIQUE INDEX board_rules_trigger ON work.board_rules(trigger);

-- Seed: an operations board for everyone and team boards, each with the default columns.
INSERT INTO work.boards(key, name, description, kind, team_role, position) VALUES
 ('general', '일반 업무', '모든 직원의 공통 업무 보드', 'OPERATIONS', NULL, 0),
 ('distribution', '배급 운영', 'DSP 전송·반려·장애 처리', 'TEAM', 'OPERATOR', 1),
 ('rights', '권리 검토', '권리 서류·QC·2차 승인', 'TEAM', 'REVIEWER', 2),
 ('support', '고객 지원', '고객 문의·수정 요청', 'TEAM', 'SUPPORT', 3),
 ('engineering', '엔지니어링', '자동화 실패·시스템 개선', 'TEAM', 'ADMIN', 4);
INSERT INTO work.board_columns(board_id, name, status, position)
SELECT b.id, c.name, c.status, c.pos
  FROM work.boards b
 CROSS JOIN (VALUES ('Inbox','TODO',0), ('To Do','TODO',1), ('In Progress','IN_PROGRESS',2), ('Review','IN_REVIEW',3),
                    ('Waiting','BLOCKED',4), ('Blocked','BLOCKED',5), ('Done','DONE',6)) AS c(name, status, pos);

-- Existing open and finished tasks go to the general board, into the first column of their status.
UPDATE work.tasks t SET board_id = c.board_id, column_id = c.id, position = extract(epoch FROM t.created_at)
  FROM (SELECT DISTINCT ON (bc.status) bc.id, bc.board_id, bc.status
          FROM work.board_columns bc JOIN work.boards b ON b.id = bc.board_id AND b.key = 'general'
         ORDER BY bc.status, bc.position) c
 WHERE c.status = t.status AND t.parent_id IS NULL;

INSERT INTO work.board_rules(board_id, column_id, trigger)
SELECT b.id, (SELECT bc.id FROM work.board_columns bc WHERE bc.board_id = b.id ORDER BY bc.position LIMIT 1), r.trigger
  FROM (VALUES ('distribution','DSP_REJECTION'), ('distribution','DISTRIBUTION_FAILURE'), ('distribution','METADATA_CONFLICT'),
               ('rights','QC_FAILURE'), ('rights','MISSING_RIGHTS_DOCUMENT'), ('rights','APPROVAL_REQUIRED'),
               ('support','SUPPORT_INQUIRY'), ('engineering','AUTOMATION_FAILURE'), ('general','SETTLEMENT_MISMATCH')) AS r(board, trigger)
  JOIN work.boards b ON b.key = r.board;

-- ===========================================================================
-- Docs
-- ===========================================================================
CREATE TABLE work.docs (
 id uuid PRIMARY KEY,
 parent_id uuid NULL REFERENCES work.docs ON DELETE RESTRICT,
 -- 0 = top level; at most 5 levels so the tree stays readable
 depth smallint NOT NULL DEFAULT 0 CHECK (depth BETWEEN 0 AND 4),
 title text NOT NULL DEFAULT '' CHECK (length(title) <= 200),
 icon text NULL CHECK (icon IS NULL OR length(icon) BETWEEN 1 AND 16),
 -- ProseMirror/Tiptap JSON, validated by the API (never HTML)
 content jsonb NOT NULL DEFAULT '{"type":"doc","content":[]}',
 -- plain text of the content (search, version compare), derived by the API
 content_text text NOT NULL DEFAULT '' CHECK (length(content_text) <= 1000000),
 search tsvector GENERATED ALWAYS AS (to_tsvector('simple', coalesce(title, '') || ' ' || content_text)) STORED,
 status text NOT NULL DEFAULT 'DRAFT' CHECK (status IN ('DRAFT','IN_REVIEW','APPROVED')),
 review_requested_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 review_requested_at timestamptz NULL,
 approved_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 approved_at timestamptz NULL,
 rejected_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 rejected_at timestamptz NULL,
 rejection_reason text NULL CHECK (rejection_reason IS NULL OR length(rejection_reason) <= 1000),
 -- what every staff member may do; NULL = only owner, ADMIN and explicit grants
 staff_access text NULL DEFAULT 'EDITOR' CHECK (staff_access IN ('VIEWER','EDITOR')),
 -- optimistic concurrency: every save bumps it, a save must name the version it edited
 version int NOT NULL DEFAULT 1,
 position double precision NOT NULL DEFAULT 0,
 template_id uuid NULL,
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 -- tombstone: gone for everyone, history kept for audit
 deleted_at timestamptz NULL,
 deleted_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((archived_at IS NULL) = (archived_by IS NULL)),
 CHECK ((deleted_at IS NULL) = (deleted_by IS NULL)),
 CHECK (deleted_at IS NULL OR archived_at IS NOT NULL),
 CHECK (parent_id IS NULL OR parent_id <> id)
);
CREATE INDEX docs_parent ON work.docs(parent_id, position) WHERE deleted_at IS NULL;
CREATE INDEX docs_search ON work.docs USING gin (search) WHERE deleted_at IS NULL;
CREATE INDEX docs_updated ON work.docs(updated_at DESC) WHERE deleted_at IS NULL;

-- Per-document grants beyond staff_access: a staff role or one person.
CREATE TABLE work.doc_permissions (
 doc_id uuid NOT NULL REFERENCES work.docs ON DELETE RESTRICT,
 principal_kind text NOT NULL CHECK (principal_kind IN ('ROLE','USER')),
 principal_id text NOT NULL CHECK (length(principal_id) BETWEEN 1 AND 64),
 level text NOT NULL CHECK (level IN ('VIEWER','EDITOR','MANAGER')),
 granted_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 granted_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (doc_id, principal_kind, principal_id)
);
CREATE INDEX doc_permissions_principal ON work.doc_permissions(principal_kind, principal_id);

-- Snapshots: on a new editor or after 10 minutes of autosaves, on status
-- changes, restores and "save version" — never every keystroke.
CREATE TABLE work.doc_versions (
 doc_id uuid NOT NULL REFERENCES work.docs ON DELETE RESTRICT,
 version int NOT NULL,
 title text NOT NULL,
 icon text NULL,
 content jsonb NOT NULL,
 content_text text NOT NULL,
 reason text NOT NULL CHECK (reason IN ('AUTOSAVE','MANUAL','RESTORE','STATUS','CREATE')),
 label text NULL CHECK (label IS NULL OR length(label) <= 120),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (doc_id, version)
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON work.doc_versions
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

CREATE TABLE work.doc_templates (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 key text NULL UNIQUE CHECK (key IS NULL OR key ~ '^[a-z][a-z0-9_]{1,39}$'),
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
 description text NOT NULL DEFAULT '' CHECK (length(description) <= 300),
 icon text NULL CHECK (icon IS NULL OR length(icon) BETWEEN 1 AND 16),
 content jsonb NOT NULL,
 position double precision NOT NULL DEFAULT 0,
 created_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
ALTER TABLE work.docs ADD CONSTRAINT docs_template FOREIGN KEY (template_id) REFERENCES work.doc_templates ON DELETE RESTRICT;

-- Who may see / edit a document: 0 none · 1 viewer · 2 editor · 3 manager · 4 owner.
-- ADMIN manages every document; deleted documents are invisible to all.

-- Templates (content is the same block JSON documents use)
INSERT INTO work.doc_templates(key, name, description, icon, content, position) VALUES
 ('meeting_notes', '회의록', '참석자·안건·결정·할 일', '📝', '{"type": "doc", "content": [{"type": "table", "content": [{"type": "tableRow", "content": [{"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "항목"}]}]}, {"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "내용"}]}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "일시"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "참석자"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "작성자"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "안건"}]}, {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph"}]}, {"type": "listItem", "content": [{"type": "paragraph"}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "논의 내용"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "결정 사항"}]}, {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph"}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "할 일"}]}, {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "담당자 · 기한 · 내용"}]}]}]}]}'::jsonb, 0),
 ('incident_report', '장애 보고서', '영향·타임라인·원인·재발 방지', '🚨', '{"type": "doc", "content": [{"type": "callout", "attrs": {"tone": "danger"}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "심각도와 영향 범위를 먼저 적어 주세요. 관련 발매·DSP는 /release, /dsp 블록으로 연결하세요."}]}]}, {"type": "table", "content": [{"type": "tableRow", "content": [{"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "항목"}]}]}, {"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "내용"}]}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "심각도"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "P0 / P1 / P2"}]}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "발생 시각"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "해결 시각"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "영향 범위"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "담당자"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "요약"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "타임라인"}]}, {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "hh:mm — 감지"}]}]}, {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "hh:mm — 조치"}]}]}, {"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "hh:mm — 복구"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "원인"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "조치 내용"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "재발 방지"}]}, {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph"}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "관련 업무"}]}, {"type": "paragraph", "content": [{"type": "text", "text": "/kanban 으로 관련 카드를 연결하세요."}]}]}'::jsonb, 1),
 ('dsp_issue_report', 'DSP 이슈 보고', 'DSP 전송 실패·반려 분석', '📡', '{"type": "doc", "content": [{"type": "table", "content": [{"type": "tableRow", "content": [{"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "항목"}]}]}, {"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "내용"}]}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "DSP"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "발매"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "오류 코드"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "최초 발생"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "재시도 횟수"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "현상"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "DSP 응답 / 반려 사유"}]}, {"type": "codeBlock", "attrs": {"language": null}, "content": [{"type": "text", "text": "응답 원문을 붙여 넣으세요"}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "원인 분석"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "조치"}]}, {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "메타데이터 수정"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "재전송"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "DSP 담당자 회신"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "재발 방지"}]}, {"type": "paragraph"}]}'::jsonb, 2),
 ('rights_review', '권리 검토', '권리 증빙·판단 근거 기록', '⚖️', '{"type": "doc", "content": [{"type": "callout", "attrs": {"tone": "info"}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "/release 블록으로 검토 대상 발매를 연결하면 최신 상태가 함께 보여요."}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "검토 대상"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "제출 서류"}]}, {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph"}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "확인 사항"}]}, {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "원권리자 서명"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "사용 범위(지역·기간·매체)"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "샘플·커버 허락 범위"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "판단"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "판단 근거"}]}, {"type": "paragraph"}]}'::jsonb, 3),
 ('policy', '정책', '목적·적용 범위·규정·예외', '📘', '{"type": "doc", "content": [{"type": "table", "content": [{"type": "tableRow", "content": [{"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "항목"}]}]}, {"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "내용"}]}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "정책 번호"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "시행일"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "책임 부서"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "검토 주기"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "목적"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "적용 범위"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "정책"}]}, {"type": "orderedList", "attrs": {"start": 1}, "content": [{"type": "listItem", "content": [{"type": "paragraph"}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "예외"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "변경 이력"}]}, {"type": "paragraph"}]}'::jsonb, 4),
 ('sop', 'SOP (표준 작업 절차)', '누가·언제·어떻게 — 단계별 절차', '🧭', '{"type": "doc", "content": [{"type": "callout", "attrs": {"tone": "info"}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "각 단계는 누구나 따라 할 수 있게 구체적으로 적어 주세요."}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "목적"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "담당"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "사전 조건"}]}, {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph"}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "절차"}]}, {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "1단계"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "2단계"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "3단계"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "확인 방법"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "문제가 생기면"}]}, {"type": "paragraph"}]}'::jsonb, 5),
 ('release_issue', '발매 이슈 조사', '발매 지연·오류 원인 조사', '🔎', '{"type": "doc", "content": [{"type": "callout", "attrs": {"tone": "warning"}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "/release, /qc, /distribution 블록으로 발매의 현재 상태를 함께 보여 주세요."}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "발매"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "문제"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "확인한 내용"}]}, {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "QC 결과"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "권리 검토 상태"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "DSP 전송 상태"}]}]}, {"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph", "content": [{"type": "text", "text": "아티스트 문의 내역"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "원인"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "조치와 고객 안내"}]}, {"type": "paragraph"}]}'::jsonb, 6),
 ('customer_escalation', '고객 에스컬레이션', '고객 요청·영향·대응 계획', '📣', '{"type": "doc", "content": [{"type": "table", "content": [{"type": "tableRow", "content": [{"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "항목"}]}]}, {"type": "tableHeader", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "내용"}]}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "고객"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "문의"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "요청 내용"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "긴급도"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}, {"type": "tableRow", "content": [{"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "답변 기한"}]}]}, {"type": "tableCell", "content": [{"type": "paragraph"}]}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "상황"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "고객 영향"}]}, {"type": "paragraph"}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "대응 계획"}]}, {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [{"type": "paragraph"}]}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "고객 안내 문구"}]}, {"type": "blockquote", "content": [{"type": "paragraph"}]}, {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "결과"}]}, {"type": "paragraph"}]}'::jsonb, 7);

CREATE FUNCTION work.doc_level(p_doc uuid, p_user uuid, p_role text) RETURNS int
 LANGUAGE sql STABLE SET search_path = pg_catalog AS $$
  SELECT CASE
   WHEN d.id IS NULL OR d.deleted_at IS NOT NULL THEN 0
   WHEN d.created_by = p_user THEN 4
   WHEN p_role = 'ADMIN' THEN 3
   ELSE greatest(
     CASE d.staff_access WHEN 'EDITOR' THEN 2 WHEN 'VIEWER' THEN 1 ELSE 0 END,
     coalesce((SELECT max(CASE p.level WHEN 'MANAGER' THEN 3 WHEN 'EDITOR' THEN 2 ELSE 1 END)
                 FROM work.doc_permissions p
                WHERE p.doc_id = d.id
                  AND ((p.principal_kind = 'USER' AND p.principal_id = p_user::text)
                    OR (p.principal_kind = 'ROLE' AND p.principal_id = p_role))), 0))
  END
  FROM (SELECT 1) one LEFT JOIN work.docs d ON d.id = p_doc
$$;

-- Comments are notes: replies (one level) and an optional anchor (block id / text range) for later.
ALTER TABLE work.notes
 ADD COLUMN parent_id uuid NULL REFERENCES work.notes ON DELETE RESTRICT,
 ADD COLUMN anchor jsonb NULL CHECK (anchor IS NULL OR (jsonb_typeof(anchor) = 'object' AND length(anchor::text) <= 2000));
CREATE INDEX notes_parent ON work.notes(parent_id) WHERE parent_id IS NOT NULL;

-- Images and files of a document (download follows the document's ACL).
ALTER TABLE work.attachments ADD COLUMN doc_id uuid NULL REFERENCES work.docs ON DELETE RESTRICT;
CREATE INDEX attachments_doc ON work.attachments(doc_id) WHERE doc_id IS NOT NULL;

-- New linkable kinds: internal documents and tracks; documents can hold links.
ALTER TABLE work.notes DROP CONSTRAINT notes_target_kind_check,
 ADD CONSTRAINT notes_target_kind_check CHECK (target_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task','relationship','channel','doc','track','approval'));
ALTER TABLE work.links DROP CONSTRAINT links_entity_kind_check,
 ADD CONSTRAINT links_entity_kind_check CHECK (entity_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task','relationship','channel','doc','track','approval'));
ALTER TABLE work.links DROP CONSTRAINT links_subject_kind_check,
 ADD CONSTRAINT links_subject_kind_check CHECK (subject_kind IN ('task','relationship','doc'));
-- links a document holds because its content embeds them (kept in sync on save)
ALTER TABLE work.links ADD COLUMN origin text NOT NULL DEFAULT 'manual' CHECK (origin IN ('manual','embed'));

-- Live state per DSP in document embeds: staff read across workspaces like
-- the other delivery tables (0078).
CREATE POLICY live_bindings_staff_read ON execution.live_bindings FOR SELECT
 USING (current_setting('app.staff', true) = 'on');

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT SELECT, INSERT, UPDATE ON work.boards, work.board_columns, work.board_rules, work.docs, work.doc_templates TO audeniq_api';
    EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON work.doc_permissions TO audeniq_api';
    EXECUTE 'GRANT SELECT, INSERT ON work.doc_versions TO audeniq_api';
    EXECUTE 'GRANT EXECUTE ON FUNCTION work.doc_level(uuid, uuid, text) TO audeniq_api';
  END IF;
END
$$;
