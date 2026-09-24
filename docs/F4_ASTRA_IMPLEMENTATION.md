# F4 Astra preparation components

Base: `foundation/f3-stage2-review` at `eb3134ef8ccccfdaa8cfae4da7c0001b0abb4e7b`.

## Integration contract for Muse

The base contains no `distribution.rs`, `CanonicalRelease`, `VerificationPackage`,
or `DspScope` Rust types. `review.rs` stores the F3 package as JSON with
`approved_scope: { dsp_ids: [...] }`. `preparation_model.rs` holds temporary,
explicit inputs; it does not create snapshots or implement any worker behavior.
When Muse's canonical contract lands, adapt/re-export these inputs or replace their
imports. Do not deserialize an entire F3 row into `CanonicalRelease`.

- `ern::generate_ern(&CanonicalRelease) -> Result<String>`: deterministic, pure,
  sorted track/DSP output; rejects invalid identifiers, XML control characters,
  duplicate resources/positions and missing metadata. No DB or external service.
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
`app.org_id` locally; superuser CI uses an explicit non-bypass role to test RLS.

## Profile boundary

This is an internal partner-neutral **synthetic** ERN preparation profile,
namespace `urn:audeniq:ern:synthetic:1`, not a claim of DDEX ERN 3/4 conformance.
The fixed grammar has a checked-in synthetic XSD. Runtime validation requires
exact equality to the deterministic serializer's bytes, rejecting DTDs, unknown
elements, altered values/references and even alternate whitespace. CI separately
parses all three generated fixtures with `xmllint --nonet --schema`.
Actual DDEX/partner XSDs, licenses, contracts, full territory/use grants and
transport adapters remain the commercial integration gate. Do not transmit these
fixtures to a DSP or treat this mapping as an executable route.

The existing `ObjectStore::get` buffers objects. This pass checks HEAD first and
refuses declared assets over 64 MiB; it does not solve the existing store's
unbounded-response/HEAD-to-GET race. Production large masters need a bounded
streaming/version-pinned storage reader. Hash mismatch and unavailable storage
remain blocked/unknown. No passing preparation result itself enables delivery.

## Ownership

Unchanged: `distribution.rs`, `operations.rs`, all existing migrations, especially
Muse's reserved 0010, snapshot/hash persistence and `prepare_release` wiring.
Only module exports in `lib.rs` are shared. Merge Muse's exports additively.

## Verification

The F4 acceptance workflow runs the requested fmt, warning-free workspace clippy,
and `DATABASE_URL=postgres://f2test:f2test-local-dev-only@localhost/audeniq_f2`
workspace tests against disposable PostgreSQL. Three synthetic JSON fixtures
(single, EP, multi-disc multilingual album) and negative tests cover preparation.
Database tests exercise concurrent assignment, retry/conflict, SQL constraints,
immutability and RLS. Execution results must be taken from the specific Actions
commit/run, not inferred from this document or test source.
