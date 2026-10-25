-- 0155: artist studio (crates/core/src/artist_studio.rs).
--
-- One screen where the artist runs everything fans see: the artist page,
-- smart links, news, cheers and how they are doing.
--
-- The artist's profile photo: an uploaded image (the same sanitized IMAGE
-- upload as cover art), shown on the artist page and sent to the platforms
-- that take an artist photo from the distributor. Changing it keeps the old
-- asset (a sent request still points at the photo it carried).
CREATE TABLE portal.artist_photos (
 org_id uuid NOT NULL,
 artist_id uuid NOT NULL,
 asset_id uuid NOT NULL,
 updated_by uuid NOT NULL REFERENCES identity.users ON DELETE RESTRICT,
 updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY (org_id, artist_id),
 FOREIGN KEY (org_id, artist_id) REFERENCES catalog.artists(org_id, id) ON DELETE RESTRICT,
 FOREIGN KEY (org_id, asset_id) REFERENCES catalog.assets(org_id, id) ON DELETE RESTRICT
);

-- A profile photo change at one platform (PROFILE_IMAGE): staff send the
-- photo the request carries to the platform and track it like the other
-- profile requests.
-- The request also names the artist, so the artist screen shows each
-- platform's state.
ALTER TABLE portal.platform_requests ADD COLUMN asset_id uuid NULL, ADD COLUMN artist_id uuid NULL;
ALTER TABLE portal.platform_requests
  ADD CONSTRAINT platform_requests_asset_fk FOREIGN KEY (org_id, asset_id) REFERENCES catalog.assets(org_id, id) ON DELETE RESTRICT,
  ADD CONSTRAINT platform_requests_artist_fk FOREIGN KEY (org_id, artist_id) REFERENCES catalog.artists(org_id, id) ON DELETE RESTRICT;
CREATE INDEX platform_requests_artist ON portal.platform_requests(org_id, artist_id, created_at DESC) WHERE artist_id IS NOT NULL;
ALTER TABLE portal.platform_requests DROP CONSTRAINT IF EXISTS platform_requests_kind_check;
ALTER TABLE portal.platform_requests ADD CONSTRAINT platform_requests_kind_check CHECK (kind IN (
 'PROFILE_MISMATCH','PROFILE_SPLIT','PROFILE_MERGE','OAC','PROFILE_IMAGE',
 'CID_ALLOWLIST','CID_RELEASE_CLAIM','CID_DISPUTE','CID_OPT_OUT','CID_OPT_IN'));
ALTER TABLE portal.platform_requests
  ADD CONSTRAINT platform_requests_image CHECK ((kind = 'PROFILE_IMAGE') = (asset_id IS NOT NULL)),
  ADD CONSTRAINT platform_requests_image_artist CHECK (kind <> 'PROFILE_IMAGE' OR artist_id IS NOT NULL);

CREATE OR REPLACE FUNCTION portal.platform_request_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
DECLARE what text; link text;
BEGIN
 IF TG_OP = 'UPDATE' AND NEW.status IS NOT DISTINCT FROM OLD.status THEN RETURN NEW; END IF;
 what := CASE NEW.kind
   WHEN 'PROFILE_MISMATCH' THEN '아티스트 페이지 오매칭 수정' WHEN 'PROFILE_SPLIT' THEN '아티스트 페이지 분리'
   WHEN 'PROFILE_MERGE' THEN '아티스트 페이지 통합' WHEN 'OAC' THEN '유튜브 공식 아티스트 채널'
   WHEN 'PROFILE_IMAGE' THEN '프로필 사진 변경'
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
