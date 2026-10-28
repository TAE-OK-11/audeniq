#!/usr/bin/env bash
# Disposable PostgreSQL only: SQLx creates/drops isolated test databases.
# Requires Docker and cached/pullable postgres:18-trixie, plus the Rust toolchain.
set -euo pipefail
cd "$(dirname "$0")/../.."
image_test_container="audeniq-image-tests-$$"
cleanup() { docker rm -f -v "$image_test_container" >/dev/null 2>&1 || true; }
trap cleanup EXIT
# Random loopback port, no persistent volume, no application/live credentials.
docker run -d --name "$image_test_container" \
  -e POSTGRES_USER=audeniq_test -e POSTGRES_PASSWORD=local-image-test-only \
  -e POSTGRES_DB=postgres -p 127.0.0.1::5432 postgres:18-trixie >/dev/null
for ((attempt=0; attempt<60; attempt++)); do
  if docker exec "$image_test_container" pg_isready -U audeniq_test -d postgres >/dev/null 2>&1; then break; fi
  sleep 1
done
docker exec "$image_test_container" pg_isready -U audeniq_test -d postgres >/dev/null
image_test_port=$(docker port "$image_test_container" 5432/tcp)
export DATABASE_URL="postgres://audeniq_test:local-image-test-only@127.0.0.1:${image_test_port##*:}/postgres"
cargo test --locked -p audeniq-core --lib image_
cargo test --locked -p audeniq-core --lib studio_static
cargo test --locked -p audeniq-core --test image_serving -- --nocapture
cargo test --locked -p audeniq-core --test execution \
  artist_studio_insights_photo_and_platform_photo_requests -- --exact --nocapture
