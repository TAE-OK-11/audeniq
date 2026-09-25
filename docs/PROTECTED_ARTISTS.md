# Protected artist names (impersonation hard block)

In the 2026-09 sandbox test (round 2) a random account released as
"Taylor Swift" and the release reached the MockDSP as `DELIVERED`. Artist
impersonation is now a **hard block**: a name on the protected list is
refused with a correctable error. It never becomes a review item or a late
failure.

Code: `crates/core/src/protected_names.rs`, `crates/core/src/protected_admin.rs`,
`crates/core/src/bin/audeniq-admin.rs`. Schema: `migrations/0033_protected_artists.sql`,
`migrations/0035_protected_artist_policy.sql` (round 3: per-name policy, seed list, change log).

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

## Per-name policy (round 3)

Each entry name and each alias has two settings:

| Setting | Values | Meaning |
|---|---|---|
| `action` | `BLOCK` (default) | Refused at input and submit (422 `ARTIST_NAME_PROTECTED`). Stage 1 returns CORRECTION_REQUIRED. |
| | `REVIEW` | Accepted at input. Stage 1 records `ARTIST_NAME_REVIEW` = REVIEW_REQUIRED, and a person decides. |
| `match_mode` | `CONTAINS` (default) | Word-boundary match with full folding, including leetspeak and separator removal (below). |
| | `TOKEN` | The name must appear as whole words. Words are split on spaces and punctuation, with the plain fold and no leetspeak. For Hangul names, a trailing Korean particle is allowed. |

Rules of thumb:

- Use **CONTAINS + BLOCK** for distinctive full names and Korean spellings
  (`Taylor Swift`, `테일러 스위프트`, `BLACKPINK`, `방탄소년단`).
- Use **TOKEN + REVIEW** for short or generic names. `BTS` is also
  "behind the scenes", and `IU`, `Drake`, `Adele`, `TWICE` and
  `SEVENTEEN` are ordinary words or names, so they must never hard-block
  unrelated text or match inside words (`Subtitles`, `Iuliana`).
- Use **TOKEN + BLOCK** for short but distinctive Korean spellings
  (`아이유`, `뉴진스`, `트와이스`). `아이유의 노래` is blocked;
  `아이유니버스` is not.
- Minimum length: CONTAINS names need at least 4 folded characters, or 3
  for Hangul. TOKEN names need at least 2.

The seed (0035) contains Taylor Swift with Korean aliases, BTS, BLACKPINK,
NewJeans, IU, TWICE, SEVENTEEN, Stray Kids, Beyoncé, Ed Sheeran,
Billie Eilish, The Weeknd, Ariana Grande, Bruno Mars, Justin Bieber, Rihanna,
Coldplay, Bad Bunny, Olivia Rodrigo, Dua Lipa, Drake and Adele. Every entry
has Korean aliases, and each name carries the policy described above. To see
the live list, run `audeniq-admin protected list`.

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
   and `Taylor-Swift` all match. Hangul is compared as whole syllables,
   and spaces don't matter, so `테일러스위프트` equals `테일러 스위프트`.
7. **Leetspeak** (CONTAINS only) is tried as extra readings of a character:
   - `4` and `@` read as a
   - `0` reads as o
   - `3` reads as e
   - `7` and `+` read as t
   - `$` and `5` read as s
   - `1`, `!` and `|` read as i or l
   - `8` reads as b
   - `9` and `6` read as g
   - `2` reads as z

   A symbol can still act as a separator, so `T4ylor Swift`, `Tayl0r Sw1ft`,
   `7aylor $wift`, `Love Story (T4ylor's Version)` and `Taylor + Swift` are
   all blocked. `Tayl0rmade Sw1fts` and `Swift 4 Taylor` are not.

A hit is a match of a folded entry inside the folded text that **starts at a
word start and ends at a word end of the original text**. So:

| Text | Result |
|---|---|
| `Taylor Swift`, `taylor swift`, `TAYLOR SWIFT`, `TaylorSwift` | blocked |
| `T a y l o r S w i f t`, `T.aylor Swift`, `Tay\u200Blor Swift`, `Taylor\u202E Swift` | blocked |
| `Тaylor Swіft` (Cyrillic Т, і), `Ｔａｙｌｏｒ Ｓｗｉｆｔ` | blocked |
| `Song (feat. Taylor Swift)`, `DJ X ft. Taylor Swift`, `Me & Taylor Swift`, `Me x Taylor Swift` | blocked |
| `Love Story (Taylor's Version)` (signature phrase) | blocked |
| `T4ylor Swift`, `Tayl0r Sw1ft`, `7aylor $wift` (leet) | blocked |
| `테일러 스위프트`, `테일러스위프트`, `Song (feat. 테일러 스위프트)`, `테일러 스위프트의 노래` | blocked |
| `BTS`, `Song (feat. BTS)` | accepted, Stage 1 review |
| `Subtitles`, `Iuliana`, `Behind the scenes` | allowed |
| `Taylor`, `Swift`, `Taylor Made`, `Swift Boys`, `Taylor Swiftly` | allowed |

CONTAINS aliases shorter than 4 folded characters (3 for Hangul) are ignored,
because they would match too much. Use TOKEN for these instead.

The unit tests in `protected_names.rs` cover the evasion variants and the
non-matches. `crates/core/tests/sandbox_regressions.rs` covers the API paths:
artist create, track title, a featured artist in the release profile, a
credited party, submit, and an allowlisted org.

## Managing the list

The runtime roles have SELECT only (`deploy/grants.sql`). Operators change the
list with the **`audeniq-admin`** CLI, which runs with the schema-owner
`DATABASE_URL` (the same role as `audeniq-migrate`). Every change needs an
operator name and is written in the same transaction to:

- `catalog.protected_artist_changes`: operator, op, entry, details as JSON.
  The table is append-only.
- `operations.audit_events`: `actor_service = audeniq-admin:<operator>`,
  `action = protected_artist.<op>`.

Changes take effect immediately, because the list is read on every check
with no cache.

```sh
export DATABASE_URL=postgres://<owner>@.../audeniq AUDENIQ_OPERATOR=ops-kim
audeniq-admin protected list                                   # JSON: entries, policy, aliases, exceptions
audeniq-admin protected add "Artist Name" --note "ticket 123"  # CONTAINS + BLOCK
audeniq-admin protected add "XY" --mode TOKEN --action REVIEW  # short/generic name
audeniq-admin protected alias "Artist Name" "아티스트 네임"       # Korean alias (CONTAINS + BLOCK)
audeniq-admin protected alias "Artist Name" "Signature Title" --phrase
audeniq-admin protected remove-alias "Artist Name" "아티스트 네임"
audeniq-admin protected remove "Artist Name"                   # deactivate (history kept)
audeniq-admin protected activate "Artist Name"
audeniq-admin protected grant-exception "Artist Name" <org uuid> --reason "verified label, contract ref ..."
audeniq-admin protected revoke-exception "Artist Name" <org uuid>
```

`add` on an existing name reactivates it and updates its policy. `alias` on an
existing alias updates its kind and policy. For bulk seeding, add a new
migration with the same shape as `0035_protected_artist_policy.sql`.

## Known limits

- Phonetic spellings and romanizations (`Teilor Swifft`, `Teillreo Seuwipeuteu`)
  aren't matched automatically. Korean and other spellings must be added as
  aliases; the seed covers the common Korean spellings.
- Only single-character leet readings are covered. Multi-character forms
  such as `|-|` for H or `\/` for V are not.
- TOKEN names don't collapse spelled-out initials (`B.T.S.` is three words).
- There is no web admin UI. The CLI above is the supported path.
