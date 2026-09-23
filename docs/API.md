# Foundation HTTP contract

Private API base `http://127.0.0.1:8080`. Every request, including health probes, requires `X-Audeniq-Service` from the edge/service secret. Browser code never receives that secret. Use the Rust BFF for browser traffic.

Authenticated requests carry a server-issued HttpOnly cookie. Mutations require the exact `Origin`, acceptable Fetch Metadata and `X-CSRF-Token` returned at login. Registration/login require Origin and shared database rate limiting, but cannot require a session CSRF token before a session exists. Cookies are host-only; production additionally uses Secure and the `__Host-` prefix. Sessions expire in 12 hours; token and CSRF values are SHA-256 digests in PostgreSQL. Passwords use Argon2id with random salt. The Rust process caps simultaneous password hashing at two.

Request JSON rejects unknown top-level fields. Body limit 64 KiB. Errors have `{ "error": { "code": "..." } }` for domain errors; malformed/excessive JSON uses Axum's 4xx extraction response. Successful API responses are JSON, currently HTTP 200. Writes use `row_version` optimistic locking. Resource IDs are UUIDs. Lists are capped at 100, ordered by ID; pagination is deferred before large catalogs.

| Method | Path | Body / response |
|---|---|---|
| GET | `/health`, `/ready` | Process / DB readiness, all release execution gates false |
| POST | `/api/auth/register` | `{email,password}` → user_id, personal org_id, party_id, submission_enabled=false |
| POST | `/api/auth/login` | `{email,password}` → Set-Cookie + user_id, csrf_token, expires_in |
| POST | `/api/auth/logout` | `{}` → revoked=true; expires cookie and revokes server session |
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
| POST | `/api/orgs/{org}/releases/{id}/submit` | Always authenticated/authorized **501 PRE_SUBMIT_NOT_IMPLEMENTED**; no revision/job created |
| POST | `/api/orgs/{org}/uploads` | `{kind:AUDIO or IMAGE,size_bytes,content_type}` → upload_session_id, asset_id, expected_key, PUT grant |
| POST | `/api/orgs/{org}/uploads/{id}/complete` | `{asset_id,expected_key}` → REGISTERED, QC_PENDING, duplicate flag; server checks both against stored session |
| GET | `/api/orgs/{org}/assets/{id}` | Authorized metadata, no private key, signed URL or automatic playback access |

Organization membership alone does not unlock existing resources: explicit read/write ACLs are required. The creator receives both; owners may delegate only resources they can write. Roles grant management capabilities, never musical rights. Labels link to a Party in their own organization. Artists do not merge by name. Tracks use composite organization foreign keys for artist/asset/release links. Internal UUIDs do not claim ISRC/UPC issuance.

`profile` is a mutable draft/profile JSON object, not verified rights/contract input. No draft field can activate submission or a DSP route. Soft archive preserves database references; no user hard-delete endpoint exists. Track replacement/removal and credit editing APIs remain follow-up work; the F1 endpoint only appends a track to an authorized draft.

Auth failure audits contain no email/password/token. Other audits contain actor, org, resource, action, reason code, request UUID and DB time. There is no general arbitrary audit JSON field into which credentials or document bodies could leak. Request logs omit bodies, URLs/query strings and cookie headers.
