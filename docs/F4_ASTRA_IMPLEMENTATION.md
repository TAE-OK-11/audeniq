# F4 Astra preparation components

Base: `foundation/f3-stage2-review` at `eb3134ef8ccccfdaa8cfae4da7c0001b0abb4e7b`.

## Integration contract for Muse

Muse's Stage 3 branch is merged and wired into the worker. The public API is
`ern::generate_prepared_ern(&preparation_model::PreparedRelease) -> Result<String>`.
Since migration 0012 (`CanonicalRelease.schema_version = 2`), the canonical
snapshot carries UPC, the artwork asset reference and each track's audio
object key. `PreparedRelease::from_canonical(pool, snapshot_id, canonical)`
reads those supplements plus catalog metadata (release date, artist, language,
P/C-lines, ISRCs, asset hashes/sizes/content types) from the pinned snapshot
and the database. Nothing is invented: a missing UPC, artwork, ISRC, file
bytes or stale pin fails closed in `run_prepare_release` before the release
can reach READY_FOR_DELIVERY. Canonical-only XML is not a ready submission.

- `ern::generate_ern` and `generate_prepared_ern`: deterministic, pure, sorted output;
  reject invalid identifiers, XML controls, duplicates and missing metadata. No DB
  or external service. Both profiles use only the internal synthetic namespace.
- `route_plan::plan_submissions(canonical, verification)`: checks the actual F3
  JSON digest, row org/revision/epoch, revision hash and exact approved DSP set,
  then maps each approved DSP to XML/audio/artwork references. Empty approval
  produces zero submissions. It does **not** choose/activate commercial endpoints,
  fabricate region/use rights or compute fees. Every item has delivery disabled.
- `preflight::preflight`: independent XML, metadata, object and rights checks.
  Pass requires all four PASS. Storage errors produce UNKNOWN, never PASS.
  File reads compare exact sizes/media types and SHA-256 (not ETag-as-hash).
  Rights checking reuses `domain::freshness_guard` and binds its expected package
  digest to the actual supplied XML bytes. Muse must load current pin/hold/contract
  facts from trusted database state inside the orchestration boundary.
- `identifiers::record_existing`: caller-owned transaction, explicit org/release/
  revision/track linkage, global identifier uniqueness, append-only assignment.
  Exact-target retry returns the original assignment; other-target reuse or a
  changed identifier fails for review, including legitimate reissue/migration
  cases that require a later reuse relationship model. ISRC is normalized 12-char
  syntax; UPC is UPC-A with checksum, not EAN-13. Syntax does not prove ownership.
  `issue_identifier` always returns `IDENTIFIER_ISSUANCE_OFF`.

Migration **0011** is independent of 0010. It enforces both Rust identifier formats
in SQL, composite tenant FKs, track/release matching, uniqueness, immutability and
forced RLS. No runtime grants are added. An authorized transaction must set
`app.org_id`; the tests set it explicitly on every connection/transaction
(Astra's CI ran as a postgres superuser, which silently bypasses RLS — the
tests were fixed to not depend on that). A non-bypass role is still used to
verify RLS org isolation, with role membership granted so `SET ROLE` works for
non-superuser test users.

## Profile boundary

This is an internal partner-neutral **synthetic** ERN preparation profile,
namespace `urn:audeniq:ern:synthetic:1`, not a claim of DDEX ERN 3/4 conformance.
The fixed grammar has a checked-in synthetic XSD. Runtime validation requires
exact equality to the deterministic serializer's bytes, rejecting DTDs, unknown
elements, altered values/references and even alternate whitespace. CI separately
parses six XML outputs from three generated fixtures with `xmllint --nonet --schema`.
Actual DDEX/partner XSDs, licenses, contracts, full territory/use grants and
transport adapters remain the commercial integration gate. Do not transmit these
fixtures to a DSP or treat this mapping as an executable route.

The existing `ObjectStore::get` buffers objects. This pass checks HEAD first and
refuses declared assets over 64 MiB; it does not solve the existing store's
unbounded-response/HEAD-to-GET race. Production large masters need a bounded
streaming/version-pinned storage reader. Hash mismatch and unavailable storage
remain blocked/unknown. No passing preparation result itself enables delivery.

## Ownership

Merged 2026-09-25 into `foundation/f4-stage3-distribution` and wired into the
worker: `distribution::run_prepare_release` now drives canonical → freeze →
`PreparedRelease::from_canonical` → synthetic ERN → four preflight checks →
route plan → (pass only) READY_FOR_DELIVERY. `operations::execute` routes the
`prepare_release` job through the real handler with storage; an
`IDENTIFIER_CONFLICT` dead-letters immediately instead of retrying.

Migration **0012** adds `catalog.releases.upc` / `artwork_asset_id` and bumps
the canonical schema to v2. Migration **0013** adds append-only
`distribution.preparation_artifacts` (one row per frozen package: ERN SHA-256,
preflight report JSON, route plan JSON, immutable trigger) written in the same
commit that flips the release to READY_FOR_DELIVERY. The frozen package body
stays the immutable canonical snapshot; its `identifier_refs`/`route_id`/
`dsp_packages`/`preflight_ref` placeholders resolve via the artifact row and
the identifier ledger, not by mutating the package.

`identifiers::record_existing` is called by the worker inside the
READY_FOR_DELIVERY transaction (with `app.org_id` set for the RLS-protected
ledger): the release UPC plus every track ISRC. Exact-target retries are
idempotent; a cross-target conflict is a permanent integrity failure.

Only module exports in `lib.rs` are shared. Merge Muse's exports additively.

## Verification

The F4 acceptance workflow runs the requested fmt, warning-free workspace clippy,
and `DATABASE_URL=postgres://f2test:f2test-local-dev-only@localhost/audeniq_f2`
workspace tests against disposable PostgreSQL. Three synthetic JSON fixtures
(single, EP, multi-disc multilingual album) and negative tests cover preparation.
Database tests exercise concurrent assignment, retry/conflict, SQL constraints,
immutability and RLS. Execution results must be taken from the specific Actions
commit/run, not inferred from this document or test source.
