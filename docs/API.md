# Foundation HTTP contract

Private API base `http://127.0.0.1:8080`. Every request, including health probes, requires `X-Audeniq-Service` from the edge/service secret. Browser code never receives that secret. Use the Rust BFF for browser traffic.

Authenticated requests carry a server-issued HttpOnly cookie. Mutations require the exact `Origin`, acceptable Fetch Metadata and `X-CSRF-Token` returned at login. Registration/login require Origin and shared database rate limiting, but cannot require a session CSRF token before a session exists. Cookies are host-only; production additionally uses Secure and the `__Host-` prefix. Sessions expire in 12 hours; token and CSRF values are SHA-256 digests in PostgreSQL. Passwords use Argon2id with random salt. The Rust process caps simultaneous password hashing at two.

Request JSON rejects unknown top-level fields. Body limit 64 KiB. Errors have `{ "error": { "code": "...", "message": "..." } }` for domain errors (`code` is stable and machine-readable; `message` is human-readable English and may change). Upload errors are specific: `UPLOAD_TYPE_UNSUPPORTED`, `UPLOAD_EMPTY`, `UPLOAD_AUDIO_TOO_LARGE` (512 MiB), `UPLOAD_IMAGE_TOO_LARGE` (20 MiB), and at completion `UPLOAD_CONTENT_MISMATCH` (422, bytes do not match the declared type). A successful `complete` returns `sha256` and `detected_container`. See `docs/AUDIO_QC_POLICY.md` for Stage 1 audio checks; malformed/excessive JSON uses Axum's 4xx extraction response. Successful API responses are JSON, currently HTTP 200. Writes use `row_version` optimistic locking. Resource IDs are UUIDs. Catalog and session lists accept `?limit=50&after=<UUID>` (limit 1–100). Responses include `items`, `limit`, and nullable `next_cursor`. Follow that cursor until null. Each page rechecks current permissions; pagination is not a frozen snapshot.

| Method | Path | Body / response |
|---|---|---|
| GET | `/health`, `/ready` | Process / DB readiness, all release execution gates false |
| POST | `/api/auth/register` | `{email,password}` → user_id, personal org_id, party_id, submission_enabled=false |
| POST | `/api/auth/login` | `{email,password}` → Set-Cookie + user_id, csrf_token, expires_in |
| POST | `/api/auth/logout` | `{}` → revoked=true; expires cookie and revokes server session |
| GET | `/api/auth/sessions` | Current user's active sessions: public id, created_at, expires_at, current; no token/digest returned |
| POST | `/api/auth/sessions/{id}/revoke` | `{}` → revoked, reauthentication_required; other user's ID returns 404 |
| POST | `/api/auth/logout-all` | `{}` → revokes all current user's sessions and expires cookie |
| POST | `/api/auth/password` | `{current_password,new_password}` → changed, reauthentication_required; revokes all sessions |
| GET | `/api/me` | user_id, party_id; never treats headers as user identity |
| GET | `/api/orgs` | Current ACTIVE memberships only |
| POST | `/api/orgs` | `{name,kind: LABEL or COMPANY}` → org_id, unverified party_id |
| PUT | `/api/orgs/{org}/memberships` | `{user_id,role: EDITOR or VIEWER,status: ACTIVE or REVOKED}`; OWNER only, cannot edit owner/self. A user without an ACTIVE membership is only **invited** (`status: INVITED` in the response) and has no access until they accept; role changes of ACTIVE members and revocations apply directly |
| POST | `/api/orgs/{org}/memberships/accept` | `{}` from the invitee's own session → ACTIVE (records accepted_at); 404 when there is no pending invitation |
| POST | `/api/orgs/{org}/reviews/overrides` | `{revision_id,check_code,proposed_status,reason}` → `APPLIED` / `ALREADY_APPLIED` (override_id, reevaluation_queued) or `PENDING_SECOND_APPROVAL` (override_request_id, expires_at). `second_approver_user_id` is refused (422). See docs/REVIEW_OVERRIDES.md |
| GET | `/api/orgs/{org}/reviews/overrides` | Open (PENDING, unexpired) override requests in the org |
| POST | `/api/orgs/{org}/reviews/overrides/{request}/approve` | `{}` from the second approver's own session → APPLIED; requester, VIEWER, un-tenured or non-member approvers are refused |
| POST | `/api/orgs/{org}/reviews/overrides/{request}/decline` | `{}` requester withdraws or an OWNER/EDITOR declines |
| PUT | `/api/orgs/{org}/resources/{id}/acl` | `{user_id,action: read or write,revoked}`; active target membership, OWNER + write ACL required |
| POST/GET | `/api/orgs/{org}/artists` | Create `{name,profile?,party_id?,label_id?}` / authorized list |
| GET/PUT/DELETE | `/api/orgs/{org}/artists/{id}` | Read / replace profile and refs with row_version / archive `{row_version}` |
| POST/GET | `/api/orgs/{org}/labels` | Create `{name,party_id,profile?}` / authorized list |
| GET/PUT/DELETE | `/api/orgs/{org}/labels/{id}` | Read / replace with row_version / archive `{row_version}` |
| POST/GET | `/api/orgs/{org}/releases` | Create `{name,release_type:SINGLE or EP or ALBUM,profile?}` / authorized list |
| GET/PUT/DELETE | `/api/orgs/{org}/releases/{id}` | Read with tracks and separate empty DSP axes / replace DRAFT with row_version / archive DRAFT |
| POST | `/api/orgs/{org}/releases/{id}/tracks` | `{title,disc_number,track_number,artist_id,asset_id?,row_version}` → track ID and new release version |
| PUT | `/api/orgs/{org}/releases/{id}/tracks/{track}` | Same fields as track creation; replaces metadata/file reference, increments release row_version |
| DELETE | `/api/orgs/{org}/releases/{id}/tracks/{track}` | `{row_version}` → archive track, preserve its internal ID, increment release version |
| PUT | `/api/orgs/{org}/releases/{id}/tracks/{track}/credits` | `{row_version,credits:[{party_id,role}]}` → atomic replacement, empty list clears draft credits |
| POST | `/api/orgs/{org}/parties` | `{display_name}` → party_id, created. A PERSON credit party (composer, lyricist, …) in the org; OWNER/EDITOR. The same name returns the existing party (`created:false`). A display name only: no rights, no verified identity |
| GET | `/api/orgs/{org}/releases/{id}/preflight` | release_id, row_version, issues, explicit unmet gates, ready_to_submit=false |
| POST | `/api/orgs/{org}/releases/{id}/submit` | Always authenticated/authorized **501 PRE_SUBMIT_NOT_IMPLEMENTED**; no revision/job created |
| POST | `/api/orgs/{org}/uploads` | `{kind:AUDIO, IMAGE or DOCUMENT,size_bytes,content_type}` → upload_session_id, asset_id, expected_key, PUT grant. DOCUMENT (rights proofs) accepts application/pdf, image/jpeg, image/png up to 20 MiB (`UPLOAD_DOCUMENT_TOO_LARGE`) |
| POST | `/api/orgs/{org}/uploads/{id}/complete` | `{asset_id,expected_key}` → REGISTERED, QC_PENDING, duplicate flag; server checks both against stored session |
| GET | `/api/orgs/{org}/uploads/{id}` | asset_id, status (ISSUED/COMPLETED/CANCELLED), expires_at, completed_at, expired; no object key or grant |
| POST | `/api/orgs/{org}/uploads/{id}/cancel` | `{}` → cancelled, duplicate; completed sessions return 409 |
| GET | `/api/orgs/{org}/assets/{id}` | Authorized metadata, no private key, signed URL or automatic playback access |

Organization membership alone does not unlock existing resources: explicit read/write ACLs are required. The creator receives both; owners may delegate only resources they can write. Roles grant management capabilities, never musical rights. Labels link to a Party in their own organization. Artists do not merge by name. Tracks use composite organization foreign keys for artist/asset/release links. Internal UUIDs do not claim ISRC/UPC issuance.

`profile` is a mutable draft/profile JSON object, not verified rights/contract input. No draft field can activate submission or a DSP route. Soft archive preserves database references; no user hard-delete endpoint exists. Track/credit writes require an active DRAFT and the release's current row_version. An invalid credit party or missing/archived track rolls back the version increment, audit and event. Removed tracks retain their ID and historical credits; their position can be reused by a new track. Release detail includes active tracks and their credits. Credit roles are bounded descriptive strings, not legal grants. Credit parties must belong to the organization.

Auth failure audits contain no email/password/token. Other audits contain actor, org, resource, action, reason code, request UUID and DB time. There is no general arbitrary audit JSON field into which credentials or document bodies could leak. Request logs omit bodies, URLs/query strings and cookie headers.

## Additional behavior

Password changes require the current password, 12–128 byte new password, CSRF and Origin. Per-user rate limit is 5 attempts per 15 minutes. All sessions are revoked in the password-change transaction. Login rechecks the exact password hash under a user row lock before issuing a session, preventing a login that verified the old password from racing password rotation.

Preflight is a read-only draft checklist. It reports TRACK_REQUIRED, AUDIO_REQUIRED, AUDIO_NOT_REGISTERED, AUDIO_QC_PENDING_OR_BLOCKED and NOT_DRAFT as applicable. It does not issue consent, run QC, verify rights or create a revision. The legal/consent/Stage 1 gates always keep ready_to_submit=false.

Cancellation prevents asset registration and records audit/Outbox atomically. It does not revoke an already-issued S3 signature or delete bytes. The URL may remain writable until its expiry; a private quarantine lifecycle/cleanup policy remains required. Completed assets cannot be cancelled through this endpoint. The expired field describes the upload grant's deadline, not whether a completed asset has expired.

### Browser session bootstrap

`POST /api/auth/csrf` with `{}` and the HttpOnly session cookie recovers the CSRF token after reload. Exact `Origin` (and same-origin Fetch Metadata when supplied) is mandatory; an old CSRF header is not required for this endpoint. Response: `{ "csrf_token": "..." }`, no-store. Token is HMAC-derived per session and stable across tabs, not a session credential; DB still stores only its digest. Anonymous/revoked/expired sessions receive 401; bad Origin receives 403; per-user limit is 120 per 15 minutes. Subsequent mutations require `X-CSRF-Token` normally. Service secret remains mandatory.

Upload completion is limited to 60 requests per user per 15 minutes, including duplicate or failed requests. Expired uploads are rejected before storage IO, with an additional wall-clock check before copy and the existing final atomic expiry check. Lock timeout/deadlock/serialization conflict returns 409; refresh/reconcile before retrying a mutation.

## Portal (studio artist features)

Implemented in `crates/core/src/portal.rs`, schema `portal` (migration 0039). Same session, CSRF, Origin and service-secret rules as above. Every call rechecks the ACTIVE membership; release-scoped records also need the release ACL. VIEWER members can read; writes need OWNER/EDITOR; payout account and payout requests need OWNER.

| Method | Path | Body / response |
|---|---|---|
| GET/PUT | `/api/me/profile` | Per-user artist profile `{display_name,contact_email,bio(multi-line),country(ISO-2),row_version}`; PUT with `row_version` 0 creates, otherwise optimistic update (409 on stale) |
| GET/PUT | `/api/orgs/{org}/payout-account` | GET → `{registered:false}` or `{payee_type,holder_name,bank_name,account_last4,registered_at}`. PUT (OWNER) `{payee_type:INDIVIDUAL/SOLE_PROPRIETOR/CORPORATION,holder_name,bank_name,account_number}`; the full number is sealed with AES-256-GCM (`PAYOUT_ACCOUNT_KEY`, 64 hex chars, org id as associated data) and never returned. Missing key → 422 `PAYOUT_ACCOUNT_KEY_MISSING`; bad number → `ACCOUNT_NUMBER_INVALID` |
| GET/POST | `/api/orgs/{org}/inquiries` | List (with release title, message count) / create `{category:RELEASE/SETTLEMENT/CONTRACT/ACCOUNT/OTHER,release_id?,subject,body}` |
| GET | `/api/orgs/{org}/inquiries/{id}` | Thread with messages (`author_kind` ARTIST/STAFF) |
| POST | `/api/orgs/{org}/inquiries/{id}/messages` | `{body}`; reopens an ANSWERED thread; CLOSED → 422 `INQUIRY_CLOSED` |
| POST | `/api/orgs/{org}/inquiries/{id}/close` | `{}` |
| GET | `/api/orgs/{org}/notifications` | Latest 100 for the org or the user, with per-user `read`, plus `unread` |
| POST | `/api/orgs/{org}/notifications/read` | `{ids:[...]}` or `{all:true}` |
| GET/POST | `/api/orgs/{org}/documents` | Agreements and rights proofs. POST `{release_id,title,body?,asset_id?,file_name?}` adds a rights proof (with a file → REVIEW, without → AWAITING_DOCUMENTS) |
| POST | `/api/orgs/{org}/documents/{id}/check` | `{}` read confirmation → row_version |
| POST | `/api/orgs/{org}/documents/{id}/sign` | `{signer_name,signature(PNG data URL ≤60000),row_version}`; only APPROVED + checked agreements (`DOCUMENT_NOT_APPROVED` / `DOCUMENT_NOT_CHECKED`) |
| POST | `/api/orgs/{org}/documents/{id}/proof` | `{asset_id(DOCUMENT/IMAGE, registered),file_name,row_version}` → REVIEW |
| GET/POST | `/api/orgs/{org}/releases/{id}/application` | Signed studio application `{application_no:AUD-YYYYMMDD-XXXXXX,form,content_hash(sha256 hex),signer_name,signer_role,agreements[],signature,submitted_at}`. Same number + same hash is idempotent, same number + other hash → 409. Recording opens (or re-opens) the release's AGREEMENT document for staff review |
| GET | `/api/orgs/{org}/finance/summary` | KRW `payable` (ROYALTY_PAYABLE credits − debits), `pending` (REQUESTED portal requests + unsettled payout orders), `available`, `minimum_payout`, `account_registered` |
| GET | `/api/orgs/{org}/finance/statements` | Ledger transactions touching ROYALTY_PAYABLE (description, source_ref, signed amount) |
| GET/POST | `/api/orgs/{org}/finance/payouts` | Payout requests with linked order status / request `{amount(integer KRW ≥ 10000),idempotency_key}` (OWNER). Errors: `PAYOUT_ACCOUNT_REQUIRED`, `PAYOUT_BELOW_MINIMUM`, `PAYOUT_EXCEEDS_BALANCE`, `PAYEE_ON_HOLD`. Requests are serialised per org |
| GET | `/api/orgs/{org}/reports` | Last 12 months of AUTO-matched royalty lines: `by_month`, `by_dsp`, `by_release`, and `rows` (month × dsp × release) |

The API never writes finance tables. Operations turns a `portal.payout_requests` row (status REQUESTED) into a `finance.payout_orders` row, links it (`payout_order_id`, status ORDERED) and approves it under the manual payout policy. `portal.open_account()` in Rust decrypts the account number for that tooling only.

Notifications are raised in the database by SECURITY DEFINER triggers, so the worker role needs no portal grants: release status (SUBMITTED, *_CORRECTION, ON_HOLD_RIGHTS, READY_FOR_DELIVERY, LIVE, TAKEN_DOWN), document status (agreement APPROVED, proof AWAITING_DOCUMENTS/NEEDS/APPROVED), staff inquiry replies and payout order SETTLED/FAILED/RETURNED. The API adds account-registered and payout-requested notices.

Staff actions (no browser endpoint): `SELECT portal.staff_reply(inquiry_id, 'answer')`; `UPDATE portal.documents SET status='APPROVED'|'NEEDS', review_note=... WHERE id=...`; rights proof requests are `INSERT INTO portal.documents(... kind='RIGHTS_PROOF', status='AWAITING_DOCUMENTS')`.

## Notices and events (edge Worker + D1)

Served by `crates/edge` from the D1 binding `CONTENT_DB` (migrations in `crates/edge/migrations`), never proxied to the private API.

| Method | Path | Notes |
|---|---|---|
| GET | `/api/notices`, `/api/notices/{id}` | Published (`published_at` ≤ now), not deleted; pinned first. `Cache-Control: public, max-age=60` |
| GET | `/api/events`, `/api/events/{id}` | Adds `status` upcoming/ongoing/ended from KST dates (`starts_on`, `ends_on`) |
| POST | `/api/content/notices`, `/api/content/events` | `Authorization: Bearer <CONTENT_ADMIN_TOKEN>`. Notice `{id?,title,body,pinned,published_at:"YYYY-MM-DDTHH:MM:SSZ"}`; event `{id?,title,summary,body,place,starts_on,ends_on?,link_url?(https),published_at}` |
| PUT/DELETE | `/api/content/{notices|events}/{id}` | Replace / soft delete (`deleted_at`) |

A future `published_at` schedules a post. IDs are lowercase letters, digits and hyphens (≤ 64). Text rejects control, bidi and zero-width characters.
