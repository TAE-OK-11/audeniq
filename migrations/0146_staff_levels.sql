-- 0146: staff levels and the admin console's operations (crate::staff_ops).
--
-- Four levels: SUPPORT (1) < REVIEWER, OPERATOR (2) < ADMIN (3) < OWNER (4).
-- OWNER is new: staff management and the riskiest operations. It is granted
-- only by the operator CLI (`audeniq-admin staff grant EMAIL OWNER`).
ALTER TABLE identity.staff_members DROP CONSTRAINT staff_members_role_check;
ALTER TABLE identity.staff_members ADD CONSTRAINT staff_members_role_check
  CHECK (role IN ('OWNER','ADMIN','REVIEWER','OPERATOR','SUPPORT'));

-- The console changes staff through these two functions only: the staff
-- operations login gets EXECUTE on them, never a write on staff_members or
-- staff_mfa. They hold the rule in the database itself: the actor is an
-- active OWNER, never changes their own row, never touches another OWNER,
-- and never grants OWNER.
CREATE FUNCTION identity.staff_set(p_actor uuid, p_target uuid, p_role text, p_by text)
 RETURNS text LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE
  prev text;
  prev_status text;
BEGIN
  IF NOT EXISTS (SELECT 1 FROM identity.staff_members s JOIN identity.users u ON u.id = s.user_id
                  WHERE s.user_id = p_actor AND s.role = 'OWNER' AND s.status = 'ACTIVE' AND u.status = 'ACTIVE') THEN
    RAISE EXCEPTION 'STAFF_OWNER_REQUIRED' USING ERRCODE = '42501';
  END IF;
  IF p_actor = p_target THEN
    RAISE EXCEPTION 'STAFF_SELF_CHANGE' USING ERRCODE = '42501';
  END IF;
  IF p_role IS NOT NULL AND p_role NOT IN ('ADMIN','REVIEWER','OPERATOR','SUPPORT') THEN
    RAISE EXCEPTION 'STAFF_ROLE_INVALID' USING ERRCODE = '22023';
  END IF;
  IF length(btrim(coalesce(p_by, ''))) NOT BETWEEN 1 AND 120 THEN
    RAISE EXCEPTION 'STAFF_ROLE_INVALID' USING ERRCODE = '22023';
  END IF;
  SELECT role, status INTO prev, prev_status FROM identity.staff_members WHERE user_id = p_target FOR UPDATE;
  IF prev = 'OWNER' AND prev_status = 'ACTIVE' THEN
    RAISE EXCEPTION 'STAFF_OWNER_PROTECTED' USING ERRCODE = '42501';
  END IF;
  IF p_role IS NULL THEN
    UPDATE identity.staff_members SET status = 'REVOKED', revoked_at = now()
     WHERE user_id = p_target AND status = 'ACTIVE';
    IF NOT FOUND THEN
      RAISE EXCEPTION 'STAFF_NOT_FOUND' USING ERRCODE = 'P0002';
    END IF;
  ELSE
    INSERT INTO identity.staff_members(user_id, role, granted_by) VALUES (p_target, p_role, btrim(p_by))
    ON CONFLICT (user_id) DO UPDATE SET role = EXCLUDED.role, status = 'ACTIVE', revoked_at = NULL,
      granted_by = EXCLUDED.granted_by, granted_at = now();
  END IF;
  RETURN CASE WHEN prev_status = 'ACTIVE' THEN prev END;
END $$;

-- Lost authenticator: the target enrolls a new one at the next sign-in.
CREATE FUNCTION identity.staff_reset_mfa(p_actor uuid, p_target uuid)
 RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM identity.staff_members s JOIN identity.users u ON u.id = s.user_id
                  WHERE s.user_id = p_actor AND s.role = 'OWNER' AND s.status = 'ACTIVE' AND u.status = 'ACTIVE') THEN
    RAISE EXCEPTION 'STAFF_OWNER_REQUIRED' USING ERRCODE = '42501';
  END IF;
  IF p_actor = p_target THEN
    RAISE EXCEPTION 'STAFF_SELF_CHANGE' USING ERRCODE = '42501';
  END IF;
  IF EXISTS (SELECT 1 FROM identity.staff_members WHERE user_id = p_target AND role = 'OWNER' AND status = 'ACTIVE') THEN
    RAISE EXCEPTION 'STAFF_OWNER_PROTECTED' USING ERRCODE = '42501';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM identity.staff_members WHERE user_id = p_target AND status = 'ACTIVE') THEN
    RAISE EXCEPTION 'STAFF_NOT_FOUND' USING ERRCODE = 'P0002';
  END IF;
  DELETE FROM identity.staff_mfa WHERE user_id = p_target;
  UPDATE identity.sessions SET revoked_at = now() WHERE user_id = p_target AND revoked_at IS NULL;
END $$;

REVOKE ALL ON FUNCTION identity.staff_set(uuid, uuid, text, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION identity.staff_reset_mfa(uuid, uuid) FROM PUBLIC;

-- Partner onboarding is operator-owned state (0027: RLS, no policy). The
-- staff operations login is the operator's, through the console; no other
-- runtime role gets a grant on it.
CREATE POLICY partner_onboarding_staff_ops ON execution.partner_onboarding
  USING (current_user = 'audeniq_staff_ops') WITH CHECK (current_user = 'audeniq_staff_ops');
