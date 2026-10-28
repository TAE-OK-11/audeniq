# Deployment follow-up (after PRs #348–#351)

`deploy.sh` validates registry, tag and digest syntax before login/pull and
validates the resolved digest before editing production.env. Invalid values are
not echoed. This keeps newline and sed replacement characters out of the env
file. The accepted reference is an explicit tag or lowercase SHA-256 digest;
unqualified names and implicit `latest` are rejected.

The existing all-profile Compose validation also plans dependency pulls. Only
services whose resolved image exactly equals the verified immutable application
digest are omitted. Other images, including override services and inactive
profiles, still refresh; PgBouncer still builds with `--pull` and its existing
fallback. Both `up --no-build` calls, authenticated DB probes, migrations/grants,
readiness budgets, stability watch, and rollback hints remain in place.

Use `DEPLOY_TIMING=1 ./deploy.sh <reference>` on the server to compare
`image_pull`, `dependency_pull`, and total times across comparable releases.
The regression tests verify the command selection with fake Docker. The dependency phase also records the selected service count. No live
Docker timing or measured speedup is available from these tests.

Studio release paths are immutable once installed. Installing a retained SHA
validates its files and switches to it without extracting or replacing the
archive. Use a new SHA to publish new bytes. This also avoids redundant
extraction (`DEPLOY_TIMING=1` emits successful install/reuse elapsed seconds) and keeps Studio's canonical-path snapshot cache correct. Rollback
rejects malformed history identifiers and symlink release directories before
following them. Installation and rollback still hold the release lock, switch
`current` atomically, and retain the current and previous releases during prune.

Surveyed and deliberately retained:

- Runtime containers already drop all capabilities, use read-only roots and
  no-new-privileges. PostgreSQL's root entrypoint still needs its normal ownership
  setup before dropping privileges; no untested capability changes were added.
- PgBouncer already runs as postgres, renders secrets into tmpfs, validates
  config against line injection and classifies authenticated readiness probes.
- check-cpu.sh already checks every exposed core before application execution.
- API boundary and error responses already redact SQL details, stamp stable
  codes, and enforce no-store. No public error shapes or codes changed.
- Studio's encoding negotiation, representation ETags, cache overrides,
  dashboard behavior, and closed preflight remain unchanged.
- CloudFront template and edge code already isolate cache keys for public
  assets and disable API caching. No AWS, WAF, DNS, TLS, pool sizing, or server
  values were changed; further tuning requires actual traffic/host evidence.
