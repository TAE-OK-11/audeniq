# CloudFront migration follow-up — local review record

Scope: checkout `codex-audit-fixes-20261028`, baseline `b12aa5f`.
No commit, push, merge, deployment, AWS command, production request or live-server inspection performed.

## Results and limitations

- 1-1: Successful health/readiness payloads explicitly use a shared no-store response builder. A unit test invokes the health handler and readiness success renderer without a database; the existing router test already checks health's no-store header. A successful database readiness request was not exercised against a running DB.
- 1-2: No MP4/MOV/WebM files were found in this checkout's Studio build inputs or checked-in public artifacts. Static Range/206 remains unsupported and documented. This does not establish the contents of an uninspected live release. User video uploads use a separate path and were untouched.
- 1-3: Recommend retaining current private image serving. A public immutable revision URL alone does not preserve immediate visibility revocation. Future offload would need an authorization/revocation design, a proven invalidation bound, negotiated-format cache keys, hash validation and pending-derivative behavior preserved. Do not turn existing API images into public edge-cacheable objects. No image implementation changed.
- 2-1/2-2: Gateway 502/504 replacements use MAINTENANCE JSON for `/api`, `/api/*`, `/ready`, `/health`; HTML uses an inline v9-style agent draft. Replacements are no-store. Application 500, authorization 403 and not-found 404 remain unchanged deliberately. CloudFront Functions cannot read the origin response body, so **all 503 responses pass through**, preserving backend maintenance metadata. Consequently generic origin 503 responses are also not redesigned here. This is a conservative limitation of the requested preservation requirement.
- 2-3: Studio template embeds checked-in v9 viewer-request and maintenance viewer-response code instead of the legacy allowlist/CORS response function. Existing uncached behavior associations refer to those separate functions. **Cached behavior associations remain unchanged pending Muse's live behavior inventory**, as instructed for unconfirmed connections. Existing paths, TTLs, origins and forwarding choices remain unchanged and are explicitly not certified as live-equivalent. No independent error origin or CustomErrorResponses was invented. This template is a review candidate, not a live-reconciliation completion or deployment instruction.
- 3-1: Term/splits/smart-link/platform-request reads show retryable failures. Delivery-detail reads also stop silently returning an empty list. The cited ReleaseDetail line 267 is actually a withdrawal-quota preflight: its failure now explains the failure and asks the user to retry the existing cancel button, without proceeding with unknown quota. Mutation requests themselves were not changed.
- 3-2: Downloads have a 120-second total network/body deadline and explicit Korean timeout error; no streaming change.
- 3-3: Admin recovery access is retained, consistent with `site_content::gate`'s admin bypass, with an explanatory comment. Removed the time-based server-issue suppression; each failure invalidates older recovery probes, while an already-visible dialog is deduplicated. Version/device states are already independent and serial display priority prevents stacked modals. Static non-GET/HEAD JSON 405 is retained and documented.

## Undecided — no implementation

- CORS: existing documentation describes closed/same-origin transport. Whether cross-origin access should exist remains undecided. No CORS policy was added or widened.
- Final visitor response on origin errors: checked-in transition documentation already warns that viewer-response maintenance is not origin-error failover. Actual live execution was not tested. Independent error origin and CustomErrorResponses, their associations and infrastructure rollout remain user/Muse decisions.
- Exact cached-behavior Function associations and complete live/IaC parity require Muse's inventory.

## Differences from the audit assumptions

- The common API middleware already sets no-store, and an existing router test already asserts it on `/health`; explicit handler headers add a local contract rather than establish that live probes were previously cacheable.
- Checked-in v9 says NOT DEPLOYED; error-page README also labels the assets prepared/not deployed. These comments cannot establish the current live version.
- Version/device modal state is already separate; withdrawal quota (the cited ReleaseDetail line) is a preflight read rather than a hidden section. A separate delivery read did silently suppress errors and was fixed.
- Reliable selective preservation of backend 503 based on its JSON body is unavailable in a CloudFront Function; preserving every 503 is intentional.

## Files changed

| File | Reason |
| --- | --- |
| `crates/core/src/api.rs` | Explicit no-store probe success responses and unit coverage. |
| `crates/core/src/studio_static.rs` | Explain unsupported Range and retained JSON 405. |
| `deploy/aws/template.ts` | Embed checked-in Studio functions and mark unconfirmed topology. |
| `deploy/aws/test/aws.test.ts` | Verify source identity and confirmed/pending associations. |
| `deploy/cloudfront/studio-maintenance-function.js` | Align gateway JSON, preserve 503, no-store, v9-style draft. |
| `deploy/cloudfront/verify-maintenance.cjs` | Local 35-case response matrix. |
| `web/studio/app/src/components/ReadFailure.tsx` | Shared accessible read-error/retry surface. |
| `web/studio/app/src/components/TermCard.tsx` | Surface term read failure with retry. |
| `web/studio/app/src/components/SplitsCard.tsx` | Surface splits read failure with retry. |
| `web/studio/app/src/components/SmartLinkCard.tsx` | Surface smart-link read failure with retry. |
| `web/studio/app/src/pages/Inquiries.tsx` | Preserve failed platform-request state and expose retry. |
| `web/studio/app/src/pages/ReleaseDetail.tsx` | Surface delivery/quota reads that formerly swallowed errors. |
| `web/studio/app/src/api/http.ts` | Bound download headers/body wait and clean up timeout. |
| `web/studio/app/tests/download-timeout.cjs` | Exercise stalled headers/body without external dependencies. |
| `web/studio/app/src/components/SystemStatus.tsx` | Remove server cooldown suppression and document admin/modal intent. |
| `deploy/cloudfront/audit-fixes-20261028.md` | Record decisions, omissions and local validation. |

Section 5 runtime behavior is unchanged. `studio_static.rs` received comments only; routing, caching, ETag/304, Brotli choice and SPA fallback were not modified. Authentication boundaries, image authorization, direct uploads and idempotency were untouched.

## Validation

- `node --experimental-strip-types deploy/aws/test/aws.test.ts`: 9 passed.
- `node deploy/cloudfront/verify-maintenance.cjs`: 35 matrix cases passed, including 503 window preservation and replacement-header checks.
- `node web/studio/app/tests/download-timeout.cjs`: stalled response headers and stalled body deadline cases passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `npm --prefix web/studio/app run check`: unavailable (`tsc: not found`); dependencies are absent and were not fetched. React rendering/type checks remain unverified.
- `cargo test --offline -p audeniq-core successful_probes_are_not_cacheable`: initial default build failed when the normal library compiler was terminated by SIGKILL; no source diagnostic was reported.
- `cargo test --offline -p audeniq-core --lib -j 1 --config profile.dev.package.audeniq-core.debug=0 successful_probes_are_not_cacheable`: passed, 1 test, 460 filtered out. Only command-local build settings changed; repository profiles were not modified. Tests invoke the actual health handler and readiness success renderer; a running database was not required.
