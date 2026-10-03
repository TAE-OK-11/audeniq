#!/usr/bin/env bash
# Disposable CI signature database; contains only the harmless EICAR pattern.
set -euo pipefail
av_test_dir=/tmp/audeniq-ci-antivirus
mkdir -p "$av_test_dir/database" "$av_test_dir/socket"
python3 - "$av_test_dir/database/audeniq.ndb" <<'PY'
import pathlib, sys
pattern = 'X5O!P%@AP[4' + '\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*'
pathlib.Path(sys.argv[1]).write_text('Audeniq.EICAR.Test:0:*:' + pattern.encode().hex() + '\n')
PY
sed -e "s|DatabaseDirectory .*|DatabaseDirectory $av_test_dir/database|" \
    -e "s|LocalSocket .*|LocalSocket $av_test_dir/socket/real.sock|" \
    -e "s|TemporaryDirectory .*|TemporaryDirectory $av_test_dir|" \
    deploy/clamd.conf > "$av_test_dir/clamd.conf"
clamd --foreground --config-file="$av_test_dir/clamd.conf" > "$av_test_dir/clamd.log" 2>&1 &
for _ in $(seq 1 30); do
    [ ! -S "$av_test_dir/socket/real.sock" ] || break
    sleep 1
done
[ -S "$av_test_dir/socket/real.sock" ] || { cat "$av_test_dir/clamd.log"; exit 1; }
python3 deploy/tests/serve_test_antivirus.py /tmp/audeniq-ci-av.sock "$av_test_dir/socket/real.sock" \
    > "$av_test_dir/relay.log" 2>&1 &
for _ in $(seq 1 20); do [ ! -S /tmp/audeniq-ci-av.sock ] || exit 0; sleep 0.1; done
exit 1
