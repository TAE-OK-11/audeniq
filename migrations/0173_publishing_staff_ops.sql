-- 0173: staff answers to what artists ask in publishing, and royalties on the server.
--
--  * Share changes of a managed work (pending_contributors): staff apply or reject them. The outcome and the
--    reason stay on the work so the artist sees why.
--  * Contract renewal / termination requests: staff renew (end date moves) or end the contract, or reject the
--    request with a reason.
--  * Open A&R: staff claim, ask for a fix, review, propose, close; the artist answers a fix with a note.
--  * Royalties: staff import a society statement (one row per work and kind); each row keeps the amount collected
--    for AUDENIQ's managed share, the commission of the work's contract and the artist's net. Rows start estimated
--    or confirmed and are marked paid per statement. Posting to the shared balance stays with the finance owner CLI.
ALTER TABLE portal.pub_works
 ADD COLUMN change_outcome text NULL CHECK (change_outcome IN ('applied','rejected')),
 ADD COLUMN change_note text NULL CHECK (length(change_note) <= 1000),
 ADD COLUMN change_decided_at timestamptz NULL;

ALTER TABLE portal.pub_contracts
 ADD COLUMN request_outcome text NULL CHECK (request_outcome IN ('renewed','renewal_rejected','ended','termination_rejected')),
 ADD COLUMN request_note text NULL CHECK (length(request_note) <= 1000),
 ADD COLUMN request_decided_at timestamptz NULL;

ALTER TABLE portal.pub_ar_submissions
 ADD COLUMN claimed_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 ADD COLUMN fix_reply text NULL CHECK (length(fix_reply) <= 1000);

CREATE TABLE portal.pub_royalty_statements (
 id uuid PRIMARY KEY,
 source text NOT NULL CHECK (length(btrim(source)) BETWEEN 1 AND 60),
 period text NOT NULL CHECK (period ~ '^[0-9]{4}-(0[1-9]|1[0-2])$'),
 status text NOT NULL CHECK (status IN ('estimated','confirmed','paid')),
 file_name text NULL CHECK (length(file_name) <= 200),
 row_count integer NOT NULL CHECK (row_count >= 0),
 total_collected bigint NOT NULL CHECK (total_collected >= 0),
 imported_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE portal.pub_royalties (
 id uuid PRIMARY KEY,
 statement_id uuid NOT NULL REFERENCES portal.pub_royalty_statements ON DELETE CASCADE,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 work_id uuid NOT NULL,
 kind text NOT NULL CHECK (kind IN ('performance','mechanical','digital','sync','youtube','print','other')),
 source text NOT NULL,
 period text NOT NULL,
 country text NOT NULL CHECK (country ~ '^[A-Z]{2}$|^WORLD$'),
 currency text NOT NULL DEFAULT 'KRW' CHECK (currency ~ '^[A-Z]{3}$'),
 collected bigint NOT NULL CHECK (collected >= 0),
 share_bps integer NOT NULL CHECK (share_bps BETWEEN 0 AND 10000),
 fee bigint NOT NULL CHECK (fee >= 0 AND fee <= collected),
 net bigint NOT NULL CHECK (net = collected - fee),
 status text NOT NULL CHECK (status IN ('estimated','confirmed','paid')),
 paid_at timestamptz NULL,
 FOREIGN KEY (org_id, work_id) REFERENCES portal.pub_works(org_id, id) ON DELETE RESTRICT
);
CREATE INDEX pub_royalties_org ON portal.pub_royalties(org_id, period DESC);
CREATE INDEX pub_royalties_work ON portal.pub_royalties(work_id, period DESC);
CREATE INDEX pub_royalties_statement ON portal.pub_royalties(statement_id);
