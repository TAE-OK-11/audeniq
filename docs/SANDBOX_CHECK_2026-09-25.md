# Sandbox inspection — 2026-09-25

Full distribution pipeline exercised against a real PostgreSQL sandbox
(`audeniq_sandbox`: migrations through 0022 + `deploy/grants.sql` applied),
with background worker loops running the same `operations::claim` /
`operations::execute` code as the `audeniq-worker` binary, and the MockDSP
wire path.

Driver: `sandbox_full_distribution_run` in `crates/core/tests/execution.rs`
(`#[ignore]`; run with `SANDBOX_DATABASE_URL=... cargo test -p audeniq-core
--test execution sandbox_full_distribution_run -- --ignored --nocapture`).
It seeds its own org/user/release with run-unique audio bytes, UPC
(valid UPC-A check digit) and ISRC, so reruns in one shared sandbox DB do
not trip the cross-org duplicate check.

## What passed

- Binaries: `audeniq-worker` boots and polls all five queues cleanly;
  `audeniq-api` serves (unauthenticated root correctly 403).
- `audeniq-migrate` + `deploy/grants.sql` apply cleanly on a fresh DB.
- Automatic pipeline, end to end, ~2s in sandbox:
  submit -> stage1 (QC) -> stage2 (rights) -> prepare_release ->
  READY_FOR_DELIVERY -> delivery.enqueue -> delivery.send ->
  MockDSP wire + ACK -> DELIVERED.
- `distribution.ddex_messages`: exactly 1 row per partner; ERN XML sha256
  verified against `delivery_attempts.request_sha256`; DPIDs
  `sender=TESTDPID-SANDBOX-0001`, `recipient=TESTDPID-MOCKDSP-0001`.
- `execution.delivery_attempts`: 1 wire attempt, outcome ACCEPTED,
  `partner_message_id` recorded.
- FORCE RLS holds for the worker-shaped reads used by the driver.
- Duplicate detection works in practice: a rerun with the fixture's fixed
  UPC/ISRC in a second org was routed to `STAGE2_REVIEW` with
  `REVIEW_REQUIRED:S2_CATALOG_IDENTIFIERS` (not an infringement finding,
  just a duplicate-claim candidate) — the expected gate fired before any
  package was built.

## Gaps found

1. **`delivery.poll` is never scheduled.** The E-4 live-poll handler exists
   in `operations.rs`, but no code path enqueues a `delivery.poll` job —
   not after DELIVERED, not from the API, not from reconcile. After a
   successful wire + ACK, `execution.live_bindings` sits at `INGESTING`
   forever; nothing drives it to `LIVE` automatically. Same for
   `delivery.reconcile` (E-5): handler exists, never scheduled.
   Decision needed: enqueue `delivery.poll` on DELIVERED (immediate poll
   vs delayed poll — the queue has no delay mechanism today), or keep
   live-polling a manual/operator action.
2. **`outbox.record` piles up on the `interactive` queue** when only the
   distribution queues are polled. Internal-only (event receipts for
   `foundation.internal`, marks outbox rows published) — no external side
   effects. In production the worker's interactive poller (default
   concurrency 1) drains these.
3. This dev VM's `audeniq_api` / `audeniq_worker` roles are NOLOGIN, so the
   binaries cannot log in as the runtime roles here. Worker-shaped RLS
   behavior is covered by `dsp_worker_role_rls_delivery` (SET ROLE), which
   is the real mechanism; the binary smoke ran as the owner role.

## Not covered by this inspection

- Real partner credentials, contracts, sandbox endpoints (F6 blockers).
- Official ERN 3.8.2 XSD validation / partner-profile conformance.
- R2 storage (sandbox used the in-memory FileStore; binaries only speak
  S3 or Disabled).
- HTTP API user journey (signup -> upload -> release); seeding used the
  same internal paths the API handlers call.

## Adversarial scenarios (red-team, 2026-09-25)

Driver: `sandbox_adversarial_submissions` in `crates/core/tests/execution.rs`
(`#[ignore]`). Each scenario seeds a fresh org/user/release and drives the
real pipeline; verdicts observed in the sandbox DB, not assumed.

| # | Scenario | Result | Verdict |
|---|----------|--------|---------|
| S1 | Plagiarism: byte-identical re-upload of another org's released audio, new ISRC/UPC | `STAGE2_REVIEW` (`REVIEW_REQUIRED:S2_CATALOG_IDENTIFIERS`) | **Caught** — exact SHA-256 match across orgs |
| S2 | Plagiarism: same music re-encoded as MP3 (different bytes) | `READY_FOR_DELIVERY` | **Gap** — `S2_CATALOG_FINGERPRINT` is `NOT_APPLICABLE`; audio fingerprinting engine does not exist ("comparison engine is F4+", review.rs) |
| S3 | Minor declares `minority_declared=true` at consent | HTTP 422 `MINORITY_REVIEW_REQUIRED` | **Blocked** — hard gate at consent/submit |
| S4 | Minor lies (`minority_declared=false`) | `READY_FOR_DELIVERY` | **Gap** — no DOB collected at signup (`/api/auth/register` takes email+password only); no age verification anywhere |
| S5 | Cover song ("Blinding Lights (Cover)") | `READY_FOR_DELIVERY` | **Gap** — no cover-declaration field in any draft/release input (`deny_unknown_fields` everywhere); stage2 `special_flags` (`COVER`/`REMIX`/`SAMPLE`/`AI`) is hardcoded to `[]` in the validation-package builder (`submission.rs`), so the detection logic in `review.rs` is unreachable dead code |
| S6 | Explicit lyrics | HTTP 422 (field rejected) | **Gap** — no lyrics channel exists in the API at all; nothing to screen. QC is audio-technical only (SHA-256, container magic, duration, image dimensions) |

AI-generated music: same as S5 — no disclosure field, no detection, no
provenance check; the `AI` special-flag path is equally unreachable.

Notes:
- S2/S4/S5 passing means: the pipeline cannot distinguish these from
  legitimate submissions today. All three need product decisions (human
  review queues, declarations with legal weight, third-party fingerprinting
  / age-verification vendors) before customer operation.
- S1's catch is exact-match only: trim, re-encode, or pitch-shift the audio
  and it becomes S2.
