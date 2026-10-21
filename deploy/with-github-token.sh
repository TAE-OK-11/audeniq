#!/bin/sh
# Run a command that fetches the AUDENIQ engine repository (audeniq-media,
# which builds audeniq-music). With the BuildKit secret `github_token` (CI:
# AUDENIQ_GITHUB_TOKEN, a read-only token for that repository) git asks a
# credential helper that reads the secret file at fetch time: the token is
# never written to a layer, and the helper's config lives in a temporary HOME
# removed afterwards. Without the secret the command runs unchanged.
set -eu
secret=${GITHUB_TOKEN_FILE:-/run/secrets/github_token}
if [ ! -s "$secret" ]; then
  exec "$@"
fi
# The temporary HOME must not move the toolchain: keep rustup/cargo where they are.
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
cat > "$home/.gitconfig" <<EOF
[credential "https://github.com"]
	helper = "!f() { test \\"\$1\\" = get || exit 0; echo username=x-access-token; echo \\"password=\$(tr -d '[:space:]' < '$secret')\\"; }; f"
EOF
HOME="$home" "$@"
