# Backend passkey latency — 2026-10-29

This round measures backend ceremony handlers, then reuses validated PostgreSQL
connections where the measurements showed serial database overhead. It changes no
SQL verification statements, transaction isolation, lock order, API fields,
Argon2 settings, rate thresholds, session issuance, or audit writes. No frontend
files, dependencies, migrations, deployment, push, or PR are involved.

## Method

- Disposable PostgreSQL 16.15 on loopback, with `jit=off`, `shared_buffers=16MB`
  and `max_connections=30`; existing SQLx test pool configuration and dev profile
  (application unoptimized, dependencies optimized as specified in Cargo.toml).
- The ignored `ceremony_latency_sections` integration test sends real requests
  through the Axum router. Each of 45 samples creates an account, registers with
  the unchanged password verifier, logs in with email binding, obtains a fresh
  session-bound proof, and registers a second credential using that proof.
- Samples alternate Ed25519 and ES256. First five samples of each run are warm-up.
  Registration options occur twice per sample. No browser or biometric latency is
  included. Software signing and account fixture setup are outside handler timers.
- Static debug events use the `passkey_timing` tracing target. No identifiers,
  challenges, keys, signatures, emails, passwords, or tokens are logged. Timers
  subtract synchronous trace formatting/writing overhead. Handler totals include
  response-value construction, but exclude HTTP middleware, body serialization,
  network/TLS, and client work. Section medians need not sum to the total median.
- Source rate buckets are cleared **only by the probe, between samples in its
  disposable database**. Every measured handler still runs the original rate
  enforcement. Ordinary security tests do not use this reset.
- Two baseline and two optimized runs check host variability. ±5% is noise;
  small section shifts, especially zero/sub-microsecond labels, are not evidence
  of improvement. This is a local serial comparison, not a production capacity
  or real-device benchmark.

Reproduce using a disposable PostgreSQL with database-creation permissions:

```sh
export DATABASE_URL='postgres://.../postgres'
export ADMIN_ORIGIN='http://admin.localhost:5174'
export PASSKEY_RP_ID=localhost
cargo test -p audeniq-core --locked --offline --test passkeys ceremony_latency_sections -- --ignored --nocapture --test-threads=1 > run.log 2>&1
python3 scripts/summarize-passkey-latency.py run=run.log
```

## Measurement-led changes

The first baseline showed median registration options at 2.648 ms, email-bound
login at 3.7075 ms, and proof registration at 3.3655 ms. Serial database sections
cost hundreds of microseconds each, while signature/client/authenticator
verification was about 0.16 ms. Password registration spent 26.529 ms in password
re-authentication; that intentional cost was left unchanged.

- Registration options reuse one checked-out connection for credential exclusions,
  encrypted/plaintext-compatible email lookup, and bounded challenge cleanup and
  issuance, in the original statement order.
- Login/step-up reuse the lookup connection for the transaction. Account, session,
  and credential locks remain separate statements in the original order.
- Registration acquires its transaction connection after password work, then reuses
  it after commit to read the response list. No connection is held during Argon2
  execution or password-slot waiting.
- Normal pool validation remains enabled. Each new checkout is validated, and
  SQLx still performs its normal connection release behavior. This reduces
  redundant checkouts within a request; it does not disable pool health checks.

## Security and compatibility

All challenge consumption SQL is unchanged: purpose/user/session binding, atomic
one-time consumption, `FOR UPDATE SKIP LOCKED`, and 300-second wall-clock expiry.
The clientData type/origin/cross-origin/challenge checks, RP hash, UP/UV, stored
key/algorithm signature verification, advancing-counter rule with `(0,0)` exception,
ACTIVE-owner equality, email/user-handle checks, password version recheck under
lock, proof freshness and consumption, rate limits, last-method protection,
sessions, and success/failure audit events remain in place. Helpers accept an
existing PostgreSQL executor so callers can reuse a connection; API inputs and
outputs are unchanged.

The integration suite now explicitly attempts a login with a registered
credential ID signed by a different private key and checks rejection without a
session cookie. Existing tests exercise the other required security invariants.

## Limitations

Production topology, WAN/pooler latency, concurrent throughput/pool saturation,
real browsers/authenticators, and RS256 latency were not measured. Holding one
connection through adjacent SQL statements can affect pool fairness under load;
serial local measurements do not establish that behavior. No production latency
or p95 improvement is implied by a median improvement. The exact two-query
credential-owner insertion race remains source-reviewed rather than reproduced;
its owner-equality guard and locking statements are unchanged.

## Before/after measurements

Combined runs below use 80 measured samples per variant (160 registration-options
calls). Durations are microseconds. Full precision and sample counts are also in
[the measurement JSON](PASSKEY_LATENCY_20261029.json).

| Handler / section | Before p50 | After p50 | Before p95 | After p95 |
| --- | ---: | ---: | ---: | ---: |
| `login/begin_consume` | 608.0 | 426.5 | 1586 | 946 |
| `login/counter_update` | 273.5 | 253.0 | 727 | 636 |
| `login/email` | 550.5 | 533.5 | 1430 | 1116 |
| `login/origin_rate_decode` | 616.0 | 627.5 | 1732 | 1289 |
| `login/owner_credential_locks` | 490.0 | 299.0 | 1835 | 1183 |
| `login/session_audit_commit` | 808.5 | 690.0 | 2250 | 1682 |
| `login/total` | 3707.5 | 3051.0 | 8670 | 6142 |
| `login/verification` | 164.5 | 166.0 | 272 | 238 |
| `login_options/challenge` | 706.5 | 718.5 | 1978 | 1705 |
| `login_options/origin_rate` | 693.5 | 597.0 | 2270 | 1497 |
| `login_options/total` | 1520.5 | 1428.5 | 4100 | 3276 |
| `register_options/actor_rate` | 922.0 | 791.5 | 3209 | 2084 |
| `register_options/challenge` | 706.5 | 492.5 | 1608 | 1514 |
| `register_options/credentials` | 388.0 | 306.5 | 1106 | 1112 |
| `register_options/email` | 366.5 | 230.0 | 1033 | 584 |
| `register_options/total` | 2681.0 | 2011.5 | 6827 | 4417 |
| `register_password/actor_rate_decode` | 944.5 | 885.0 | 3282 | 2257 |
| `register_password/audit` | 262.5 | 227.5 | 846 | 774 |
| `register_password/commit` | 250.0 | 244.5 | 574 | 662 |
| `register_password/consume` | 407.5 | 384.0 | 826 | 976 |
| `register_password/count` | 257.0 | 230.0 | 893 | 901 |
| `register_password/insert` | 354.5 | 339.0 | 918 | 957 |
| `register_password/list` | 634.5 | 350.5 | 1267 | 713 |
| `register_password/reauth_password` | 27559.0 | 26163.5 | 35749 | 35350 |
| `register_password/total` | 33101.5 | 30860.5 | 49163 | 40833 |
| `register_password/transaction_reauth_locks` | 1162.0 | 1002.5 | 4111 | 3492 |
| `register_password/verification` | 78.5 | 84.0 | 112 | 111 |
| `register_proof/actor_rate_decode` | 820.5 | 774.5 | 2242 | 2498 |
| `register_proof/audit` | 215.5 | 190.5 | 747 | 711 |
| `register_proof/commit` | 169.5 | 180.5 | 541 | 656 |
| `register_proof/consume` | 292.5 | 298.5 | 866 | 1267 |
| `register_proof/count` | 207.5 | 192.5 | 586 | 895 |
| `register_proof/insert` | 279.0 | 271.5 | 765 | 769 |
| `register_proof/list` | 469.0 | 269.0 | 784 | 723 |
| `register_proof/reauth_password` | 0.0 | 0.0 | 1 | 1 |
| `register_proof/total` | 3314.5 | 2911.0 | 8245 | 14225 |
| `register_proof/transaction_reauth_locks` | 652.0 | 617.5 | 1760 | 2835 |
| `register_proof/verification` | 36.0 | 35.0 | 69 | 62 |
| `step_up/begin_consume` | 603.5 | 443.0 | 1312 | 1111 |
| `step_up/counter_update` | 277.5 | 237.0 | 825 | 807 |
| `step_up/email` | 1.0 | 89.5 | 2 | 577 |
| `step_up/origin_rate_decode` | 458.0 | 369.0 | 1357 | 840 |
| `step_up/owner_credential_locks` | 502.0 | 406.0 | 2280 | 1770 |
| `step_up/total` | 2894.5 | 2670.0 | 7179 | 6648 |
| `step_up/verification` | 152.0 | 145.5 | 247 | 203 |

Endpoint medians per independent run (milliseconds):

| Handler | Before 1 → after 1 | Before 2 → after 2 | Interpretation |
| --- | ---: | ---: | --- |
| Registration options | 2.648 → 1.820 | 2.690 → 2.453 | Lower in both; −31.3% / −8.8% |
| Email-bound login | 3.708 → 2.838 | 3.700 → 3.409 | Lower in both; −23.5% / −7.9% |
| Password registration | 32.091 → 30.669 | 33.874 → 31.025 | First change is noise; unchanged Argon2 varies |
| Proof registration | 3.366 → 2.525 | 3.248 → 3.215 | Second change is noise; no consistent total gain |
| Login options (control) | 1.499 → 1.325 | 1.657 → 1.548 | No checkout-reuse change here; host variation/control |
| Step-up authenticate | 2.851 → 2.440 | 2.935 → 2.930 | Second change is noise; no consistent total gain |

The response-list section fell in both password runs (579→340.5 and
650.5→360.5 µs) and proof runs (519→255.5 and 432.5→316.5 µs).
Login begin/consume fell 598→404.5 and 638.5→440 µs. Options email reads fell
374→210.5 and 362.5→267.5 µs; challenge issuance fell 685→447 and
712.5→596 µs. These are the targeted sections with repeatable changes.

Other sections and the unchanged login-options control moved as well, so the
entire endpoint delta cannot be attributed to connection reuse. Combined proof
registration p95 **worsened** from 8.245 to 14.225 ms; no tail-latency improvement
is claimed. Password verification remains unchanged; its varying wall time must
not be described as an Argon2 optimization. Any ±5% shift is noise regardless of
whether its sign is favorable.

## Validation

- `cargo fmt --all` and `cargo fmt --all --check`: passed.
- `git diff --check`: passed.
- `cargo clippy -p audeniq-core --all-targets --locked --offline -- -D warnings`:
  passed, zero warnings.
- `cargo test -p audeniq-core --locked --offline --test passkeys --test social_login --test password_reset --test account -- --test-threads=1`:
  9 passed (passkeys 6, social_login 1, password_reset 1, account 1); manual
  latency probe ignored in this ordinary regression run.
- `cargo test -p audeniq-core --locked --offline --lib passkey -- --test-threads=1`:
  3 passed, including old-transaction wall-clock expiry, locked-challenge
  rejection, and committed one-time consumption/replay rejection.
- Four reported latency-probe runs passed. The measurement JSON has 80 samples
  per handler/section and 160 for registration options per variant.
- All 26 SQL string literals in `passkey.rs` were compared with the original
  branch HEAD: byte-for-byte unchanged, in the same order. The optimized source
  used for integration tests and measurements differs from the final source
  only by a comment. No dependency manifest or lockfile changed.

Database commands used `DATABASE_URL` pointing only to the disposable loopback
instance, plus `ADMIN_ORIGIN=http://admin.localhost:5174` and
`PASSKEY_RP_ID=localhost` at process startup. An initial serial integration run
without startup `ADMIN_ORIGIN` failed one passkey test with 403 after a preceding
test initialized the memoized site registry. Re-running all four suites with the
fixture origin initialized at startup passed; no runtime policy was relaxed.
The probe's first build also exposed an unused email fixture; the probe was
corrected to send it, then both reported baseline runs included email binding.

Security regression coverage includes challenge replay and expiry (including
expiry while registration waits on an account lock), missing UP/UV, wrong
origin/RP, registered-ID/wrong-key signature failure, mismatched email and user
handle, unregistered credentials, clone counters and synced zero counters,
wrong-password registration, session-bound one-use expiring re-authentication
proofs, owner-scoped removal, last-sign-in-method preservation under concurrent
removals, and audit/session assertions. No full repository-wide security audit
or entire crate-wide test suite is claimed; the required authentication suites
and passkey unit tests were run.
