-- 0116: playlist pitching (crates/core/src/pitching.rs).
--
-- The artist describes one focus track of a submitted release for platform
-- editors at least 7 days before the release date; staff forward it to the
-- platforms' editorial teams and record the outcome. One active pitch per
-- release (a withdrawn one can be replaced). Each step notifies the org.
CREATE TABLE portal.playlist_pitches (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NOT NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 track_id uuid NOT NULL REFERENCES catalog.tracks ON DELETE RESTRICT,
 requested_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 description text NOT NULL CHECK (length(btrim(description)) BETWEEN 20 AND 1000),
 genres text NOT NULL DEFAULT '' CHECK (length(genres) <= 200),
 moods text NOT NULL DEFAULT '' CHECK (length(moods) <= 200),
 instruments text NOT NULL DEFAULT '' CHECK (length(instruments) <= 200),
 similar_artists text NOT NULL DEFAULT '' CHECK (length(similar_artists) <= 200),
 marketing text NOT NULL DEFAULT '' CHECK (length(marketing) <= 1000),
 status text NOT NULL DEFAULT 'SUBMITTED'
   CHECK (status IN ('SUBMITTED','IN_REVIEW','PITCHED','SELECTED','NOT_SELECTED','WITHDRAWN')),
 platforms text[] NOT NULL DEFAULT '{}',
 result text NOT NULL DEFAULT '' CHECK (length(result) <= 300),
 staff_note text NOT NULL DEFAULT '' CHECK (length(staff_note) <= 1000),
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX playlist_pitches_one_active ON portal.playlist_pitches(release_id) WHERE status <> 'WITHDRAWN';
CREATE INDEX playlist_pitches_queue ON portal.playlist_pitches(status, created_at);

CREATE FUNCTION portal.playlist_pitch_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE t text;
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 SELECT coalesce(nullif(btrim(title), ''), '발매') INTO t FROM catalog.releases WHERE id = NEW.release_id;
 IF NEW.status = 'SUBMITTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 플레이리스트 피칭이 접수됐어요.',
   '담당자가 확인한 뒤 플랫폼 에디터에게 전달해요.', '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'PITCHED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 피칭을 플랫폼 에디터에게 전달했어요.',
   '선정 여부는 플랫폼이 정해요. 결과가 나오면 알려 드려요.', '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'SELECTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 플레이리스트에 선정됐어요!',
   left(coalesce(nullif(NEW.result, ''), '자세한 내용은 발매 상세에서 확인해 주세요.'), 300), '/releases/' || NEW.release_id);
 ELSIF NEW.status = 'NOT_SELECTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' 이번에는 플레이리스트에 선정되지 않았어요.',
   left(coalesce(nullif(NEW.staff_note, ''), '다음 발매 때 다시 피칭할 수 있어요.'), 300), '/releases/' || NEW.release_id);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER playlist_pitches_notice AFTER INSERT OR UPDATE OF status ON portal.playlist_pitches
 FOR EACH ROW EXECUTE FUNCTION portal.playlist_pitch_notice();
