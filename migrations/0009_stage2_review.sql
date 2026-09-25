-- F3: Stage 2 review artifacts (BLUEPRINT §5).
-- grant_atoms: the atomic rights grants the 2-A/2-B engine checks.
CREATE TABLE rights.grant_atoms (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 party_id uuid NOT NULL, -- grantee
 target_kind text NOT NULL CHECK(target_kind IN ('RELEASE','TRACK')),
 target_id uuid NOT NULL,
 right_type text NOT NULL CHECK(length(btrim(right_type))>0),
 territory_set text[] NOT NULL DEFAULT '{}',
 use_set text[] NOT NULL DEFAULT '{}',
 start_at timestamptz, end_exclusive timestamptz,
 exclusive boolean NOT NULL DEFAULT false,
 sublicensable boolean NOT NULL DEFAULT false,
 parent_grant_id uuid REFERENCES rights.grant_atoms(id) ON DELETE RESTRICT,
 contract_revision_id uuid REFERENCES rights.contract_revisions(id) ON DELETE RESTRICT,
 revoked_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(org_id,party_id) REFERENCES identity.parties(org_id,id) ON DELETE RESTRICT,
 CHECK(end_exclusive IS NULL OR start_at IS NULL OR end_exclusive > start_at),
 CHECK(revoked_at IS NULL OR revoked_at >= created_at),
 UNIQUE(org_id,id)
);
CREATE INDEX grant_atoms_target ON rights.grant_atoms(org_id,target_kind,target_id,revoked_at);
-- Reviewer overrides never mutate check_results; they are separate rows.
CREATE TABLE rights.review_overrides (
 id uuid PRIMARY KEY, org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 revision_id uuid NOT NULL REFERENCES catalog.application_revisions ON DELETE RESTRICT,
 check_code text NOT NULL CHECK(length(btrim(check_code))>0),
 original_status text NOT NULL, proposed_status text NOT NULL
  CHECK(proposed_status IN ('PASS','CORRECTION_REQUIRED','REVIEW_REQUIRED','BLOCKED')),
 reason text NOT NULL CHECK(length(btrim(reason))>0),
 actor_user_id uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 second_approver_user_id uuid REFERENCES identity.users ON DELETE RESTRICT,
 expires_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK(actor_user_id <> second_approver_user_id),
 UNIQUE(org_id,id)
);
CREATE INDEX review_overrides_revision ON rights.review_overrides(org_id,revision_id);
-- Per-release rights epoch pinned into each verification package.
CREATE TABLE rights.rights_epochs (
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 epoch bigint NOT NULL CHECK(epoch>=0),
 PRIMARY KEY(org_id,release_id)
);
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['rights.grant_atoms','rights.review_overrides','rights.rights_epochs'] LOOP
  EXECUTE format('CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON %s FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation()',t);
 END LOOP;
END $$;
-- Deliberately no runtime grants for the new tables (same stance as F2's
-- validation_packages/check_results): deploy/grants.sql stays closed until the
-- runtime-role hardening pass.
