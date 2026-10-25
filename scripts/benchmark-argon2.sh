#!/usr/bin/env bash
set -euo pipefail

# Argon2id time per hash of crate::argon2id against the argon2 crate (its
# test oracle), both compiled with rustc -O. Verifies identical tags first.
task_repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
task_target_dir=${CARGO_TARGET_DIR:-"$task_repo_root/target"}
task_rlibs=$(cargo build --locked -p argon2@0.6.0 -p blake2@0.11.0 --message-format=json --manifest-path "$task_repo_root/Cargo.toml" |
    python3 -c 'import json,sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m["target"]["name"] in ("argon2", "blake2"):
        rlib = next(f for f in m["filenames"] if f.endswith(".rlib"))
        print("--extern", m["target"]["name"] + "=" + rlib)')
task_work=$(mktemp -d "${TMPDIR:-/tmp}/audeniq-argon2-bench.XXXXXX")
trap 'rm -rf -- "$task_work"' EXIT
# shellcheck disable=SC2086
rustc --edition=2024 -O "$task_repo_root/scripts/benchmark-argon2.rs" $task_rlibs \
    -L "dependency=$task_target_dir/debug/deps" -o "$task_work/benchmark"
"$task_work/benchmark"
