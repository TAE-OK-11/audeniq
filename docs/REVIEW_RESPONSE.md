# Review response — 2026-09-24

Reviewed `REVIEW_F0F1.md` and `QUALITY_F0F1.md` added in commits 2e7d788 / 7308b1e.
The original reviews are preserved. This record distinguishes implementation from deferred suggestions.

| Finding | Decision and evidence |
| --- | --- |
| M2 / upload expiry | Use wall clock at initial validation and recheck after HEAD, before CopyObject; retain final atomic expiry condition. Expired-session test asserts zero storage calls. A copy that finishes after expiry can still leave an orphan; retention/reconciliation remains required. |
| M3 / upload completion rate | Per-user 60 requests / 15 minutes before transaction or storage calls. Test forces exhausted bucket and checks 429 plus no storage calls. |
| E2 / credit queries | Validate unique party IDs in one `ANY` query. Repeated roles for one party remain supported. Inserts still participate in the business transaction. |
| E4 / password lock | Verify and hash before user row lock, then recheck exact hash version and active session under lock. Concurrent credential updates cannot use stale verification. |
| m3 / Q2 | Release Argon2 semaphore immediately after CPU work for register/login/password change. |
| m5 | Adding ACTIVE membership requires ACTIVE target user under shared lock. Revocation remains possible for inactive users. |
| m7 | Trim cookie values before length/digest validation. |
| m8 / E9 states | Parse the authoritative state contract once via OnceLock; no alternate state list. Schema-validator caching remains deferred. |
| m2 / E1 | Lock timeout, deadlock and serialization failures become 409 instead of 500. Row locks around upload storage IO remain: splitting this safely needs a durable completion lease, cancellation fencing and orphan reconciliation. Not falsely reported as solved. |
| M1 | Owner-only future package/check repositories remain unavailable to runtime roles. Do not widen permissions for handlers that do not exist. F2 requires minimal grants plus tests with actual roles before activation. |
| m1 health | Keep service-secret requirement, including health/readiness. Compose CI already probes with the secret. No public monitor endpoint is needed for the private topology. |
| Q1 / m6 submit | Keep organization/resource authorization even while gated. Removing it would violate mandatory access checking on protected resource endpoints. |
| E5 / E6 / Q3 | Keep transaction-scoped permission locks and lock ordering. These reads are not actually a single unprotected SELECT; moving them outside a transaction weakens revocation guarantees. |
| E7 | Keep database clock authoritative for upload expiry. |
| Q4 / m9 | row_version is a public optimistic concurrency contract needed by UI; it is not a secret. Explicit field projections remain a future API versioning improvement. |
| m4 / E3 / E8 / E10 / Q5 | Rate-bucket retention, sweep cadence, no-op upsert reduction, optional NOTIFY and rate-bucket contention remain optimization/operations tasks. |
| asynchronous audit/outbox suggestion | Rejected: audit, domain changes and Outbox must commit atomically. Do not move this work to an uncoordinated post-commit call. |
| merge suggestion | No automatic merge or production deployment performed. User-authorized commits stay on foundation/f0-f1. |

## Additional technology review

`TECH_REVIEW_F0F1.md` (9a37fa7) was added while this work was in progress and is preserved in the branch ancestry. Its BFF warning is accepted: edge code only checks boundaries, serves assets and proxies, while Rust API owns sessions/authorization/business transactions. The TypeScript replacement suggestion conflicts with the user's explicit Rust-only application requirement, so it is not adopted. The original React plan likewise does not override that later instruction; existing visual source remains untouched and connected application logic is Rust/WASM.

Retention/partitioning risks and a price-input cost worksheet are now documented in `OPERATIONS_RETENTION.md`. No purge of immutable data or paid service activation was introduced. The review's older 26-table/28-test numbers are historical, not the latest validation counts. Rust types do not by themselves prove legal or financial correctness; SQLx runtime `query` calls here are verified by PostgreSQL integration tests, not compile-time query macros.
