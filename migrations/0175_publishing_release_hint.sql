-- 0175: a release went live → suggest publishing for its songs.
--
-- Distribution only collects the recording's money; the songwriter's share (performance, mechanical) is collected by
-- the societies. When the first platform puts a release live (execution.live_bindings, as the "released" notice of
-- 0045) and some of its tracks are not yet linked to a work (pub_work_recordings), the artist gets one notice pointing
-- at the release's publishing tab, where the songs are prepared from the release data. A separate trigger so
-- live_binding_notice stays as it is; its name sorts after live_bindings_notice, so "released" comes first.
CREATE FUNCTION portal.release_publishing_hint() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE rel uuid; t text; n integer;
BEGIN
 IF NEW.live_status IS DISTINCT FROM 'LIVE' OR TG_OP = 'UPDATE' AND OLD.live_status IS NOT DISTINCT FROM 'LIVE' THEN RETURN NEW; END IF;
 SELECT cr.release_id, '‘' || left(coalesce(nullif(btrim(r.title), ''), '발매'), 80) || '’' INTO rel, t
   FROM distribution.distribution_packages dp
   JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
   JOIN catalog.releases r ON r.org_id = cr.org_id AND r.id = cr.release_id
  WHERE dp.id = NEW.package_id;
 IF rel IS NULL THEN RETURN NEW; END IF;
 -- Once per release: not again for the next platform, nor for a later package of the same release.
 IF EXISTS (SELECT 1 FROM execution.live_bindings b
             JOIN distribution.distribution_packages dp ON dp.id = b.package_id
             JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
            WHERE cr.org_id = NEW.org_id AND cr.release_id = rel AND b.id <> NEW.id AND b.live_status = 'LIVE') THEN
  RETURN NEW;
 END IF;
 SELECT count(*) INTO n FROM catalog.tracks k
  WHERE k.org_id = NEW.org_id AND k.release_id = rel
    AND NOT EXISTS (SELECT 1 FROM portal.pub_work_recordings w WHERE w.org_id = k.org_id AND w.track_id = k.id);
 IF n > 0 THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || ' ' || n || '곡의 작사·작곡 저작권도 맡겨 보세요.',
   '발매 정보로 곡 정보가 채워져 있어요. 확인하고 신청하면 관리 단체에서 저작권료를 받아 드려요.',
   '/releases/' || rel || '?tab=publishing');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER live_bindings_publishing_hint AFTER INSERT OR UPDATE OF live_status ON execution.live_bindings
 FOR EACH ROW EXECUTE FUNCTION portal.release_publishing_hint();
