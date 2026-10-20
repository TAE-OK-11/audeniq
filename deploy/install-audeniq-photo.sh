#!/bin/sh
# Build the pinned AUDENIQ image toolkit (audeniq-photo: metadata, QR, OCR and
# upload sanitization) and install it as DEST/bin/audeniq-photo. It lives in
# the audeniq-qc repository (photo-cli, release-photo profile); the commit
# lives in deploy/audeniq-photo.rev so the image, CI and local runs use the
# same reader; bumping it can change cover measurements and sanitized bytes.
set -eu
dest=${1:?usage: install-audeniq-photo.sh DEST}
rev=$(tr -d '[:space:]' < "$(dirname "$0")/audeniq-photo.rev")
cargo install --locked --quiet \
  --git https://github.com/TAE-OK-11/audeniq-qc --rev "$rev" \
  --profile release-photo --root "$dest" photo-cli
"$dest/bin/audeniq-photo" --version >/dev/null 2>&1 || {
  # Cross-target builds cannot run here; the image smoke test checks them.
  [ -n "${CARGO_BUILD_TARGET:-}" ] || exit 1
}
