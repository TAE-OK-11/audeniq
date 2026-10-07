#!/bin/sh
# Build the pinned AUDENIQ media engine (audeniq-qc) and install it as
# DEST/bin/audeniq-qc. The commit lives in deploy/audeniq-qc.rev so the image,
# CI and local runs use the same engine; bumping it changes QC measurements,
# so it goes together with the QC rule / fingerprint version it was qualified for.
set -eu
dest=${1:?usage: install-audeniq-qc.sh DEST}
rev=$(tr -d '[:space:]' < "$(dirname "$0")/audeniq-qc.rev")
cargo install --locked --quiet \
  --git https://github.com/TAE-OK-11/audeniq-qc --rev "$rev" \
  --root "$dest" audeniq-qc
"$dest/bin/audeniq-qc" capabilities >/dev/null 2>&1 || {
  # Cross-target builds cannot run here; the image smoke test checks them.
  [ -n "${CARGO_BUILD_TARGET:-}" ] || exit 1
}
