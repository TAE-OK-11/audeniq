#!/usr/bin/env python3
"""Exercise orchestration with a fake Docker CLI; never contact a daemon."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

DEPLOY = Path(__file__).resolve().parents[1]
PINNED = 'ghcr.io/test/app@sha256:' + 'a' * 64

DOCKER = '''#!/usr/bin/env python3
import os, sys, json
from pathlib import Path
args = sys.argv[1:]
with open(os.environ['CALLS'], 'a') as log:
    log.write(repr(args) + '\\n')
if args[:2] == ['volume', 'inspect']: sys.exit(1)
if args[:2] == ['image', 'inspect']:
    if '.RepoDigests' in args[args.index('--format') + 1]: print(os.environ.get('RESOLVED_DIGEST', os.environ['PINNED']))
    else: print('amd64')
    sys.exit(0)
if args[0] == 'run':
    if 'sh' in args: print('x86-64')
    sys.exit(0)
if args[0] == 'inspect':
    fmt = args[args.index('--format') + 1]
    if fmt == '{{.Config.Image}}': print(os.environ['PINNED'])
    elif fmt == '{{.RestartCount}}': print('0')
    elif fmt == '{{.State.Status}} {{.RestartCount}}': print('running 0')
    elif fmt == '{{.State.Running}} {{.RestartCount}}': print('true 0')
    else: print('healthy')
    sys.exit(0)
if args[0] == 'compose':
    if 'config' in args:
        if os.environ.get('FAIL_CONFIG') == '1':
            print('test-secret-must-not-appear', file=sys.stderr); sys.exit(1)
        services = {'pgbouncer': {'build': {'context': './pgbouncer'}},
                    'api': {'image': os.environ['PINNED']},
                    'worker': {'image': os.environ['PINNED']},
                    'migrate': {'image': os.environ['PINNED']},
                    'postgres': {'image': 'postgres:18-trixie'},
                    'grants': {'image': 'postgres:18-trixie'}}
        # Model a merged Compose result from the actual selected file list.
        for i, arg in enumerate(args):
            if arg == '-f' and args[i+1] in ['compose.override.yaml', 'compose.tls.yaml']:
                content = Path(args[i+1]).read_text()
                name = 'local-helper' if args[i+1] == 'compose.override.yaml' else 'tls-helper'
                services[name] = {'build': {'context': '.'}} if 'build:' in content else {'image': 'test/helper:ready'}
        print(json.dumps({'services': services})); sys.exit(0)
    if 'build' in args:
        if os.environ.get('FAIL_BUILD') == 'all': sys.exit(1)
        if '--pull' in args and os.environ.get('FAIL_BUILD') == 'pull': sys.exit(1)
    if 'ps' in args: print(args[-1] + '-id')
    if 'exec' in args:
        print('db_check path=direct result=ok')
        if os.environ.get('FAIL_DB') == '1': sys.exit(20)
'''


class DeploySpeedTests(unittest.TestCase):
    def run_deploy(self, override=None, tls=None, ref=PINNED, **extra):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ['deploy.sh', 'compose.production.yaml', 'check-cpu.sh']:
                shutil.copy(DEPLOY / name, root)
            shutil.copytree(DEPLOY / 'pgbouncer', root / 'pgbouncer')
            (root / 'production.env').write_text('AUDENIQ_IMAGE=' + PINNED + '\nPAYOUT_ACCOUNT_KEY=test-only\n')
            (root / 'production.env').chmod(0o600)
            if override is not None:
                (root / 'compose.override.yaml').write_text(override)
            if tls is not None:
                (root / 'compose.tls.yaml').write_text(tls)
                with (root / 'production.env').open('a') as envfile:
                    envfile.write('TLS_BIND=0.0.0.0:8443\nTLS_ACME_DOMAIN=test.invalid\n')
            binpath = root / 'bin'
            binpath.mkdir()
            for name, body in [('docker', DOCKER), ('sleep', '#!/bin/sh\nexit 0\n'),
                               ('uname', '#!/bin/sh\necho x86_64\n')]:
                (binpath / name).write_text(body)
                (binpath / name).chmod(0o755)
            env = dict(os.environ, PATH=str(binpath) + ':' + os.environ['PATH'],
                       CALLS=str(root / 'calls'), PINNED=PINNED, DEPLOY_TIMING='1', **extra)
            initial_env = (root / 'production.env').read_text()
            result = subprocess.run(['bash', str(root / 'deploy.sh'), ref],
                                    env=env, capture_output=True, text=True)
            # Guard failures must not rewrite production.env.
            if result.returncode and ('unsupported buildable' in result.stderr or 'cannot validate Compose' in result.stderr):
                self.assertEqual(initial_env, (root / 'production.env').read_text())
                self.assertNotIn('phase=environment_update', result.stderr)
            return result, (root / 'calls').read_text() if (root / 'calls').exists() else '', (root / 'deploy-history.log').exists()

    def test_invalid_references_fail_before_pull(self):
        for ref in [PINNED + '&x', 'ghcr.io/test/app:tag#oops',
                    'ghcr.io/test/app@sha256:abc', 'ghcr.io/test/app:tag\nBAD=x']:
            with self.subTest(ref=ref):
                result, calls, history = self.run_deploy(ref=ref)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('invalid deployment image reference', result.stderr)
                self.assertNotIn("'pull'", calls)
                self.assertFalse(history)

    def test_explicit_tag_resolves_to_verified_digest(self):
        result, calls, history = self.run_deploy(ref='ghcr.io/test/app:sha-deadbeef')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(history)
        self.assertIn('.RepoDigests', calls)

    def test_invalid_resolved_digest_fails_before_verification(self):
        result, calls, history = self.run_deploy(
            ref='ghcr.io/test/app:sha-deadbeef', RESOLVED_DIGEST=PINNED + '&unsafe')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('invalid resolved image digest', result.stderr)
        self.assertNotIn("'run'", calls)
        self.assertFalse(history)

    def test_dependency_pull_skips_only_verified_image(self):
        import ast
        result, calls, history = self.run_deploy(override='services:\n  local-helper:\n    image: test/helper:ready\n')
        self.assertEqual(result.returncode, 0, result.stderr)
        pulls = [ast.literal_eval(line) for line in calls.splitlines() if "'compose'" in line and "'pull'" in line]
        self.assertEqual(len(pulls), 1)
        self.assertEqual(pulls[0][-3:], ['grants', 'local-helper', 'postgres'])
        for name in ['api', 'worker', 'migrate']:
            self.assertNotIn(name, pulls[0])
        self.assertTrue(history)

    def test_override_build_fails_before_up_or_build(self):
        result, calls, history = self.run_deploy(override='services:\n  local-helper:\n    build: .\n    profiles: [manual]\n')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('local-helper', result.stderr)
        self.assertIn('prepare each image separately', result.stderr)
        self.assertIn('./deploy.sh --rollback', result.stderr)
        self.assertIn("'--profile', '*', 'config', '--format', 'json'", calls)
        self.assertNotIn("'up'", calls)
        self.assertNotIn("'build'", calls)
        self.assertFalse(history)

    def test_override_without_build_uses_both_no_build_calls(self):
        result, calls, history = self.run_deploy(override='services:\n  local-helper:\n    image: test/helper:ready\n')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("'-f', 'compose.override.yaml'", calls)
        self.assertEqual(calls.count("'--no-build'"), 2)
        self.assertTrue(history)

    def test_tls_build_is_also_rejected(self):
        result, calls, history = self.run_deploy(tls='services:\n  tls-helper:\n    build: .\n')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('tls-helper', result.stderr)
        self.assertNotIn("'up'", calls)
        self.assertFalse(history)

    def test_invalid_compose_fails_closed_without_logging_config(self):
        result, calls, history = self.run_deploy(FAIL_CONFIG='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('cannot validate Compose', result.stderr)
        self.assertNotIn('test-secret-must-not-appear', result.stderr + result.stdout)
        self.assertNotIn("'up'", calls)
        self.assertFalse(history)

    def test_single_build_and_combined_verification(self):
        result, calls, history = self.run_deploy()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls.count("'build', '--pull'"), 1)
        self.assertEqual(calls.count("'--no-build'"), 2)
        self.assertEqual(calls.count("'run', '--rm'"), 2)
        self.assertIn('command -v', calls)
        self.assertIn('upload-sandbox-check', calls)
        self.assertIn('phase=api_readiness', result.stderr)
        self.assertIn('exit=0', result.stderr)
        self.assertTrue(history)

    def test_pull_failure_uses_local_build(self):
        result, calls, history = self.run_deploy(FAIL_BUILD='pull')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("'build', '--quiet', 'pgbouncer'", calls)
        self.assertTrue(history)

    def test_build_failure_keeps_rollback_hint_and_stops_before_migrations(self):
        result, calls, history = self.run_deploy(FAIL_BUILD='all')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('./deploy.sh --rollback', result.stderr)
        self.assertIn('exit=1', result.stderr)
        self.assertNotIn("'up'", calls)
        self.assertFalse(history)

    def test_db_failure_blocks_runtime_replacement(self):
        result, calls, history = self.run_deploy(FAIL_DB='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('./deploy.sh --rollback', result.stderr)
        self.assertNotIn("'--remove-orphans'", calls)
        self.assertFalse(history)


if __name__ == '__main__':
    unittest.main()
