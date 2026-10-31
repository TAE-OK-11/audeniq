-- 0169: where a publishing work comes from (퍼블리싱 접수 세 갈래).
--
-- Publishing takes a song in one of three ways, and the screens differ by it:
--  * unreleased     — written, not released yet: pitched through Open A&R; administration waits for a release.
--  * own_release    — written and released by the artist through AUDENIQ: the release (title, credits, audio,
--                     lyrics, cover) is read from the catalog, nothing is typed twice. Linking a recording of the
--                     org's own release makes a work own_release.
--  * other_release  — written by the artist, released by someone else: the release is not in the catalog, so the
--                     artist gives it (artist, title, date, ISRC, label) in external_release.
ALTER TABLE portal.pub_works
 ADD COLUMN origin text NOT NULL DEFAULT 'unreleased' CHECK (origin IN ('unreleased','own_release','other_release')),
 ADD COLUMN external_release jsonb NULL CHECK (external_release IS NULL OR jsonb_typeof(external_release) = 'object');

UPDATE portal.pub_works w SET origin = 'own_release'
 WHERE EXISTS (SELECT 1 FROM portal.pub_work_recordings r WHERE r.work_id = w.id);
