#!/bin/sh
# Build the pinned AUDENIQ media engine (audeniq-music: audio probe, lossless
# FLAC conversion, QC decode and fingerprint windows as `audeniq-music qc`;
# image/audio metadata, QR, OCR and upload sanitization as
# `audeniq-music photo`) and install it as DEST/bin/audeniq-music. The commit
# lives in deploy/audeniq-music.rev so the image, CI and local runs use the
# same engine; bumping it can change QC measurements, cover measurements and
# sanitized bytes, so it goes together with the QC rule / fingerprint version
# it was qualified for.
set -eu
dest=${1:?usage: install-audeniq-music.sh DEST}
rev=$(tr -d '[:space:]' < "$(dirname "$0")/audeniq-music.rev")
cargo install --locked --quiet \
  --git https://github.com/TAE-OK-11/audeniq-media --rev "$rev" \
  --profile release-music --root "$dest" audeniq-music
"$dest/bin/audeniq-music" qc capabilities >/dev/null 2>&1 || {
  # Cross-target builds cannot run here; the image smoke test checks them.
  [ -n "${CARGO_BUILD_TARGET:-}" ] || exit 1
}
