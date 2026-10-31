# Publishing (퍼블리싱) — server stages

The Studio screens for publishing (작사·작곡 저작권 관리) shipped first against the demo store
(`web/studio/app/src/api/publishing.ts`). The server follows in stages, keeping the same routes and
camelCase shapes so the screens need no change.

## Stage 1 — works and applications (migration 0167, `crates/core/src/publishing.rs`)

Distribution already has an application, signing, documents, notices, review and audit. Publishing reuses
them and adds tables only for what has no distribution equivalent.

| Need | Reused from distribution | New |
|---|---|---|
| Signed application (신청서 접수) | `portal.release_applications` with `form='PUBLISHING'` and no release: same `AUD-YYYYMMDD-XXXXXX` number, signature check (`upload_safety::signature`), agreements, server copy + hash, signer identity (`consents::signer_identity`), document seal (`seal::Kind::Application`) | `portal.pub_applications`: review state, territories, rights, idempotency key, works |
| Documents staff ask for | `portal.documents` `RIGHTS_PROOF` rows (`work_id` instead of `release_id`): 권리·보완 서류 screen, `/documents/{id}/proof` upload, staff document review, "제출할 서류가 있어요" notice | — |
| Notices | `portal.notify` (trigger on the work's registration, like `release_status_notice`) | trigger only |
| History | `operations.audit_events` (`publishing.*`), shown as the application's history | — |
| Staff review | Release review shapes: `claim`, then `decision` with `action` APPROVE / REQUEST_CORRECTION / REJECT (+ REGISTER) and `reason` | routes |
| Attachments | `catalog.assets` uploads | `portal.pub_work_attachments` (kind, text body for lyrics) |
| Works, shares, recordings | — | `portal.pub_works`, `pub_work_contributors` (basis points), `pub_work_recordings`, `pub_work_status_log` |

Rules:
- Shares are drafts until submitted (never over 100%); submission needs 100%, roles, a composer, no dispute.
  The applicant's own row is confirmed; another row whose share changes goes back to `pending`.
- A work in review cannot change shares; when asked to fix it can. A registered work keeps its shares in force
  and holds a change as `pendingChange`.
- A fix is answered with `registration/resubmit` once every requested document is in (`WORK_DOCUMENTS_DUE`).
- Works are archived, never deleted, and not once applied (`WORK_IN_USE`).

## Contracts (migration 0168)

Like distribution, the application (신청서) and the contract (계약서) are two documents:
- The application is where the artist picks works and conditions (territories, rights; commission and term
  shown) and signs the application with its own confirmations. Fixes happen on the song screens.
- Approving an application makes `portal.pub_contracts` with the confirmed terms and a server-rendered body.
  The artist reads it on 계약서 (in the bundle of the release its works are on), ticks every confirmation and
  signs; the hash over body + terms + ticks + signature is kept. The term starts on the day of signing.
- Society registration (REGISTER) waits for the signed contract (`CONTRACT_NOT_SIGNED`). A work's `contractId`
  is its signed contract. Renewal and termination are requests on a signed contract.

## Stage 2 — not yet

- Contributor invites and their own confirmation (`rights/confirmation` answers 501 `NOT_IMPLEMENTED`), mail.
- Society registration feeds, royalties (statements, posting to the shared balance), CSV from the server.
- Attachment preview links (signed URLs for demo/score/proof); Open A&R submissions.
