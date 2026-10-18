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
  POST /api/mcp                 forwards Authorization + MCP headers           crate::mcp::protocol  (JSON-RPC)
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
| `POST /api/oauth/token` | `authorization_code` (PKCE) and `refresh_token` |
| `POST /api/oauth/revoke` | RFC 7009 revocation (ends the whole grant) |
| `GET /api/auth/ai-connections`, `POST /api/auth/ai-connections/{id}/revoke` | The user's connected AI apps (Studio session) |

Flow: the client gets a 401 with `WWW-Authenticate: Bearer resource_metadata=…`,
discovers the authorization server, registers (CIMD or DCR), opens
`/api/oauth/authorize` in the user's browser. The user must already be signed in
to AUDENIQ Studio in that browser (the page links to `/login` otherwise; the AI
client never sees a password). The consent page shows the client name, the
host it will return to (and the CIMD URL), a workspace selector and the
permissions. `releases:submit` is never pre-selected. Approval redirects with
`code`, `state` and `iss`.

- **Clients.** Client ID Metadata Documents are fetched only from hosts in
  `AUDENIQ_MCP_CIMD_HOSTS` (default `chatgpt.com,claude.ai`; HTTPS, no port,
  5 s timeout, 64 KiB, no redirects, re-fetched after 24 h, cached copy used up
  to 7 days when the host is down). The document's `client_id` must equal its
  URL and it must allow `none` authentication. DCR clients get a random
  `client_id`; confidential ones a secret stored as SHA-256. Redirect URIs:
  HTTPS without fragment, or loopback `http` (any port, RFC 8252).
- **Codes/tokens.** Codes: 10 min, once, PKCE S256 only, bound to client and
  redirect URI; a replayed code revokes the grant. Access tokens: 1 h. Refresh
  tokens: 30 days, rotated on every use; reuse revokes the grant. A grant ends
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

### Scopes

| Scope | Grants |
|---|---|
| `releases:read` | Always granted: read releases, requirements, checks, status |
| `releases:write` | Create/edit drafts, tracks and credits (only in editable states) |
| `releases:submit` | Opt-in: submit a previewed, artist-confirmed application |

Insufficient scope → HTTP 403 with `WWW-Authenticate: Bearer error="insufficient_scope", scope="…"`.
No scope reaches payouts, payout accounts, contracts/signatures, withdrawals,
takedowns, members or account settings.

## Tools

All tools declare `inputSchema` with `additionalProperties: false`, MCP
annotations (`readOnlyHint`, `destructiveHint: false`, `idempotentHint`,
`openWorldHint: false`), `securitySchemes` (OAuth2 + scope) and, where useful,
the release-card UI resource. Results carry `structuredContent` (with a `view`
field) plus a text rendering. Failures are tool results with `isError: true`
and `{code, message}` (stable API codes, human messages from
`error::message`); internal details are never returned.

| Tool | Scope | Backed by | What it does |
|---|---|---|---|
| `get_account_context` | read | `catalog::workspace` | Workspace, role, connection scopes, `can_submit` |
| `list_distribution_platforms` | read | `api::dsp_availability`, `dsp_registry` | Platforms, availability now, per-platform requirements (artwork px, sample rate/bit depth, lossless, lead days, required credits, loudness) |
| `get_release_requirements` | read | constants from `submission`, `uploads` | Required fields, release types/track counts, file types/limits, credit roles, declarations |
| `list_releases` | read | `catalog::list` | Releases with status, paged |
| `get_release` | read | `catalog::get`, `catalog::party_names` | Release, tracks, credits (names), audio attached, agreement/delivery/live state |
| `create_release_draft` | write | `catalog::create` (artist via find-or-create) | New draft (title, type, artist + optional fields) |
| `update_release_draft` | write | `catalog::update` (optimistic lock) | Change given fields only; refused once under review |
| `upsert_track` | write | `catalog::track`, `drafts::replace_track` | Add/edit a track; omitted fields keep their values; `asset_id` only for an already uploaded file |
| `set_track_credits` | write | `catalog::ensure_party`, `drafts::replace_credits` | Composers, lyricists, arrangers, producers (Studio roles) |
| `validate_release` | read | `drafts::preflight`, `submission::presubmit`, `submission::draft_review`, `uploads::checks` | Blocking findings (field-level, with track ids), items for human review, audio QC, missing files + Studio upload link |
| `preview_submission` | read | same + declarations | Full application preview; when ready and the grant has `releases:submit`, a `confirmation_token` (15 min) |
| `submit_release` | submit | `submission::create_consent`, `submission::submit` | Submits exactly the previewed content |
| `get_submission_status` | read | `submission::submission_status`, `catalog::get` | Pipeline status explained, checks, corrections, staff notes, agreement and follow-up |

Draft fields map onto the Studio's draft keys (`artist`, `language`, `genre`,
`label`, `release_date`, `release_time`, `preorder_start`, `territories`,
`platforms`, `p_line`/`phonogram`, `c_line`/`copyright`, `notes_lines`, `type`),
and tracks/credits are mirrored into `draftTracks` (credit order preserved), so
a release prepared through MCP opens and re-saves correctly in the Studio.

### Submission confirmation

1. `preview_submission(release_id, declarations)` runs every check, including
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
Studio. A stronger option, if required, is a Studio-side "approve pending
submission" step (not implemented; needs a frontend change).

After submission the artist still signs the release application and the
distribution agreement in AUDENIQ Studio; delivery waits for the signed,
approved agreement (`execution::enqueue_delivery_jobs`). The MCP server cannot
sign. `get_submission_status` says so in `follow_up`.

## Security controls

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
| `AUDENIQ_MCP_CIMD_HOSTS` | `chatgpt.com,claude.ai` | Hosts whose client metadata documents may be fetched |
| `AUDENIQ_MCP_DYNAMIC_REGISTRATION` | `true` | RFC 7591 endpoint |

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

Turning it on in production (after the deploy above):

1. On the server, in `production.env`: `AUDENIQ_MCP_ENABLED=true`
   (optionally `AUDENIQ_MCP_CIMD_HOSTS`, `AUDENIQ_MCP_DYNAMIC_REGISTRATION`).
2. Re-run the deploy (or `docker compose up -d api` in the deploy folder) so
   the API reads it.
3. Check: `curl https://studio.audeniq.com/.well-known/oauth-authorization-server`
   returns JSON with `"issuer": "https://studio.audeniq.com"`, and
   `curl -X POST https://studio.audeniq.com/api/mcp -H 'content-type: application/json'
   -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'` lists 13 tools.
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
discovery, PKCE, DCR or CIMD from an allowlisted host) can connect. To accept
another CIMD host, add it to `AUDENIQ_MCP_CIMD_HOSTS`.

Implementing MCP does not publish an app, guarantee directory approval, or make
an assistant recommend AUDENIQ; listing and discovery are separate tasks.

## Known limitations

- Files (audio, artwork, rights proofs) are uploaded in AUDENIQ Studio; tools
  return the Studio link. Special-content options (cover licences, sample
  clearances, AI tool details, express review) are entered in the Studio.
- Signing the application/agreement, withdrawal, takedown, payouts, members,
  account settings: not exposed by design.
- Studio UI for listing/revoking AI connections is not built; the API exists.
- No progress notifications or SSE (all tools finish within the deadline).
- Minors: submission is refused (`MINORITY_REVIEW_REQUIRED`), as in the Studio.
- Not run here: real ChatGPT/Claude clients, the deployed Worker, and the
  native QC/photo binaries (unit/integration tests that need them fail in an
  environment without them, unchanged from before this change).

## Operations

- Enable per environment with `AUDENIQ_MCP_ENABLED`; disable to cut all AI
  access instantly (tokens remain stored but unusable while disabled).
- Disable one client: `UPDATE identity.mcp_clients SET disabled_at=now() WHERE client_id=…`.
- Watch: `mcp_tool` log lines (error rate by tool), `RATE_LIMITED` responses,
  `mcp.grant_revoked` audit rows with `REFRESH_REUSED`/`CODE_REPLAYED`
  (possible token theft), and `submission.created` rows following
  `mcp.submit_release`.
- Spent and expired tokens of a grant are deleted when it issues new tokens;
  revoked grants and used authorization codes are kept for audit (no
  retention job yet).
