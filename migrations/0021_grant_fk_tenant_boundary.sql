-- F3 follow-up: tenant boundary on grant foreign keys.
--
-- rights.grant_atoms.parent_grant_id and .contract_revision_id referenced the
-- bare global id, so a grant in org A could name a parent grant or contract
-- revision belonging to org B. Both are now composite (org_id, id) foreign
-- keys, matching the tenant-scoped pattern used elsewhere
-- (e.g. FOREIGN KEY(org_id,party_id) REFERENCES identity.parties(org_id,id)).
--
-- contract_revisions only had UNIQUE(org_id,contract_id,id); the new FK needs
-- a UNIQUE(org_id,id) target, added here. grant_atoms already has
-- UNIQUE(org_id,id) from 0009.
--
-- The old single-column FKs were created inline (PG-generated names), so a
-- DO block drops whichever FK constraints sit on those two columns rather
-- than guessing constraint names. Existing rows are fully validated by the
-- new constraints: any cross-org link fails the migration instead of being
-- silently grandfathered.

-- FK target for (org_id, contract_revision_id).
ALTER TABLE rights.contract_revisions
  ADD CONSTRAINT contract_revisions_org_id_key UNIQUE (org_id, id);

-- Drop the old tenant-blind FKs on the two columns.
DO $$
DECLARE
  col text;
  cname text;
BEGIN
  FOREACH col IN ARRAY ARRAY['parent_grant_id', 'contract_revision_id'] LOOP
    SELECT con.conname INTO cname
    FROM pg_constraint con
    WHERE con.conrelid = 'rights.grant_atoms'::regclass
      AND con.contype = 'f'
      AND con.conkey = ARRAY(
        SELECT a.attnum
        FROM pg_attribute a
        WHERE a.attrelid = 'rights.grant_atoms'::regclass
          AND a.attname = col
      );
    IF cname IS NOT NULL THEN
      EXECUTE format('ALTER TABLE rights.grant_atoms DROP CONSTRAINT %I', cname);
    END IF;
  END LOOP;
END $$;

-- Tenant-scoped replacements.
ALTER TABLE rights.grant_atoms
  ADD CONSTRAINT grant_atoms_parent_org_fkey
    FOREIGN KEY (org_id, parent_grant_id)
    REFERENCES rights.grant_atoms (org_id, id)
    ON DELETE RESTRICT,
  ADD CONSTRAINT grant_atoms_contract_rev_org_fkey
    FOREIGN KEY (org_id, contract_revision_id)
    REFERENCES rights.contract_revisions (org_id, id)
    ON DELETE RESTRICT;
