-- 0148: guardian consent for a minor artist's release (crates/core/src/signing.rs guardian_consent).
-- The legal guardian verifies their identity and signs electronically like a rights holder; the
-- document kind is 'guardian' and the signer role '법정대리인' (only together). Submission of a
-- release declared as a minor's needs it signed and a family relation certificate attached
-- (crates/core/src/submission.rs); staff confirm the relation in review.
ALTER TABLE portal.signing_requests DROP CONSTRAINT signing_requests_document_kind_check;
ALTER TABLE portal.signing_requests ADD CONSTRAINT signing_requests_document_kind_check
  CHECK(document_kind IN ('master','artwork','composition','sample','performer','shared','guardian'));
ALTER TABLE portal.signing_requests DROP CONSTRAINT signing_requests_signer_role_check;
ALTER TABLE portal.signing_requests ADD CONSTRAINT signing_requests_signer_role_check
  CHECK(signer_role IN ('권리자 본인','권리자의 위임을 받은 대리인','법인 대표자','법정대리인'));
ALTER TABLE portal.signing_requests ADD CONSTRAINT signing_requests_guardian_role
  CHECK((document_kind = 'guardian') = (signer_role = '법정대리인'));
-- The family relation certificate that goes with the consent: a rights document of the release
-- marked so submission and review can find it.
ALTER TABLE portal.documents ADD COLUMN purpose text NULL CHECK(purpose IS NULL OR purpose IN ('GUARDIAN_RELATION'));
CREATE INDEX signing_requests_guardian ON portal.signing_requests(release_id) WHERE document_kind='guardian';
