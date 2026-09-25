-- Sandbox round 3 (protected artists re-test on 62838a6):
-- * Korean spellings ("테일러 스위프트") passed: the seed had no Korean alias.
-- * Only Taylor Swift was listed; BTS / 방탄소년단 were delivered.
-- * Short or generic names ("BTS", "IU", "Drake") cannot be substring-matched
--   without false positives, so every name/alias now carries a policy:
--     match_mode CONTAINS (default; word-boundary match with full evasion
--                folding incl. leetspeak) | TOKEN (whole words only, no leet)
--     action     BLOCK (default; input refused, Stage 1 correction)
--              | REVIEW (accepted, Stage 1 ARTIST_NAME_REVIEW for a human)
-- * The list was only editable as schema owner with raw SQL. Changes now go
--   through `audeniq-admin protected ...`, which writes
--   catalog.protected_artist_changes (who/what/when) plus an audit event.
-- See docs/PROTECTED_ARTISTS.md.
ALTER TABLE catalog.protected_artists
  ADD COLUMN match_mode text NOT NULL DEFAULT 'CONTAINS' CHECK(match_mode IN ('CONTAINS','TOKEN')),
  ADD COLUMN action text NOT NULL DEFAULT 'BLOCK' CHECK(action IN ('BLOCK','REVIEW'));
ALTER TABLE catalog.protected_artist_aliases
  ADD COLUMN match_mode text NOT NULL DEFAULT 'CONTAINS' CHECK(match_mode IN ('CONTAINS','TOKEN')),
  ADD COLUMN action text NOT NULL DEFAULT 'BLOCK' CHECK(action IN ('BLOCK','REVIEW'));

CREATE TABLE catalog.protected_artist_changes (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 occurred_at timestamptz NOT NULL DEFAULT now(),
 operator text NOT NULL CHECK(length(btrim(operator)) > 0),
 op text NOT NULL CHECK(length(btrim(op)) > 0),
 protected_artist_id uuid,
 detail jsonb NOT NULL DEFAULT '{}'::jsonb
);
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE ON catalog.protected_artist_changes
  FOR EACH ROW EXECUTE FUNCTION operations.reject_mutation();

-- Seed. Distinctive names and Korean spellings BLOCK on CONTAINS; short or
-- dictionary-word names use TOKEN and mostly REVIEW (e.g. "BTS" is also
-- "behind the scenes", "Drake"/"Adele"/"TWICE" are ordinary words/names).
CREATE TEMP TABLE seed_protected(name text, mode text, action text, note text) ON COMMIT DROP;
CREATE TEMP TABLE seed_alias(name text, alias text, kind text, mode text, action text) ON COMMIT DROP;
INSERT INTO seed_protected VALUES
 ('BTS','TOKEN','REVIEW','short name: whole word, review'),
 ('BLACKPINK','CONTAINS','BLOCK',NULL),
 ('NewJeans','CONTAINS','REVIEW','"new jeans" is an ordinary phrase: review'),
 ('IU','TOKEN','REVIEW','short name: whole word, review'),
 ('TWICE','TOKEN','REVIEW','dictionary word: whole word, review'),
 ('SEVENTEEN','TOKEN','REVIEW','number word: whole word, review'),
 ('Stray Kids','CONTAINS','BLOCK',NULL),
 ('Beyoncé','CONTAINS','BLOCK',NULL),
 ('Ed Sheeran','CONTAINS','BLOCK',NULL),
 ('Billie Eilish','CONTAINS','BLOCK',NULL),
 ('The Weeknd','CONTAINS','BLOCK',NULL),
 ('Ariana Grande','CONTAINS','BLOCK',NULL),
 ('Bruno Mars','CONTAINS','BLOCK',NULL),
 ('Justin Bieber','CONTAINS','BLOCK',NULL),
 ('Rihanna','CONTAINS','BLOCK',NULL),
 ('Coldplay','CONTAINS','BLOCK',NULL),
 ('Bad Bunny','CONTAINS','BLOCK',NULL),
 ('Olivia Rodrigo','CONTAINS','BLOCK',NULL),
 ('Dua Lipa','CONTAINS','BLOCK',NULL),
 ('Drake','TOKEN','REVIEW','ordinary name/word: whole word, review'),
 ('Adele','TOKEN','REVIEW','ordinary given name: whole word, review');
INSERT INTO seed_alias VALUES
 ('Taylor Swift','테일러 스위프트','NAME','CONTAINS','BLOCK'),
 ('Taylor Swift','테일러 앨리슨 스위프트','NAME','CONTAINS','BLOCK'),
 ('BTS','방탄소년단','NAME','CONTAINS','BLOCK'),
 ('BTS','Bangtan Boys','NAME','CONTAINS','BLOCK'),
 ('BTS','Bangtan Sonyeondan','NAME','CONTAINS','BLOCK'),
 ('BLACKPINK','블랙핑크','NAME','CONTAINS','BLOCK'),
 ('NewJeans','뉴진스','NAME','TOKEN','BLOCK'),
 ('IU','아이유','NAME','TOKEN','BLOCK'),
 ('TWICE','트와이스','NAME','TOKEN','BLOCK'),
 ('SEVENTEEN','세븐틴','NAME','TOKEN','REVIEW'),
 ('Stray Kids','스트레이 키즈','NAME','CONTAINS','BLOCK'),
 ('Beyoncé','비욘세','NAME','TOKEN','BLOCK'),
 ('Ed Sheeran','에드 시런','NAME','CONTAINS','BLOCK'),
 ('Billie Eilish','빌리 아일리시','NAME','CONTAINS','BLOCK'),
 ('The Weeknd','위켄드','NAME','TOKEN','REVIEW'),
 ('Ariana Grande','아리아나 그란데','NAME','CONTAINS','BLOCK'),
 ('Bruno Mars','브루노 마스','NAME','CONTAINS','BLOCK'),
 ('Justin Bieber','저스틴 비버','NAME','CONTAINS','BLOCK'),
 ('Rihanna','리한나','NAME','TOKEN','BLOCK'),
 ('Coldplay','콜드플레이','NAME','CONTAINS','BLOCK'),
 ('Bad Bunny','배드 버니','NAME','CONTAINS','BLOCK'),
 ('Olivia Rodrigo','올리비아 로드리고','NAME','CONTAINS','BLOCK'),
 ('Dua Lipa','두아 리파','NAME','CONTAINS','BLOCK'),
 ('Drake','드레이크','NAME','TOKEN','REVIEW'),
 ('Adele','아델','NAME','TOKEN','REVIEW');
INSERT INTO catalog.protected_artists(name, note, match_mode, action)
SELECT name, COALESCE(note, 'Seed entry (sandbox round 3)'), mode, action FROM seed_protected
ON CONFLICT (name) DO NOTHING;
INSERT INTO catalog.protected_artist_aliases(protected_artist_id, alias, kind, match_mode, action)
SELECT p.id, s.alias, s.kind, s.mode, s.action
  FROM seed_alias s JOIN catalog.protected_artists p ON p.name = s.name
ON CONFLICT (protected_artist_id, alias) DO NOTHING;
INSERT INTO catalog.protected_artist_changes(operator, op, detail)
VALUES ('migration-0035', 'seed', jsonb_build_object('entries', (SELECT count(*) FROM seed_protected), 'aliases', (SELECT count(*) FROM seed_alias)));
