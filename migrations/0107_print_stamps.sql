-- 출력물 식별: 약관·신청서·계약서·권리 서류를 인쇄·PDF로 내보낼 때마다 기록을 남긴다.
--  * 문서 식별코드(document_code): 같은 문서·같은 내용이면 늘 같은 코드.
--  * 원본 검증코드: 출력할 때마다 새로 발급. 종이에만 적히고 DB에는 해시로만 남는다(단방향).
--    누가(계정·서명 링크), 언제, 어느 기기(브라우저가 만든 기기 id·세션)·어디서(IP·브라우저)
--    출력했는지를 이 기록으로 확인한다. IP·브라우저 정보는 개인정보 키링으로 봉인한다.
CREATE TABLE operations.print_stamps (
 id uuid PRIMARY KEY,
 verification_hash bytea NOT NULL UNIQUE CHECK(octet_length(verification_hash)=32),
 document_code text NOT NULL CHECK(document_code ~ '^AQ-[A-Z]{3}-[0-9A-Z]{10}$'),
 doc_kind text NOT NULL CHECK(doc_kind IN ('TERMS','SETTLEMENT_TERMS','PRIVACY','APPLICATION','DOCUMENT','SIGNING','STAFF_SHEET')),
 doc_ref text NOT NULL CHECK(length(doc_ref) BETWEEN 1 AND 80),
 title text NOT NULL CHECK(length(btrim(title)) BETWEEN 1 AND 200),
 -- 화면에 보인 문서 본문의 SHA-256 (브라우저가 계산)
 content_hash text NULL CHECK(content_hash IS NULL OR content_hash ~ '^[0-9a-f]{64}$'),
 org_id uuid NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 user_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 signing_request_id uuid NULL REFERENCES portal.signing_requests ON DELETE RESTRICT,
 staff boolean NOT NULL DEFAULT false,
 session_id uuid NULL,
 device_id uuid NULL,
 -- {"ip":..., "user_agent":...} 봉인 (AAD: 열 + 이 행 id)
 source_cipher bytea NULL CHECK(source_cipher IS NULL OR octet_length(source_cipher) BETWEEN 29 AND 2048),
 printed_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX print_stamps_document ON operations.print_stamps(document_code, printed_at DESC);
CREATE INDEX print_stamps_user ON operations.print_stamps(user_id, printed_at DESC) WHERE user_id IS NOT NULL;
-- 출력 기록은 고칠 수 없다.
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON operations.print_stamps
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
