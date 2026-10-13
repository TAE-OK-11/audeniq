-- Email copies of studio notifications (crates/core/src/email.rs).
--
-- Every new portal.notifications row queues one email per recipient: the
-- addressed user, or every active member of the org for org-wide notices,
-- unless the user turned email notifications off. The API (which holds the
-- PII keyring for login emails) sends them over SMTP and records the result.
-- The queue holds no address or text: both are read at send time, so a
-- changed email or an opt-out before sending is respected.

CREATE TABLE portal.notification_settings (
  user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE RESTRICT,
  email_enabled boolean NOT NULL DEFAULT true,
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE portal.notification_emails (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  notification_id uuid NOT NULL REFERENCES portal.notifications ON DELETE CASCADE,
  user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
  status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','SENT','FAILED','SKIPPED')),
  attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  -- Also the send lease: a claim pushes it forward so another API replica
  -- does not pick the same row while it is being sent.
  next_attempt_at timestamptz NOT NULL DEFAULT now(),
  provider text NULL CHECK (provider IS NULL OR length(provider) <= 20),
  -- Fixed codes only (EMAIL_*), never a provider response or an address.
  last_error text NULL CHECK (last_error IS NULL OR last_error ~ '^[A-Z0-9_]{1,64}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  sent_at timestamptz NULL,
  UNIQUE (notification_id, user_id)
);
CREATE INDEX notification_emails_due ON portal.notification_emails (next_attempt_at) WHERE status = 'PENDING';
CREATE INDEX notification_emails_user ON portal.notification_emails (user_id);
CREATE INDEX notification_emails_done ON portal.notification_emails (created_at) WHERE status <> 'PENDING';

-- Runs as the migration role: notices are inserted by API and worker paths
-- (and by other triggers) that hold no grant on the email queue.
CREATE OR REPLACE FUNCTION portal.queue_notification_emails()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
  INSERT INTO portal.notification_emails (notification_id, user_id)
  SELECT NEW.id, m.user_id
  FROM identity.memberships m
  JOIN identity.users u ON u.id = m.user_id
  LEFT JOIN portal.notification_settings s ON s.user_id = m.user_id
  WHERE m.org_id = NEW.org_id
    AND m.status = 'ACTIVE'
    AND u.status = 'ACTIVE'
    AND (NEW.user_id IS NULL OR m.user_id = NEW.user_id)
    AND COALESCE(s.email_enabled, true)
  ON CONFLICT (notification_id, user_id) DO NOTHING;
  RETURN NEW;
END;
$$;
REVOKE ALL ON FUNCTION portal.queue_notification_emails() FROM PUBLIC;

CREATE TRIGGER notifications_queue_emails
AFTER INSERT ON portal.notifications
FOR EACH ROW EXECUTE FUNCTION portal.queue_notification_emails();
