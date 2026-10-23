# AUDENIQ REST API and OAuth 2.0 developer apps

Apps that are not AI assistants (a label's dashboard, a release tool, a
desktop uploader) can connect an AUDENIQ account with OAuth 2.0 and call a
REST API. The API opens exactly what the MCP server opens (docs/MCP.md): every
route runs one MCP tool through the same code path (`tools::call`), so
scopes, input schemas, rate limits, audit rows, retry-safe creates, payout
limits and the "approve once in the Studio" actions are identical. Only the
transport differs.

Code: `crates/core/src/mcp/rest.rs` (routes, arguments, statuses, OpenAPI),
`crates/core/src/mcp/developer.rs` (developer apps), `oauth.rs` (the shared
authorization server). Migration `0145_developer_apps.sql`.

## Turning it on

| Variable | Default | |
|---|---|---|
| `AUDENIQ_MCP_ENABLED` | `false` | The authorization server; required |
| `AUDENIQ_API_ENABLED` | `false` | `/api/v1`, its resource metadata and developer apps; 404 when off |

## Two resources, separate tokens

The authorization server issues tokens for one resource (RFC 8707
`resource`), stored on the grant:

| Resource | URL | Who may ask for it |
|---|---|---|
| MCP | `https://<studio host>/api/mcp` | AI apps (DCR, CIMD, pre-registered), never developer apps |
| API | `https://<studio host>/api/v1` | Developer apps (and admin clients) only |

- An MCP token is refused on `/api/v1` and an API token on `/api/mcp`
  (`401 invalid_token`).
- No `resource` (or the issuer) means MCP, as before. A refresh or code
  exchange naming the other resource answers `invalid_target`.
- The device code, password and agent-key grants issue MCP tokens only.
- Protected resource metadata (RFC 9728):
  `/.well-known/oauth-protected-resource/api/v1`. A `401` from the API
  carries `WWW-Authenticate: Bearer resource_metadata=…` pointing there.

## Developer apps

Registered in the Studio: Profile → 개발자 → 개발자 앱 (`/profile?open=developer`).

| Field | |
|---|---|
| Name | Shown on the consent screen; can't be changed later (make a new app) |
| Redirect URIs | 1 to 10; `https`, or `http` on a loopback host (`localhost`, `127.0.0.1`, `[::1]`, any port at runtime, RFC 8252) for native and development apps |
| Type | Server app: `client_secret_basic` (or `_post`), secret shown once, stored as SHA-256. Mobile/desktop: public client, PKCE only |
| Description, website | Optional, shown on the consent screen |

- client_id `audeniq-app-<32 hex>`; at most 10 live apps per user.
- **Visibility**: a new app is `PRIVATE` (나만 연결): only its owner can
  connect it; anyone else gets "이 앱은 아직 만든 사람만 연결할 수 있어요."
  Asking for review makes it `REVIEW` (검토 중, still owner-only). An
  operator approves it (`PUBLIC`, 공개) or returns it with a note
  (`PRIVATE` again, and connections of anyone but the owner end).
- New secret: the old one stops working at once.
- Delete: the app is disabled and every connection to it ends.
- The consent screen (`/ai/connect`) shows "{site} · 개발자 앱", the
  description, "내 앱" for the owner, and a note while the app is private.

Endpoints (Studio session cookie; writes need CSRF):
`GET/POST /api/developer/apps`, `PATCH/DELETE /api/developer/apps/{id}`,
`POST …/{id}/secret`, `POST …/{id}/review`. Errors: `APP_NAME_INVALID`,
`APP_REDIRECT_URI_INVALID`, `APP_DESCRIPTION_TOO_LONG`, `APP_WEBSITE_INVALID`,
`APP_LIMIT_REACHED`, `APP_HAS_NO_SECRET`, `APP_REVIEW_NOT_AVAILABLE`.
Audit actions `api.app_created|updated|secret_rotated|review_requested|deleted|reviewed`.

### Review (operators)

```sh
audeniq-admin developer-app list [PRIVATE|REVIEW|PUBLIC]
audeniq-admin developer-app approve CLIENT_ID [--note TEXT] --operator NAME
audeniq-admin developer-app return CLIENT_ID --note TEXT --operator NAME
```

Check the name, website and redirect URIs belong to the same developer and
that the description says what the app does with the account.

## Moving to a developer platform

Developer apps live in the Studio for now and move to a separate developer
platform later. The boundary is kept so that move is a frontend change:

- Backend: app management is only `/api/developer/*` (module
  `mcp/developer.rs`); nothing else in the Studio API depends on it.
- Studio: everything is in `web/studio/app/src/developer/` (the API client,
  the screen and its test); `pages/Profile.tsx` only mounts it.
- What stays where it is: the authorization server (`/authorize`, `/token`),
  the consent screen (`/ai/connect`, the artist's own account) and
  `/api/v1` — developers' apps keep the same URLs after the move.

When the platform comes, follow the admin console (`web/admin`,
`crates/core/src/sites.rs`): a new site value in `x-audeniq-site` with its
own origin variable, a path allowlist of the login paths, `/api/me` and
`/api/developer/*`, an edge worker that proxies `/api/*`, and the
`src/developer` screens moved over. The Studio row then links to the platform.

## Connecting (authorization code + PKCE)

```text
GET https://<studio host>/authorize?response_type=code&client_id=audeniq-app-…
    &redirect_uri=…&code_challenge=…&code_challenge_method=S256
    &scope=releases:read%20releases:write&state=…
    &resource=https://<studio host>/api/v1

POST https://<studio host>/token   (Basic auth for server apps)
grant_type=authorization_code&code=…&redirect_uri=…&code_verifier=…
&resource=https://<studio host>/api/v1
→ {"access_token":"…","refresh_token":"…","token_type":"Bearer","expires_in":3600,"scope":"…"}
```

Refresh tokens rotate (`grant_type=refresh_token`). Scopes are the MCP scopes
(docs/MCP.md "Scopes"); the user can disconnect the app in the Studio like any
AI app.

## Calling the API

```sh
curl -H "Authorization: Bearer $TOKEN" https://<studio host>/api/v1/releases
curl -X PATCH -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"title":"New title"}' https://<studio host>/api/v1/releases/<release_id>
```

- Arguments: path parameters, then the query string (GET/DELETE, converted
  with the tool's input schema) or the JSON body (POST/PUT/PATCH/DELETE). A
  value given twice differently answers `400`.
- Success: `200` with the tool's structured result.
- Failure: `{"error":{"code","message","details"?}}`:

| Status | Codes |
|---|---|
| 400 | `INVALID_ARGUMENTS`, `INVALID_INPUT`, `INVALID_JSON` |
| 401 | missing/expired token, or a token for another resource |
| 403 | `FORBIDDEN`, `INSUFFICIENT_SCOPE` (`WWW-Authenticate … error="insufficient_scope"`), a browser request from a foreign Origin |
| 404 / 405 | unknown route / method |
| 409 | `CONFLICT`, `INVARIANT_CONFLICT`, `REQUEST_IN_PROGRESS` |
| 422 | domain rules (a draft that is not complete, …) |
| 429 | `RATE_LIMITED` |
| 501 / 503 / 504 / 500 | not implemented / unavailable / timeout / internal |

- The confirmation rules of the MCP tools still hold: `POST /releases/{id}/submission`
  needs the `confirmation_token` from `GET …/submission/preview` (15 minutes, this exact content), payouts need
  `GET /payouts/preview`, and the `request_*` routes return an `approve_url`
  the artist opens in the Studio; nothing happens until they approve.
- `GET /api/v1/openapi.json` (no token): OpenAPI 3.1 generated from the tool
  schemas, with the OAuth flow.

## Routes

Every route is one MCP tool (a unit test checks all tools are mapped).

| Route | MCP tool |
|---|---|
| `GET /me` | `get_account_context` |
| `GET /members` | `list_members` |
| `POST /members/invitations` | `request_member_invite` |
| `DELETE /members/invitations/{user_id}` | `cancel_member_invite` |
| `GET /platforms` | `list_distribution_platforms` |
| `GET /requirements` | `get_release_requirements` |
| `GET /status` | `get_service_status` |
| `GET /announcements` | `list_announcements` |
| `GET /announcements/{kind}/{id}` | `get_announcement` |
| `GET /notifications` | `list_notifications` |
| `POST /notifications/read` | `mark_notifications_read` |
| `GET /documents` | `list_documents` |
| `GET /signing-requests` | `list_signing_requests` |
| `DELETE /signing-requests/{request_id}` | `cancel_signing_request` |
| `GET /withdrawal-quota` | `get_withdrawal_quota` |
| `GET /releases` | `list_releases` |
| `POST /releases` | `create_release_draft` |
| `GET /releases/{release_id}` | `get_release` |
| `PATCH /releases/{release_id}` | `update_release_draft` |
| `DELETE /releases/{release_id}` | `delete_release_draft` |
| `POST /releases/{release_id}/tracks` | `upsert_track` |
| `DELETE /releases/{release_id}/tracks/{track_id}` | `remove_track` |
| `PUT /releases/{release_id}/tracks/{track_id}/credits` | `set_track_credits` |
| `PUT /releases/{release_id}/special-options` | `set_special_options` |
| `GET /releases/{release_id}/validation` | `validate_release` |
| `GET /releases/{release_id}/upload-link` | `get_upload_link` |
| `GET /releases/{release_id}/submission/preview` | `preview_submission` |
| `POST /releases/{release_id}/submission` | `submit_release` |
| `GET /releases/{release_id}/submission` | `get_submission_status` |
| `DELETE /releases/{release_id}/submission` | `withdraw_pending_submission` |
| `GET /releases/{release_id}/handover` | `get_release_handover` |
| `GET /releases/{release_id}/guardian-consent` | `get_guardian_consent` |
| `GET /releases/{release_id}/delivery` | `get_delivery_status` |
| `GET /releases/{release_id}/content-id` | `get_content_id_status` |
| `GET /releases/{release_id}/corrections` | `get_release_corrections` |
| `POST /releases/{release_id}/corrections` | `request_release_correction` |
| `DELETE /releases/{release_id}/corrections` | `cancel_release_correction` |
| `GET /releases/{release_id}/pitch` | `get_playlist_pitch` |
| `POST /releases/{release_id}/pitch` | `submit_playlist_pitch` |
| `DELETE /releases/{release_id}/pitch` | `withdraw_playlist_pitch` |
| `GET /releases/{release_id}/smart-link` | `get_smart_link` |
| `PUT /releases/{release_id}/smart-link` | `save_smart_link` |
| `GET /releases/{release_id}/smart-link/check` | `check_smart_link` |
| `GET /releases/{release_id}/splits` | `get_release_splits` |
| `POST /releases/{release_id}/splits/{share_id}/resend` | `resend_split_invite` |
| `POST /releases/{release_id}/split-change` | `request_split_change` |
| `GET /releases/{release_id}/infringement-claims` | `list_infringement_claims` |
| `POST /infringement-claims/{claim_id}/counter-notice` | `request_counter_notice` |
| `DELETE /infringement-claims/{claim_id}/counter-notice` | `withdraw_counter_notice` |
| `POST /releases/{release_id}/withdrawal` | `request_release_withdrawal` |
| `POST /releases/{release_id}/takedown` | `request_takedown` |
| `DELETE /releases/{release_id}/takedown` | `cancel_takedown` |
| `POST /releases/{release_id}/end` | `request_release_end` |
| `DELETE /releases/{release_id}/end` | `cancel_release_end` |
| `GET /release-codes` | `list_release_codes` |
| `POST /uploads` | `start_upload` |
| `POST /uploads/import` | `import_file_from_url` |
| `GET /uploads/{upload_session_id}` | `get_upload_status` |
| `POST /uploads/{upload_session_id}/complete` | `complete_upload` |
| `GET /assets/{asset_id}/checks` | `get_file_checks` |
| `POST /rights-proofs` | `attach_rights_proof` |
| `DELETE /rights-proofs/{document_id}` | `withdraw_rights_proof` |
| `GET /inquiries` | `list_inquiries` |
| `POST /inquiries` | `create_inquiry` |
| `GET /inquiries/{inquiry_id}` | `get_inquiry` |
| `POST /inquiries/{inquiry_id}/replies` | `reply_inquiry` |
| `POST /inquiries/{inquiry_id}/close` | `close_inquiry` |
| `GET /platform-requests` | `list_platform_requests` |
| `POST /platform-requests` | `create_platform_request` |
| `DELETE /platform-requests/{request_id}` | `cancel_platform_request` |
| `GET /artist-pages` | `list_artist_pages` |
| `PUT /artist-pages` | `set_artist_pages` |
| `GET /settlement` | `get_settlement_summary` |
| `GET /settlement/statements` | `list_settlement_statements` |
| `GET /settlement/royalty-reports` | `get_royalty_reports` |
| `GET /settlement/export` | `export_csv` |
| `GET /splits/received` | `list_received_splits` |
| `GET /payouts` | `list_payouts` |
| `GET /payouts/preview` | `preview_payout` |
| `POST /payouts` | `request_payout` |
| `POST /payouts/large` | `request_large_payout` |
| `DELETE /payouts/{payout_id}` | `cancel_payout` |
| `POST /contract/end` | `request_contract_end` |
| `DELETE /contract/end` | `cancel_contract_end` |
| `POST /permission-requests` | `request_permission` |
| `GET /permission-requests/{request_id}` | `get_permission_request` |
| `DELETE /permission-requests/{request_id}` | `withdraw_permission_request` |
| `GET /action-requests/{request_id}` | `get_action_request` |
| `DELETE /action-requests/{request_id}` | `withdraw_action_request` |
