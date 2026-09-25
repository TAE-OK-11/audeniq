-- F3 follow-up: automatic rights epoch bumps.
--
-- The epoch counter existed and the E-1 rights-drift guard (preparation +
-- execution) read it, but nothing ever advanced it except a manual UPDATE.
-- Rights facts are append-only (the F3 immutable triggers still reject
-- UPDATE/DELETE on both tables), so INSERT is the only mutation point. This
-- migration adds AFTER INSERT triggers on rights.grant_atoms and
-- rights.review_overrides that bump rights.rights_epochs for every release
-- the new row touches:
--   * grant_atoms target_kind='RELEASE' -> target_id is the release id
--   * grant_atoms target_kind='TRACK'   -> resolved via catalog.tracks
--   * review_overrides                  -> resolved via the revision's release
-- A grant that names an unknown track (or an override on a revision outside
-- the org) fails the write instead of silently skipping the bump: rights
-- data must be fail-closed.
--
-- The bump runs in a SECURITY DEFINER function owned by the migration role.
-- The epoch write is a mandatory derived side effect of the rights write, so
-- runtime roles must not need extra direct grants for it: audeniq_worker
-- holds INSERT on review_overrides/rights_epochs but not UPDATE on
-- rights_epochs, and the bump must still succeed when the worker records an
-- override. search_path is locked down and every object is schema-qualified.
CREATE OR REPLACE FUNCTION rights.bump_epoch_for_rights_write()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
  v_release_id uuid;
BEGIN
  IF TG_TABLE_NAME = 'grant_atoms' THEN
    IF NEW.target_kind = 'RELEASE' THEN
      v_release_id := NEW.target_id;
    ELSIF NEW.target_kind = 'TRACK' THEN
      SELECT t.release_id INTO v_release_id
      FROM catalog.tracks t
      WHERE t.org_id = NEW.org_id AND t.id = NEW.target_id;
      IF v_release_id IS NULL THEN
        RAISE EXCEPTION 'rights grant references unknown track % (org %)',
          NEW.target_id, NEW.org_id;
      END IF;
    ELSE
      RAISE EXCEPTION 'unknown grant target_kind %', NEW.target_kind;
    END IF;
  ELSIF TG_TABLE_NAME = 'review_overrides' THEN
    SELECT r.release_id INTO v_release_id
    FROM catalog.application_revisions r
    WHERE r.org_id = NEW.org_id AND r.id = NEW.revision_id;
    IF v_release_id IS NULL THEN
      RAISE EXCEPTION 'review override references unknown revision % (org %)',
        NEW.revision_id, NEW.org_id;
    END IF;
  ELSE
    RAISE EXCEPTION 'bump_epoch_for_rights_write attached to unexpected table %',
      TG_TABLE_NAME;
  END IF;

  -- First rights write for a release seeds the row at 1; every later write
  -- increments. The 0017 monotonic trigger enforces NEW.epoch > OLD.epoch on
  -- the conflicting UPDATE branch.
  INSERT INTO rights.rights_epochs(org_id, release_id, epoch)
  VALUES (NEW.org_id, v_release_id, 1)
  ON CONFLICT (org_id, release_id)
  DO UPDATE SET epoch = rights.rights_epochs.epoch + 1;

  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS bump_rights_epoch ON rights.grant_atoms;
CREATE TRIGGER bump_rights_epoch
 AFTER INSERT ON rights.grant_atoms
 FOR EACH ROW EXECUTE FUNCTION rights.bump_epoch_for_rights_write();

DROP TRIGGER IF EXISTS bump_rights_epoch ON rights.review_overrides;
CREATE TRIGGER bump_rights_epoch
 AFTER INSERT ON rights.review_overrides
 FOR EACH ROW EXECUTE FUNCTION rights.bump_epoch_for_rights_write();
