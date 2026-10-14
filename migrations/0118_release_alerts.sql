-- 0118: release alerts (crates/core/src/release_alerts.rs).
--
-- A fan leaves an email on a smart link before the release date and gets one
-- mail on release day. The address is sealed (payout key ring, field
-- ReleaseAlertEmail) and erased as soon as the mail is sent, cancelled or
-- expired; `email_bidx` (keyed blind index) only stops duplicate sign-ups.
CREATE TABLE portal.release_alerts (
 id uuid PRIMARY KEY,
 link_id uuid NOT NULL REFERENCES portal.smart_links ON DELETE CASCADE,
 email_cipher bytea NULL,
 email_bidx text NOT NULL CHECK (email_bidx ~ '^[0-9a-f]{64}$'),
 status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','SENT','FAILED','CANCELLED','EXPIRED')),
 attempts integer NOT NULL DEFAULT 0,
 next_attempt_at timestamptz NOT NULL DEFAULT now(),
 last_error text NULL CHECK (last_error IS NULL OR last_error ~ '^[A-Z_]{1,60}$'),
 provider text NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 sent_at timestamptz NULL,
 UNIQUE (link_id, email_bidx),
 -- Only a waiting alert keeps the address.
 CHECK ((status = 'PENDING') = (email_cipher IS NOT NULL))
);
CREATE INDEX release_alerts_pending ON portal.release_alerts(next_attempt_at) WHERE status = 'PENDING';
