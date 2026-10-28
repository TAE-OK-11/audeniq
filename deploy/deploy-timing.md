# Deployment timing and cache reuse

During a separately authorized server deployment, enable timing with:

```sh
DEPLOY_TIMING=1 ./deploy.sh ghcr.io/tae-ok-11/audeniq@sha256:<digest>
```

This **performs a deployment**; it is not a dry run. Timing lines go to stderr
and contain only fixed phase labels, whole elapsed seconds, total seconds and
the exit code. Normal deployment output is unchanged. Success history remains
in its original timestamp/image format so rollback parsing is unchanged.
No server measurements were made for this change.

Phases follow the script's actual execution order:

1. `preflight`: lock, environment ownership/mode and tmpfs checks, volume
   upgrade guard, feature/key checks, optional registry login.
2. `image_pull`: pull requested image, resolve tags to immutable digest.
3. `image_verification`: architecture inspect, shell-only metadata and binary
   presence checks, host CPU check, then a separate upload sandbox execution.
4. `environment_update`: atomically replace the image in the mode-0600 env file.
5. `dependency_pull`: Compose image refresh (including shared PostgreSQL image).
   Existing warning/local-image fallback remains. Compose and Docker retain
   responsibility for shared image/layer reuse and any pull concurrency.
6. `pgbouncer_build`: build with base refresh and Docker's input-aware cache.
   Unchanged cached layers avoid reinstalling packages; changed base/context
   invalidates the corresponding layers. If refresh fails, try the existing
   local-cache build path; if that fails, stop with the rollback hint.
7. `database_start_migrate_grants`: Compose starts PostgreSQL, waits for health,
   runs migrations, runs grants with ON_ERROR_STOP, then starts PgBouncer.
   Both Compose up calls now use --no-build after the successful explicit build,
   avoiding the two redundant build requests. The supplied Compose flow builds
   only PgBouncer; server overrides adding other buildable services must prepare
   those images separately before deployment.
8. `database_readiness`: authenticate API and worker directly and through the
   pooler, with classified failures and the original 120-second budget. Retry
   sleep is 1 second rather than 2; probe execution time is additional.
9. `runtime_start_and_drain`: Compose starts/reconciles the full stack; this
   includes API graceful shutdown (30s grace), worker drain (11m grace, delivery
   drain defaults to 600s), and stopping disabled antivirus services. Compose
   may revisit migration/grants dependencies here; those checks are retained.
10. `runtime_stability`: original 15 two-second sleeps plus inspection overhead;
    verify exact image and unchanged restart counts for API and worker.
11. `api_readiness`: inspect Compose API health, check DB on failed readiness,
    retain the original additional 120-second budget and final runtime restart
    checks. Retry sleep is 1 second rather than 2. This observes health results;
    it does not make Docker run the API healthcheck more often.

Compose healthcheck intervals/timeouts/retries/start periods are unchanged:
PostgreSQL 3s/3s/20; PgBouncer 5s/20s/12 with 20s start period;
API 10s/9s/3 with 30s start period. Slow probes can extend elapsed wall time
beyond a loop deadline because deadlines are checked between probes.

Compare phase lines across comparable approved releases on the same host,
including cold-cache, warm-cache and registry-failure cases. The instrumentation
reports aggregate Compose startup/migration/grants time; container logs are
needed to attribute time within that phase. Actual registry latency, layer
reuse, base refresh, migration duration, graceful draining and readiness
improvements remain unmeasured locally.

`studio-release.sh` already skips an identical current release, switches files
atomically without restarting API, and serializes install/rollback. Its archive
validation, extraction and build steps are retained. `check-cpu.sh` is a small
local feature scan and remains unchanged. No Rust/frontend code was changed.
