#!/usr/bin/env python3
"""Exercise probe routing/classification without a DB, Docker or stored secrets."""
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import unittest

ENTRYPOINT = Path(__file__).resolve().parents[1] / 'pgbouncer/entrypoint.sh'


class ProbeTests(unittest.TestCase):
    def probe(self, mode, failure='', fail_role='', fail_host=''):
        with tempfile.TemporaryDirectory() as directory:
            psql = Path(directory) / 'psql'
            psql.write_text('''#!/bin/sh
host= role=
while [ "$#" -gt 0 ]; do
  case "$1" in -h) host=$2; shift ;; -U) role=$2; shift ;; esac
  shift
done
printf '%s %s\\n' "$host" "$role" >> "$CALL_LOG"
if [ -n "$FAILURE" ] && { [ -z "$FAIL_ROLE" ] || [ "$FAIL_ROLE" = "$role" ]; } &&
   { [ -z "$FAIL_HOST" ] || [ "$FAIL_HOST" = "$host" ]; }; then
  printf '%s\\n' "$FAILURE" >&2
  exit 2
fi
printf '1\\n'
''')
            psql.chmod(0o700)
            log = Path(directory) / 'calls'
            env = dict(os.environ, PATH=directory + ':' + os.environ['PATH'],
                       PGBOUNCER_DB='audeniq_prod', POSTGRES_HOST='postgres',
                       API_DB_PASSWORD=secrets.token_urlsafe(),
                       WORKER_DB_PASSWORD=secrets.token_urlsafe(),
                       FAILURE=failure, FAIL_ROLE=fail_role, FAIL_HOST=fail_host,
                       CALL_LOG=str(log))
            result = subprocess.run(['sh', str(ENTRYPOINT), mode], env=env,
                                    capture_output=True, text=True, timeout=5)
            self.assertNotIn(env['API_DB_PASSWORD'], result.stdout + result.stderr)
            self.assertNotIn(env['WORKER_DB_PASSWORD'], result.stdout + result.stderr)
            self.assertEqual(result.stderr, '')
            return result, log.read_text().splitlines()

    def test_success_checks_both_roles_on_both_paths(self):
        result, calls = self.probe('--healthcheck')
        self.assertEqual(result.returncode, 0)
        self.assertEqual(calls, ['postgres audeniq_api', 'postgres audeniq_worker',
                                 '127.0.0.1 audeniq_api', '127.0.0.1 audeniq_worker'])

    def test_auth_and_configuration_fail_fast_without_raw_detail(self):
        for message, kind, code in [
            ('password authentication failed', 'auth', 20),
            ('SASL authentication failed', 'auth', 20),
            ('no pg_hba.conf entry', 'auth', 20),
            ('role does not exist', 'configuration', 21),
            ('no such database', 'configuration', 21),
        ]:
            with self.subTest(message=message):
                result, calls = self.probe('--healthcheck', message + ' private-detail')
                self.assertEqual(result.returncode, code)
                self.assertEqual(len(calls), 1)
                self.assertIn('path=direct role=audeniq_api kind=' + kind, result.stdout)
                self.assertNotIn('private-detail', result.stdout)

    def test_cached_backend_error_is_retryable(self):
        result, _ = self.probe('--check-pooled', 'cached error: server_login_retry')
        self.assertEqual(result.returncode, 1)
        self.assertIn('kind=backend_login_retry', result.stdout)

    def test_network_failure_is_retryable(self):
        result, _ = self.probe('--check-direct', 'connection refused private-detail')
        self.assertEqual(result.returncode, 1)
        self.assertIn('kind=unreachable_or_timeout', result.stdout)
        self.assertNotIn('private-detail', result.stdout)

    def test_worker_pooled_failure_identifies_exact_path(self):
        result, calls = self.probe('--healthcheck', 'password authentication failed',
                                   'audeniq_worker', '127.0.0.1')
        self.assertEqual(result.returncode, 20)
        self.assertEqual(len(calls), 4)
        self.assertIn('path=pooled role=audeniq_worker kind=auth', result.stdout)


class DeployGateTests(unittest.TestCase):
    def gate(self, results):
        source = (ENTRYPOINT.parents[1] / 'deploy.sh').read_text()
        gate = source[source.index('wait_database() {'):source.index('\nwait_database\n')]
        with tempfile.TemporaryDirectory() as directory:
            steps = Path(directory) / 'steps'
            steps.write_text('\n'.join(str(code) for code in results) + '\n')
            script = r'''set -euo pipefail
die() { echo "$*"; exit 1; }
probe() {
  code=$(head -n 1 "$STEPS")
  sed -i '1d' "$STEPS"
  echo "db_check path=direct role=audeniq_api kind=test"
  return "${code:-1}"
}
COMPOSE=(probe)
sleep() { SECONDS=$((SECONDS + 60)); }
''' + gate + '\nwait_database\n'
            return subprocess.run(['bash', '-c', script],
                                  env=dict(os.environ, STEPS=str(steps)),
                                  capture_output=True, text=True, timeout=5)

    def test_ready(self):
        self.assertEqual(self.gate([0]).returncode, 0)

    def test_transient_failure_recovers(self):
        result = self.gate([1, 0])
        self.assertEqual(result.returncode, 0)
        self.assertIn('DB not ready yet', result.stdout)

    def test_auth_and_config_fail_immediately(self):
        for code in (20, 21):
            result = self.gate([code, 0])
            self.assertEqual(result.returncode, 1)
            self.assertIn('DB login/config broken', result.stdout)
            self.assertIn('--rollback', result.stdout)
            self.assertNotIn('DB not ready yet', result.stdout)

    def test_transient_failure_times_out(self):
        result = self.gate([1, 1, 0])
        self.assertEqual(result.returncode, 1)
        self.assertIn('DB readiness timeout', result.stdout)
        self.assertIn('--rollback', result.stdout)


if __name__ == '__main__':
    unittest.main()
