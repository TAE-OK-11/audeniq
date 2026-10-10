-- 0079: 업무 Phase 3 — CRM Lite (아티스트·레이블·DSP·파트너 관계 관리). 설계: docs/WORK_MANAGEMENT_PLAN.md
--
-- A relationship points at the existing record (artist, label, DSP, workspace)
-- and only adds what AUDENIQ does not know yet: who owns the relationship, its
-- status, the next contact date, the people to call and the contact history.
-- Names, contract status (signed agreements, DSP contract route), releases and
-- delivery numbers are read from their sources. Only a partner or prospect with
-- no record of its own keeps a name (and, then, a manually tracked contract
-- status). Related releases / contracts are `work.links`, notes `work.notes`.
-- DSP operations need no table at all.

CREATE TABLE work.relationships (
 id uuid PRIMARY KEY,
 kind text NOT NULL CHECK (kind IN ('ARTIST','LABEL','DSP','PARTNER','ORG')),
 entity_kind text NULL CHECK (entity_kind IN ('artist','label','dsp','org')),
 entity_id text NULL CHECK (entity_id IS NULL OR entity_id ~ '^[A-Za-z0-9-]{1,64}$'),
 -- only without a source record
 name text NULL CHECK (name IS NULL OR length(btrim(name)) BETWEEN 1 AND 120),
 manual_contract_status text NULL CHECK (manual_contract_status IN ('NONE','NEGOTIATING','SIGNED','EXPIRED','TERMINATED')),
 owner_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('PROSPECT','ACTIVE','AT_RISK','INACTIVE')),
 next_contact_at timestamptz NULL,
 tags text[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 8),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 row_version int NOT NULL DEFAULT 1,
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((entity_kind IS NULL) = (entity_id IS NULL)),
 -- a name and a manual contract status only when there is no source record
 CHECK ((entity_kind IS NULL) = (name IS NOT NULL)),
 CHECK (entity_kind IS NULL OR manual_contract_status IS NULL),
 CHECK (entity_kind IS NULL OR kind = upper(entity_kind)),
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
-- one active relationship per source record
CREATE UNIQUE INDEX relationships_entity ON work.relationships(entity_kind, entity_id) WHERE archived_at IS NULL AND entity_kind IS NOT NULL;
CREATE INDEX relationships_owner ON work.relationships(owner_id) WHERE archived_at IS NULL;

-- People at the other side (new information; not stored anywhere else).
CREATE TABLE work.relationship_contacts (
 id uuid PRIMARY KEY,
 relationship_id uuid NOT NULL REFERENCES work.relationships ON DELETE RESTRICT,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
 role text NOT NULL DEFAULT '' CHECK (length(role) <= 80),
 email text NULL CHECK (email IS NULL OR (length(email) <= 254 AND email ~ '^[^@\s]+@[^@\s]+$')),
 phone text NULL CHECK (phone IS NULL OR phone ~ '^[0-9+() -]{4,32}$'),
 is_primary boolean NOT NULL DEFAULT false,
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE INDEX relationship_contacts_rel ON work.relationship_contacts(relationship_id) WHERE archived_at IS NULL;

-- Contact history (append-only). "Last contact" is the latest entry.
CREATE TABLE work.relationship_activities (
 id uuid PRIMARY KEY,
 relationship_id uuid NOT NULL REFERENCES work.relationships ON DELETE RESTRICT,
 kind text NOT NULL CHECK (kind IN ('CALL','EMAIL','MEETING','NOTE')),
 summary text NOT NULL CHECK (length(btrim(summary)) BETWEEN 1 AND 2000),
 contact_id uuid NULL REFERENCES work.relationship_contacts ON DELETE RESTRICT,
 at timestamptz NOT NULL,
 by_user uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX relationship_activities_rel ON work.relationship_activities(relationship_id, at DESC);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON work.relationship_activities
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- Relationships are linkable (tasks, notes, related releases / contracts).
ALTER TABLE work.links DROP CONSTRAINT links_subject_kind_check,
 ADD CONSTRAINT links_subject_kind_check CHECK (subject_kind IN ('task','relationship'));
ALTER TABLE work.links DROP CONSTRAINT links_entity_kind_check,
 ADD CONSTRAINT links_entity_kind_check CHECK (entity_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task','relationship'));
ALTER TABLE work.notes DROP CONSTRAINT notes_target_kind_check,
 ADD CONSTRAINT notes_target_kind_check CHECK (target_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task','relationship'));

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT SELECT, INSERT, UPDATE ON work.relationships, work.relationship_contacts TO audeniq_api';
    EXECUTE 'GRANT SELECT, INSERT ON work.relationship_activities TO audeniq_api';
  END IF;
END
$$;
