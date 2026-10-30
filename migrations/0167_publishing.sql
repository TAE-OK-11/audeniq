-- 0167: publishing stage 1 (crates/core/src/publishing.rs, docs/PUBLISHING.md).
--
-- A musical work (작사·작곡 저작물) is separate from a recording (catalog.tracks):
-- one work can be recorded many times, and a work can exist before any
-- recording. Each work names its contributors and their creative shares
-- (basis points, 10000 = 100%) and how much of each share AUDENIQ is asked to
-- administer. The applicant submits works in a publishing administration
-- application (저작권 관리 신청); staff review it (approve, ask for fixes,
-- reject). Contracts, society registration and royalties are stage 2.
--
-- Shares are drafts until submitted: they may not add up to 100% while being
-- edited; submission requires exactly 100%. Once a work is registered, a share
-- change is held as a pending change until it is reviewed, and the registered
-- values stay in force (pending_contributors).
CREATE SEQUENCE portal.pub_work_code_seq START 101;
CREATE SEQUENCE portal.pub_application_seq START 1;

CREATE TABLE portal.pub_works (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 internal_code text NOT NULL UNIQUE CHECK (internal_code ~ '^AQW-[0-9]{6,}$'),
 title text NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 300),
 alt_titles text[] NOT NULL DEFAULT '{}' CHECK (cardinality(alt_titles) <= 10),
 work_type text NOT NULL DEFAULT 'original' CHECK (work_type IN ('original','arrangement','translation','instrumental','medley')),
 language text NOT NULL DEFAULT 'ko' CHECK (language IN ('ko','en','ja','zh','es','zxx','other')),
 genre text NOT NULL DEFAULT '' CHECK (length(genre) <= 60),
 description text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
 visibility text NOT NULL DEFAULT 'private' CHECK (visibility IN ('private','shared')),
 iswc text NULL CHECK (iswc ~ '^T-?[0-9]{3}\.?[0-9]{3}\.?[0-9]{3}-?[0-9]$'),
 has_prior_publisher boolean NOT NULL DEFAULT false,
 prior_publisher text NULL CHECK (length(prior_publisher) <= 200),
 registration text NOT NULL DEFAULT 'none' CHECK (registration IN
  ('none','preparing','internal_review','needs_fix','ready','submitted','external_review','registered','rejected')),
 -- Staff fix request: {"reason":..,"fields":[..],"documents":[..],"requestedAt":..}
 fix jsonb NULL,
 pending_contributors jsonb NULL,
 pending_at timestamptz NULL,
 row_version bigint NOT NULL DEFAULT 1,
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 archived_at timestamptz NULL,
 UNIQUE (org_id, id),
 CHECK ((pending_contributors IS NULL) = (pending_at IS NULL)),
 CHECK ((registration = 'needs_fix') OR fix IS NULL)
);
CREATE INDEX pub_works_org ON portal.pub_works(org_id, updated_at DESC) WHERE archived_at IS NULL;

CREATE TABLE portal.pub_work_contributors (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL,
 work_id uuid NOT NULL,
 position integer NOT NULL CHECK (position BETWEEN 0 AND 19),
 -- The AUDENIQ party behind this contributor, when known (the applicant).
 party_id uuid NULL,
 name text NOT NULL CHECK (length(name) <= 100),
 roles text[] NOT NULL DEFAULT '{}' CHECK (roles <@ ARRAY['lyricist','composer','arranger','other']::text[]),
 society text NULL CHECK (length(society) <= 60),
 ipi text NULL CHECK (ipi ~ '^[0-9]{1,11}$'),
 share_bps integer NOT NULL CHECK (share_bps BETWEEN 0 AND 10000),
 publisher_share_bps integer NOT NULL DEFAULT 0 CHECK (publisher_share_bps BETWEEN 0 AND 10000),
 confirm text NOT NULL DEFAULT 'pending' CHECK (confirm IN ('confirmed','pending','disputed')),
 external boolean NOT NULL DEFAULT true,
 confirm_requested_at timestamptz NULL,
 FOREIGN KEY (org_id, work_id) REFERENCES portal.pub_works(org_id, id) ON DELETE CASCADE,
 CHECK (publisher_share_bps <= share_bps),
 UNIQUE (work_id, position) DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX pub_work_contributors_work ON portal.pub_work_contributors(work_id);

CREATE TABLE portal.pub_work_recordings (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL,
 work_id uuid NOT NULL,
 release_id uuid NOT NULL,
 track_id uuid NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (org_id, work_id) REFERENCES portal.pub_works(org_id, id) ON DELETE CASCADE,
 FOREIGN KEY (org_id, release_id) REFERENCES catalog.releases(org_id, id) ON DELETE RESTRICT,
 FOREIGN KEY (org_id, track_id) REFERENCES catalog.tracks(org_id, id) ON DELETE RESTRICT,
 UNIQUE (work_id, track_id)
);
CREATE INDEX pub_work_recordings_track ON portal.pub_work_recordings(org_id, track_id);
CREATE INDEX pub_work_recordings_release ON portal.pub_work_recordings(org_id, release_id);

CREATE TABLE portal.pub_work_attachments (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL,
 work_id uuid NOT NULL,
 kind text NOT NULL CHECK (kind IN ('demo','lyrics','score','proof')),
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
 size_bytes bigint NULL CHECK (size_bytes >= 0),
 asset_id uuid NULL,
 -- Plain-text body (lyrics .txt) shown in place; files go through assets.
 text_body text NULL CHECK (length(text_body) <= 200000),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (org_id, work_id) REFERENCES portal.pub_works(org_id, id) ON DELETE CASCADE,
 FOREIGN KEY (org_id, asset_id) REFERENCES catalog.assets(org_id, id) ON DELETE RESTRICT
);
CREATE INDEX pub_work_attachments_work ON portal.pub_work_attachments(work_id);

-- Registration progress shown on the work (RegProgress): one row per step.
CREATE TABLE portal.pub_work_status_log (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL,
 work_id uuid NOT NULL,
 status text NOT NULL,
 note text NULL CHECK (length(note) <= 500),
 society text NULL,
 at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (org_id, work_id) REFERENCES portal.pub_works(org_id, id) ON DELETE CASCADE
);
CREATE INDEX pub_work_status_log_work ON portal.pub_work_status_log(work_id, at);

CREATE TABLE portal.pub_applications (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 number text NOT NULL UNIQUE,
 status text NOT NULL DEFAULT 'submitted' CHECK (status IN ('submitted','in_review','needs_fix','approved','rejected','withdrawn')),
 territories text[] NOT NULL CHECK (cardinality(territories) BETWEEN 1 AND 10),
 rights text[] NOT NULL CHECK (cardinality(rights) BETWEEN 1 AND 10),
 policy_version text NOT NULL,
 agreed text[] NOT NULL,
 signature_name text NOT NULL CHECK (length(btrim(signature_name)) BETWEEN 1 AND 200),
 signature_method text NOT NULL CHECK (signature_method IN ('typed','drawn')),
 signature_data text NULL CHECK (length(signature_data) <= 300000),
 idempotency_key text NOT NULL CHECK (length(idempotency_key) BETWEEN 8 AND 120),
 fix_reason text NULL CHECK (length(fix_reason) <= 2000),
 submitted_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 submitted_at timestamptz NOT NULL DEFAULT now(),
 decided_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 decided_at timestamptz NULL,
 UNIQUE (org_id, id),
 UNIQUE (org_id, idempotency_key)
);
CREATE INDEX pub_applications_queue ON portal.pub_applications(status, submitted_at) WHERE status IN ('submitted','in_review');

CREATE TABLE portal.pub_application_works (
 org_id uuid NOT NULL,
 application_id uuid NOT NULL,
 work_id uuid NOT NULL,
 PRIMARY KEY (application_id, work_id),
 FOREIGN KEY (org_id, application_id) REFERENCES portal.pub_applications(org_id, id) ON DELETE RESTRICT,
 FOREIGN KEY (org_id, work_id) REFERENCES portal.pub_works(org_id, id) ON DELETE RESTRICT
);
CREATE INDEX pub_application_works_work ON portal.pub_application_works(work_id);

-- What happened to an application, newest last (append-only).
CREATE TABLE portal.pub_application_events (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL,
 application_id uuid NOT NULL,
 text text NOT NULL CHECK (length(text) BETWEEN 1 AND 500),
 actor_user_id uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (org_id, application_id) REFERENCES portal.pub_applications(org_id, id) ON DELETE RESTRICT
);
CREATE INDEX pub_application_events_app ON portal.pub_application_events(application_id, at);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON portal.pub_application_events
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- The signed-in writer's own publishing details (활동명은 계정 프로필, IPI·단체는 여기).
CREATE TABLE portal.pub_creator_profiles (
 user_id uuid PRIMARY KEY REFERENCES identity.users ON DELETE RESTRICT,
 roles text[] NOT NULL DEFAULT '{}' CHECK (roles <@ ARRAY['artist','composer','lyricist','arranger','producer','label']::text[]),
 society text NULL CHECK (length(society) <= 60),
 ipi text NULL CHECK (ipi ~ '^[0-9]{1,11}$'),
 updated_at timestamptz NOT NULL DEFAULT now()
);
