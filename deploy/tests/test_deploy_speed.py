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
import os, sys
from pathlib import Path
args = sys.argv[1:]
with open(os.environ['CALLS'], 'a') as log:
    log.write(repr(args) + '\\n')
if args[:2] == ['volume', 'inspect']: sys.exit(1)
if args[:2] == ['image', 'inspect']: print('amd64'); sys.exit(0)
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
    if 'build' in args:
        if os.environ.get('FAIL_BUILD') == 'all': sys.exit(1)
        if '--pull' in args and os.environ.get('FAIL_BUILD') == 'pull': sys.exit(1)
    if 'ps' in args: print(args[-1] + '-id')
    if 'exec' in args:
        print('db_check path=direct result=ok')
        if os.environ.get('FAIL_DB') == '1': sys.exit(20)
'''


class DeploySpeedTests(unittest.TestCase):
    def run_deploy(self, **extra):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ['deploy.sh', 'compose.production.yaml', 'check-cpu.sh']:
                shutil.copy(DEPLOY / name, root)
            shutil.copytree(DEPLOY / 'pgbouncer', root / 'pgbouncer')
            (root / 'production.env').write_text('AUDENIQ_IMAGE=' + PINNED + '\nPAYOUT_ACCOUNT_KEY=test-only\n')
            (root / 'production.env').chmod(0o600)
            binpath = root / 'bin'
            binpath.mkdir()
            for name, body in [('docker', DOCKER), ('sleep', '#!/bin/sh\nexit 0\n'),
                               ('uname', '#!/bin/sh\necho x86_64\n')]:
                (binpath / name).write_text(body)
                (binpath / name).chmod(0o755)
            env = dict(os.environ, PATH=str(binpath) + ':' + os.environ['PATH'],
                       CALLS=str(root / 'calls'), PINNED=PINNED, DEPLOY_TIMING='1', **extra)
            result = subprocess.run(['bash', str(root / 'deploy.sh'), PINNED],
                                    env=env, capture_output=True, text=True)
            return result, (root / 'calls').read_text(), (root / 'deploy-history.log').exists()

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
