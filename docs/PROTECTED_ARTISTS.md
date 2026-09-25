# Protected artist names (impersonation hard block)

In the 2026-09 sandbox test (round 2) a random account released as
"Taylor Swift" and the release reached the MockDSP as `DELIVERED`. Artist
impersonation is now a **hard block**: a name on the protected list is
refused with a correctable error. It never becomes a review item or a late
failure.

Code: `crates/core/src/protected_names.rs`. Schema: `migrations/0033_protected_artists.sql`.

## Policy

- A protected name, any of its aliases, or one of its signature phrases may not
  appear in:
  - artist, label and organization names;
  - the release profile, including display/primary artist, featured artists
    (`X (feat. Taylor Swift)`, `feat.`, `ft.`, `with`, `x`, `&`) and every
    other string in the profile JSON;
  - release titles, track titles and track version strings;
  - credited party names (contributors/credits).
- Enforcement points:
  1. **At input.** Create and update of artists, labels, releases and orgs,
     track create/replace, and credit replace are refused with
     **HTTP 422 `ARTIST_NAME_PROTECTED`**. The message reads: "This name or
     title matches a protected artist. Releasing under it requires verified
     rights."
  2. **At submit.** The frozen revision's published texts (release,
     profile, tracks, credits, artist and label names) are re-checked and
     refused with the same 422, so nothing edited through another path gets
     through.
  3. **In Stage 1.** The check `ARTIST_NAME_PROTECTED` returns
     CORRECTION_REQUIRED. This is defense in depth, for example when an
     entry is added after a draft was written. The release goes to
     `STAGE1_CORRECTION` and the artist can fix it. It never fails late in
     packaging or the dead-letter queue.
- **Exceptions (allowlist).** An organization with an unrevoked row in
  `catalog.protected_artist_exceptions` for an entry is exempt from that entry
  only. This is how the artist's real label or verified rights holder
  releases normally. By default nobody is allowlisted.

## Matching

Each character goes through these steps:

1. Unicode **NFKC** normalization. Fullwidth `Ｔａｙｌｏｒ` becomes `Taylor`.
2. Removal of invisible and format characters: U+200B–U+200F (ZWSP,
   ZWNJ, ZWJ, LRM, RLM), U+202A–U+202E (bidi embeddings and overrides,
   including U+202E), U+2060–U+206F (word joiner, isolates), U+FEFF, soft
   hyphen, variation selectors, tag characters, and control characters.
3. Lower-casing.
4. The **UTS #39 confusables skeleton**. Cyrillic and Greek look-alikes
   (`Тауlor Ѕwіft`) and similar characters fold to Latin.
5. Removal of diacritics (NFD, then combining marks dropped).
6. Only letters and digits are kept. Spaces, dots, dashes and other
   punctuation are dropped, so `T a y l o r  S w i f t`, `T.aylor Swift`
   and `Taylor-Swift` all match.

A hit is a match of a folded entry inside the folded text that **starts at a
word start and ends at a word end of the original text**. So:

| Text | Result |
|---|---|
| `Taylor Swift`, `taylor swift`, `TAYLOR SWIFT`, `TaylorSwift` | blocked |
| `T a y l o r S w i f t`, `T.aylor Swift`, `Tay\u200Blor Swift`, `Taylor\u202E Swift` | blocked |
| `Тaylor Swіft` (Cyrillic Т, і), `Ｔａｙｌｏｒ Ｓｗｉｆｔ` | blocked |
| `Song (feat. Taylor Swift)`, `DJ X ft. Taylor Swift`, `Me & Taylor Swift`, `Me x Taylor Swift` | blocked |
| `Love Story (Taylor's Version)` (signature phrase) | blocked |
| `Taylor`, `Swift`, `Taylor Made`, `Swift Boys`, `Taylor Swiftly` | allowed |

Aliases shorter than 4 folded characters are ignored, because they would
match too much.

The unit tests in `protected_names.rs` cover the evasion variants and the
non-matches. `crates/core/tests/sandbox_regressions.rs` covers the API paths:
artist create, track title, a featured artist in the release profile, a
credited party, submit, and an allowlisted org.

## Managing the list

The list is managed by operators. The runtime roles have SELECT only
(`deploy/grants.sql`), so changes run as the schema owner (migration role)
through `psql` or a seed migration. Entries take effect immediately: the list
is read on every check, with no cache.

Add an artist with aliases and signature phrases:

```sql
WITH p AS (
  INSERT INTO catalog.protected_artists(name, note)
  VALUES ('Artist Name', 'why / ticket reference') RETURNING id
)
INSERT INTO catalog.protected_artist_aliases(protected_artist_id, alias, kind)
SELECT p.id, v.alias, v.kind FROM p, (VALUES
  ('Legal Name Of Artist', 'NAME'),     -- alternative names
  ('Signature Phrase',     'PHRASE')    -- titles like "Taylor's Version"
) AS v(alias, kind);
```

Add an alias to an existing entry:

```sql
INSERT INTO catalog.protected_artist_aliases(protected_artist_id, alias, kind)
SELECT id, 'Another Alias', 'NAME' FROM catalog.protected_artists WHERE name = 'Artist Name';
```

Deactivate or reactivate an entry. Rows are kept for audit:

```sql
UPDATE catalog.protected_artists SET active = false WHERE name = 'Artist Name';
```

Grant an exception to a verified rights holder, after verifying the label
agreement or letter of direction:

```sql
INSERT INTO catalog.protected_artist_exceptions(protected_artist_id, org_id, reason, granted_by)
SELECT id, '<org uuid>', 'Verified label: contract ref ...', 'ops:<your name>'
FROM catalog.protected_artists WHERE name = 'Artist Name';
```

Revoke an exception:

```sql
UPDATE catalog.protected_artist_exceptions SET revoked_at = now()
WHERE org_id = '<org uuid>'
  AND protected_artist_id = (SELECT id FROM catalog.protected_artists WHERE name = 'Artist Name');
```

For bulk seeding, add a new migration with the same `INSERT` shape as the
seed in `0033_protected_artists.sql`. Keep entries to full names and
distinctive phrases. Single common words such as "Swift" are not suitable
entries.

## Known limits

- Phonetic or transliterated spellings (`Teilor Swifft`, `테일러 스위프트`)
  are not matched unless they are added as aliases.
- There is no admin API or UI yet. The list is managed with SQL (see above).
