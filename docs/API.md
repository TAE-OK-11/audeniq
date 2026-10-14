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
| POST | `/api/auth/password-reset/start` | No session. `{email, phone}` → `{reset_id, expires_at, phone_last4}`. A 6-digit code is texted only when the email's active account has this phone registered (`/api/me/phone/*`); otherwise the same answer shape and no SMS (no account enumeration). 422 `SMS_PROVIDER_NOT_CONFIGURED` before an SMS contract (`SMS_PROVIDER`; `test` only outside production), 400 `PHONE_INVALID` (Korean mobile only). Rate limited per source, email and phone |
| POST | `/api/auth/password-reset/verify` | `{reset_id, code}` → `{reset_token, expires_at}` (one-time, 15 minutes). Codes live 3 minutes, 5 tries; wrong, expired, exhausted or unknown requests all answer 400 `RESET_CODE_INVALID` |
| POST | `/api/auth/password-reset/complete` | `{reset_token, new_password(12–128)}` → changed, reauthentication_required. Revokes all sessions and any other open reset; reused or expired token 400 `RESET_TOKEN_INVALID` |
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
| GET/PUT/DELETE | `/api/orgs/{org}/releases/{id}` | Read with tracks, `agreement_status` (the distribution agreement: REVIEW · APPROVED · NEEDS · SIGNED, null if none) and the latest package's per-partner `delivery_status_by_dsp` / `live_status_by_dsp` (also on each list item) / replace DRAFT with row_version / archive DRAFT. Create and replace also take optional `upc` (UPC-A, 12 digits, check digit verified; empty = Stage 3 issues one) and `artwork_asset_id` (a REGISTERED IMAGE asset of the org) |
| POST | `/api/orgs/{org}/releases/{id}/tracks` | `{title,disc_number,track_number,artist_id,asset_id?,isrc?,row_version}` → track ID and new release version. `isrc` is normalized (dashes, case); empty = Stage 3 issues one |
| PUT | `/api/orgs/{org}/releases/{id}/tracks/{track}` | Same fields as track creation; replaces metadata/file reference, increments release row_version |
| DELETE | `/api/orgs/{org}/releases/{id}/tracks/{track}` | `{row_version}` → archive track, preserve its internal ID, increment release version |
| PUT | `/api/orgs/{org}/releases/{id}/tracks/{track}/credits` | `{row_version,credits:[{party_id,role}]}` → atomic replacement, empty list clears draft credits |
| POST | `/api/orgs/{org}/parties` | `{display_name}` → party_id, created. A PERSON credit party (composer, lyricist, …) in the org; OWNER/EDITOR. The same name returns the existing party (`created:false`). A display name only: no rights, no verified identity |
| GET | `/api/orgs/{org}/releases/{id}/preflight` | release_id, row_version, issues, explicit unmet gates, ready_to_submit=false |
| POST | `/api/orgs/{org}/releases/{id}/submit` | Always authenticated/authorized **501 PRE_SUBMIT_NOT_IMPLEMENTED**; no revision/job created |
| POST | `/api/orgs/{org}/uploads` | `{kind:AUDIO, IMAGE or DOCUMENT,size_bytes,content_type}` → upload_session_id, asset_id, expected_key, PUT grant (a presigned URL bound to this key, exact byte count, content type, nonce and If-None-Match: * — single use, a second PUT gets 412 — for 10 minutes, created when the Studio asks for it; the storage credentials never leave the server). AUDIO accepts audio/wav, audio/flac, audio/mp4 (ALAC), audio/aiff (integer PCM), audio/wavpack (pure lossless integer PCM), and audio/tta. All audio, including FLAC, is scanned and normalized to a FLAC master (compression level 5) with identical decoded PCM SHA-256; lossy/hybrid/float/DSD inputs are refused, and a WAV whose data chunk is cut short is refused as `UPLOAD_AUDIO_TRUNCATED`. Normalization requires 16/24 bit, 44.1–192 kHz, mono/stereo, and at most 512 MiB decoded PCM. Completion may return 503 `UPLOAD_BUSY` with `Retry-After: 5`: retry the same completion request without re-uploading; a busy answer does not count against the 60-per-15-minutes completion limit. An uploaded file can be completed until 30 minutes after the PUT grant expires (`complete_by`); expiry is rechecked after hashing/conversion, before registration. DOCUMENT (rights proofs) accepts application/pdf, image/jpeg, image/png up to 20 MiB (`UPLOAD_DOCUMENT_TOO_LARGE`) |
| POST | `/api/orgs/{org}/uploads/{id}/complete` | `{asset_id,expected_key}` → REGISTERED, QC_PENDING, duplicate flag; safety_status SANITIZED with antivirus off (default), VERIFIED with antivirus on. Frozen bytes are always normalized in a sandbox; enabled antivirus scans source and derivative (docs/UPLOAD_SECURITY.md). Server checks both identifiers against the stored session |
| GET | `/api/orgs/{org}/uploads/{id}` | asset_id, status (ISSUED/COMPLETED/CANCELLED), expires_at (PUT grant), complete_by (last moment completion is accepted), completed_at, expired; no object key or grant |
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
| GET | `/api/me/account` | Per-user account info: `{real_name, verified:{provider,method,verified_at,adult}\|null, tax:{foreign,country,tin_last4,us_person,row_version,updated_at}\|null, identity_provider:{ready,name}}`. Birth date and CI are never returned (`adult` = 만 19세 이상) |
| POST | `/api/me/identity` | `{transaction_id}` from the identity provider (same `IDENTITY_PROVIDER` integration as rights signing). The server verifies the transaction and stores the provider's name as `real_name`; the browser's name is not trusted. 422 `IDENTITY_PROVIDER_NOT_CONFIGURED` before a provider contract, 400 `IDENTITY_NOT_VERIFIED` (failed or reused transaction), 400 `IDENTITY_ACCOUNT_MISMATCH` (re-verification by a different person). CI kept only as a peppered hash. Rate limited |
| PUT | `/api/me/tax` | Foreign tax residency `{foreign, country?(US,JP,CN,GB,CA,AU,DE,FR,SG,OTHER), tin?, keep_tin?, us_person?(US only), row_version}`; `row_version` 0 creates (rows start at 1), 409 on stale. TIN (4–30 letters/digits) is sealed with the payout account key (`PAYOUT_ACCOUNT_KEY`); only `tin_last4` is returned. `keep_tin` keeps the stored number |
| POST | `/api/me/phone/start`, `/api/me/phone/verify` | Register the phone used for password reset: `{phone}` texts a 6-digit code → `{challenge_id, expires_at, phone_last4}`; `{challenge_id, code}` stores it (400 `SMS_CODE_INVALID`). Only a peppered hash and the last 4 digits are kept. `GET /api/me/account` adds `phone:{last4,verified_at}\|null` and `sms_ready` |
| GET/PUT | `/api/orgs/{org}/payout-account` | GET → `{registered:false}` or `{payee_type,holder_name,bank_name,account_last4,registered_at}`. PUT (OWNER) `{payee_type:INDIVIDUAL/SOLE_PROPRIETOR/CORPORATION,holder_name,bank_name,account_number}`; the full number is sealed with AES-256-GCM (`PAYOUT_ACCOUNT_KEY`, 64 hex chars, org id as associated data) and never returned. Missing key → 422 `PAYOUT_ACCOUNT_KEY_MISSING`; bad number → `ACCOUNT_NUMBER_INVALID` |
| GET/POST | `/api/orgs/{org}/inquiries` | List (with release title, message count) / create `{category:RELEASE/SETTLEMENT/CONTRACT/ACCOUNT/OTHER,release_id?,subject,body}` |
| GET | `/api/orgs/{org}/inquiries/{id}` | Thread with messages (`author_kind` ARTIST/STAFF) |
| POST | `/api/orgs/{org}/inquiries/{id}/messages` | `{body}`; reopens an ANSWERED thread; CLOSED → 422 `INQUIRY_CLOSED` |
| POST | `/api/orgs/{org}/inquiries/{id}/close` | `{}` |
| GET | `/api/orgs/{org}/notifications` | Latest 100 for the org or the user, with per-user `read`, plus `unread` |
| POST | `/api/orgs/{org}/notifications/read` | `{ids:[...]}` or `{all:true}` |
| GET/POST | `/api/orgs/{org}/documents` | Agreements and rights proofs. POST `{release_id,title,body?,asset_id?,file_name?,electronic?}` adds a rights proof (file or signed electronic document → REVIEW, otherwise → AWAITING_DOCUMENTS). Electronic documents return `electronic_record`, signing fields and the stored body |
| POST | `/api/orgs/{org}/documents/{id}/check` | `{}` read confirmation → row_version |
| POST | `/api/orgs/{org}/documents/{id}/sign` | `{signer_name,signature(PNG data URL ≤60000),row_version,confirmations?:[id]}`; only APPROVED + checked agreements (`DOCUMENT_NOT_APPROVED` / `DOCUMENT_NOT_CHECKED`). An `AUD-DIST 2.0` agreement (with `agreement_terms`) also needs every `required` confirmation id from `agreement_terms.confirmations` (`AGREEMENT_CONFIRMATION_REQUIRED`; unknown ids → 400); the ticks are stored in `confirmations` with a SHA-256 over body, terms, ticks and signature |
| POST | `/api/orgs/{org}/documents/{id}/proof` | `{asset_id(DOCUMENT/IMAGE, registered),file_name,row_version}` → REVIEW. Signed electronic originals are immutable (`DOCUMENT_NOT_EDITABLE`); create a new document to correct them |
| GET/POST | `/api/orgs/{org}/releases/{id}/application` | Signed studio application `{application_no:AUD-YYYYMMDD-XXXXXX,form,content_hash(sha256 hex),signer_name,signer_role,agreements[],signature,submitted_at}`. Same number + same hash is idempotent, same number + other hash → 409. Recording opens (or re-opens) the release's AGREEMENT document for staff review |
| GET | `/api/orgs/{org}/finance/summary` | KRW `payable` (ROYALTY_PAYABLE credits − debits), `pending` (REQUESTED portal requests + unsettled payout orders), `available`, `minimum_payout` (0 until 2027-12-31 23:59:59 KST, then 10000), `payout_policy` (version, fee rule), `account_registered` |
| GET | `/api/orgs/{org}/finance/payouts/quote?amount=N` | `{amount, fee, net, minimum_payout, policy_version, available, within_balance}`. Fee 500 (VAT included, never added again) for requests ≤ 30000, free above; `PAYOUT_NET_NOT_POSITIVE` when nothing is left after the fee (`crate::payout_policy`) |
| POST | `/api/orgs/{org}/finance/payouts/{id}/cancel` | OWNER. Cancels before the bank transfer: a REQUESTED request, or its order while PENDING_APPROVAL/APPROVED (`finance.cancel_unsent_payout`). After SUBMITTED: `PAYOUT_NOT_CANCELLABLE`. The reserved amount returns to `available` |
| GET | `/api/orgs/{org}/finance/statements?before&limit` | Ledger transactions touching ROYALTY_PAYABLE (description, source_ref, signed amount), newest first. Whole history in pages: `limit` 1–500 (default 200); pass the response's `next_before` (`<created_at>|<id>`, null on the last page) as `before`. A malformed cursor is 400 STATEMENT_CURSOR_INVALID |
| GET | `/api/orgs/{org}/finance/statements.csv` | Every statement line as CSV (UTF-8 with BOM; text cells starting with `= + - @` are prefixed with `'`) |
| GET/POST | `/api/orgs/{org}/finance/payouts` | Payout requests with linked order status, `fee`, `net`, frozen `account` (masked) and `cancellable` / request `{amount(integer KRW),idempotency_key}` (OWNER). The request stores the fee, net, policy version and the payout account at request time (a later account change does not move it). Errors: `PAYOUT_ACCOUNT_REQUIRED`, `PAYOUT_BELOW_MINIMUM`, `PAYOUT_NET_NOT_POSITIVE`, `PAYOUT_EXCEEDS_BALANCE`, `PAYEE_ON_HOLD`. Requests are serialised per org. Operations: `audeniq-admin --operator STAFF_EMAIL payout order|approve|submit|result|cancel` (request → order in one transaction; the approver must differ from the order's creator; SETTLED posts the ledger debit ROYALTY_PAYABLE / PAYOUT_CLEARING + PAYOUT_FEE_REVENUE in the same transaction; a bank transaction id completes one order only) |
| GET | `/api/orgs/{org}/reports?from&to` | AUTO-matched royalty lines of the period: `by_month`, `by_dsp`, `by_release`, `rows` (month × dsp × release, top 500) and `by_currency`, plus the resolved `from`/`to`. `from`/`to` are `YYYY-MM` (KST), inclusive; both omitted = the last 12 months, `to` alone = the 12 months ending there. At most 120 months, otherwise 400 REPORT_PERIOD_INVALID |
| GET | `/api/orgs/{org}/reports.csv?from&to` | Every month × platform × release × currency row of the period as CSV (no 500-row cap) |

The API never writes finance tables. Operations turns a `portal.payout_requests` row (status REQUESTED) into a `finance.payout_orders` row, links it (`payout_order_id`, status ORDERED) and approves it under the manual payout policy. `portal.open_account()` in Rust decrypts the account number for that tooling only.

Notifications are raised in the database by SECURITY DEFINER triggers, so the worker role needs no portal grants: release status (SUBMITTED, *_CORRECTION, ON_HOLD_RIGHTS, READY_FOR_DELIVERY, LIVE, TAKEN_DOWN), document status (agreement APPROVED, proof AWAITING_DOCUMENTS/NEEDS/APPROVED), staff inquiry replies and payout order SETTLED/FAILED/RETURNED. The API adds account-registered and payout-requested notices.

Staff actions (agreement/proof review, proof requests, inquiry replies) now go through the staff portal API below. The SQL functions remain for operations tooling.

Electronic rights documents (form `AUD-RIGHTS 2.0`) are signed by the rights holder themselves through a signing request (below). The old direct path — `POST /documents` with `electronic:{form:"AUD-RIGHTS 1.0",…}`, signed on the submitter's device without verifying the signer — now returns 400 `SIGNING_REQUEST_REQUIRED` (an operator can reopen it with `LEGACY_DIRECT_SIGNING=1`); existing 1.0 documents stay readable.

### Rights signing (권리자 본인확인 + 직접 서명)

| Method | Path | Notes |
|---|---|---|
| POST | `/api/orgs/{org}/signing-requests` | `{release_id,document_no(UUID),document_kind:master\|artwork\|composition\|sample\|performer\|shared,title,body,rights_holder,signer_name,signer_role:"권리자 본인"\|"권리자의 위임을 받은 대리인"\|"법인 대표자",channel:LINK\|IN_PERSON}` (release write ACL) → `{id,token,expires_at,status}`. The 64-hex token is returned once and only its SHA-256 is stored. LINK lives 7 days, IN_PERSON 1 hour. Same `document_no` + same text while PENDING → the same request with a fresh token (old one stops); changed text → 409 |
| GET | `/api/orgs/{org}/signing-requests?release_id` | Requests on releases the caller can read: status (`PENDING`, `VERIFIED`, `SIGNED`, `DECLINED`, `CANCELLED`, or `EXPIRED` past the deadline), signer, channel, `document_id` once signed, `certificate_hash`, identity summary (provider, method, verified_at). Never the token, birth date or CI hash |
| POST | `/api/orgs/{org}/signing-requests/{id}/reissue` | `{channel}` → new token + deadline, only while nobody has verified (`SIGNING_REQUEST_CLOSED`) |
| POST | `/api/orgs/{org}/signing-requests/{id}/cancel` | PENDING/VERIFIED → CANCELLED |
| GET | `/api/sign/{token}` | No session. The document for the signer: title, body, body_hash, holder/signer, release, requester, deadline, `identity_provider:{ready,name}`; once SIGNED also the signature, `certificate_hash` and the event log (readable 90 days after signing). The first open records `VIEWED` |
| POST | `/api/sign/{token}/identity` | `{transaction_id}` from the identity provider. 422 `IDENTITY_PROVIDER_NOT_CONFIGURED` until a provider is set (`IDENTITY_PROVIDER`; `test` is accepted only outside production). The verified name must equal the signer name ignoring spaces/case (`IDENTITY_NAME_MISMATCH`); a provider transaction verifies one request only. Stores name, birth date, method, transaction id and a peppered hash of the CI (never the CI or phone number). 10 tries per token per 15 minutes |
| POST | `/api/sign/{token}/sign` | `{signature(PNG data URL),consents:{document,electronic_signature,privacy}}` — all three `true` (`SIGNING_CONSENT_REQUIRED`), identity verified within 30 minutes (`IDENTITY_EXPIRED`). Creates the RIGHTS_PROOF document in REVIEW with `electronic_record` = `{form:"AUD-RIGHTS 2.0",document_no,document_kind,rights_holder,signer_name,signer_role,content_hash,signing_request_id,channel,identity:{provider,method,name,verified_at},consents,certificate_hash}` and returns `{document_id,signed_at,certificate_hash}`. `content_hash` has the 1.0 shape; `certificate_hash` covers the body hash, content hash, signature hash, full identity record, consents, signing time and the event chain head |
| POST | `/api/sign/{token}/decline` | `{reason?}` → DECLINED |

Every step is an append-only `portal.signing_events` row (`CREATED`, `LINK_REISSUED`, `VIEWED`, `IDENTITY_VERIFIED`, `IDENTITY_FAILED`, `SIGNED`, `DECLINED`, `CANCELLED`) with time, peppered IP hash, user agent, and a hash over the previous row's hash; a trigger refuses UPDATE/DELETE. Signing writes go through the edge with the site Origin like any other write.

## Release delivery status (artist)

| Method | Path | Body / response |
|---|---|---|
| GET | `/api/orgs/{org}/releases/{id}/delivery` | Per-DSP status of the release's latest staged package (release read ACL). `items[]`: `{dsp:"D-5", slug:"spotify", name, stage, readiness, approval, delivery_status, issues[], staged_at}`. `stage` is `NEEDS_CORRECTION` · `IN_REVIEW` · `PREPARING` (content fine, partner onboarding pending) · `SCHEDULED` (staff approved) · `ON_HOLD` · `SENDING` · `DELIVERED`. `issues` lists only CONTENT findings the artist can fix. Empty before Stage 3 |

`GET .../submission` also returns `effective_status` per check (a reviewer override replaces the recorded status; `severity` follows it) and `review_notes[]` (`{check_code|null, decision, note, at}`) written by staff.

## Staff portal (`/api/staff/*`)

Implemented in `crates/core/src/staff.rs` (docs/DISTRIBUTION_STAGING.md). Same session, CSRF, Origin and service-secret rules. The caller needs an ACTIVE `identity.staff_members` row, granted only by `audeniq-admin staff grant EMAIL ROLE` (schema-owner login); non-staff get 403. Every write is audited with the staff user.

| Role | Duties |
|---|---|
| ADMIN | everything, payout-request list |
| REVIEWER | release decisions, second approvals, documents, inquiries |
| OPERATOR | delivery staging decisions, re-stage |
| SUPPORT | inquiries |

All roles can read every list below.

| Method | Path | Body / response |
|---|---|---|
| GET | `/api/staff/me` | `{user_id, role, duties[]}` |
| GET/POST | `/api/staff/content-access` | ADMIN only: `{user_id,role}`. D1 content Workers verify the current session here; POST additionally enforces Origin and CSRF before a content mutation. Inactive staff and other roles are denied |
| GET | `/api/staff/overview` | Queue counts: review, correction, in_pipeline, second_approvals, documents, inquiries, deliveries_to_approve, deliveries_blocked, payout_requests |
| GET | `/api/staff/releases?status=PENDING&limit&offset` | Cross-org release queue with artist, release date, requested platforms as D-codes and `agreement` (the signed application's status). `PENDING` (default) = waiting on a staff decision: STAGE2_REVIEW, or READY_FOR_DELIVERY with the application (AGREEMENT) in REVIEW/PREPARED. Any release status also filters |
| GET | `/api/staff/releases/{id}` | Review sheet: release, frozen application (tracks, credits, declarations, platforms), exact Stage 1 package refs (all tracks) and latest Stage 2 run refs, `open_checks` (what holds Stage 2, with server `needs_second_approval`), overrides, notes, second approvals, documents, `signed_application` (the latest signed record: number, form, content_hash, signer, agreements, `signature` image, `received_at`, and `contact_email` = the artist profile's contact email or the submitting account's email), `draft` (everything the artist entered in the Studio wizard for this submitted revision: credits, lyrics, add-on services, rights confirmations and the signed application record with its signature), current-revision delivery staging rows, audit timeline. `review_context`: `decision_kind` (CHECKS/APPLICATION/null), role-aware `allowed_actions`, `requires_second_approval`, live current-revision `pending_second_approval_id`, effective `check_counts`. Checks include `id`, `stage`, `original_status`; Stage 2 `status` includes live overrides. `track_audio` maps frozen track IDs to persisted duration/sample rate/channels/bit depth for the matching asset content hash |
| GET | `/api/staff/releases/{id}/timeline?limit=` | Everything that happened to the release in time order (default 500, max 2000; `truncated` when more): `audit` events (release, application revisions, packages, delivery jobs, jobs, track assets), `job` rows (every job records its `release_id`, plus upload-time analysis of its audio), non-passing `check`s, `staff_decision`s per DSP, `dsp_request` send attempts and `dsp_ack`s. Worker and API log lines carry the same `release_id` (job and request spans) |
| POST, DELETE | `/api/staff/releases/{id}/claim` | 심사 담당. POST `{take_over?:bool}` claims a release in review (STAGE2_REVIEW, or READY_FOR_DELIVERY with its application waiting); claiming your own claim again is a no-op, another reviewer's claim → 422 `REVIEW_CLAIMED_BY_OTHER` unless an ADMIN passes `take_over`. DELETE gives it up (the claimer, or ADMIN). Audited as `staff.review_claimed` / `staff.review_taken_over` / `staff.review_released`. Decisions are claimer-only: unclaimed → 422 `REVIEW_CLAIM_REQUIRED`, someone else's → `REVIEW_CLAIMED_BY_OTHER`. The sheet's `review_context` carries `claim {user_id,email,at,mine}`, `can_claim`, `can_take_over`, and `allowed_actions` is empty unless the claim is yours; queue rows carry `claim` |
| PUT | `/api/staff/releases/{id}/agreement-terms` | REVIEW duty. `{exclusivity:NON_EXCLUSIVE\|EXCLUSIVE,fee_bps(AUDENIQ share, 0–10000),rate_note?,territory_note?,min_payout_note?,special_terms?}` on the release's agreement while it waits on staff (else `AGREEMENT_NOT_IN_REVIEW`). The server renders the `AUD-DIST 2.0` agreement body (parties, release, exclusivity/territory, DSPs, term, fee, settlement payee, special terms, grant, rights confirmation, representative/AI articles when declared, terms-of-service precedence, e-contract, track annex, confirmation annex) and stores `agreement_terms` with the confirmation list the artist must tick. Approving a release whose waiting agreement has no terms returns 422 `AGREEMENT_TERMS_REQUIRED`; a new application clears the terms |
| POST | `/api/staff/releases/{id}/decision` | `{action: APPROVE|REQUEST_CORRECTION|REJECT, revision_id, reason, notes?:[{check_code,note}]}`. Release must be STAGE2_REVIEW, or READY_FOR_DELIVERY with its application waiting, on `revision_id` (else 409/422 `RELEASE_NOT_IN_REVIEW`). For a waiting application APPROVE clears the agreement for signing, REQUEST_CORRECTION moves the release to STAGE3_CORRECTION (agreement NEEDS) and REJECT to WITHDRAWN (agreement REJECTED). In STAGE2_REVIEW the agreement follows the decision too. APPROVE writes PASS overrides and queues Stage 2 when every open check is low-risk; otherwise (duplicates, fingerprints, protected names, rights/money classes, anything BLOCKED) it returns `PENDING_SECOND_APPROVAL` + `approval_id`. The sheet also lists `advisories` (loudness, clipping). REQUEST_CORRECTION turns every open check into CORRECTION_REQUIRED (one reviewer) and stores notes for the artist. REJECT moves the release to WITHDRAWN and notifies the org |
| POST | `/api/staff/releases/{id}/reissue-identifiers` | `{reason}`: READY_FOR_DELIVERY release with VIRTUAL UPC/ISRC → STAGE3_CORRECTION once real ranges are registered (`NO_VIRTUAL_IDENTIFIERS`, `REGISTERED_ISSUER_MISSING`, `PACKAGE_ALREADY_WITH_PARTNER`) |
| GET | `/api/staff/approvals` | Open second-person approvals |
| POST | `/api/staff/approvals/{id}/approve` · `/decline` | `{}`. A different staff reviewer must approve (`SECOND_APPROVER_MUST_DIFFER`); approval writes the PASS overrides with `second_approver_user_id` and queues Stage 2 |
| GET | `/api/staff/documents?status=REVIEW` | Rights proofs (AWAITING_DOCUMENTS, REVIEW, NEEDS, APPROVED), including signed electronic body/signature/receipt/hash for review. Agreements are decided with the release |
| POST | `/api/staff/documents/{id}/review` | `{status: APPROVED|NEEDS, note, row_version}`; NEEDS requires a note. Rights proofs in REVIEW only; the trigger notifies the org |
| GET | `/api/staff/documents/{id}/file` | The document's inspected static derivative (rights proof or agreement attachment) for reviewers to open. DOCUMENTS or REVIEW duty; registered uploads within the 20 MB document limit only (404 otherwise). Only PDF / JPEG / PNG with matching inspection evidence are served `inline`; other and uninspected types return 404; `Content-Disposition` carries an RFC 6266 UTF-8 file name; `Cache-Control: private, no-store`, `nosniff` and a no-script CSP. Each view is audited as `staff.document_viewed` |
| POST | `/api/staff/orgs/{org}/documents` | `{release_id,title,body?}` → rights-proof request (AWAITING_DOCUMENTS) |
| GET | `/api/staff/inquiries?status=OPEN` · `/api/staff/inquiries/{id}` | Threads across orgs |
| POST | `/api/staff/inquiries/{id}/reply` | `{body}` → STAFF message; thread becomes ANSWERED, author notified. CLOSED → 422 `INQUIRY_CLOSED` |
| GET | `/api/staff/deliveries?approval=PENDING&readiness=&dsp=D-5` | Staging rows with blocker codes |
| GET | `/api/staff/deliveries/{package}/{dsp}/ern` | The exact ERN 3.8.2 XML (or placeholder-DPID preview) staff approve, `application/xml` |
| POST | `/api/staff/deliveries/{package}/{dsp}/decision` | `{action: APPROVE|HOLD, note?, ern_sha256?, acknowledge_warnings?}`. APPROVE refuses CONTENT_BLOCKED rows (`DELIVERY_CONTENT_BLOCKED`), a changed ERN (409) and unacknowledged audio advisories (`WARNINGS_NOT_ACKNOWLEDGED`), then queues E-0; HOLD needs a note. Rows of a superseded package: `STAGING_SUPERSEDED` |
| POST | `/api/staff/deliveries/{package}/restage` | Re-run staging after onboarding or issuer changes |
| POST | `/api/staff/dsps/{dsp}/route` | ADMIN: `{route: DIRECT|MERLIN, note}` — the DSP's contract route. MERLIN only for DSPs with a Merlin deal (`MERLIN_NOT_AVAILABLE_FOR_DSP`). `GET /api/staff/dsps` items carry `name_ko`, `ern_version`, `deal`, `channel`, `choreography`, policy flags and `contract_route`; delivery rows carry `dsp_name` |
| POST | `/api/staff/deliveries/{package}/{dsp}/live` | `{partner_release_id?, note}` — record LIVE evidence for a platform that never reports it (DELIVERY duty; only DELIVERED jobs; applied by the worker job `delivery.mark_live`, audited) |
| GET | `/api/staff/dsps` | D-1..D-11 registry with spec, route profile and onboarding gaps |
| GET | `/api/staff/payouts?status=REQUESTED` | ADMIN only; read-only (money still moves through operations tooling) |

## Staff work module (`/api/staff/work/*`)

Internal work management for staff (`/admin/tasks`). Design, duplicate check and phases: `docs/WORK_MANAGEMENT_PLAN.md`.
Code: `crates/core/src/work/`, schema `work` (migration `0076`). Same session, Origin, CSRF and staff-role checks as the
staff portal (every request also lands in `privacy.staff_access_logs`); every mutation is audited as `work.*`. Nothing is
deleted — tasks, links, notes and attachments are archived. Customers have no route to any of it.

Phase 1 (task engine, notes, mentions):

| Method & path | Purpose |
|---|---|
| `GET /people` | Active staff (assignee / reviewer pickers, @mentions) |
| `GET /entities/search?kind=&q=` | Find an entity to link. Kinds: `release, org, artist, label, dsp, inquiry, document, payout, settlement_report, task`. Labels come from the source tables |
| `GET /tasks?view=mine\|review\|created\|all&status=open\|done\|all\|archived&type=&priority=&assignee=&due=overdue\|today\|week&entity_kind=&entity_id=&q=&subtasks=` | Task list (top-level unless `subtasks` or an entity filter) with subtask counts and resolved links |
| `POST /tasks` | Create (`parent_id` for a subtask — one level; `links[]`; `recurrence {freq,interval}`) |
| `GET /tasks/{id}` | Task, parent, subtasks, event log |
| `POST /tasks/{id}` | Update with `row_version` (409 when stale): fields, `status`, or `review: APPROVE\|REQUEST_CHANGES` (+ `review_note`, required for changes). With a reviewer other than the caller, `DONE` becomes `IN_REVIEW`; only the reviewer or ADMIN decides; open subtasks block `DONE` (`TASK_SUBTASKS_OPEN`); a done recurring task creates the next one with the same people and links |
| `POST /tasks/{id}/archive` | Archive with its subtasks (creator, assignee or ADMIN) |
| `POST /tasks/{id}/links` · `POST /links/{id}/archive` | Link an existing entity (must exist) · unlink (archived) |
| `GET /notes?target_kind=&target_id=&archived=` · `POST /notes` | Internal notes on any entity; `mentions` must be active staff; `attachments` are ready uploads of the author |
| `POST /notes/{id}` · `GET /notes/{id}/revisions` | Author-only edit; the previous text is kept |
| `POST /notes/{id}/resolve {resolved}` · `POST /notes/{id}/archive` | Resolve / reopen · archive (author or ADMIN) |
| `GET /mentions?resolved=` | Notes mentioning the caller, with the target's label |
| `POST /attachments {name,content_type,size_bytes}` → `{id, grant}` · `POST /attachments/{id}/complete` · `GET /attachments/{id}` | Same path as studio uploads: presigned PUT to quarantine, completion checks size/type/nonce, freezes, hashes and runs the antivirus scan when enabled. ≤ 20 MiB, documents/images/spreadsheets/zip. Downloads are `attachment` only and audited |

Phase 2 (inbox, SLA, exception queue — migration `0078`). Inbox items and queue deadlines are computed from the
sources (tasks, release review claims, second approvals, document review, inquiries, notes, exception sources); only read
marks and triage are stored.

| Method & path | Purpose |
|---|---|
| `GET /inbox?category=&type=&priority=&unread=&due=overdue\|today\|week&entity_kind=` | Everything waiting on the caller: `ASSIGNED, REVIEW_REQUEST, APPROVAL_REQUEST, MENTION, FAILED_AUTOMATION, SLA_WARNING, OVERDUE, SUPPORT_ESCALATION, RIGHTS_ISSUE, DISTRIBUTION_FAILURE, EXCEPTION`, with counts. Item ids carry a version (`task:{id}@{row_version}`), so a changed item is unread again |
| `POST /inbox/read {ids}` | Mark items read (≤ 500) |
| `GET /summary` | Navigation badges: inbox total/unread, overdue, SLA warnings, open/mine/critical exceptions |
| `GET /sla` | Policies, calendars with holidays, deadline board of the existing queues, open tasks with deadlines. Also runs the escalation sweep: an open task past its deadline (earlier of `due_at` and `sla_due_at`) goes to level 1 (priority raised one step once, when the policy says so) and, overdue by another full window, level 2 — each recorded as a `SLA_ESCALATED` task event and audited; the inbox shows it to the policy's `escalate_to` role |
| `POST /sla/policies` · `POST /sla/policies/{id}/archive` | ADMIN: create/update (`id` present) a policy for a task type or queue (`RELEASE_REVIEW, SECOND_APPROVAL, DOCUMENT_REVIEW, SUPPORT_INQUIRY, EXCEPTION`), optionally `scope_kind: dsp` + D-code; one active policy per type and scope (`SLA_POLICY_EXISTS`) · archive |
| `POST /calendars/{id}` · `POST /calendars/{id}/holidays {day, name?}` | ADMIN: working hours and workdays · add/rename a holiday, or remove it without `name`. Business time skips nights, non-working days and holidays |
| `GET /exceptions?view=open\|mine\|unassigned\|resolved\|all&kind=&severity=&source=&dsp=&q=` | Current failures from the sources (`check, staging, document, rights_hold, reconciliation, delivery_job, report, job, manual`) with triage, SLA and whether the caller may act |
| `POST /exceptions/{source}/{id} {action}` | `ASSIGN {assignee_id}` · `ACK` · `RESOLVE {resolution?}` · `IGNORE {resolution}` · `REOPEN` · `RETRY` (delivery sources only: the existing restage, ≤ once per 2 min) · `TO_TASK {title?, priority?, assignee_id?}` (typed, prioritised by severity, linked to the entity, once). Duty of the source: QC → REVIEW, rights proofs → DOCUMENTS, delivery → DELIVERY, settlement/automation → ADMIN. Resolved/ignored items come back when the source fails again |
| `POST /exceptions/reports {kind, severity, title, detail, entity_kind?, entity_id?}` | Report a problem with no source record (e.g. a settlement mismatch) |

Tasks also carry `sla_due_at`, `deadline`, `sla_state` (`OK\|WARNING\|BREACHED`), `escalation_level`, `priority_raised`.

Phase 3 (CRM Lite, DSP operations — migration `0079`). A relationship points at the source record (`artist, label,
dsp, org`); only a partner/prospect without one keeps its own name and a hand-tracked contract status. Names, contract
status (workspace distribution agreements, DSP contract route/onboarding), releases and delivery numbers are read from the
sources. Inbox category `CONTACT_DUE`: relationships the caller owns whose next contact has come.

| Method & path | Purpose |
|---|---|
| `GET /relationships?kind=&status=&owner=&contact=due\|week&q=&archived=` · `POST /relationships` | List (with derived contract, last contact, open tasks, notes, primary contact, counts by kind) · create (`kind` must match the entity; one active relationship per record: `RELATIONSHIP_EXISTS`; `NAME_FROM_SOURCE` / `CONTRACT_FROM_SOURCE` when a source exists) |
| `GET /relationships/{id}` · `POST /relationships/{id}` | Detail: contacts, contact history, linked records, the workspace's releases and agreements · update with `row_version` (owner, status, next contact, tags; name / manual contract only without a source) |
| `POST /relationships/{id}/archive` | ADMIN only |
| `POST /relationships/{id}/contacts` · `POST /relationship-contacts/{id}` | Add · edit or archive (`archived: true`) a contact; first contact is primary |
| `POST /relationships/{id}/activities {kind: CALL\|EMAIL\|MEETING\|NOTE, summary, at?, contact_id?, next_contact_at?}` | Log contact history (append-only); "last contact" is the latest entry |
| `POST /relationships/{id}/links` · `POST /relationships/{id}/links/{link}/archive` | Link / unlink related records. Notes (`target_kind=relationship`) and tasks link to relationships like any entity |
| `GET /dsp-ops?dsp=` | Per DSP: registry, contract route and onboarding (same as `/api/staff/dsps`), last 30 days of delivery jobs (total, delivered, failed, in flight, failure rate, partner rejections, average minutes), staging pending/blocked, open exceptions, the CRM relationship and DSP-scoped SLA policies; with `dsp`, that DSP's recent failures and open exceptions |

Team channels (migration `0080`). Messages are internal notes with `target_kind=channel` (same mentions → inbox,
attachments, edit history, archive); only the channel and each person's read position are stored.

| Method & path | Purpose |
|---|---|
| `GET /channels?archived=` · `POST /channels {name, topic}` | Channels with the caller's `unread`, `mentions`, `last_message_at` · create (any staff; unique name: `CHANNEL_EXISTS`) |
| `POST /channels/{id}` · `POST /channels/{id}/archive` | Rename / archive — creator or ADMIN (system channels: ADMIN). Posting to an archived channel: `CHANNEL_ARCHIVED` |
| `POST /channels/{id}/read` | Mark read up to now |
| `GET /notes?target_kind=channel&target_id=&latest=&before=` · `POST /notes` | Newest `latest` messages (≤ 200) older than `before`, shown oldest-first, with `has_more` · post a message |

`GET /summary` also returns `chat.unread`.

### Kanban boards (migration `0081`)

Cards are tasks (`work.tasks` + `board_id`, `column_id`, `position`, `source`); columns map to a task status, so
inbox, SLA and review rules apply unchanged. Board kinds: `PERSONAL` (owner only — other staff get 404 for the
board, its cards, their notes and embeds), `TEAM` (written by its `team_role` and ADMIN, read by all staff),
`OPERATIONS` (all staff). Every change is audited (`work.board_*`, `work.task_moved`, `work.task_reordered`).

| Method & path | Purpose |
|---|---|
| `GET /boards` · `POST /boards {name, description, kind, team_role}` | Boards the caller can see (`access.write/manage`, `open_cards`) · create (PERSONAL: anyone; TEAM/OPERATIONS: ADMIN), seeded with Inbox/To Do/In Progress/Review/Waiting/Blocked/Done |
| `GET /boards/{id}?assignee=&priority=&column=&tag=&type=&due=&creator=&source=&entity_kind=&q=&done_days=` | `{board, columns[count, wip_limit], cards, tags, truncated}`; `assignee` = uuid · `me` · `none`; `source` = `manual` · `auto` · a source; `q` matches title, description, key and linked entity names |
| `POST /boards/{id}` · `POST /boards/{id}/archive` | Rename (with `row_version`) · archive (the general board cannot be archived; its rules stop) |
| `POST /boards/{id}/columns {name, status, wip_limit}` · `POST /boards/{id}/columns/order {ids}` | Add (≤ 20) · reorder (full id list). Board managers only |
| `POST /board-columns/{id} {name?, status?, wip_limit?}` · `POST /board-columns/{id}/archive {move_to}` | Edit (a status change moves its cards' status, with events) · archive (`MOVE_TO_REQUIRED` when not empty; one column must remain) |
| `POST /tasks/{id}/move {row_version, column_id, before_id, after_id}` | Move/reorder; `before_id` = card directly above, `after_id` = directly below. Stale `row_version` → 409. Status follows the column through the task rules (`TASK_NO_REVIEWER` 422 for a review column without reviewer; a review redirect lands in the review column, `redirected: true`) |
| `GET /board-rules` · `POST /board-rules {trigger, board_id, column_id, enabled}` (ADMIN) | Which board/column receives automatic cards per trigger |
| `POST /automation/run` (ADMIN) | Run the automatic-card sweep now (it also runs at most once a minute from staff reads) |

Automatic cards (`source` ≠ `manual`, `created_by` null, tag `자동`) come from the exception queue (QC failure,
metadata conflict, missing rights document, DSP rejection, delivery failure, settlement mismatch, automation
failure), new customer inquiries, pending second approvals and overdue release reviews. Each has a unique
`source_key` (event identity), so repeated or concurrent sweeps never create a second card.

### AUDENIQ Docs (migration `0081`)

Block documents stored as validated block JSON (Tiptap/ProseMirror shape — never HTML). The server accepts only
known nodes/marks/attributes (`DOC_CONTENT_INVALID`, `DOC_ATTR_INVALID`, `DOC_MARK_INVALID`), links must be
`http(s)`/`mailto` (`DOC_LINK_INVALID`), embeds must name a known kind and id (`DOC_EMBED_INVALID`); 2 MB / 60k nodes.
Access is `work.doc_level`: owner (creator) 4, ADMIN 3, else the higher of the staff-wide access
(`staff_access` `EDITOR`/`VIEWER`/null = restricted) and per-role/per-user grants (`VIEWER`/`EDITOR`/`MANAGER`).
A document the caller cannot see is a **404 everywhere** (detail, tree, search, entity search, comments, file
downloads, embeds, inbox mentions); a visible one without enough level is a 403.

| Method & path | Purpose |
|---|---|
| `GET /docs?parent=&archived=` | One tree level (lazy), `has_children`; visible children of hidden parents appear at the top |
| `GET /docs/search?q=&status=&author=&entity_kind=&entity_id=&limit=` | Full-text (title + body, PostgreSQL FTS) and title match; plain-text `snippet` |
| `POST /docs {parent_id, title, icon, template_id, link}` | Create (parent needs EDITOR; depth ≤ 4: `DOC_TOO_DEEP`); inherits the parent's access and grants; `link` connects a card (`task`) or record |
| `GET /docs/{id}` | `{doc (+content, level), breadcrumb, related, cards, backlinks, open_comments}` |
| `POST /docs/{id} {base_version, title?, icon?, content?}` | Save (EDITOR). `base_version` ≠ current → 409 (never a silent overwrite). Returns `{version, status, saved_at}`; editing an approved/in-review document returns it to `DRAFT` |
| `POST /docs/{id}/move {parent_id, before_id, after_id}` · `/duplicate` · `/archive` · `/restore` · `/delete` | Tree move (MANAGER; `DOC_MOVE_CYCLE`, depth) · duplicate · archive / restore (with the subtree) · delete = tombstone, archived documents only, owner or ADMIN |
| `POST /docs/{id}/status {action, reason}` | `REQUEST_REVIEW` (EDITOR) · `APPROVE` / `REJECT` (MANAGER; requester cannot approve own request except ADMIN: `DOC_SELF_APPROVAL`; reject needs `reason`) · `REOPEN` |
| `GET /docs/{id}/versions` · `GET /docs/{id}/versions/{v}` · `POST /docs/{id}/versions {label}` · `POST /docs/{id}/restore-version {version, base_version}` | Version list (snapshots per editing session — another editor or 10 quiet minutes —, status changes, restores, manual saves; immutable) · one version · save a labelled version · restore as a new version |
| `GET /docs/{id}/permissions` · `POST /docs/{id}/permissions {staff_access, grants, apply_to_children}` | Sharing (MANAGER); replaces the grant list |
| `GET /doc-templates` · `POST /doc-templates {from_doc, name, description}` · `POST /doc-templates/{id}/archive` | Templates (8 built in; ADMIN saves a document without files as a template) |
| `GET /embeds?refs=kind:id[:view],…` | Live values of embedded blocks, one batched query per kind (≤ 60 refs): release (`summary`/`qc`/`rights`/`distribution`), track, artist, dsp, inquiry, task (card), approval, doc, `audit` (resource audit trail; REVIEWER/ADMIN — others get `restricted`), other kinds by label. Hidden or missing → `{missing:true}` |

Comments are notes on `target_kind=doc` with `parent_id` (one reply level: `NOTE_PARENT_INVALID`) and an optional
`anchor {block, from, to, quote}`. Document files use the note attachment upload; a file is bound to the
document on save and downloads (`GET /attachments/{id}`, `?inline=true` for PNG/JPEG/WebP only, sandboxed CSP,
`nosniff`, `private`) follow the document's access — or the note target's for note attachments; an unattached
upload only for its uploader. Link-preview blocks are written by the author: the server never fetches URLs.

Phase 4 (analytics) is specified in the plan and not yet served.

## Partner webhooks (`/api/partner-hooks/*`)

`POST /api/partner-hooks/{partner_id}` receives a contracted partner's
notifications (ingestion ACK, live, takedown confirmation). It reaches the API
through the edge Worker like any other call but carries **no browser Origin**;
the edge forwards the partner's `x-*` headers (never `x-audeniq-*`). The body
(≤ 64 KiB) must be signed with the webhook secret from the partner's config
file: `hex(HMAC-SHA256(secret, timestamp + "." + body))` in the configured
signature header (default `X-Signature`, optional `sha256=` prefix) with the
Unix timestamp in `X-Timestamp` (±300 s). Responses: `202 {"received":true}`
filed; `200 {"duplicate":true}` same body already filed; `401` bad/missing
signature or stale timestamp; `404` unknown partner or no webhook configured.
Filed bodies are applied by the worker (`delivery.ack`): correlated to one
delivery by `partner_message_id` (our submission id) or `partner_release_id`,
deduplicated on `event_id`. Default JSON shape:
`{"event_id","type":"accepted|rejected|live|takedown_confirmed","partner_message_id","partner_release_id","code"}`.
See [PARTNER_DELIVERY.md](PARTNER_DELIVERY.md).

## Notices and events (edge Worker + D1)

Served by `crates/edge` from the D1 binding `CONTENT_DB` (migrations in `crates/edge/migrations`), never proxied to the private API.

| Method | Path | Notes |
|---|---|---|
| GET | `/api/notices`, `/api/notices/{id}` | Published (`published_at` ≤ now), not deleted; pinned first. `Cache-Control: public, max-age=60` |
| GET | `/api/events`, `/api/events/{id}` | Adds `status` upcoming/ongoing/ended from KST dates (`starts_on`, `ends_on`) |
| GET | `/api/status` | `{now, maintenance:{active, upcoming}}`: the window in progress and the next one starting within 72 hours (published, not deleted). The `MAINTENANCE_MODE` Worker secret/var (optional `MAINTENANCE_TITLE`, `MAINTENANCE_MESSAGE`, `MAINTENANCE_UNTIL`) forces an active `emergency` window without D1. `no-store` |
| GET/POST | `/api/content/{notices|events|maintenance}` | Active ADMIN session (CSRF on writes, checked through `/api/staff/content-access`) or `Authorization: Bearer <CONTENT_ADMIN_TOKEN>`. Notice `{id?,title,body,pinned,published_at:"YYYY-MM-DDTHH:MM:SSZ"}`; event `{id?,title,summary,body,place,starts_on,ends_on?,link_url?(https),published_at}`; maintenance `{id?,title,body?,starts_at,ends_at,published_at?,kind?:"scheduled"\|"emergency",end_unknown?}` (UTC timestamps, `ends_at > starts_at`) |
| PUT/DELETE | `/api/content/{notices|events|maintenance}/{id}` | Replace / soft delete (`deleted_at`) |

While a maintenance window is active the edge answers every other private API call with `503 {"error":{"code":"MAINTENANCE"},"maintenance":{…}}` and `Retry-After: 60`, without contacting the API. `GET /api/me`, `GET /api/staff/me` and `POST /api/auth/{login,logout,csrf}` stay available so ADMIN can manage content and end the window.

A future `published_at` schedules a post. IDs are lowercase letters, digits and hyphens (≤ 64). Text rejects control, bidi and zero-width characters.

Staff sheets preserve every Stage 1 track result using `stage1_check_refs`. Stage 2 records
`stage2.checks_recorded` audit snapshots of exact result IDs for each run, including cached
rows; this internal snapshot event is excluded from the user-facing timeline. Existing
runs use a compatibility fallback for Stage 2 codes and carried Stage 1 holds. Original
immutable results remain unchanged; Stage 2 effective status includes unexpired overrides.
Second-approval counts/queues include only unexpired requests for a release's current
revision in STAGE2_REVIEW. Renewing an expired request closes it with an expiry audit event;
correction/rejection closes outstanding requests with a cancellation audit event.
