# Foundation HTTP contract

Private API base `http://127.0.0.1:8080`. Every request, including health probes, requires `X-Audeniq-Service` from the edge/service secret. Browser code never receives that secret. Use the Rust BFF for browser traffic.

Authenticated requests carry a server-issued HttpOnly cookie. Mutations require the exact `Origin`, acceptable Fetch Metadata and `X-CSRF-Token` returned at login. Registration/login require Origin and shared database rate limiting, but cannot require a session CSRF token before a session exists. Cookies are host-only; production additionally uses Secure and the `__Host-` prefix. Sessions expire in 12 hours; token and CSRF values are SHA-256 digests in PostgreSQL. Passwords use Argon2id with random salt. The Rust process caps simultaneous password hashing at two.

Request JSON rejects unknown top-level fields. Body limit 64 KiB. Errors have `{ "error": { "code": "..." } }` for domain errors; malformed/excessive JSON uses Axum's 4xx extraction response. Successful API responses are JSON, currently HTTP 200. Writes use `row_version` optimistic locking. Resource IDs are UUIDs. Catalog and session lists accept `?limit=50&after=<UUID>` (limit 1–100). Responses include `items`, `limit`, and nullable `next_cursor`. Follow that cursor until null. Each page rechecks current permissions; pagination is not a frozen snapshot.

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
| PUT | `/api/orgs/{org}/memberships` | `{user_id,role: EDITOR or VIEWER,status: ACTIVE or REVOKED}`; OWNER only, cannot edit owner/self |
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
| GET | `/api/orgs/{org}/releases/{id}/preflight` | release_id, row_version, issues, explicit unmet gates, ready_to_submit=false |
| POST | `/api/orgs/{org}/releases/{id}/submit` | Always authenticated/authorized **501 PRE_SUBMIT_NOT_IMPLEMENTED**; no revision/job created |
| POST | `/api/orgs/{org}/uploads` | `{kind:AUDIO or IMAGE,size_bytes,content_type}` → upload_session_id, asset_id, expected_key, PUT grant |
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
