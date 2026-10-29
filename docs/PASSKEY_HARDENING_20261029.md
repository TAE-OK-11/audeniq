# Backend passkey hardening review — 2026-10-29

This document records the implementation and review notes that would accompany a future PR. This round does not create a PR, push, deploy, or modify frontend files.

## Changes and compatibility

- Challenge issuance performs bounded expiry cleanup (at most 256 rows, retaining the existing one-hour grace) and insertion in one SQL statement instead of two round trips. Cleanup skips locked rows; the existing expiry index serves the candidate scan. No expiry or verification check is removed.
- Challenge consumption locks its candidate without waiting and checks wall-clock expiry after obtaining that lock. Previously registration could begin a transaction, wait for an account/session lock, and accept a challenge after expiry because PostgreSQL `now()` stays at transaction start. Concurrent consumption returns the existing `PASSKEY_CHALLENGE_INVALID` response. Freshness is checked at consumption, not at the later session commit.
- Authentication requires the credential owner to match the account actually locked in the preceding query. A credential registered between the two READ COMMITTED queries must retry with a new challenge; it cannot issue a session without the intended account lock. Existing account/session/credential lock order remains intact.
- Emergency-password verification and session issuance use zeroizing buffers for their owned password/token allocations. Intentional response/cookie copies still exist; this is memory-lifetime reduction, not a claim that every process allocation is wiped.
- Registration reads AAGUID bytes from the attested credential data already returned by `getAuthenticatorData()`. No attestation-object input or external dependency is required. A recognized provider becomes the initial stored `name`; custom renames remain supported. A separate nullable `authenticator_name` remains the provider hint. Unknown, zero, and historic missing AAGUIDs preserve nickname behavior.

No routes are added. `GET /api/auth/passkeys` gains `items[].authenticator_name` (string or null), retaining all previous fields. Registration, rename, and deletion responses use the same list serializer and gain that field too. Login, options, step-up, and session response schemas remain unchanged. Migration 0164 must run before this binary starts. It adds nullable columns without guessing historic provider names.

## Storage and leak audit

| Data | Existing/current storage | Assessment |
| --- | --- | --- |
| Challenge | SHA-256 of a 32-byte OS-random value, 300-second expiry, atomic one-use consumption | A high-entropy verifier should remain a one-way hash; reversible encryption would not improve it. Fixed wall-clock expiry boundary. |
| Credential ID | Plain bytea, unique index, owner-scoped exclusion options | Public opaque lookup identifier, not a bearer secret. Required for authenticator lookup. Keep out of logs and general passkey lists. Database access/backups still reveal account-to-credential associations. |
| Public key | Plain SPKI DER, algorithm constrained to existing Ed25519/ES256/RS256 verification | Public verification material, explicitly unencrypted by migration 0104. Private keys never reach the backend. Confidentiality encryption is not a signature-strength upgrade and would not by itself protect against a database writer changing account ownership. No speculative cipher change. |
| AAGUID / provider | Nullable UUID / text in passkey row | Nonunique model metadata; never used as attestation trust, MFA evidence, or authorization. Redacted zero becomes null. Raw AAGUID is not exposed in list output. |
| Session / CSRF / reauthentication token | SHA-256 token verifiers; raw session only in HttpOnly cookie, CSRF/proof returned intentionally | Existing high-entropy token hashing is appropriate; session issuance now wipes its owned raw-token buffer on drop. Proof remains user/session bound and single-use. |
| Emergency password | Argon2 verifier in users; transient request plaintext | Registration now wipes its owned password allocation, including proof-based and early-rejection paths. Password worker, throttling, and under-lock current-password recheck remain. |
| Audit and error logs | Static action/reason codes; bounded error code for server failures | No passkey payload, credential ID, AAGUID, key, password, challenge, or raw token logging found in passkey handlers or shared error/request boundary. No dynamic DB error details added. |

Existing personal-data encryption uses versioned AES-256-GCM with row/field-bound associated data (`payout_keys.rs`). No insufficient stored secret cipher was found in the passkey path. This round does not claim an encryption-strength increase where the stored material is public or already appropriately hashed. Infrastructure disk/backup encryption, TLS deployment, production log retention, and production key custody cannot be established from source inspection alone.

The registration contract still trusts the browser-provided SPKI with attestation `none`, as before. Provider names are unverified display hints and may be spoofed or privacy-redacted. This change does not claim attestation verification or provider coverage where no AAGUID is supplied.

## Registry provenance and scope

Five exact mappings curated from the [passkeydeveloper community registry](https://github.com/passkeydeveloper/passkey-authenticator-aaguids/blob/main/aaguid.json), checked 2026-10-29:

| AAGUID | Name |
| --- | --- |
| bada5566-a7aa-401f-bd96-45619a55120d | 1Password |
| d548826e-79b4-db40-a3d8-11116f7e8349 | Bitwarden |
| 50726f74-6f6e-5061-7373-50726f746f6e | Proton Pass |
| 531126d6-e717-415c-9320-3d9aa6981239 | Dashlane |
| ea9b8d66-4d01-1d21-3ce4-b6b48cb575d4 | Google Password Manager |

This is a deliberate subset, not the FIDO Metadata Service or a complete model registry. No downloaded icons, runtime fetches, telemetry, or new crates. Providers can have additional AAGUIDs; add only independently checked exact entries. Do not infer provider from user agent, nickname, or zero UUID. Existing records cannot be backfilled because their registration AAGUID was never retained.

## UX research and future frontend work

[Ranked research](PASSKEY_UX_RECOMMENDATIONS_20261029.md) recommends conditional authentication/autofill first, then reuse of discoverable credentials, credential synchronization with platform regression gates, retention of required UV, and deferral of conditional creation and PRF/largeBlob. Conditional creation may omit UP/UV and therefore conflicts with this round's mandatory verification invariants; it needs a separate future policy decision. Research features are not implemented.

Future Studio/signup work may consume `authenticator_name` with nickname fallback, preserving custom rename semantics and treating provider detection as a hint. Conditional authentication/creation and Signal API integration require separate implementation authorization, browser/provider testing, and any required backend enrollment/snapshot contract. No screen, wording, CSS, or visual design decisions are made here.

## Verification

- `cargo fmt --all` and `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- `DATABASE_URL=<disposable PostgreSQL 16.15> cargo test -p audeniq-core --offline --lib passkey`: 3 passed.
- `DATABASE_URL=<same instance> cargo test -p audeniq-core --offline --test passkeys --test social_login --test password_reset --test account`: 9 passed (passkeys 6, social_login 1, password_reset 1, account 1).
- `cargo clippy -p audeniq-core --offline --lib --bins --test passkeys -- -D warnings`: passed with zero warnings.

Initial setup attempts were not clean: the first database unit test lacked `DATABASE_URL`; a sandboxed integration attempt could not connect to localhost (`Operation not permitted`). Both were rerun successfully with an explicit URL and local network access.

 Security regression tests cover wall-clock expiry in an old transaction and in a real registration blocked behind an account lock, locked-candidate rejection, successful one-time consumption, bounded cleanup with a locked expired row, preserved 300-second live challenge hashing, exact AAGUID parsing including truncation, recognized/unknown/redacted/historic provider storage and API serialization, and custom rename compatibility. Existing suites cover UP/UV, origin/RP, signatures, clone counters, synced zero counters, proof/session binding, replay, and concurrent sign-in-method removal.

The owner-equality guard was independently source-reviewed; its exact two-query insertion race has not been reproduced deterministically. No production latency benchmark or real-device/browser enrollment test is claimed.
