-- 0070: 발매 심사 담당 (review claim).
--
-- A reviewer claims a release before deciding it; from then on only that
-- reviewer can approve, request a correction or reject. ADMIN may take a
-- claim over (audited). One row per release; releasing the claim deletes it.
-- Second approval stays a different person's decision and is not claimed.
CREATE TABLE rights.review_claims (
 release_id uuid PRIMARY KEY REFERENCES catalog.releases ON DELETE CASCADE,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 claimed_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 claimed_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX review_claims_by ON rights.review_claims(claimed_by);

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audeniq_api') THEN
    EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON rights.review_claims TO audeniq_api';
  END IF;
END
$$;
