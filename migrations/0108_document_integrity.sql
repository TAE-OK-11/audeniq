-- 전자문서 무결성·원본 보관.
--
-- 1) 문서 봉인 (operations.document_seals)
--    신청서 접수, 계약서 서명, 권리 서류 서명, 서명 요청 서명·거절, 약관 동의가 일어나는 그 트랜잭션에서
--    문서 내용(정규화 JSON)의 SHA-256을 서버 키(Ed25519)로 전자서명하고, 앞 봉인의 해시를 이어
--    해시 체인을 만든다. DB의 문서 행이 나중에 바뀌면 다시 계산한 해시가 봉인과 달라지고,
--    봉인 행을 바꾸거나 지우면 체인이 끊긴다. 체인의 끝은 매일 외부 WORM 저장소에 고정한다(anchor).
-- 2) 약관 동의 기록 (identity.terms_acceptances): 어떤 판의 약관에 언제 동의했는지 서버에 남긴다.
-- 3) 원본 보관 (operations.archive_runs / archive_objects / integrity_anchors): 매달 문서 원본을
--    별도 저장소(S3·R2, 객체 잠금)에 올리고, 서명된 목록(manifest)으로 묶는다.
-- 모두 추가만 할 수 있다(수정·삭제 불가, 보존기간이 끝난 보관 객체의 파기 표시는 예외).

CREATE TABLE operations.document_seals (
 seq bigserial PRIMARY KEY,
 doc_kind text NOT NULL CHECK(doc_kind IN ('APPLICATION','AGREEMENT','RIGHTS_DOCUMENT','SIGNING_REQUEST','TERMS_ACCEPTANCE')),
 doc_id uuid NOT NULL,
 org_id uuid NULL,
 event text NOT NULL CHECK(length(event) BETWEEN 1 AND 40),
 content_sha256 text NOT NULL CHECK(content_sha256 ~ '^[0-9a-f]{64}$'),
 sealed_at timestamptz NOT NULL DEFAULT now(),
 -- Ed25519 서명(64바이트)과 키 버전. 키가 없는 환경에서는 비어 있다(해시 체인만).
 signature bytea NULL CHECK(signature IS NULL OR octet_length(signature)=64),
 key_version smallint NULL,
 prev_hash text NOT NULL CHECK(prev_hash ~ '^[0-9a-f]{64}$'),
 hash text NOT NULL UNIQUE CHECK(hash ~ '^[0-9a-f]{64}$'),
 CHECK((signature IS NULL) = (key_version IS NULL))
);
CREATE INDEX document_seals_doc ON operations.document_seals(doc_kind, doc_id, seq DESC);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON operations.document_seals
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

CREATE TABLE identity.terms_acceptances (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 doc text NOT NULL CHECK(doc IN ('TERMS','PRIVACY_NOTICE','SETTLEMENT_TERMS')),
 version text NOT NULL CHECK(version ~ '^[A-Za-z0-9 ._-]{1,60}$'),
 channel text NOT NULL CHECK(channel IN ('SIGNUP','GOOGLE','APPLE','REACCEPT')),
 accepted_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(user_id, doc, version)
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON identity.terms_acceptances
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

CREATE TABLE operations.archive_runs (
 id uuid PRIMARY KEY,
 period text NOT NULL CHECK(period ~ '^[0-9]{4}-[0-9]{2}$'),
 operator text NOT NULL CHECK(length(btrim(operator)) BETWEEN 1 AND 120),
 software text NOT NULL CHECK(length(software) BETWEEN 1 AND 120),
 started_at timestamptz NOT NULL DEFAULT now(),
 finished_at timestamptz NULL,
 status text NOT NULL DEFAULT 'RUNNING' CHECK(status IN ('RUNNING','COMPLETED','FAILED')),
 uploaded integer NOT NULL DEFAULT 0,
 total_objects integer NOT NULL DEFAULT 0,
 manifest_key text NULL,
 manifest_sha256 text NULL CHECK(manifest_sha256 IS NULL OR manifest_sha256 ~ '^[0-9a-f]{64}$'),
 prev_manifest_sha256 text NULL CHECK(prev_manifest_sha256 IS NULL OR prev_manifest_sha256 ~ '^[0-9a-f]{64}$'),
 error text NULL CHECK(error IS NULL OR length(error) <= 500)
);
CREATE UNIQUE INDEX archive_runs_one_completed ON operations.archive_runs(period) WHERE status='COMPLETED';

CREATE TABLE operations.archive_objects (
 id uuid PRIMARY KEY,
 object_kind text NOT NULL CHECK(object_kind IN ('DOCUMENT','FILE')),
 doc_kind text NOT NULL,
 doc_id uuid NOT NULL,
 seal_seq bigint NULL REFERENCES operations.document_seals ON DELETE RESTRICT,
 content_sha256 text NOT NULL CHECK(content_sha256 ~ '^[0-9a-f]{64}$'),
 object_key text NOT NULL UNIQUE CHECK(length(object_key) BETWEEN 1 AND 512),
 object_sha256 text NOT NULL CHECK(object_sha256 ~ '^[0-9a-f]{64}$'),
 size_bytes bigint NOT NULL CHECK(size_bytes >= 0),
 encrypted boolean NOT NULL,
 key_version smallint NULL,
 run_id uuid NOT NULL REFERENCES operations.archive_runs ON DELETE RESTRICT,
 archived_at timestamptz NOT NULL DEFAULT now(),
 -- 법정 보존 기한 (상법 제33조: 영업에 관한 중요서류 10년)
 retain_until date NOT NULL,
 purged_at timestamptz NULL,
 UNIQUE(object_kind, doc_kind, doc_id, content_sha256)
);
CREATE INDEX archive_objects_retention ON operations.archive_objects(retain_until) WHERE purged_at IS NULL;
-- 보관 객체 기록은 보존기한이 지난 뒤의 파기 표시(purged_at)만 바꿀 수 있다.
CREATE FUNCTION operations.archive_objects_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN RAISE EXCEPTION 'archive objects are append-only'; END IF;
  IF OLD.purged_at IS NOT NULL OR NEW.purged_at IS NULL OR OLD.retain_until > current_date
     OR (to_jsonb(NEW) - 'purged_at') IS DISTINCT FROM (to_jsonb(OLD) - 'purged_at') THEN
    RAISE EXCEPTION 'archive objects change only to mark expired ones purged';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER guard BEFORE UPDATE OR DELETE ON operations.archive_objects
 FOR EACH ROW EXECUTE FUNCTION operations.archive_objects_guard();

-- 봉인 체인의 끝을 외부 WORM 저장소에 고정한 기록 (매일, 매달 보관 때)
CREATE TABLE operations.integrity_anchors (
 id uuid PRIMARY KEY,
 head_seq bigint NOT NULL,
 head_hash text NOT NULL CHECK(head_hash ~ '^[0-9a-f]{64}$'),
 object_key text NOT NULL UNIQUE,
 anchored_at timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON operations.integrity_anchors
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- Google·Apple 가입도 동의한 약관 판을 state에 실어 계정을 만들 때 기록한다.
ALTER TABLE identity.oauth_states
 ADD COLUMN terms_version text NULL CHECK(terms_version IS NULL OR terms_version ~ '^[A-Za-z0-9 ._-]{1,60}$'),
 ADD COLUMN privacy_notice_version text NULL CHECK(privacy_notice_version IS NULL OR privacy_notice_version ~ '^[A-Za-z0-9 ._-]{1,60}$');
