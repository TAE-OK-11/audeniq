#!/usr/bin/env bash
set -euo pipefail

# Text filters before (917861d, before #325) and now, in one program on this
# machine: profanity filter, protected artist names, fingerprint report
# parsing. Verifies identical results before timing. rustc -O for both.
task_repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
task_base=${1:-917861d3a1a3174d4d8068707666896d4680f74b}
# CI checks out one commit: fetch the baseline when it is missing.
git -C "$task_repo_root" cat-file -e "$task_base^{commit}" 2>/dev/null ||
    git -C "$task_repo_root" fetch --no-tags --depth=1 origin "$task_base"
task_target_dir=${CARGO_TARGET_DIR:-"$task_repo_root/target"}
task_externs=$(cargo build --locked -p aho-corasick@1.1.5 -p unicode-normalization@0.1.26 -p unicode-security@0.1.2 \
    -p serde@1.0.229 -p serde_json@1.0.154 --message-format=json --manifest-path "$task_repo_root/Cargo.toml" |
    python3 -c 'import json,sys
want = {"aho_corasick", "unicode_normalization", "unicode_security", "serde", "serde_json"}
for line in sys.stdin:
    m = json.loads(line)
    name = m.get("target", {}).get("name", "").replace("-", "_")
    if m.get("reason") == "compiler-artifact" and name in want:
        rlib = next((f for f in m["filenames"] if f.endswith(".rlib")), None)
        if rlib:
            print("--extern", name + "=" + rlib)')
task_work=$(mktemp -d "${TMPDIR:-/tmp}/audeniq-before-after.XXXXXX")
trap 'rm -rf -- "$task_work"' EXIT
mkdir -p "$task_work/old_profanity" "$task_work/new_profanity"
git -C "$task_repo_root" show "$task_base:crates/core/src/profanity/mod.rs" > "$task_work/old_profanity/full.rs"
git -C "$task_repo_root" show "$task_base:crates/core/src/profanity/words.rs" > "$task_work/old_profanity/words.rs"
git -C "$task_repo_root" show "$task_base:crates/core/src/protected_names.rs" > "$task_work/old_names_full.rs"
cp "$task_repo_root/crates/core/src/profanity/mod.rs" "$task_work/new_profanity/full.rs"
cp "$task_repo_root/crates/core/src/profanity/words.rs" "$task_work/new_profanity/words.rs"
cp "$task_repo_root/crates/core/src/protected_names.rs" "$task_work/new_names_full.rs"
python3 - "$task_work" "$task_repo_root/migrations/0035_protected_artist_policy.sql" <<'PY'
import re, sys
work, seed_sql = sys.argv[1], sys.argv[2]
for side in ("old", "new"):
    # Profanity: the pure filter, without the HTTP handlers and tests.
    s = open(f"{work}/{side}_profanity/full.rs").read()
    s = s[: s.index("#[derive(serde::Deserialize)]")].replace("\nmod words;", "\npub mod words;")
    open(f"{work}/{side}_profanity/mod.rs", "w").write(s)
    # Protected names: matching only, without the database loader and tests.
    s = open(f"{work}/{side}_names_full.rs").read()
    s = s.replace("use crate::error::{Error, Result};\n", "").replace("use sqlx::PgConnection;\n", "").replace("use uuid::Uuid;\n", "")
    a, b = s.index("/// Active protected entries"), s.index("/// Every string inside a JSON value")
    s = s[:a]
    open(f"{work}/{side}_names.rs", "w").write(s)
# The production seed list (migration 0035).
s = open(seed_sql).read()
a, b = s.index("INSERT INTO seed_protected VALUES"), s.index("INSERT INTO seed_alias VALUES")
c = s.index(";", b)
q = lambda x: '"' + x.replace("''", "'").replace('"', '\\"') + '"'
prot = re.findall(r"\('((?:[^']|'')*)','(\w+)','(\w+)'", s[a:b])
ali = re.findall(r"\('((?:[^']|'')*)','((?:[^']|'')*)','\w+','(\w+)','(\w+)'\)", s[b:c])
with open(f"{work}/seed.rs", "w") as f:
    f.write("pub const PROT: &[(&str, &str, &str)] = &[" + ",".join(f"({q(n)},{q(m)},{q(x)})" for n, m, x in prot) + "];\n")
    f.write("pub const ALIAS: &[(&str, &str, &str, &str)] = &[" + ",".join(f"({q(n)},{q(al)},{q(m)},{q(x)})" for n, al, m, x in ali) + "];\n")
PY
cp "$task_repo_root/scripts/benchmark-before-after.rs" "$task_work/benchmark.rs"
# shellcheck disable=SC2086
rustc --edition=2024 -O --cap-lints allow "$task_work/benchmark.rs" $task_externs \
    -L "dependency=$task_target_dir/debug/deps" -o "$task_work/benchmark"
"$task_work/benchmark"
