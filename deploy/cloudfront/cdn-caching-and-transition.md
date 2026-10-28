# Studio CDN caching and transition

Review baseline: main bce4ca3, 2026-10-28. This is a repository design and an
operator proposal, not evidence that any live setting has changed. The three
2026-10-28 migration audits supplied with this task describe E29B1S5744RYI0
(studio.audeniq.com) with Default CachingOptimized and /api/* CachingDisabled.
No AWS state was queried during this review.

## Intended request path

Browser → studio.audeniq.com → CloudFront viewer-request sanitization →
api-origin.audeniq.com:443 → Rust TLS listener → static snapshot or API boundary →
PgBouncer/Postgres. TLS restricts CloudFront source ranges, SNI/Host and service
secret; the loopback HTTP listener exists for container readiness checks only.
Origin custom headers must set x-audeniq-service (secret supplied outside Git)
and x-audeniq-site=studio. Viewer-supplied x-audeniq-* must be removed before
CloudFront supplies those headers. Client IP comes from event.viewer.ip.

The API retains service authentication, same-origin write checks, closed CORS
preflight, existing error codes and response shapes. /health is process liveness;
/ready is the existing bounded, authenticated database-readiness check. Neither
is a publicly cached status page. /dashboard/ path semantics are unchanged.

## Cache classes

| Class | Origin Cache-Control | Reason |
| --- | --- | --- |
| /assets/name-HASH.ext, eight URL-safe hash characters | public, max-age=31536000, immutable | Vite content fingerprints, including imported images/fonts; changed bytes require a new URL |
| HTML, including every SPA fallback | no-cache | Revalidate the entry point and discover new bundle names |
| /static/* logos, bank/brand images, unsupported.js | public, max-age=86400, stale-while-revalidate=604800 | Preserve the earlier review's intentional static stale-serving window; these names are mutable |
| Other public images, including favicon.ico | public, max-age=3600, must-revalidate | Useful CDN caching without falsely claiming immutable names |
| Other mutable files, robots.txt, assets/compat-entries.json | public, max-age=0, must-revalidate | Version-sensitive metadata and unversioned files must revalidate |
| Existing /.well-known/* files | no-store | Discovery must not inherit broad asset cache rules |
| /api, /api/*, /health, /ready, API discovery and errors | no-store (existing private immutable API images remain private) | Never share sessions, authenticated covers, readiness or failures |
| Redirects and missing static files | no-store | Avoid sticky routing and deployment-time negative caches |

`studio_static.rs` enforces this table after parsing _headers, so an old build's
blanket /assets/* immutable rule cannot freeze a mutable manifest. ETag/304 and
Brotli representation selection retain the same policy. Frontend references use
/static/* for public logos/brand images and /api/* for authorized covers; signed
object-store upload URLs are deliberate data-plane transfers, not UI/CDN bypasses.
Vite's normal output naming supplies eight-character hashes. Any future custom
naming scheme needs a matching origin classification change.

## Required distribution behaviors (proposed live changes)

The Studio branch of deploy/aws/template.ts models this configuration:

* Default: managed CachingDisabled, all viewer headers except Host plus cookies
  and query strings forwarded; preserve the audited viewer-request Function.
  This includes /, /index.html, /studio, SPA paths, /api and /api/*, /health,
  /ready and /.well-known/*. The existing /api/* disabled behavior may remain
  explicit. Do not attach a CORS policy that opens preflight.
* /assets/*, /static/* and /favicon.ico: custom AssetCache policy with minimum
  TTL 0, default TTL 0, maximum TTL 31536000; normalize/cache Gzip and Brotli;
  exclude cookies, query strings and viewer headers. GET/HEAD only. Origin
  custom service/site headers still apply. The backend selects shorter TTLs
  for mutable assets. These public static requests do not forward forged
  viewer service/site headers or authorization cookies.
* Set error caching minimum TTL to 0 for supported 4xx/5xx entries, including
  403/404/500/502/503/504. Never map API errors to HTML or map errors to HTTP 200.
  Do not apply a broad origin-response override that erases origin cache headers.

[CloudFront documents that a positive minimum TTL overrides no-cache, no-store
and private](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/cache-key-understand-cache-policy.html).
The origin headers alone therefore cannot repair the audited live Default policy.
The template is a reference for a reviewed distribution diff, not an instruction
to create a second distribution or overwrite unrelated existing WAF settings.

## Release freshness

Production builds reject VITE_API_BASE overrides: API URLs must remain relative
and traverse the Studio CloudFront hostname. The dev server still accepts a
local backend target; the former EDGE_CONTENT_URL Worker split is removed.

studio-release.sh validates entry-point asset references, installs a complete
release, then atomically switches current; concurrent installs/rollbacks are
serialized. The Rust snapshot holds HTML and assets together and retains its
previous valid snapshot if a replacement fails to load. The existing build
retainAssets plugin retains three previous JS/CSS import graphs when the prior
build directory is available. Fresh CI checkouts do not guarantee those old
assets: an already-open tab requesting an evicted lazy chunk can still require
reload. This review does not claim infinite old-release support.

The existing five-minute version check fetches /?__v=<time> with no-store and
compares bundle names. With Default CachingDisabled this always reaches the
origin, even when the query is not part of a cache key. New navigation and SPA
entry points also reach the current snapshot. After approving the behavior
change, invalidate /* once on E29B1S5744RYI0 to remove pre-transition HTML, errors
and incorrectly immutable mutable files. Ordinary subsequent deploys change
fingerprinted URLs and require no immutable-asset invalidation. A live acceptance
check must compare / and /?__v=... bundle references with the installed release
and confirm no Age/cache hit for entry points, readiness, API or discovery.

## Security review findings

* SQL slow-statement and lock-wait logging can disclose SQL literals/context;
  Compose disables SQL-text logging paths while retaining bounded container logs.
* grants (one-shot psql) runs as the postgres OS user with read-only filesystem, dropped capabilities,
  no-new-privileges, PID/memory/CPU limits; PgBouncer gains a PID cap and hardened
  tmpfs. Existing API/worker read-only/nonprivileged limits remain in place.
  PostgreSQL keeps its writable data path and entrypoint privilege requirements.
* Runtime PgBouncer credentials no longer have stats-console access (SHOW can
  expose other connections/queries). Administrative troubleshooting must use a
  separately authorized path. SCRAM and authenticated direct/pooled readiness
  remain intact. Config and password line-break injection fail before rendering;
  secrets are not printed and generated files use umask 077 in tmpfs.
* API errors now carry no-store even when rejected before boundary response
  stamping. Caught panic payloads are redacted; the production binary replaces
  the default payload/backtrace-printing panic hook with a generic log event.
* deploy.sh serializes concurrent deployments, requires owner-only 0600 env
  files (including the existing tmpfs-secret checks), isolates image binary
  inspection from the network, and checks runtime restart/running state before
  accepting API health. Secret values remain external; no secrets were added.
* bootstrap already creates separate NOSUPERUSER/NOCREATEDB/NOCREATEROLE/
  NOREPLICATION/NOBYPASSRLS runtime roles. Existing grants are granular and
  preserve append-only protections. The optional staff role now receives explicit
  CONNECT after PUBLIC's database access is revoked; no broader object grants.

## Remaining live transition proposals — none executed

1. Inspect E29B1S5744RYI0's actual behaviors/policies, then apply the exact cache
   changes above and perform the one-time invalidation after origin rollout.
   Retain WAF association, TLS settings and Function sanitization on uncached
   behaviors. Check /api exact as well as /api/* and every SPA entry path.
2. Retrieve and archive the actual LIVE v5 and prepared v7 Function code before
   replacing anything. This worktree contains only studio-edge-function.js and
   studio-maintenance-function.js: no v7 file and no error-page directory at
   baseline. The checked-in scanner function blocks case-sensitive raw prefixes;
   it is noise filtering, not an authentication boundary. Do not broaden bot-UA
   blocking without v5/v7 comparison and false-positive tests (including genuine
   well-known clients). Verify header stripping on an uncached origin request.
3. Prepare independent private S3 error-page storage with CloudFront OAC and
   bucket policy restricted to this distribution. `errors/503.html` now supplies
   the existing maintenance Function HTML as a standalone page, preserving UI
   copy. Reuse it for 500/502/503/504 while retaining each error status; prepare
   a generic 403 body separately. Set response headers (no-store, nosniff, frame
   denial, no-referrer) before uploading;
   do not claim the old viewer-response maintenance Function handles origin
   errors: CloudFront skips it for origin 400+ responses. Preserve API JSON;
   distribution-global HTML error mappings require a separate design for API
   behavior. WAF custom block bodies require separate WAF configuration.
4. Verify no requests/routes/domains remain on audeniq-studio and
   audeniq-studio-proxy. Disable workers.dev and preview URLs in their live
   settings, detach any Studio custom domain/routes, revoke their service-secret
   bindings, then delete retired Workers after the rollback window. Repository
   wrangler.jsonc disables future preview/worker.dev publication; it does not
   delete an already-live Worker. web/studio/worker.ts, web/shared/transport.ts
   and their tests remain historical/regression code. Admin/landing/survey
   Workers are separate scopes and must not be deleted just for this change.
5. Inventory server cloudflared containers/systemd services. Stop/disable only
   the retired Studio tunnel daemons; remove tunnel-token files, old
   CLOUDFLARED_IMAGE/CLOUDFLARED_TOKEN environment entries, the retired tunnel
   credential and api-tunnel DNS record after verifying no other application
   uses them. No server command has been run here.
6. Move the authoritative zone from Cloudflare to Route 53 only after copying
   the full zone, ACM validation, mail, DKIM/DMARC, CAA and any DNSSEC records.
   Set Studio A/AAAA alias to this distribution, preserve api-origin's real
   server record as DNS-only, update registrar NS (and DS if applicable), verify
   resolution and certificate renewal before removing the old zone. Actual zone
   values/registrar state must be obtained live; no values are invented here.
7. Restrict server ingress to CloudFront origin-facing ranges (plus necessary
   administration/ACME paths), retain application source/Host/secret checks,
   verify origin TLS expiry/renewal and create external /health and /ready
   monitors through CloudFront. Align measured origin timeout with the existing
   backend timeout contract; do not shorten long user operations blindly.
   WAF Count→Block, rate rules, IAM/OAC and logging changes need live inspection
   and false-positive review; none are silently enabled by this repository work.

Other remaining references: deploy/aws/README.md and the older deployment guides
contain migration-era Worker/Tunnel procedures; the primary Studio guides now
point here. README.md, docs/BLUEPRINT.md, deploy/ORACLE_CLOUD.md and historical
review/checklist documents retain earlier topology discussions. api-origin
references in TLS config/tests and deploy/aws are intentional CloudFront origin
names, not browser API URLs. R2/OCI/S3 CSP and signed uploads remain supported
storage provider paths. Survey D1 and admin/landing legacy resources must be
inventoried separately before any shared credential, DNS zone or D1 deletion.

## Local verification for this change

* `cargo check -p audeniq-core`: passed.
* Focused Rust tests: studio_static::tests 11 passed, error::unavailable_tests
  4 passed, api::tests 4 passed initially. The fifth API test (real localhost
  compression/decoding) was denied a socket by the sandbox, then passed when
  rerun with the socket restriction lifted: 20 distinct focused tests passed.
  The full Rust suite was not run.
* `bun run check`: passed. Full frontend suite: 68 files passed, 288 tests
  passed, 1 skipped. Production Vite build to /tmp passed; a build with an
  external VITE_API_BASE was correctly rejected. Existing Vite import/chunk
  warnings are non-fatal.
* AWS template/Function configuration harness: 8 passed. Legacy Studio static
  regressions: 3 passed. DB probe/gate tests: 9 passed. New security hardening
  regressions: 4 passed (including six injection subcases).
* `bash -n`: all three changed shell scripts passed. Compose YAML parsed and
  inherited runtime restrictions were checked. `git diff --check`: passed.
* Standalone error HTML matches the existing maintenance Function's UI and has
  no external resource dependencies. No Function JS source was changed; no
  claim is made of a v5/v7 comparison with the missing source versions.
* Docker is unavailable here; real container/SQL integration and live AWS,
  CloudFront, WAF, DNS, TLS and release acceptance checks remain unverified.
