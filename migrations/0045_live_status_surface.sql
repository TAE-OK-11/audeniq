-- 0045: surface partner live state to the artist.
--
-- Live state lives per (package, partner) in execution.live_bindings; the
-- release row stays READY_FOR_DELIVERY, so the release-status notice for
-- LIVE / TAKEN_DOWN never fired. Notify from the binding instead: the first
-- partner going LIVE, and any takedown confirmation.
CREATE FUNCTION portal.live_binding_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE rel uuid; t text;
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.live_status IS NOT DISTINCT FROM OLD.live_status THEN RETURN NEW; END IF;
 IF NEW.live_status NOT IN ('LIVE','TAKEN_DOWN') THEN RETURN NEW; END IF;
 SELECT cr.release_id, coalesce(nullif(btrim(r.title), ''), '발매') INTO rel, t
   FROM distribution.distribution_packages dp
   JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
   JOIN catalog.releases r ON r.org_id = cr.org_id AND r.id = cr.release_id
  WHERE dp.id = NEW.package_id;
 IF rel IS NULL THEN RETURN NEW; END IF;
 IF NEW.live_status = 'LIVE' THEN
  -- Only the first platform going live announces the release.
  IF EXISTS (SELECT 1 FROM execution.live_bindings b
              WHERE b.package_id = NEW.package_id AND b.id <> NEW.id AND b.live_status = 'LIVE') THEN
   RETURN NEW;
  END IF;
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '이(가) 발매됐어요.',
   '플랫폼에 공개됐어요. 플랫폼별 상태는 발매 상세에서 볼 수 있어요.', '/releases/' || rel);
 ELSE
  PERFORM portal.notify(NEW.org_id, 'RELEASE', t || '이(가) 플랫폼에서 내려갔어요.',
   '자세한 내용은 문의로 확인해 주세요.', '/releases/' || rel);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER live_bindings_notice AFTER INSERT OR UPDATE OF live_status ON execution.live_bindings
 FOR EACH ROW EXECUTE FUNCTION portal.live_binding_notice();
