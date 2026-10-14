-- 0115: metadata corrections of releases already sent to platforms
-- (crates/core/src/corrections.rs).
--
-- Only changes platforms take as a plain metadata update are allowed: track
-- credits (composer, lyricist, arranger, producer, engineer, ...) and the
-- genre. Title, artist names, featured artists, audio, identifiers, release
-- date and rights lines stay locked (a new release instead). The artist asks
-- with a reason; staff with the review duty approve (the live catalog is
-- corrected and an update message goes to every platform still carrying the
-- release, `delivery.update` jobs) or decline with a note.
CREATE TABLE portal.metadata_corrections (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 package_id uuid NOT NULL,
 requested_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 changes jsonb NOT NULL CHECK (jsonb_typeof(changes) = 'object'),
 -- What approval wrote: credits resolved to the org's parties.
 applied jsonb NULL,
 reason text NOT NULL CHECK (length(btrim(reason)) BETWEEN 1 AND 1000),
 status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','APPROVED','DECLINED','CANCELLED')),
 decided_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 decided_at timestamptz NULL,
 decision_note text NOT NULL DEFAULT '' CHECK (length(decision_note) <= 1000),
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK ((status = 'PENDING') = (decided_at IS NULL)),
 CHECK ((status = 'APPROVED') = (applied IS NOT NULL))
);
CREATE UNIQUE INDEX metadata_corrections_one_open ON portal.metadata_corrections(release_id) WHERE status = 'PENDING';
CREATE INDEX metadata_corrections_applied ON portal.metadata_corrections(release_id, decided_at) WHERE status = 'APPROVED';
CREATE INDEX metadata_corrections_open ON portal.metadata_corrections(created_at) WHERE status = 'PENDING';

CREATE FUNCTION portal.metadata_correction_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text;
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 SELECT coalesce(nullif(btrim(title), ''), '발매') INTO t FROM catalog.releases WHERE id = NEW.release_id;
 IF NEW.status = 'PENDING' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 정보 수정 요청이 접수됐어요.',
   '담당자가 확인한 뒤 각 플랫폼에 수정 내용을 보내요.', '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'APPROVED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 정보 수정을 플랫폼에 보냈어요.',
   '플랫폼마다 반영되는 데 며칠 걸릴 수 있어요.', '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'DECLINED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 정보 수정 요청이 반려됐어요.',
   left(coalesce(nullif(NEW.decision_note, ''), '자세한 내용은 문의로 확인해 주세요.'), 300), '/releases/' || NEW.release_id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER metadata_corrections_notice AFTER INSERT OR UPDATE OF status ON portal.metadata_corrections
 FOR EACH ROW EXECUTE FUNCTION portal.metadata_correction_notice();
