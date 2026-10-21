-- 0138: 미성년자도 직원 실명인증을 받을 수 있게 (crates/core/src/identity_desk.rs).
--
-- 만 17세 미만은 주민등록증이 없다. 미성년자만 쓸 수 있는 서류 두 가지를 더한다.
-- - YOUTH_CARD: 청소년증 (청소년복지 지원법 — 법령상 신분증)
-- - FAMILY_RELATION_CERT: 가족관계증명서 + 동행한 법정대리인의 신분증
-- 법정대리인과 함께 확인했으면 관계(부모·후견인)를 남긴다. 법정대리인의 이름·신분증 번호는 받지 않는다.
-- 미성년자인지는 서버가 생년월일로 판단한다 (CHECK로는 오늘 날짜를 쓸 수 없다).
ALTER TABLE portal.offline_identity_checks DROP CONSTRAINT IF EXISTS offline_identity_checks_document_type_check;
ALTER TABLE portal.offline_identity_checks
 ADD CONSTRAINT offline_identity_checks_document_type_check CHECK(document_type IN (
   'RESIDENT_CARD','DRIVER_LICENSE','PASSPORT','ALIEN_REGISTRATION','DISABILITY_CARD','VETERAN_CARD',
   'YOUTH_CARD','FAMILY_RELATION_CERT')),
 ADD COLUMN guardian_relation text CHECK(guardian_relation IN ('PARENT','GUARDIAN')),
 ADD CONSTRAINT offline_identity_checks_family_cert_guardian
   CHECK(document_type <> 'FAMILY_RELATION_CERT' OR guardian_relation IS NOT NULL);
