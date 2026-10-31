-- 0176: who wrote it, under which name, and what shows it.
--
-- Pen names: writers often credit songs under a pen name or alias, sometimes with an IPI of its own. Listed on the creator profile, those names are the
-- writer themselves: preparing works from release credits makes such a row "me" (no confirmation sent to oneself), and
-- staff see it as the signed-in writer's pen name (the verified legal identity goes with the application).
--
-- Evidence of authorship: when the work was finished and how it was made (free text), plus two more attachment kinds —
-- project files (DAW project, MIDI, stems) and a signed split sheet. An application needs at least one piece of
-- evidence per work (any attachment; a release's audio is attached when works are prepared from it).
-- Each pen name may carry its own IPI: a society registers pseudonyms under their own name number.
ALTER TABLE portal.pub_creator_profiles
 ADD COLUMN pen_names jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(pen_names) = 'array' AND jsonb_array_length(pen_names) <= 5);
-- A work already registered with a society keeps that society's work code, so it is not registered twice.
ALTER TABLE portal.pub_works
 ADD COLUMN created_on date NULL,
 ADD COLUMN creation_note text NULL CHECK (length(creation_note) <= 1000),
 ADD COLUMN society_code text NULL CHECK (length(btrim(society_code)) BETWEEN 1 AND 40);
ALTER TABLE portal.pub_work_attachments DROP CONSTRAINT pub_work_attachments_kind_check;
ALTER TABLE portal.pub_work_attachments
 ADD CONSTRAINT pub_work_attachments_kind_check CHECK (kind IN ('demo','lyrics','score','proof','project','split_sheet'));
