#!/usr/bin/env bash
set -euo pipefail

# MD5 throughput of crate::md5 against md-5 0.11 (its test oracle), both
# compiled with rustc -O on this machine. Verifies identical digests first.
task_repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
task_target_dir=${CARGO_TARGET_DIR:-"$task_repo_root/target"}
task_md5_rlib=$(cargo build --locked -p md-5@0.11.0 --message-format=json --manifest-path "$task_repo_root/Cargo.toml" |
    python3 -c 'import json,sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m["target"]["name"] == "md5":
        print(next(f for f in m["filenames"] if f.endswith(".rlib")))')
task_work=$(mktemp -d "${TMPDIR:-/tmp}/audeniq-md5-bench.XXXXXX")
trap 'rm -rf -- "$task_work"' EXIT
rustc --edition=2024 -O "$task_repo_root/scripts/benchmark-md5.rs" --extern "md5=$task_md5_rlib" \
    -L "dependency=$task_target_dir/debug/deps" -o "$task_work/benchmark"
"$task_work/benchmark"
