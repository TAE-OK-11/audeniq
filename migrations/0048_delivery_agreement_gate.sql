-- 0048: delivery waits for the signed distribution agreement.
--
-- READY_FOR_DELIVERY used to fan out to every routable partner right away,
-- so the local MockDSP (no staging row, no staff approval) went LIVE before
-- staff reviewed the agreement or the artist signed it, and Studio showed
-- the release as released. E-0 (execution::enqueue_delivery_jobs) now sends
-- nothing until the release's AGREEMENT document is SIGNED; signing re-runs
-- E-0. Signing itself needs staff APPROVED + the artist's read confirmation.
--
-- The worker has no portal grants: it asks through one narrow SECURITY
-- DEFINER function in a schema it already uses.
CREATE FUNCTION execution.agreement_signed(p_org uuid, p_release uuid)
RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog AS $$
 SELECT EXISTS (
  SELECT 1 FROM portal.documents d
   WHERE d.org_id = p_org AND d.release_id = p_release
     AND d.kind = 'AGREEMENT' AND d.status = 'SIGNED')
$$;
REVOKE ALL ON FUNCTION execution.agreement_signed(uuid, uuid) FROM PUBLIC;

-- A test partner (activation_kind MOCK) going live is not a release: no
-- "released" notice for it. Real partners keep the 0045 behaviour, and only
-- the first real partner going live announces the release.
CREATE OR REPLACE FUNCTION portal.live_binding_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE rel uuid; t text;
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.live_status IS NOT DISTINCT FROM OLD.live_status THEN RETURN NEW; END IF;
 IF NEW.live_status NOT IN ('LIVE','TAKEN_DOWN') THEN RETURN NEW; END IF;
 IF EXISTS (SELECT 1 FROM execution.adapter_profiles p
             WHERE p.partner_id = NEW.partner_id AND p.activation_kind = 'MOCK') THEN
  RETURN NEW;
 END IF;
 SELECT cr.release_id, coalesce(nullif(btrim(r.title), ''), '발매') INTO rel, t
   FROM distribution.distribution_packages dp
   JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
   JOIN catalog.releases r ON r.org_id = cr.org_id AND r.id = cr.release_id
  WHERE dp.id = NEW.package_id;
 IF rel IS NULL THEN RETURN NEW; END IF;
 IF NEW.live_status = 'LIVE' THEN
  IF EXISTS (SELECT 1 FROM execution.live_bindings b
              JOIN execution.adapter_profiles p ON p.partner_id = b.partner_id
             WHERE b.package_id = NEW.package_id AND b.id <> NEW.id AND b.live_status = 'LIVE'
               AND p.activation_kind <> 'MOCK') THEN
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
