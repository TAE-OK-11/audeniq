-- 0139: 본인인증·실명인증 문의 분류 (crates/core/src/portal.rs CATEGORIES).
-- 인증 기관(PASS 등)으로 본인인증이 안 되는 사용자가 고객센터 실명인증(identity_desk)을 신청하는 통로.
DO $$
DECLARE c text;
BEGIN
  FOR c IN SELECT conname FROM pg_constraint
            WHERE conrelid = 'portal.inquiries'::regclass AND contype = 'c'
              AND pg_get_constraintdef(oid) LIKE '%category%'
  LOOP
    EXECUTE format('ALTER TABLE portal.inquiries DROP CONSTRAINT %I', c);
  END LOOP;
END $$;
ALTER TABLE portal.inquiries ADD CONSTRAINT inquiries_category_check
 CHECK(category IN ('RELEASE','SETTLEMENT','CONTRACT','ACCOUNT','IDENTITY','OTHER'));
