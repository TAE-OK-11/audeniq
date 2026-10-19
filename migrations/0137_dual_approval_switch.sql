-- 0137: 2인 승인을 끄면(DUAL_APPROVAL_ENABLED=false) 모두 1인 처리 (docs/REVIEW_OVERRIDES.md).
-- 꺼지기 전에 남은 스태프 2차 승인 요청은 요청한 사람도 닫을 수 있는데, 이 테이블의 CHECK가
-- decided_by = requested_by 승인을 막아 오류가 났다. 다른 사람이어야 한다는 규칙은 스위치가
-- 켜져 있을 때 코드(crates/core/src/staff.rs)가 지킨다. review_overrides의 CHECK(실행자 ≠ 2차 승인자)는
-- 그대로 두고, 혼자 처리한 기록은 2차 승인자를 비워 둔다.
DO $$
DECLARE c text;
BEGIN
  FOR c IN SELECT conname FROM pg_constraint
            WHERE conrelid = 'rights.staff_approvals'::regclass AND contype = 'c'
              AND pg_get_constraintdef(oid) LIKE '%decided_by <> requested_by%'
  LOOP
    EXECUTE format('ALTER TABLE rights.staff_approvals DROP CONSTRAINT %I', c);
  END LOOP;
END $$;
