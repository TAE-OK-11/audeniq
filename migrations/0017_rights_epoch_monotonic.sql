-- F5: rights_epochs is a version counter, but the F3 blanket immutable
-- trigger made the epoch write-once: nothing could ever bump it, so E-1's
-- rights-drift guard could never fire. Replace the trigger with a monotonic
-- guard: the (org_id, release_id) identity stays frozen and the epoch may
-- only strictly increase. DELETE stays forbidden (epoch rows are evidence).
-- NOTE: automatic epoch bumps when grants/overrides change are still F3
-- follow-up; F5 only needs the counter to be bumpable so the E-1 guard is
-- real and testable (DSP-08).
DROP TRIGGER immutable ON rights.rights_epochs;
CREATE OR REPLACE FUNCTION rights.guard_epoch_mutation() RETURNS trigger AS $$
BEGIN
  IF NEW.org_id IS DISTINCT FROM OLD.org_id
     OR NEW.release_id IS DISTINCT FROM OLD.release_id THEN
    RAISE EXCEPTION 'immutable epoch identity';
  END IF;
  IF NEW.epoch IS NULL OR NEW.epoch <= OLD.epoch THEN
    RAISE EXCEPTION 'rights epoch must strictly increase (was %, got %)', OLD.epoch, NEW.epoch;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER rights_epochs_monotonic
 BEFORE UPDATE ON rights.rights_epochs
 FOR EACH ROW EXECUTE FUNCTION rights.guard_epoch_mutation();
CREATE TRIGGER rights_epochs_no_delete
 BEFORE DELETE ON rights.rights_epochs
 FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();
