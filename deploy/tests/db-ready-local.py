#!/usr/bin/env python3
"""Real disposable Docker verification. Run: python3 deploy/tests/db-ready-local.py.
Requires Docker and image/package network access. Publishes no ports, uses no
host volumes or production configuration. Removes all test containers/network.
"""
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def main():
    if not shutil.which('docker'):
        raise SystemExit('PENDING: docker is not installed; no real checks ran')
    subprocess.run(['docker', 'info'], check=True, stdout=subprocess.DEVNULL)
    prefix = 'audeniq-db-test-' + secrets.token_hex(6)
    network, pg, pool, image = [prefix + s for s in ('-net', '-pg', '-pool', '-image')]
    api, worker, owner, wrong = [secrets.token_hex(24) for _ in range(4)]

    def docker(*args, check=True):
        return subprocess.run(['docker', *args], check=check, capture_output=True, text=True)

    def expect(result, code, label):
        # Never relay raw container/SQL errors or random credentials.
        if result.returncode != code or label not in result.stdout:
            raise AssertionError(f'expected exit={code} label={label}; got exit={result.returncode}')
        print(f'PASS exit={code} {label}')

    try:
        docker('build', '-t', image, str(ROOT / 'deploy/pgbouncer'))
        docker('network', 'create', network, '--internal')
        with tempfile.TemporaryDirectory() as directory:
            envfile = Path(directory) / 'postgres.env'
            envfile.write_text(f'POSTGRES_PASSWORD={owner}\nPOSTGRES_DB=probe_test\n')
            envfile.chmod(0o600)
            docker('run', '-d', '--name', pg, '--network', network, '--env-file', str(envfile), 'postgres:17')
            for _ in range(60):
                if docker('exec', pg, 'pg_isready', '-U', 'postgres', check=False).returncode == 0:
                    break
                time.sleep(1)
            else:
                raise AssertionError('throwaway postgres startup timeout')
            def sql(statement):
                r = subprocess.run(['docker', 'exec', '-i', pg, 'psql', '-U', 'postgres', '-d', 'probe_test', '-v', 'ON_ERROR_STOP=1'], input=statement, capture_output=True, text=True)
                if r.returncode:
                    raise AssertionError('throwaway SQL failed (details suppressed)')
            sql(f"CREATE ROLE audeniq_api LOGIN PASSWORD '{api}'; CREATE ROLE audeniq_worker LOGIN PASSWORD '{worker}';")
            envfile.write_text(f'PGBOUNCER_DB=probe_test\nPOSTGRES_HOST={pg}\nAPI_DB_PASSWORD={api}\nWORKER_DB_PASSWORD={worker}\n')
            docker('run', '-d', '--name', pool, '--network', network, '--env-file', str(envfile), image)
            def probe(mode, *env):
                return docker('exec', *env, pool, '/usr/local/bin/pgbouncer-entrypoint', mode, check=False)
            for _ in range(30):
                if probe('--healthcheck').returncode == 0:
                    break
                time.sleep(1)
            else:
                raise AssertionError('throwaway pool startup timeout')
            for mode, label in [('--check-direct', 'path=direct kind=ready'), ('--check-pooled', 'path=pooled kind=ready'), ('--healthcheck', 'path=pooled kind=ready')]:
                expect(probe(mode), 0, label)
            # Run the exact deploy function with a real docker exec, no deploy actions.
            source = (ROOT / 'deploy/deploy.sh').read_text()
            gate = source[source.index('wait_database() {'):source.index('\nwait_database\n')]
            def run_gate(bad=False):
                args = ['docker', 'exec'] + (['-e', f'API_DB_PASSWORD={wrong}'] if bad else []) + [pool]
                import shlex
                script = 'set -euo pipefail\ndie() { echo "$*"; exit 1; }\nprobe() { shift 3; ' + shlex.join(args) + ' "$@"; }\nCOMPOSE=(probe)\n' + gate + '\nwait_database\n'
                return subprocess.run(['bash', '-c', script], capture_output=True, text=True, timeout=135)
            expect(run_gate(), 0, 'path=pooled kind=ready')
            expect(run_gate(True), 1, 'DB login/config broken')
            for mode in ('--check-direct', '--check-pooled', '--healthcheck'):
                expect(probe(mode, '-e', f'API_DB_PASSWORD={wrong}'), 20, 'kind=auth')
            expect(probe('--check-direct', '-e', 'POSTGRES_PORT=1'), 1, 'kind=unreachable_or_timeout')
            # Keep backend login failures cached long enough for a deterministic probe.
            docker('exec', pool, 'sh', '-ec', "sed -i 's/server_login_retry = 2/server_login_retry = 60/' /run/pgbouncer/pgbouncer.ini; kill -HUP 1")
            time.sleep(1)
            sql(f"ALTER ROLE audeniq_api PASSWORD '{wrong}'; SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE usename='audeniq_api';")
            expect(probe('--check-direct'), 20, 'kind=auth')
            # A terminated idle server can first report shutdown; keep probing
            # until PgBouncer has attempted and cached the new backend login.
            for _ in range(10):
                cached = probe('--check-pooled')
                if cached.returncode == 1 and 'kind=backend_login_retry' in cached.stdout:
                    break
                time.sleep(0.1)
            expect(cached, 1, 'kind=backend_login_retry')
            print('PASS real PostgreSQL/PgBouncer probes and deploy gate')
    finally:
        for name in (pool, pg):
            docker('rm', '-f', '-v', name, check=False)
        docker('network', 'rm', network, check=False)
        docker('image', 'rm', image, check=False)


if __name__ == '__main__':
    main()
