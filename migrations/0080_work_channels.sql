-- 0080: 업무 — 직원 채널(팀 대화). 설계: docs/WORK_MANAGEMENT_PLAN.md
--
-- Messages are internal notes (`work.notes`, target_kind = 'channel'): the same
-- @mentions (→ inbox), attachments (object store + scan), edit history and
-- archive as notes on any other record. Only the channel itself and each
-- person's read position are new.

CREATE TABLE work.channels (
 id uuid PRIMARY KEY,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 40),
 topic text NOT NULL DEFAULT '' CHECK (length(topic) <= 200),
 -- NULL = created by the system (default channels)
 created_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 archived_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 CHECK ((archived_at IS NULL) = (archived_by IS NULL))
);
CREATE UNIQUE INDEX channels_name ON work.channels(lower(btrim(name))) WHERE archived_at IS NULL;

INSERT INTO work.channels(id, name, topic) VALUES
 (gen_random_uuid(), '전체', '모든 직원 공지와 일반 대화'),
 (gen_random_uuid(), '발매 심사', '심사 기준·판단이 애매한 발매 논의'),
 (gen_random_uuid(), '배급', 'DSP 전송·반려·장애 공유'),
 (gen_random_uuid(), '고객 지원', '문의 응대 방법과 에스컬레이션'),
 (gen_random_uuid(), '정산', '리포트 대사·지급 일정');

-- Where each person has read up to (unread = newer messages by others).
CREATE TABLE work.channel_reads (
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 channel_id uuid NOT NULL REFERENCES work.channels ON DELETE RESTRICT,
 last_read_at timestamptz NOT NULL,
 PRIMARY KEY (user_id, channel_id)
);

CREATE INDEX notes_channel_recent ON work.notes(target_id, created_at DESC) WHERE target_kind = 'channel';

ALTER TABLE work.notes DROP CONSTRAINT notes_target_kind_check,
 ADD CONSTRAINT notes_target_kind_check CHECK (target_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task','relationship','channel'));
ALTER TABLE work.links DROP CONSTRAINT links_entity_kind_check,
 ADD CONSTRAINT links_entity_kind_check CHECK (entity_kind IN ('release','org','artist','label','dsp','inquiry','document','payout','settlement_report','task','relationship','channel'));

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT SELECT, INSERT, UPDATE ON work.channels, work.channel_reads TO audeniq_api';
  END IF;
END
$$;
