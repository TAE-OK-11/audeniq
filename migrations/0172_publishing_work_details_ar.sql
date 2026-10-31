-- 0172: what societies need to register a work, and Open A&R submissions on the server.
--
-- Work details (asked on the song form, all optional unless the work type needs them):
--  * pub_work_contributors.publisher_name — the co-writer's own publisher, if they have one (registration names it).
--  * pub_works.original_work — for an arrangement / translation / medley: the original's title, writers, publisher
--    and whether the owner's permission is in hand ({"title","writers","publisher","permission"}).
--  * pub_works.samples — recordings or melodies used from other songs ([{"kind":"sample"|"interpolation","title","artist","owners"}]).
--  * pub_works.prior_publisher_until — the day an earlier publisher's deal ends; collection starts after it.
ALTER TABLE portal.pub_work_contributors
 ADD COLUMN publisher_name text NULL CHECK (length(publisher_name) <= 200);
ALTER TABLE portal.pub_works
 ADD COLUMN original_work jsonb NULL CHECK (original_work IS NULL OR jsonb_typeof(original_work) = 'object'),
 ADD COLUMN samples jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(samples) = 'array' AND jsonb_array_length(samples) <= 10),
 ADD COLUMN prior_publisher_until date NULL;

-- Open A&R: a demo of a song not released yet, sent to AUDENIQ A&R for pitching. One open submission per work;
-- the same idempotency key returns the first. What happened is the audit trail (publishing.ar_*), like applications.
CREATE TABLE portal.pub_ar_submissions (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 work_id uuid NOT NULL,
 genre text NOT NULL CHECK (length(btrim(genre)) BETWEEN 1 AND 60),
 mood text[] NOT NULL CHECK (cardinality(mood) BETWEEN 1 AND 9),
 description text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
 refs text NOT NULL DEFAULT '' CHECK (length(refs) <= 500),
 scope text NOT NULL CHECK (scope IN ('internal','partners')),
 sync boolean NOT NULL DEFAULT false,
 status text NOT NULL DEFAULT 'submitted' CHECK (status IN ('submitted','in_review','needs_fix','reviewed','proposing','closed')),
 fix_reason text NULL CHECK (length(fix_reason) <= 1000),
 idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 8 AND 120),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 submitted_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (org_id, work_id) REFERENCES portal.pub_works(org_id, id) ON DELETE RESTRICT,
 UNIQUE (org_id, idempotency_key)
);
CREATE UNIQUE INDEX pub_ar_one_open ON portal.pub_ar_submissions(work_id) WHERE status <> 'closed';
CREATE INDEX pub_ar_org ON portal.pub_ar_submissions(org_id, submitted_at DESC);
