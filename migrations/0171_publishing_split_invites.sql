-- 0171: co-writer split invites (공동 작가 지분 확인).
--
-- A co-writer confirms their creative share from a link — no AUDENIQ account needed, the link is the secret (as
-- portal.split_shares invites, 0119). The applicant copies the link to send it any way they like, and/or gives an
-- email address: the address is sealed (payout_keys Field::PubInviteEmail), mailed by the notification loop and
-- erased once answered. One live invite per contributor; asking again makes a new link (only its hash is kept).
-- The answer sets the contributor's confirm to confirmed / disputed (a dispute keeps the co-writer's note).
CREATE TABLE portal.pub_split_invites (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 work_id uuid NOT NULL,
 contributor_id uuid NOT NULL,
 token_hash text NOT NULL UNIQUE CHECK (token_hash ~ '^[0-9a-f]{64}$'),
 email_cipher bytea NULL,
 -- The link to mail, sealed until the mail is sent (the copied link and the mailed one are the same link).
 token_cipher bytea NULL,
 invite_status text NOT NULL DEFAULT 'NONE' CHECK (invite_status IN ('NONE','PENDING','SENT','FAILED','SKIPPED')),
 invite_attempts integer NOT NULL DEFAULT 0,
 invite_next_at timestamptz NOT NULL DEFAULT now(),
 answer text NULL CHECK (answer IN ('confirmed','disputed')),
 note text NULL CHECK (length(note) <= 1000),
 answered_at timestamptz NULL,
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (org_id, work_id) REFERENCES portal.pub_works(org_id, id) ON DELETE CASCADE,
 UNIQUE (work_id, contributor_id),
 CHECK ((answer IS NULL) = (answered_at IS NULL))
);
CREATE INDEX pub_split_invites_mail ON portal.pub_split_invites(invite_next_at) WHERE invite_status = 'PENDING';
