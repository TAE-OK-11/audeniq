# Connected Studio / Workers / rented server

## Implemented topology

The deployment entry point is `crates/edge`: Rust Workers WASM serves **generated connected Studio assets** and proxies `/api/*` through `PRIVATE_API` (Workers VPC Service). The rented server runs Rust API, worker, PostgreSQL and cloudflared. Backend ports are not published by production Compose. Browser authentication remains a Rust API session; the edge adds a separate service secret. Browser files go directly to R2.

`web/studio/public` is preserved as the original visual prototype. **Do not deploy its old localStorage/IndexedDB business logic as a connected product.** The Rust `studio-pack` program extracts the existing CSS and logo, then packages `web/studio/connected/index.html` with the Rust/WASM application. The connected shell covers implemented Foundation operations and does not auto-create contract approvals or submission successes. Landing and survey are unchanged. This is not a conversion of all prototype screens: advanced wizard, rights, reports and settlement views remain future work.

New application code and browser automation are Rust. `wasm-bindgen` emits JavaScript loader glue, and Wrangler is a deployment tool. No Python runtime is used.

## Build and local browser integration

From repository root, Rust toolchain from `rust-toolchain.toml`:

```sh
rustup target add wasm32-unknown-unknown
cargo build --locked -p audeniq-core --bins
cargo build --locked -p audeniq-studio --lib --release --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cargo run --locked -p audeniq-studio --bin studio-pack
wasm-bindgen --target web --out-dir web/studio/dist/pkg target/wasm32-unknown-unknown/release/audeniq_studio.wasm
```

Start the development Compose stack with `.env` following README. In another terminal export the same `EDGE_SERVICE_SECRET`, set `APP_ENV=development`, then run `cargo run --locked -p audeniq-core --bin audeniq-dev-web`. Open **http://localhost:5173** (not 127.0.0.1; exact Origin matters). The gateway listens only on loopback and proxies to loopback port 8080. It is excluded from the production image. It is a local integration substitute, not proof that the remote VPC works.

The browser holds CSRF only in WASM memory; session cookie is HttpOnly. `POST /api/auth/csrf` requires an authenticated session and exact Origin, and returns a stable per-session HMAC token. Refresh and multiple tabs do not rotate each other's token. Responses are no-store. No session token, signed URL or catalog copy is persisted in localStorage.

UI includes register/login/logout, organization switching, paginated catalog, artist/label/release creation and editing, archive, track addition/removal, optional WAV/FLAC direct upload, preflight blockers and session revocation. Label party IDs and track artist IDs currently require an explicit ID; selectors and richer metadata forms remain future UX work. A failed mutation is not automatically retried. Reload lists after uncertain network outcomes. Upload failures can leave unbound assets; cleanup is not implemented.

## Production server preparation (no automatic deployment)

1. On a separate Debian 13 rented instance (NHN Cloud or another provider), install Docker Engine and Compose. Keep SSH limited to authorized administration and do not expose 5432/8080. No vendor-specific account resources are created by these files.
2. Build a reviewed image from `deploy/Dockerfile` and obtain its immutable digest through your authorized registry workflow. Set `AUDENIQ_IMAGE` and a verified `CLOUDFLARED_IMAGE` digest in `deploy/production.env`, copied from the example. Generate independent URL-safe DB passwords and a random service secret, protect the file with mode 600. Never use development volumes or credentials.
3. Create/configure the Tunnel and VPC Service in the account only after deployment authorization. Place the Tunnel token in `deploy/tunnel-token` (not Git). The connector can resolve/reach Compose service `api:8080`. Configure the VPC Service HTTP target and Tunnel accordingly; validate account-specific hostname resolution before cutover. Do not create a public API hostname as a shortcut.
4. Validate the configuration without starting services:

```sh
docker compose --env-file deploy/production.env -f deploy/compose.production.yaml config --quiet
```

5. The production Compose uses its own `audeniq-production` project and `pg_prod` volume. On a **new** volume only, bootstrap creates restricted API/worker roles, migrator applies SQLx migrations, grants applies runtime permissions, then API/worker start. Existing databases require a reviewed migration/credential procedure; do not reset them or rerun role bootstrap blindly. Never use `down -v` on production.
6. After separate deployment authorization, start with that exact env/file pair. Verify private reachability, authenticated `/ready`, role privileges, backups and restoration before customer use. Resource caps are initial isolation settings, not evidence that 2 vCPU / 4GB serves production demand.

## Workers build and configuration

Build the connected assets first. Install `worker-build 0.1.12` (`cargo install worker-build --version 0.1.12 --locked`), compatible with pinned worker 0.6.7, then copy `crates/edge/wrangler.toml.example` to ignored `wrangler.toml`. Build from `crates/edge` using `worker-build --release`. Validate packaging without publishing using `npx --yes wrangler@4.137.0 deploy --dry-run --outdir /tmp/audeniq-edge-bundle`. Set the real HTTPS APP_ORIGIN, production custom-domain route and verified VPC service ID. Set `EDGE_SERVICE_SECRET` through Wrangler secret storage, matching the backend. Do not embed it in static assets. Static assets and API are served by the **same Worker and origin**; do not deploy the old Studio wrangler configuration alongside it.

`run_worker_first=true` guarantees origin checks and security headers apply to assets; `/api/admin*` remains blocked. No public-origin fallback exists. The generated frontend artifact in CI is useful for review; no CI step deploys it. Wrangler deployment is a separate authorized action.

Real R2 uploads additionally require private bucket credentials, exact-origin R2 CORS and a matching R2 account endpoint. CSP permits the R2 S3 endpoint, not arbitrary third-party upload hosts. The browser does not set Content-Length manually; it passes the File as request body and forwards only MIME and nonce headers from the grant. File registration is not QC approval.

## Validation boundaries

CI validates Rust, real PostgreSQL tests, connected WASM build and a Chromium browser against the loopback development gateway. Production Compose must also parse without public ports. Real Workers VPC/Cloudflare Tunnel/NHN routing, deployed edge Fetcher compatibility, R2 browser CORS/signature behavior, backup restoration and load capacity require separate infrastructure validation. No automatic deployments, remote account changes or customer side effects are part of this work.

Primary references checked 2026-09-24:
- https://developers.cloudflare.com/workers/static-assets/binding/
- https://developers.cloudflare.com/workers-vpc/configuration/vpc-services/
- https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/
