-- 0120: platform requests (crates/core/src/platform_requests.rs).
--
-- What an artist asks AUDENIQ to get done at a platform after release, and
-- its tracking to the end:
--  - artist profiles: a release on the wrong artist page (PROFILE_MISMATCH),
--    two artists sharing one page (PROFILE_SPLIT), duplicate pages to merge
--    (PROFILE_MERGE), a YouTube Official Artist Channel (OAC);
--  - YouTube Content ID / UGC: channels to never claim (CID_ALLOWLIST), a
--    claim to release on a video (CID_RELEASE_CLAIM), someone else's claim on
--    our recording to dispute (CID_DISPUTE), turning Content ID off or on
--    for a release (CID_OPT_OUT / CID_OPT_IN).
-- Staff move it SUBMITTED → IN_PROGRESS → SENT_TO_PLATFORM (with the
-- platform's ticket) → RESOLVED / REJECTED; the artist can cancel until it is
-- sent. Every step is kept as an event and notifies the org.
CREATE TABLE portal.platform_requests (
 id uuid PRIMARY KEY,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 release_id uuid NULL REFERENCES catalog.releases ON DELETE RESTRICT,
 kind text NOT NULL CHECK (kind IN ('PROFILE_MISMATCH','PROFILE_SPLIT','PROFILE_MERGE','OAC',
   'CID_ALLOWLIST','CID_RELEASE_CLAIM','CID_DISPUTE','CID_OPT_OUT','CID_OPT_IN')),
 platform text NOT NULL CHECK (platform ~ '^[a-z]{2,20}$'),
 artist_name text NOT NULL CHECK (length(btrim(artist_name)) BETWEEN 1 AND 200),
 urls text[] NOT NULL DEFAULT '{}' CHECK (cardinality(urls) <= 10),
 description text NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
 status text NOT NULL DEFAULT 'SUBMITTED'
   CHECK (status IN ('SUBMITTED','IN_PROGRESS','SENT_TO_PLATFORM','RESOLVED','REJECTED','CANCELLED')),
 platform_ref text NOT NULL DEFAULT '' CHECK (length(platform_ref) <= 200),
 resolution text NOT NULL DEFAULT '' CHECK (length(resolution) <= 1000),
 created_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX platform_requests_org ON portal.platform_requests(org_id, created_at DESC);
CREATE INDEX platform_requests_open ON portal.platform_requests(status, created_at)
 WHERE status IN ('SUBMITTED','IN_PROGRESS','SENT_TO_PLATFORM');

CREATE TABLE portal.platform_request_events (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 request_id uuid NOT NULL REFERENCES portal.platform_requests ON DELETE CASCADE,
 status text NOT NULL,
 note text NOT NULL DEFAULT '' CHECK (length(note) <= 1000),
 by_staff boolean NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX platform_request_events_req ON portal.platform_request_events(request_id, created_at);

-- Content ID state of a release as staff last confirmed it at YouTube.
CREATE TABLE portal.content_id_status (
 release_id uuid PRIMARY KEY REFERENCES catalog.releases ON DELETE RESTRICT,
 org_id uuid NOT NULL REFERENCES identity.orgs ON DELETE RESTRICT,
 enabled boolean NOT NULL DEFAULT true,
 monetization text NOT NULL DEFAULT 'UNKNOWN'
   CHECK (monetization IN ('UNKNOWN','MONETIZING','TRACKING','BLOCKED','INELIGIBLE')),
 allowlisted_channels text[] NOT NULL DEFAULT '{}' CHECK (cardinality(allowlisted_channels) <= 200),
 note text NOT NULL DEFAULT '' CHECK (length(note) <= 1000),
 updated_by uuid NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE FUNCTION portal.platform_request_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE what text; link text;
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 what := CASE NEW.kind
   WHEN 'PROFILE_MISMATCH' THEN '아티스트 페이지 오매칭 수정' WHEN 'PROFILE_SPLIT' THEN '아티스트 페이지 분리'
   WHEN 'PROFILE_MERGE' THEN '아티스트 페이지 통합' WHEN 'OAC' THEN '유튜브 공식 아티스트 채널'
   WHEN 'CID_ALLOWLIST' THEN 'Content ID 채널 제외' WHEN 'CID_RELEASE_CLAIM' THEN 'Content ID 소유권 주장 해제'
   WHEN 'CID_DISPUTE' THEN 'Content ID 이의제기' WHEN 'CID_OPT_OUT' THEN 'Content ID 끄기' ELSE 'Content ID 켜기' END;
 link := '/platform-requests?id=' || NEW.id;
 IF NEW.status = 'SUBMITTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 요청이 접수됐어요.', '담당자가 확인한 뒤 플랫폼에 전달해요.', link);
 ELSIF NEW.status = 'SENT_TO_PLATFORM' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 요청을 플랫폼에 전달했어요.', '처리 기간은 플랫폼마다 달라요. 결과가 나오면 알려 드려요.', link);
 ELSIF NEW.status = 'RESOLVED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 요청이 처리됐어요.', left(coalesce(nullif(NEW.resolution, ''), '자세한 내용은 요청 내역에서 확인해 주세요.'), 300), link);
 ELSIF NEW.status = 'REJECTED' THEN
  PERFORM portal.notify(NEW.org_id, 'RELEASE', what || ' 요청이 처리되지 않았어요.', left(coalesce(nullif(NEW.resolution, ''), '자세한 내용은 요청 내역에서 확인해 주세요.'), 300), link);
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER platform_requests_notice AFTER INSERT OR UPDATE OF status ON portal.platform_requests
 FOR EACH ROW EXECUTE FUNCTION portal.platform_request_notice();
