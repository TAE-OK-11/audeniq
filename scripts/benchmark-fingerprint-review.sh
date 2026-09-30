#!/usr/bin/env bash
set -euo pipefail

# Run after building audeniq-core. Uses the same RustFFT dependency for both
# source versions and verifies identical outputs before timing comparisons.
task_repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
task_base=${1:-400c6c963e71034303039dd374bde4f0f5f99ab1}
task_target_dir=${CARGO_TARGET_DIR:-"$task_repo_root/target"}
task_fft_rlib=$(find "$task_target_dir/debug/deps" -maxdepth 1 -name 'librustfft-*.rlib' -print -quit)
if [[ -z "$task_fft_rlib" ]]; then
    printf '%s\n' 'Build audeniq-core with cargo build --locked -p audeniq-core --lib first.' >&2
    exit 1
fi
task_work=$(mktemp -d "${TMPDIR:-/tmp}/audeniq-fingerprint-bench.XXXXXX")
trap 'rm -rf -- "$task_work"' EXIT
git -C "$task_repo_root" show "$task_base:crates/core/src/fingerprint.rs" > "$task_work/baseline-fingerprint.rs"
cp "$task_repo_root/crates/core/src/fingerprint.rs" "$task_work/updated-fingerprint.rs"
cp "$task_repo_root/scripts/benchmark-fingerprint-review.rs" "$task_work/benchmark.rs"
rustc --edition=2024 -O "$task_work/benchmark.rs" --extern "rustfft=$task_fft_rlib" \
    -L "dependency=$task_target_dir/debug/deps" -o "$task_work/benchmark"
"$task_work/benchmark"
