-- 0170: how AI was used in writing a publishing work (신청 자격).
--
-- Societies register only what a person wrote; a song made entirely by AI is not a work they administer, and
-- administration services exclude it. The artist answers once per work (the release form's AI answer seeds it
-- for works prepared from a release):
--  * none      — written without AI
--  * assisted  — AI helped with the melody or the lyrics; the artist wrote the rest
--  * generated — made entirely by AI: kept, but administration applications refuse it (WORK_NOT_ELIGIBLE)
ALTER TABLE portal.pub_works
 ADD COLUMN ai_use text NOT NULL DEFAULT 'none' CHECK (ai_use IN ('none','assisted','generated'));
