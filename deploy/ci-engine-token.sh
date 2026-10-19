#!/bin/sh
# CI (GitHub Actions) wrapper for building the private AUDENIQ engines on the
# runner itself: writes AUDENIQ_GITHUB_TOKEN (read-only, audeniq-qc and
# audeniq-photo Contents) to a temporary file and runs the command through
# with-github-token.sh, which swaps HOME for a credential-helper config. The
# toolchain still lives under the real HOME, so pin RUSTUP_HOME/CARGO_HOME.
set -eu
here=$(dirname "$0")
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
if [ -z "${AUDENIQ_GITHUB_TOKEN:-}" ]; then
  exec "$@"
fi
token=$(mktemp)
trap 'rm -f "$token"' EXIT
printf %s "$AUDENIQ_GITHUB_TOKEN" > "$token"
GITHUB_TOKEN_FILE="$token" sh "$here/with-github-token.sh" "$@"
