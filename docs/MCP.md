# AUDENIQ MCP server (AI assistant distribution applications)

AUDENIQ's own MCP server lets an artist prepare, check and submit a distribution
application from an AI assistant (ChatGPT, Claude, any MCP client) using their
AUDENIQ account. It is part of `audeniq-api` (no separate service), written in
Rust, and reuses the Studio's services for every rule.

**Status: implemented and tested against PostgreSQL; off by default
(`AUDENIQ_MCP_ENABLED=false`). Not deployed, not published to any AI platform
directory, not verified with a real ChatGPT or Claude client.**

## Architecture

```
AI client ──HTTPS──▶ audeniq-studio Worker (web/studio) ──tunnel + service secret──▶ audeniq-api
  POST /api/mcp[/|/public]      forwards Authorization + MCP headers           crate::mcp::protocol  (JSON-RPC)
  /api/oauth/*                  only for these paths; no Origin needed         crate::mcp::oauth     (authorization server)
  /.well-known/oauth-*          for server-to-server calls                     crate::mcp::tools     (tool handlers)
                                                                                    │ Actor (user, party) + token's org
                                                                                    ▼
                                         catalog · drafts · submission · uploads · api::dsp_availability
                                         (same functions the Studio's HTTP handlers call)
```

- `mcp/protocol.rs`: Streamable HTTP, one POST per JSON-RPC message, answered with
  a single JSON object (no SSE, no server-initiated messages, no sessions).
  Dual-era: **2026-07-28** (stateless; `_meta["io.modelcontextprotocol/protocolVersion"]`
  mirrored in `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`, validated against
  the body, `-32020` on mismatch, `-32022` for an unsupported version,
  `server/discover`) and the `initialize`-based **2025-11-25 / 2025-06-18 /
  2025-03-26** revisions (no `Mcp-Session-Id` is issued). GET/DELETE → 405.
  A browser `Origin` other than the Studio's → 403.
- `mcp/oauth.rs`: OAuth 2.1 authorization server (below).
- `mcp/tools.rs`: tool schemas, argument validation (JSON Schema, `jsonschema`
  crate already in the tree), rate limits, audit, and handlers that only call
  existing services. Nothing in the MCP layer writes catalog/submission tables
  directly.
- `mcp/widget.html`: UI resource for ChatGPT Apps / MCP Apps hosts.

No MCP SDK is used: the server needs only request/response JSON-RPC, which is a
few hundred lines here, and the Rust SDKs bring their own session/transport
layers that would bypass the API's boundary middleware (service secret,
header/body limits, deadline, maintenance gate, request ids). The protocol code
is covered by the integration tests listed below.

Small refactors made so tools share the Studio's code instead of copying it:
`api::dsp_availability` (was inline in the `/dsps` handler),
`catalog::ensure_party` (was inline in `/parties`), `catalog::workspace`,
`catalog::party_names`, `submission::submit_draft_checks` (the block of draft
checks `submit` already ran, now shared with `submission::draft_review`) and
`submission::release_type_tracks`. HTTP behavior is unchanged.

## Authentication and account linking

| Endpoint | Purpose |
|---|---|
| `GET /.well-known/oauth-protected-resource[/api/mcp]` | RFC 9728 metadata: resource `{origin}/api/mcp`, authorization server `{origin}` |
| `GET /.well-known/oauth-authorization-server` | RFC 8414 metadata (S256 only, CIMD supported, `iss` in responses) |
| `POST /api/oauth/register` | RFC 7591 dynamic registration (`AUDENIQ_MCP_DYNAMIC_REGISTRATION`) |
| `GET /api/oauth/authorize` | Consent screen (server-rendered, no script) |
| `POST /api/oauth/authorize` | Consent decision (Studio session + Origin + signed request) |
| `POST /api/oauth/token` | `authorization_code` (PKCE), `refresh_token`, `urn:ietf:params:oauth:grant-type:device_code`, `password` (trusted agents only) |
| `POST /api/oauth/device_authorization` | RFC 8628: a device code for the client, a link and user code for the artist |
| `GET /api/oauth/device`, `POST /api/oauth/device` | The link's page: Studio sign-in, code check, consent (same screen and rules as `/authorize`) |
| `POST /api/auth/ai-connections/tokens` | Studio session: make a connection token (fixed bearer) for the current workspace |
| `POST /api/oauth/revoke` | RFC 7009 revocation (ends the whole grant) |
| `GET /api/auth/ai-connections`, `POST /api/auth/ai-connections/{id}/revoke` | The user's connected AI apps (Studio session); Studio shows them under 프로필 → 로그인 → 연결된 AI 앱 |

Flow: the client gets a 401 with `WWW-Authenticate: Bearer resource_metadata=…`,
discovers the authorization server, registers (CIMD or DCR), opens
`/api/oauth/authorize` in the user's browser. The user must already be signed in
to AUDENIQ Studio in that browser (the page links to `/login` otherwise; the AI
client never sees a password). The consent page shows the client name, the
host it will return to (and the CIMD URL), a workspace selector and the
permissions. `releases:submit` is never pre-selected. Approval redirects with
`code`, `state` and `iss`.

- **Clients.** Client ID Metadata Documents are fetched only from hosts in
  `AUDENIQ_MCP_CIMD_HOSTS` (default `*`: any host, since the server advertises
  `client_id_metadata_document_supported` and a client that sees it uses a URL
  client id instead of DCR; entries can narrow it to hosts or exact document
  URLs). Fetches go to public addresses only (DNS answers that are loopback,
  private, link-local, CGNAT or reserved are dropped before connecting; no
  address literals, no proxy; HTTPS, no port,
  5 s timeout, 64 KiB, no redirects, re-fetched after 24 h, cached copy used up
  to 7 days when the host is down). The document's `client_id` must equal its
  URL and it must allow `none` authentication. DCR clients get a random
  `client_id`; confidential ones a secret stored as SHA-256. Redirect URIs:
  HTTPS without fragment, or loopback `http` (any port, RFC 8252).
- **Codes/tokens.** Codes: 10 min, once, PKCE S256 only, bound to client and
  redirect URI; a replayed code revokes the grant. Access tokens: 1 h. Refresh
  tokens: 30 days, rotated on every use; reuse revokes the grant, except within
  30 s of the first use (a retry or parallel refresh gets its own new tokens). A grant ends
  90 days after consent at the latest. All are opaque 256-bit random values
  stored as SHA-256 digests. Tokens are bound to the `{origin}/api/mcp`
  resource (RFC 8707; any other `resource` is `invalid_target`).
- **Binding.** A grant is one user + one workspace + the approved scopes.
  Tools take no workspace argument; every call re-checks the user's ACTIVE
  status and membership, resource ACLs and role (a VIEWER cannot edit even with
  `releases:write`).
- **Revocation.** Changing the password, password reset and "log out
  everywhere" revoke all AI grants of the user. Users can revoke a single
  connection; clients can revoke via RFC 7009. A disabled client
  (`identity.mcp_clients.disabled_at`) stops working at once.
- Not implemented: OpenID Connect (no ID token/UserInfo; ChatGPT only needs it
  for workspace domain restrictions), `private_key_jwt` client authentication
  (ChatGPT also offers `none`), mTLS client certificates.

### Other ways to connect

Clients that can open a browser redirect (ChatGPT, Claude) use the flow above.
Three more, by what the agent can do:

| Agent | How it connects | Grant `method` |
|---|---|---|
| Registered by an operator (client secret), no browser redirect | **Link + code** (RFC 8628; public DCR/CIMD clients get `unauthorized_client`, since without a redirect anyone could start it under ChatGPT's name and phish an approval): `POST /api/oauth/device_authorization` → the assistant shows `verification_uri_complete` and `user_code` in the chat → the artist opens it, signs in to the Studio, checks the code and approves → the client polls `/api/oauth/token` (`authorization_pending`, `slow_down` faster than every 5 s, `access_denied`, `expired_token`). | `DEVICE` |
| Has a credential form that sends the e-mail and password to AUDENIQ without the model seeing them (an operator verified this) | **Password grant**: `grant_type=password&username=&password=[&scope=][&org_id=]` with the client secret an operator issued (`audeniq-admin mcp-client add NAME --password-grant`). Login rate limits (`mcp-password` buckets) and audit apply; `payouts:request` is refused; workspace defaults to the first editable one. Public, DCR and CIMD clients get `unauthorized_client`. | `PASSWORD` |
| Anything else (only a fixed `Authorization` header) | **Agent key** (connection token): the artist makes one per agent in the Studio (프로필 → 로그인 → 연결된 AI 앱 → 에이전트 키 만들기): name, permissions (read always, write on by default, submit/settlement/payout opt-in, or "모든 권한 허용"), 7/30/90 days or no end date (`no_expiry`: works until revoked; password change and log out everywhere still end it; the Studio warns before making one). Shown once; stored as SHA-256; listed and revocable like any connection. | `TOKEN` |

**Work tokens (migration 0142).** An agent key can be used as a bearer as it
is, or exchanged for a short work token limited to what the task needs (RFC 8693):

```sh
curl -X POST https://studio.audeniq.com/api/oauth/token \
  -d grant_type=urn:ietf:params:oauth:grant-type:token-exchange \
  -d subject_token_type=urn:ietf:params:oauth:token-type:access_token \
  -d subject_token=$AUDENIQ_AGENT_KEY -d scope="releases:read releases:write"
# → {"access_token":"…","issued_token_type":"urn:ietf:params:oauth:token-type:access_token",
#    "token_type":"Bearer","expires_in":3600,"scope":"releases:read releases:write"}
```

No client authentication (the key is the credential). `scope` defaults to
everything the connection has and can never exceed it (`invalid_scope`: ask
with `request_permission`); the token's scopes are stored with it
(`identity.mcp_tokens.scopes`) and intersected with the connection's on every
call. Only agent keys (method `TOKEN`, not themselves exchanged) can be
exchanged; OAuth connections refresh instead (`invalid_grant`). 600 work
tokens per key per 15 minutes. Revoking the connection deletes the key and
every work token. `grant_types_supported` lists the exchange grant.

Never ask the artist to type a password into a chat: a model that sees it
keeps it in the conversation history. Agents that cannot keep it from the
model use a connection token or the link + code.

Operators: `audeniq-admin mcp-client list`, `audeniq-admin --operator NAME
mcp-client add NAME [--password-grant] [--secret-post] [--redirect-uri URI]...`
(prints `client_id` and `client_secret` once; without `--redirect-uri` the
agent cannot use the browser redirect), `audeniq-admin --operator NAME
mcp-client disable CLIENT_ID` (ends all of its connections). Changes are
audited as `mcp.client_registered` / `mcp.client_disabled` with the operator.

### Two endpoints

- `POST /api/mcp` (also `/api/mcp/`): the account endpoint, the URL to add as
  a connector. Every request but `server/discover` needs a token; without one
  (including `initialize` and `tools/list`) it answers `401` with
  `WWW-Authenticate: Bearer resource_metadata=…`, which is what makes Grok,
  Manus, Cursor, VS Code, ChatGPT and Claude start OAuth when they connect
  (most clients only do so on a 401, never mid-session).
- `POST /api/mcp/public`: no account. `tools/list` shows only the public tools
  below; any other tool answers a JSON-RPC error naming `/api/mcp`.

### Public tools (no account)

`get_audeniq_info` (published notices and events, as on `/api/notices` and
`/api/events`; with `view: service_status` maintenance, as on `/api/status`;
with `view: requirements` the release requirements) needs no token on
`/api/mcp/public` (on `/api/mcp` they are called with the connection's token
like the rest). Its `securitySchemes` list `noauth` first, then `oauth2`.

| Caller | Daily limit (KST day) | Over the limit |
|---|---|---|
| No token | None (not counted) | — |
| Token | `AUDENIQ_MCP_PUBLIC_DAILY_USER` (default 150) per user, or the user's own limit | Tool error `DAILY_LIMIT_REACHED` with the support link; support raises it with `audeniq-admin --operator NAME mcp-limit set EMAIL N [--note TEXT]` (`clear EMAIL` = default, `list`) |

Only the public tools count toward these limits; account tools keep their own
per-connection rate limits. A sent token must be valid (401 otherwise, so the
client refreshes); it is never silently treated as anonymous. Each result
carries `usage` (`signed_in`, and for signed-in users `limit`, `used_today`,
`remaining_today`). Calls without an account are not counted (AI platforms
call from shared server addresses, so a per-IP cap would be shared by all of
their users).
Counts live in `identity.mcp_daily_usage` (keyed HMAC of the user, like
the login limits) and are deleted after a day; per-user limits in
`identity.mcp_usage_limits` (changes audited as `mcp.usage_limit_set`).

### Scopes

| Scope | Grants |
|---|---|
| `releases:read` | Always granted: read releases, requirements, checks, status |
| `releases:write` | Create/edit drafts, tracks and credits (only in editable states) |
| `releases:submit` | Opt-in: submit a previewed, artist-confirmed application |
| `settlement:read` | Opt-in: balance, statement, royalty reports, payout requests (bank and last 4 digits only, no holder name or number) |
| `payouts:request` | Opt-in, owners only: small payouts to the account registered in Studio, within `AUDENIQ_MCP_PAYOUT_LIMIT_KRW` (default 300,000원 per 24 h); cancel unsent payouts |

Insufficient scope → HTTP 403 with `WWW-Authenticate: Bearer error="insufficient_scope", scope="…"`;
the message points the assistant at `request_permission`.

### Asking for more permissions (migration 0141)

A connection does not have to be made again to gain a permission:

1. The assistant calls `request_permission(scopes, reason)` (any connection;
   only the scopes it lacks are asked for; the same open request is returned
   if asked again; 10 per connection per 15 min). It gets `request_id` and
   `approve_url` = `{origin}/ai/approve/{id}` to show the artist, who also gets
   a Studio notice with the link.
2. The artist opens the link signed in to the Studio (only their own requests;
   `GET /api/auth/ai-permission-requests[/{id}]`) and sees the app, workspace,
   current permissions, the requested ones and the reason. Submit and payout
   permissions ask once more before approving.
3. `POST …/{id}/approve` (session + CSRF) adds the scopes to the grant
   (`identity.mcp_grants.scopes`) and audits `mcp.scopes_added`; they stay until
   the connection is revoked, and tokens the assistant already holds get them
   on the next call (scopes are read from the grant). `…/reject` changes
   nothing (`mcp.scopes_rejected`).
4. A request lapses after 60 minutes (`AI_REQUEST_EXPIRED`); a decided one
   answers `AI_REQUEST_DECIDED`; a revoked connection's request
   `AI_CONNECTION_REVOKED`. `payouts:request` cannot be asked for while the
   assistant payout limit is 0. `get_permission_request(request_id)` reports
   the outcome and the connection's permissions.
No scope reaches payout account changes, larger payouts, contracts/signatures,
withdrawals, takedowns, members or account settings.

## Tools

All tools declare `inputSchema` with `additionalProperties: false`, MCP
annotations (`readOnlyHint`, `destructiveHint: false`, `idempotentHint`,
`openWorldHint: false`), `securitySchemes` (OAuth2 + scope) and, where useful,
the release-card UI resource. Results carry `structuredContent` (with a `view`
field) plus a text rendering. Failures are tool results with `isError: true`
and `{code, message}` (stable API codes, human messages from
`error::message`); internal details are never returned.

### Published tools and operations

`tools/list` shows 25 tools. The 81 operations below are the units of scope,
schema, rate limit, audit (`mcp.<operation>`) and handler; operations of the
same kind are published as one tool, and one argument picks the operation
(`view` for reads, `action` or `kind` for writes). The other arguments are the
operation's own and each call is checked against the operation's own schema.
A grouped tool's annotations are the most careful of its operations' (read
only if all are, destructive if any is, idempotent if all are). Every group
stays within one scope, so the consent screen and `request_permission` work
as before. Clients that kept the earlier list can still call an operation by
its own name.

| Tool | Argument | Operations |
|---|---|---|
| `get_account_context` | `view`: `account` (default), `members`, `platforms`, `notifications`, `documents`, `signing_requests`, `inquiries`, `inquiry`, `artist_pages`, `platform_requests` | `get_account_context`, `list_members`, `list_distribution_platforms`, `list_notifications`, `list_documents`, `list_signing_requests`, `list_inquiries`, `get_inquiry`, `list_artist_pages`, `list_platform_requests` |
| `get_audeniq_info` (public) | `view`: `list` (default), `detail`, `service_status`, `requirements` | `list_announcements`, `get_announcement`, `get_service_status`, `get_release_requirements` |
| `list_releases` | `view`: `releases` (default), `codes` | `list_releases`, `list_release_codes` |
| `get_release` | `view`: `details` (default), `status`, `delivery`, `content_id`, `corrections`, `playlist_pitch`, `smart_link`, `smart_link_check`, `claims`, `missing_files` | `get_release`, `get_submission_status`, `get_delivery_status`, `get_content_id_status`, `get_release_corrections`, `get_playlist_pitch`, `get_smart_link`, `check_smart_link`, `list_infringement_claims`, `get_upload_link` |
| `validate_release` | `view`: `checks` (default), `preview` | `validate_release`, `preview_submission` |
| `get_file_status` | `view`: `upload`, `checks` | `get_upload_status`, `get_file_checks` |
| `get_approval_status` | `kind`: `action` (default), `permission` | `get_action_request`, `get_permission_request` |
| `get_settlement` (settlement) | `view`: `summary` (default), `statements`, `royalty_reports`, `payouts`, `splits`, `received_splits`, `csv` | `get_settlement_summary`, `list_settlement_statements`, `get_royalty_reports`, `list_payouts`, `get_release_splits`, `list_received_splits`, `export_csv` |
| `save_release_draft` | `action`: `create`, `update`, `special_options` | `create_release_draft`, `update_release_draft`, `set_special_options` |
| `upsert_track` | `action`: `save` (default), `credits` | `upsert_track`, `set_track_credits` |
| `upload_file` | `action`: `start`, `complete`, `import` | `start_upload`, `complete_upload`, `import_file_from_url` |
| `promote_release` | `action`: `pitch`, `smart_link` | `submit_playlist_pitch`, `save_smart_link` |
| `manage_artist_pages` | `action`: `links`, `platform_request` | `set_artist_pages`, `create_platform_request` |
| `manage_inquiry` | `action`: `create`, `reply`, `close` | `create_inquiry`, `reply_inquiry`, `close_inquiry` |
| `request_approval` | `action`: `withdrawal`, `takedown`, `split_change`, `release_end`, `contract_end`, `counter_notice`, `member_invite` | `request_release_withdrawal`, `request_takedown`, `request_split_change`, `request_release_end`, `request_contract_end`, `request_counter_notice`, `request_member_invite` |
| `cancel_request` | `kind`: `correction`, `playlist_pitch`, `takedown`, `release_end`, `contract_end`, `platform_request`, `approval`, `submission` | `cancel_release_correction`, `withdraw_playlist_pitch`, `cancel_takedown`, `cancel_release_end`, `cancel_contract_end`, `cancel_platform_request`, `withdraw_action_request`, `withdraw_pending_submission` |
| `request_permission` | `action`: `request` (default), `withdraw` | `request_permission`, `withdraw_permission_request` |
| `request_payout` (payout, destructive) | `action`: `small` (default), `large`, `cancel` | `request_payout`, `request_large_payout`, `cancel_payout` |

The other 7 operations are published under their own name:
`mark_notifications_read`, `delete_release_draft`,
`attach_rights_proof`, `request_release_correction`, `resend_split_invite`,
`preview_payout` and `submit_release`. They stay separate on purpose: a
different scope, a confirmation step that must stay its own call
(`preview_payout` before `request_payout`, `validate_release` preview before
`submit_release`), or a destructive or not-read-only hint that would
otherwise spread to safe operations in the same tool.

| Operation | Scope | Backed by | What it does |
|---|---|---|---|
| `get_account_context` | read | `catalog::workspace` | Workspace, role, connection scopes, `can_submit` |
| `list_distribution_platforms` | read | `api::dsp_availability`, `dsp_registry` | Platforms, availability now, per-platform requirements (artwork px, sample rate/bit depth, lossless, lead days, required credits, loudness) |
| `get_release_requirements` | read | constants from `submission`, `uploads` | Required fields, release types/track counts, file types/limits, credit roles, declarations |
| `list_releases` | read | `catalog::list` | Releases with status, paged |
| `get_release` | read | `catalog::get`, `catalog::party_names` | Release, tracks, credits (names), audio attached, agreement/delivery/live state |
| `create_release_draft` | write | `catalog::create` (artist via find-or-create) | New draft (title, type, artist + optional fields) |
| `update_release_draft` | write | `catalog::update` (optimistic lock) | Change given fields only (`artwork_asset_id` attaches a registered cover, also in the Studio editor's working copy); refused once under review |
| `upsert_track` | write | `catalog::track`, `drafts::replace_track` | Add/edit a track; omitted fields keep their values; `asset_id` attaches a registered audio file |
| `set_track_credits` | write | `catalog::ensure_party`, `drafts::replace_credits` | Composers, lyricists, arrangers, producers (Studio roles) |
| `validate_release` | read | `drafts::preflight`, `submission::presubmit`, `submission::draft_review`, `uploads::checks` | Blocking findings (field-level, with track ids), items for human review, audio QC, missing files + Studio upload link |
| `preview_submission` | read | same + declarations | Full application preview; when ready and the grant has `releases:submit`, a `confirmation_token` (15 min) |
| `submit_release` | submit | `submission::create_consent`, `submission::submit` | Submits exactly the previewed content |
| `get_submission_status` | read | `submission::submission_status`, `catalog::get` | Pipeline status explained, checks, corrections, staff notes, agreement and follow-up |
| `get_release_corrections` | read | `corrections::status` | Delivered release: current genre/credits with approved corrections, latest request and AUDENIQ's review decision/note |
| `request_release_correction` | write | `corrections::request` | Genre and non-artist credit corrections of a delivered release; staff review, then sent to every platform |
| `cancel_release_correction` | write | `corrections::cancel` | Withdraw a pending correction |
| `get_playlist_pitch` / `submit_playlist_pitch` / `withdraw_playlist_pitch` | read / write / write | `pitching::status/submit/withdraw` | Focus-track pitch to platform editors (submitted releases, ≥7 days before release), AUDENIQ forwards it |
| `get_smart_link` / `save_smart_link` | read / write | `smart_links::status/save` | Public landing page URL, platform links and checks, 30-day views/clicks, fan release-alert sign-ups |
| `list_release_codes` | read | `codes::list` | UPC and ISRCs of every submitted release |
| `list_notifications` | read | `portal::list_notifications` | Latest notifications, unread count |
| `list_documents` | read | `portal::list_documents` | Agreements/rights documents: status, review note, next step, Studio link (no legal text, signature or record) |
| `list_inquiries` / `get_inquiry` / `create_inquiry` / `reply_inquiry` | read / read / write / write | `portal` inquiries | Support conversation with AUDENIQ |
| `get_settlement_summary` | settlement | `portal::finance_summary`, `portal::get_payout_account` | Payable, pending, held, available, fee policy, account (bank/last 4), assistant payout room |
| `list_settlement_statements` | settlement | `portal::statements` | Ledger lines, paged |
| `get_royalty_reports` | settlement | `portal::reports` | Streams/revenue by month, platform and release |
| `list_payouts` | settlement | `portal::list_payouts` | Payout requests (holder name removed) |
| `preview_payout` | payout | `portal::quote_payout` + checks | Fee, net, balance, limit; `confirmation_token` (10 min) only when allowed |
| `request_payout` | payout | `portal::request_payout_capped` | The previewed payout; marked destructive for client confirmation |
| `cancel_payout` | payout | `portal::cancel_payout` | Cancel before it goes to the bank |
| `get_upload_link` | read | `catalog::get` | Studio upload page for the release (`/upload?edit=`) and which audio/cover files are missing; opened while signed in to the Studio |
| `start_upload` / `complete_upload` | write | `uploads::issue/complete` | Signed PUT for one audio master or cover, then the Studio's completion (sniff, antivirus, lossless FLAC) |
| `import_file_from_url` | write | `external_http::public_client_builder`, `uploads::issue/complete` | Fetches a public https:// link the artist gave and registers it like an upload |
| `get_upload_status` | read | `uploads::status` + `identity.mcp_uploads` | READY / PROCESSING / WAITING_FOR_FILE / EXPIRED / FAILED (error code) |
| `mark_notifications_read` | write | `portal::read_notifications` | Given notification ids, or all |
| `close_inquiry` | write | `portal::close_inquiry` | Close a resolved support inquiry |
| `attach_rights_proof` | write | `portal::attach_proof`, `portal::create_document` | A DOCUMENT/IMAGE upload becomes a rights proof (waiting document, or a new one for a release); AUDENIQ reviews it |
| `get_file_checks` | read | `uploads::checks` | Per-file audio/image checks |
| `get_delivery_status` | read | `delivery_staging::release_delivery_view` | Per-platform delivery of a release |
| `get_content_id_status` | read | `platform_requests::content_id` | YouTube Content ID state |
| `check_smart_link` | read | `smart_links::check` | Verdict per platform link, nothing saved |
| `list_signing_requests` | read | `signing::list` | Signing requests and status (no signing links) |
| `list_members` | read | `members::list` | Members and roles; e-mails masked unless owner |
| `list_infringement_claims` | read | `infringement::for_release` | Claims against a release and the counter deadline |
| `resend_split_invite` | write | `splits::resend` | E-mail a collaborator's split invite again (rate limited) |
| `export_csv` | settlement | `portal::statements_csv/reports_csv` | Statements or royalty reports as CSV text (≤ 256 KB) |
| `delete_release_draft` | write | `catalog::archive` | Never-submitted drafts only, with `artist_confirmed` |
| `request_release_withdrawal` / `request_takedown` / `request_split_change` / `request_release_end` / `request_contract_end` / `request_counter_notice` / `request_member_invite` | write | `mcp::actions` → `withdraw`, `takedown`, `splits::save`, `contract_terms`, `infringement::counter`, `members::invite` | Filed for one Studio approval (see Actions approved once) |
| `request_large_payout` | payout | `mcp::actions` → `portal::request_payout` | Any amount up to the balance, approved once in the Studio |
| `get_action_request` | read | `identity.mcp_action_requests` | PENDING / DONE (result) / FAILED (error code) / REJECTED / EXPIRED / REVOKED / WITHDRAWN |
| `withdraw_action_request` / `withdraw_permission_request` / `withdraw_pending_submission` | write (or the scope that sent it) / read / write (or submit) | `mcp::actions`, `mcp::approvals` (migration 0145) | Take back this connection's own request or application before the artist answers; the Studio then refuses it (`AI_REQUEST_WITHDRAWN`, `AI_SUBMISSION_WITHDRAWN`) |
| `cancel_takedown` / `cancel_release_end` / `cancel_contract_end` | write | `takedown::cancel`, `contract_terms::cancel_*` | Undo a pending request or scheduled end; `artist_confirmed` |
| `set_special_options` | write | `draft.options` via `catalog::update` | Cover (original work, rights, licence file), samples, AI use, express release; `artist_confirmed` |
| `list_artist_pages` / `set_artist_pages` | read / write | `artist_pages::list/set_pages` | Each artist's own page per platform (Spotify, Apple Music, Melon, …), so deliveries never land on a same-name stranger's page |
| `list_platform_requests` / `create_platform_request` / `cancel_platform_request` | read / write / write | `platform_requests::list/create/cancel` | After-release fixes AUDENIQ does at a platform: wrong artist page, page merge/split, YouTube OAC, Content ID |
| `get_release_splits` / `list_received_splits` | settlement | `splits::sheet/received` | Collaborator revenue splits of a release (masked e-mails) and shares received; changes stay in the Studio |

### Files (migration 0143)

Audio masters and cover art reach a release without the Studio
(`mcp::files`). Both ways end in the Studio's own upload completion, so the
same checks apply: the declared type must match the bytes, the antivirus scan
runs when enabled, lossless audio becomes a FLAC master, images are re-encoded
without metadata.

- **Signed PUT** (agents that can send HTTP): `upload_file {action: start, kind,
  size_bytes, content_type}` (`start_upload`) returns `upload.url`, `upload.method` and the exact
  `upload.headers` (10 minutes, single use). The agent PUTs the bytes, then
  calls `upload_file {action: complete, upload_session_id, asset_id,
  expected_key}` (`complete_upload`).
- **Import from a link** (chat assistants): `upload_file {action: import, kind,
  url, content_type?}` (`import_file_from_url`). The server fetches the file with the public-only client:
  https only, a domain name (IP literals are refused so the address check
  cannot be skipped), no user info, every redirect checked again (at most 5),
  only public addresses. The response must state `Content-Length` (within the
  upload limits) and exactly that many bytes must arrive; the type comes from
  the argument, the response header or the file extension. At most 4 imports
  run at once per server (`IMPORT_BUSY`), 30 per connection per rate window,
  15 minutes each.
- Completion waits up to 45 s inside the tool call. A long master answers
  `PROCESSING`; the work continues in the background and records its outcome
  in `identity.mcp_uploads`, which `get_upload_status` reads (work without an
  outcome after 30 minutes reports `IMPORT_INTERRUPTED`).
- The registered asset is attached with `upsert_track` (`asset_id`) or
  `update_release_draft` (`artwork_asset_id`). Rights proofs are uploaded as
  `DOCUMENT` (PDF, JPG, PNG) and attached with `attach_rights_proof`.

### Actions approved once (migration 0144)

Some actions change money, rights or legal standing. An assistant can only
ask for them (`mcp::actions`); the artist approves each one in the Studio.

- The `request_*` tool checks what it can now (fields, access to the
  release, balance and account for a payout) and stores the payload with a
  Korean summary in `identity.mcp_action_requests`. Asking again with the same
  payload returns the open request; 20 requests per connection per rate
  window. The artist gets a notice linking to `/ai/actions/{id}`.
- The approval page shows the app, the workspace, the summary lines and a
  warning. Approval locks the request with its connection (refused when
  decided, expired after 24 hours or the connection was revoked), marks it
  RUNNING and runs the action with the approving artist's own session, the
  same service call the Studio makes. The outcome is stored (DONE with the
  result, or FAILED with the error code) and shown on the page; the
  assistant reads it with `get_approval_status` (`get_action_request`).
- Payouts approved this way use the idempotency key `mcp-approval:{id}`.
- Until the artist answers, the connection that filed a request can take it
  back (`cancel_request` kind `approval`, `request_permission` action
  `withdraw`, and kind `submission` for an application waiting for Studio
  approval). The row becomes WITHDRAWN under the same compare-and-set the
  approval uses (migration 0145), so a request is either run or withdrawn,
  never both; the Studio shows it as withdrawn and refuses to approve it.
  Another connection's requests are not found. What was already carried out
  is undone with the matching `cancel_request` kind or in the Studio.
- Undoing a pending takedown or a scheduled end, and a draft's special
  options, need only the artist's explicit word in the conversation
  (`artist_confirmed`); `set_special_options` writes `draft.options` exactly
  as the Studio editor does, and the declarations at submission must match.

**Prompts** (`prompts/list`, `prompts/get`; Claude shows them as slash
commands): `prepare_release`, `check_release`, `submission_status`,
`settlement_check`, `artist_pages`. Each only fills the artist's first
message, in AUDENIQ's Korean voice; arguments are quoted as data. Every action
still goes through the tools and their confirmations.

### Payout confirmation and limit

`preview_payout` refuses (with `blocked_by`) when the caller is not the owner,
no account is registered, the balance is short, or the amount exceeds what is
left of the assistant limit in the last 24 hours (`PAYOUT_USE_STUDIO`, with the
Studio settlement link). Otherwise it returns a token bound to the grant, the
workspace, the amount and the payout account's version. `request_payout`
requires that token and `artist_confirmed: true`, refuses if the account
changed since the preview, and calls the Studio's payout function with
idempotency key `mcp:<nonce>` and a cap: inside the same transaction and
organisation lock as the payout itself, assistant payouts (keys starting with
`mcp:`, not cancelled) in the last 24 hours plus this one must stay within the
limit, so concurrent previews cannot add up past it. Retrying with the same
token returns the same request. Staff still review every payout before money
moves (existing flow). Set `AUDENIQ_MCP_PAYOUT_LIMIT_KRW=0` to switch assistant
payouts off (the consent screen then no longer offers the scope).

Draft fields map onto the Studio's draft keys (`artist`, `language`, `genre`,
`label`, `release_date`, `release_time`, `preorder_start`, `territories`,
`platforms`, `p_line`/`phonogram`, `c_line`/`copyright`, `notes_lines`, `type`),
and tracks/credits are mirrored into `draftTracks` (credit order preserved), so
a release prepared through MCP opens and re-saves correctly in the Studio.

### Submission confirmation

1. `validate_release {view: preview, release_id, declarations}`
   (`preview_submission`) runs every check, including
   the exact draft checks `submit` applies (platform selection/routability,
   territories, release time, pre-order, special options, text policy,
   protected names) and the Stage 1 field rules.
2. Only if nothing blocks and `rights_confirmed` and `adult_confirmed` are true
   does it return `confirmation_token`: an HMAC-signed value binding the grant,
   the release, the hash of the consent scope (the full draft content) and the
   declarations, valid 15 minutes.
3. `submit_release` requires that token, identical declarations and
   `artist_confirmed: true`. If the release changed since the preview it
   answers `RELEASE_CHANGED`. It then records a consent package and calls
   `submission::submit` with idempotency key `mcp:{release}:{nonce}`, so a
   retry returns the same application (`duplicate_request: true`) and never
   creates a second revision.

What the server can and cannot prove: the token proves the submission matches a
preview the server produced for this user's grant, that the user opted into
`releases:submit` on the consent screen, and that nothing changed since. It
cannot prove the human read the preview in the chat; that rests on the
assistant (server instructions, tool descriptions) and the client's own tool
confirmation UI. Declarations are recorded as the user's, exactly as in the
Studio.

**One-time approval.** A connection with `releases:write` but not
`releases:submit` may also preview and call `submit_release`; the application
then always waits for the artist's approval of exactly that content in the
Studio (the pending submissions below, whatever the Studio approval setting),
so the assistant can send one release without being given the submit
permission for good. `get_account_context` reports `can_submit_with_approval`.

After submission the artist still signs the release application and the
distribution agreement in AUDENIQ Studio; delivery waits for the signed,
approved agreement (`execution::enqueue_delivery_jobs`). The MCP server cannot
sign. `get_release {view: status}` (`get_submission_status`) says so in
`follow_up`.

## Name and voice

The server instructions (`initialize` / `server/discover`, `protocol.rs`
`INSTRUCTIONS`) tell the assistant how to speak for AUDENIQ. The artist's own
requests about language, tone and format come first; otherwise:

- The name is AUDENIQ; in Korean it is **어데닉** (never 오디닉/오데닉), and the
  studio is 어데닉 스튜디오. `get_account_context` also returns
  `brand: {name, name_ko, studio_ko}` for clients that drop server instructions.
- Korean replies follow the Studio writing rules (docs/DESIGN_SYSTEM.md 23):
  해요체, active voice, casual honorifics, plain words, no blame, a next step.
- Money, rights and legal wording is never shortened or softened (item 21).
- The widget shows Korean text in the same voice when the host locale is `ko`,
  English otherwise.

### AUDENIQ's own wording (`message_ko`)

Assistants reply in their own words, so the same result reads differently in
ChatGPT, Claude or Grok. Results whose outcome fits one sentence carry
`message_ko`, the sentence AUDENIQ would say (`mcp::voice`: 해요체, active
voice for what was done, no hype); the server instructions ask Korean replies
to start from it. Amounts, declarations and review results are never
paraphrased there; they stay in the structured fields.

### Connection page (Studio `/ai/connect`)

Approving a connection happens on a Studio page, not a server-rendered form
(`mcp::connect`, `pages/AiConnect.tsx`):

- `/api/oauth/authorize` checks the client, redirect, PKCE and scopes, seals
  the request to the signed-in session (10 minutes) and sends the browser to
  `/ai/connect?request=…`; signed out, it sends it to the Studio login, which
  comes back to the same link. `/api/oauth/device` sends the browser to
  `/ai/connect?user_code=…` (or the code entry, `?device=1`).
- The page reads `GET /api/auth/ai-connect?request=` (app name, verified host
  of its metadata document or "registered agent", where it returns, device
  code, offered scopes with their defaults and risk, workspaces) and shows it
  on one page in the release application's frame (`#view-new .wizard`: top
  bar, step title, choice and option cards, bottom actions): the requesting
  app (and device code), the workspace to connect (picked only when there is
  more than one), what to allow (a line for the always-included read scope,
  option cards for the rest and quick picks: view only, drafts — suggested —
  or all), what no permission can do, the technical details (return host,
  metadata URL) folded away, and one "allow and connect" button. Submitting or payout permission, and
  denying, ask once more in the Studio's confirm dialog.
- `POST /api/auth/ai-connect` (CSRF) approves or denies with the chosen
  workspace and scopes (read plus offered ones only). A browser connection
  answers with the client's redirect URL and the page navigates there (the
  Studio's CSP keeps forms on its own origin); a device connection ends on
  the page. `POST /api/auth/ai-connect/device` turns a code into a request.
- A request opened by another session, tampered with or expired answers
  `AI_CONNECT_OTHER_SESSION` / `AI_CONNECT_EXPIRED`, and the page says to
  start again in the AI app.

The server form endpoints (`POST /api/oauth/authorize`, `/api/oauth/device`)
remain for clients that post them directly; their result pages and errors use
the Studio's design tokens and the AUDENIQ wordmark (`mcp/logo.svg`).

## Safety (migration 0135)

- **Activity notices.** A new AI connection (any method) or connection token,
  a submission and a payout request made through an assistant become a Studio
  notice to that user (`identity.mcp_notice`, kind ACCOUNT), mailed like any
  other notice, with a link to 프로필 → 연결된 AI 앱 (`/profile?open=ai`).
  Retries of the same submission or payout do not notify twice.
- **Retry-safe creates.** `create_release_draft`, `create_inquiry` and
  `create_platform_request` accept an optional `client_request_id`; the same
  id on the same connection returns the first result (`duplicate_request:
  true`) instead of a duplicate (`identity.mcp_idempotency`, kept 7 days).
  A `submit_release` retry with the same confirmation reports the application
  it created even after the release moved on in review.
- **Studio approval (opt-in per user).** With 연결된 AI 앱 → "AI 발매 신청은
  스튜디오에서 한 번 더 승인" on (`PUT /api/auth/ai-settings`), `submit_release`
  records the confirmed content (declarations, release content hash) in
  `identity.mcp_pending_submissions` and answers `view: pending_approval`;
  nothing is submitted. The artist approves or rejects in the Studio
  (`GET /api/auth/ai-submissions`, `POST …/{id}/approve|reject`, session +
  CSRF, only their own); approval re-checks that the release is unchanged
  (`AI_SUBMISSION_STALE` otherwise) and submits through the same path, in
  their name. Pending ones lapse after 7 days.
- **Concurrency.** Parallel `upsert_track`/`set_track_credits` calls on one
  release retry on the latest version (up to 8 times, jittered) instead of
  failing; once the track write is saved, a lost draft re-sync no longer fails
  the call (`draft_sync_pending: true`). A new track at a taken position
  answers `TRACK_POSITION_TAKEN` with that track's id. Lyrics are checked
  against protected names before anything is written.
- **Retry ids** are recorded the moment the create commits, and a reservation
  whose first call never finished is reclaimed after two minutes.
- **Approvals** are claimed atomically (approve and reject at once: one
  wins), released again when approving fails for a retryable reason, and the
  consent package must carry the confirmed content hash.
- **Client allowlist.** `AUDENIQ_MCP_CIMD_HOSTS` entries may be exact
  `https://` document URLs instead of hosts, pinning one client each.

## Security controls

- Token endpoint limits count **failures only** per client address
  (`mcp-token-fail:src`, 300 per 15 min): an AI platform refreshes for all of
  its users from a few addresses, so counting every request (or every request
  of a public client) would lock them all out, and one attacker could do it on
  purpose. Server-side failures there answer in OAuth's shape
  (`temporarily_unavailable` + `Retry-After`, `server_error`).
- A refresh token reused within 30 s is a client racing itself (parallel
  calls, a retry after a lost response): refused without revoking. Later
  reuse revokes the grant (`REFRESH_REUSED`).
- Password grant: per agent (3,000 per 15 min), per agent and account (10),
  and the Studio's own per-account budget (`login:acct`).
- `GET /api/oauth/authorize` and `/api/oauth/revoke` are limited per address.
  A Client ID Metadata Document that fails to fetch or validate is not fetched
  again for 5 minutes; a usable cached copy keeps serving.
- Tool calls stop after 60 s (`TOOL_TIMEOUT`, "check the current state before
  retrying"), inside the edge's deadline. `submit_release` is marked
  destructive so hosts ask before running it.
- Maintenance: `/api/mcp` answers a JSON-RPC error (-32000 with the notice,
  echoing the request's `id`), token/registration/revocation answer
  `temporarily_unavailable`, both with `Retry-After`. Discovery and the
  read-only notice tools stay open so the assistant can explain the outage:
  `initialize`, `ping`, `*/list`, `prompts/get`, `resources/read` and
  `tools/call` for `get_service_status`, `list_announcements` and
  `get_announcement`. Bearer lookups answer 503 only when the database is
  unreachable (500 otherwise, so clients do not retry a bug).
- `audeniq-admin privacy purge-transient` also deletes expired authorization
  codes, tokens and device codes (after a day) and old daily usage counts,
  and dynamically registered clients older than 30 days that never got a
  grant (abandoned registrations).
- Submitting again with the same `client_request_id` after the review has
  already moved the release on (Studio approval, network retry) returns the
  existing revision with `deduped: true` instead of `RELEASE_NOT_SUBMITTABLE`.

- Service-secret boundary unchanged: the edge adds `X-Audeniq-Service`. Only
  `POST /api/mcp|/api/oauth/{token,register,revoke}` skip the Origin rule, and
  only those calls get `Authorization`/MCP headers forwarded by the edge.
- Consent POST: Studio Origin, Studio session, and a signed consent request
  bound to that session's hash and the validated parameters (client, redirect,
  PKCE challenge, scopes); 10-minute expiry.
- Consent page: all values HTML-escaped, `Content-Security-Policy:
  default-src 'none'; form-action 'self' <redirect origin>; frame-ancestors
  'none'`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`.
- Inputs: JSON Schema per tool (formats, lengths, enums, patterns, unknown
  fields refused), then the services' own validation and text/protected-name
  policies. 64 KiB body and 16 KiB header limits, 120 s deadline (existing).
- Untrusted text: tool results are data; server instructions tell the
  assistant to treat titles/notes/credits/review notes as data, never
  instructions. The widget only inserts text nodes (`textContent`), runs no
  external scripts, declares an empty CSP domain list and only links to
  `https://` URLs.
- Rate limits (DB-backed, 15-minute windows): registration 20/IP, token 600/IP
  and 300/client, 900 tool calls/grant, 300 writes/grant, 30 submits/grant.
- Audit (`operations.audit_events`): `mcp.client_registered`,
  `mcp.consent_denied`, `mcp.grant_approved` (with scope set),
  `mcp.grant_revoked` (`CODE_REPLAYED`, `REFRESH_REUSED`, `CLIENT_REVOKED`,
  `USER_REVOKED`), and `mcp.<tool>` for every write/submit call (`MCP_OK` /
  `MCP_REJECTED`), plus the services' own rows (`release.updated`,
  `consent.created`, `submission.created`, …) under the same request id.
- Logging: one `mcp_tool` line per call (tool, grant id, ok, elapsed). No
  tokens, arguments or personal data are logged; tokens are never stored in
  clear.

## Configuration

| Variable | Default | |
|---|---|---|
| `AUDENIQ_MCP_ENABLED` | `false` | Everything above answers 404 when off |
| `AUDENIQ_MCP_CIMD_HOSTS` | `*` | Hosts (or exact document URLs, or `*` for any public host) whose client metadata documents may be fetched |
| `AUDENIQ_MCP_DYNAMIC_REGISTRATION` | `true` | RFC 7591 endpoint |
| `AUDENIQ_MCP_PUBLIC_DAILY_USER` | `150` | Public tools per signed-in user per day (per-user override: `mcp-limit`) |
| `AUDENIQ_MCP_PAYOUT_LIMIT_KRW` | `300000` | Assistant payout limit per request and per 24 h; `0` = off |

Issuer and resource derive from `APP_ORIGIN` (the Studio origin the edge
enforces): issuer `https://<studio host>`, MCP URL `https://<studio host>/api/mcp`.
HTTPS is terminated by Cloudflare as for the Studio; the API stays private.
Migration `0131_mcp_oauth.sql` adds `identity.mcp_clients`, `mcp_grants`,
`mcp_codes`, `mcp_tokens`; `deploy/grants.sql` grants the API role access.

## Deployment (CircleCI)

The deployed front door is the `audeniq-studio` Worker (`web/studio/worker.ts`
+ `web/shared/transport.ts`), not `crates/edge`. It forwards
`/.well-known/oauth-*` (GET) to the API, and forwards `Authorization`,
`MCP-Protocol-Version`, `Mcp-Method` and `Mcp-Name` only for `POST /api/mcp`
and `/api/oauth/{token,register,revoke}`; every other `/api` call stays
cookie-only. The API itself enforces Origin, so the Worker needs no Origin
exception. `crates/edge` got the same rules for parity.

`.circleci/config.yml`:

- `backend-test` (every branch where Rust/migrations/CI changed): rustfmt,
  clippy `-D warnings`, MCP unit tests, `tests/mcp.rs` and `password_reset`
  against PostgreSQL 18, edge routing tests. No native engines or ffmpeg
  needed. On `main` the image builds require it.
- `studio` runs the Worker tests (`web/studio/test/`, includes the MCP
  forwarding rules) and deploys the Worker on `main`.
- `backend-image` → `approve-deploy` → `deploy`: `deploy.sh` runs
  `audeniq-migrate` (applies 0131) and `grants.sql` before switching api/worker.
  `deploy` refuses unless the pipeline is for the newest `main` commit (an old
  pipeline's approval cannot roll production back). A pipeline with no backend
  change deploys the last `main` image (`:main` or `:main-$BACKEND_CPU_VARIANT`)
  instead of finishing without doing anything.
- `backend-test` caches dependency build output (`cargo-target-v1-…`, keyed on
  `Cargo.lock` and the toolchain) so a push that only changes our crates does
  not recompile dependencies.

Turning it on in production (after the deploy above):

1. On the server, in `production.env`: `AUDENIQ_MCP_ENABLED=true`
   (optionally `AUDENIQ_MCP_CIMD_HOSTS`, `AUDENIQ_MCP_DYNAMIC_REGISTRATION`).
2. Re-run the deploy (or `docker compose up -d api` in the deploy folder) so
   the API reads it.
3. Check: `curl https://studio.audeniq.com/.well-known/oauth-authorization-server`
   returns JSON with `"issuer": "https://studio.audeniq.com"`, and
   `curl -X POST https://studio.audeniq.com/api/mcp/public -H 'content-type: application/json'
   -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'` lists the 4 public tools, and the
   same request to `/api/mcp` answers 401 with a `WWW-Authenticate` challenge.
4. Add `https://studio.audeniq.com/api/mcp` as a connector in ChatGPT
   (developer mode) or Claude and connect with a test account.

To turn it off, set `AUDENIQ_MCP_ENABLED=false` and restart the API.

## Local development and tests

```sh
# PostgreSQL for tests (any isolated cluster whose login can CREATE DATABASE)
export DATABASE_URL=postgres://postgres@localhost:5432/postgres
export PAYOUT_ACCOUNT_KEY=0000000000000000000000000000000000000000000000000000000000000000
cargo test -p audeniq-core --lib mcp            # protocol/OAuth/schema unit tests
cargo test -p audeniq-core --test mcp           # end-to-end against PostgreSQL
cargo test -p audeniq-edge --lib                # edge routing rules
(cd web/studio && bun test test/)               # deployed Worker forwarding rules
```

`tests/mcp.rs` covers: discovery metadata; both protocol eras, header/body
mismatch, unsupported versions, unknown methods, GET/batch/foreign Origin,
service-secret boundary; disabled-by-default 404s; missing/invalid/expired
tokens (401 + challenge), refresh rotation, revocation by logout-everywhere,
client and user; PKCE failure, code replay, foreign client, unregistered
redirect, invalid resource/scope, consent bound to session, Origin and
workspace; CIMD allowlist; scope enforcement (403 `insufficient_scope`),
cross-tenant access, argument validation, audit rows, hashed-only token
storage; the full flow from empty draft through field-level findings, missing
audio, credits, preview, declaration/confirmation enforcement, change after
preview, confirmed submission, duplicate-free retry, and status; the real
platform-selection rule with the DSP gate on.

Simulated in these tests: the AI client, audio upload (assets registered
directly; QC stays PENDING), and platforms (submission starts review only;
nothing is delivered). No DSP delivery is exercised or claimed.

To try it with a real client locally, run the API and edge per
`docs/STUDIO_DEPLOYMENT.md` with `AUDENIQ_MCP_ENABLED=true`, sign in to Studio,
then point an MCP client (for example MCP Inspector) at
`https://<studio host>/api/mcp`.

## AI platform integration

The same endpoint serves every client. Platform requirements change; check the
current documentation before submitting anything for review.

**ChatGPT (Apps SDK).** Implemented: OAuth with CIMD (`chatgpt.com` allowlisted)
and DCR, `code_challenge_methods_supported: ["S256"]`, `iss` in authorization
responses, per-tool `securitySchemes`, `openai/outputTemplate` and
`_meta.ui.resourceUri` pointing at `ui://audeniq/release-card.html`
(`text/html;profile=mcp-app`). Steps: enable MCP in a staging deployment; in
ChatGPT developer mode add the connector with URL `https://<host>/api/mcp`;
link an AUDENIQ test account; walk through draft → validate → preview →
submit on a test workspace; then prepare the app submission (name, description,
privacy policy, test credentials, screenshots) per OpenAI's current app
submission guidelines. The widget's behaviour inside ChatGPT has not been
verified.

**Claude (custom connectors / directory).** Implemented: OAuth with DCR and
CIMD (`claude.ai` allowlisted), PKCE, legacy `initialize` support. Steps: add
`https://<host>/api/mcp` as a custom connector, connect, test the same flow;
for directory listing follow Anthropic's current connector review
requirements. Claude Code / desktop clients with loopback redirect URIs work
via DCR.

**Other MCP clients.** Any client implementing MCP authorization (PRM
discovery, PKCE, DCR or CIMD) can connect: Grok, Manus and other clients that
prefer CIMD use their own metadata URL, and older SDK clients may speak
`2024-11-05`. A missing token gets `401` with
`WWW-Authenticate: Bearer resource_metadata=…, scope="releases:read releases:write"`
(no `error`, so clients start authorization); an expired or revoked one adds
`error="invalid_token"`. Unknown `/.well-known/*` paths (e.g.
`openid-configuration`) are a JSON 404, never the Studio page. To limit CIMD
to known clients, list them in `AUDENIQ_MCP_CIMD_HOSTS`.

Implementing MCP does not publish an app, guarantee directory approval, or make
an assistant recommend AUDENIQ; listing and discovery are separate tasks.

## Known limitations

- Audio, cover art and rights proofs can come through the tools (see Files).
  Special-content options (cover licences, sample
  clearances, AI tool details, express review) are entered in the Studio.
- Signing the application/agreement, withdrawal, takedown, payout account
  changes, payouts above the limit, members, account settings: not exposed by
  design.
- Correction requests are tested at the MCP layer for the refusal paths
  (not yet delivered, no pending request, other tenants); the
  request→approve→platform update flow itself is covered by the existing
  `execution` tests, not re-run through MCP.
- No progress notifications or SSE (all tools finish within the deadline).
- Minors: submission is refused (`MINORITY_REVIEW_REQUIRED`), as in the Studio.
- Not run here: real ChatGPT/Claude clients, the deployed Worker, and the
  native QC/photo binaries (unit/integration tests that need them fail in an
  environment without them, unchanged from before this change).

## Operations

- Enable per environment with `AUDENIQ_MCP_ENABLED`; disable to cut all AI
  access instantly (tokens remain stored but unusable while disabled).
- Disable one client: `audeniq-admin --operator NAME mcp-client disable CLIENT_ID`
  for operator-registered agents, otherwise `UPDATE identity.mcp_clients SET disabled_at=now() WHERE client_id=…`.
- Pending device codes expire after 10 minutes and are deleted a day later.
- Watch: `mcp_tool` log lines (error rate by tool), `RATE_LIMITED` responses,
  `mcp.grant_revoked` audit rows with `REFRESH_REUSED`/`CODE_REPLAYED`
  (possible token theft), and `submission.created` rows following
  `mcp.submit_release`.
- Spent and expired tokens of a grant are deleted when it issues new tokens;
  revoked grants and used authorization codes are kept for audit (no
  retention job yet).
