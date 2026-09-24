# AUDENIQ Backend Foundation

Rust modular monolith: `audeniq-api`, `audeniq-worker`, `audeniq-migrate`, and a Rust/WASM Cloudflare BFF. Based on FINAL 1.0 (2026-09-23) in `docs/BLUEPRINT.md`.

**Foundation implemented / NO-GO for customer operation.** Rust/API/Worker/WASM builds, real PostgreSQL integration tests and Docker Compose startup have passed GitHub Actions. Real R2 and Workers VPC/Tunnel connections remain unverified. See [implementation report](docs/IMPLEMENTATION_REPORT.md), [data model and ERD](docs/DATA_MODEL.md), and [API contract](docs/API.md) for evidence, scope and remaining work.

New application logic and tests are Rust only. The three supplied frontend packages are preserved in `web/` without redesign. Their pre-existing HTML/CSS/JavaScript is not part of the new Rust backend. No Python runtime or application dependency is introduced.

## Local development

Requires Rust 1.98.1, Docker Engine + Compose v2. PostgreSQL 17 is the target database.

1. Copy `.env.example` to `.env`. Set three different random URL-safe database passwords and a random `EDGE_SERVICE_SECRET` of at least 32 characters. Do not commit `.env`.
2. `docker compose up --build -d` starts PostgreSQL, runs SQLx migrations using the owner, applies separate runtime grants, and starts API/worker. Storage is disabled until configured.
3. `docker compose logs api worker migrate grants` shows startup results. No public host port is opened: dev ports bind loopback only.
4. Access control-plane APIs through the BFF or a development HTTP client carrying the service secret. See `docs/API.md`.

Native workflow: start an isolated PostgreSQL 17 instance, set `DATABASE_URL`, and run `cargo run -p audeniq-core --bin audeniq-migrate` with the migration owner. Apply `deploy/grants.sql` after creating the runtime roles. Switch `DATABASE_URL` to the API/worker login before `cargo run --bin audeniq-api` or `cargo run --bin audeniq-worker`. Native binaries read environment variables; they do not automatically load `.env`.

## Validation

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked -p audeniq-core --bins
cargo test --locked -p audeniq-core --lib
cargo test --locked -p audeniq-core --test foundation -- --test-threads=2
cargo build --locked -p audeniq-edge --target wasm32-unknown-unknown
```

The `foundation` integration suite requires `DATABASE_URL` for an **isolated test cluster** whose login can CREATE DATABASE. SQLx creates uniquely named test databases. It is an error, not a skipped pass, when the test DB is unavailable. Never point tests at a production database. SQLx removes passing test databases and may retain failing ones for diagnosis; inspect test output before removing only an identified test database.

Migrations are forward-only. Re-running `audeniq-migrate` is idempotent via SQLx migration history. For a clean development replay, use a NEW disposable database/Compose project volume; do not reset a shared or production volume. No automatic reset command is included.

`crates/edge/wrangler.toml.example` is a template, not an active cloud configuration. Install the compatible workers-rs `worker-build` tool and Wrangler only for an explicitly authorized deployment. See `deploy/PRODUCTION.md`.

## Foundation boundaries

- Current ACTIVE membership AND per-resource ACL on every protected operation.
- Argon2id, hashed server-session tokens, session expiry/revocation, Origin/CSRF protection, DB-backed auth rate limits.
- Artists, labels, draft releases, track editing/archiving, credit replacement, cursor pagination and a gated preflight checklist.
- Session inventory/revocation, logout-all and password changes that revoke all sessions.
- Private upload session + metadata verification + immutable-key copy; status/cancellation prevents late binding.
- PostgreSQL leased queue, fenced worker results, retries/DLQ, transactional outbox and append-only audit trail.
- Submitted revision/snapshot data and stored contract/route/package lineage are immutable. Six forward migrations create 31 tables. Routes remain disabled.
- Four package schemas and separate pipeline/eligibility/delivery/live state contracts are checked in.
- Final submission, QC PASS, rights approval, DSP send/ACK/LIVE, ISRC/UPC issuance, finance execution, email and administrator operations are not enabled.

No remote deployment or cloud account change is performed by this repository's build/test commands. GitHub Actions runs only after an authorized push. It runs tests against disposable PostgreSQL, not real customer infrastructure.

## Connected Studio and rented-server deployment

The connected frontend now has a Rust/WASM client in `crates/studio`. Build it using [the Studio deployment guide](docs/STUDIO_DEPLOYMENT.md); serve its generated assets through the Rust Worker in `crates/edge`. The original Studio prototype remains preserved but is not the connected deployment entry point. Landing/survey remain unchanged.

Separate production preparation: `deploy/compose.production.yaml` and `deploy/production.env.example`. All API/database ports remain private; `cloudflared` connects the rented server to Workers VPC. This does not provision NHN/Cloudflare accounts or deploy a service. See [review responses](docs/REVIEW_RESPONSE.md) for accepted fixes and deliberately deferred review suggestions.
