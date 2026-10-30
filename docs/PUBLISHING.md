# Publishing (퍼블리싱) — server stages

The Studio screens for publishing (작사·작곡 저작권 관리) shipped first against the demo store
(`web/studio/app/src/api/publishing.ts`). The server follows in stages, keeping the same routes and
camelCase shapes so the screens need no change.

## Stage 1 — works and applications (migration 0167, `crates/core/src/publishing.rs`)

- **Works** (`portal.pub_works`): a musical work separate from a recording. Internal code `AQW-000101`,
  optional ISWC (normalized `T-123.456.789-0`). Archived, never deleted.
- **Contributors and shares** (`portal.pub_work_contributors`): basis points (10000 = 100%). Drafts may be
  under 100%, never over. The managed (publisher) share cannot exceed the creative share. The applicant's own
  row is linked to their party and confirmed; any other row whose share changes goes back to `pending`.
- **Recordings** (`portal.pub_work_recordings`): links to `catalog.tracks`. `POST /releases/{id}/works` makes
  works from track credits (composer · lyricist · arranger), shares split evenly to be checked.
- **Attachments** (`portal.pub_work_attachments`): demo, lyrics, score, proof. Text bodies (lyrics .txt) are
  stored and shown in place; files point at `catalog.assets`.
- **Application** (`portal.pub_applications`, 신청서 접수): number `PUB-2026-0001`; territories, rights,
  agreements, signature; one per idempotency key. Submission requires every work to be at 100% with roles, a
  composer and no dispute, and not already in an active application. Works move to `internal_review`.
- **Staff review**: start review → approve (`ready`) / ask for a fix (works `needs_fix` with reason, fields,
  documents) / reject; `register` records the collecting society (`registered`). The writer answers a fix with
  `registration/resubmit`; the application returns to review once no work waits for a fix.
- **Registered works**: share changes are held as `pendingChange`; the registered values stay in force.
- Every mutation is audited (`publishing.*`). Application history is append-only.

## Stage 2 — not yet

- Contributor invites and their own confirmation (`rights/confirmation` answers 501 `NOT_IMPLEMENTED`), mail.
- Publishing contracts (signing, renewal, termination) linked to approved applications.
- Society registration feeds, royalties (statements, posting to the shared balance), CSV from the server.
- Attachment preview links (signed URLs for demo/score/proof); Open A&R submissions.
