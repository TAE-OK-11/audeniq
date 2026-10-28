#!/usr/bin/env python3
"""Local-only security regressions: no Docker, AWS, credentials or real releases."""
import fcntl
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

DEPLOY = Path(__file__).resolve().parents[1]


class SecurityTests(unittest.TestCase):
    def test_pooler_rejects_injection_without_echoing_secrets(self):
        base = dict(os.environ, PGBOUNCER_DB='test', POSTGRES_HOST='postgres',
                    API_DB_PASSWORD='test-only-value', WORKER_DB_PASSWORD='test-only-value')
        cases = [('PGBOUNCER_DB', 'test\n[pgbouncer]'),
                 ('POSTGRES_HOST', 'postgres auth_type=trust'),
                 ('PGBOUNCER_POOL_SIZE', '8\nauth_type=trust'),
                 ('PGBOUNCER_MAX_CLIENT_CONN', 'not-a-number-marker'),
                 ('API_DB_PASSWORD', 'private-value\n"injected" "password"'),
                 ('WORKER_DB_PASSWORD', 'private-value\rmore')]
        for key, value in cases:
            with self.subTest(key=key):
                result = subprocess.run(['sh', str(DEPLOY / 'pgbouncer/entrypoint.sh'), '--healthcheck'],
                                        env=dict(base, **{key: value}), capture_output=True, text=True)
                self.assertEqual(result.returncode, 21)
                self.assertNotIn(value, result.stdout + result.stderr)
                self.assertNotIn('private-value', result.stdout + result.stderr)

    def test_deploy_rejects_public_env_file_before_docker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copy(DEPLOY / 'deploy.sh', root)
            envfile = root / 'production.env'
            envfile.write_text('')
            envfile.chmod(0o644)
            result = subprocess.run(['bash', str(root / 'deploy.sh'), '--status'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('0600', result.stderr)

    def test_deploy_rejects_env_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copy(DEPLOY / 'deploy.sh', root)
            (root / 'private.env').write_text('')
            (root / 'production.env').symlink_to('private.env')
            result = subprocess.run(['bash', str(root / 'deploy.sh'), '--status'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('symlink', result.stderr)

    def test_release_is_atomic_and_serialized(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / 'studio-release.sh'
            shutil.copy(DEPLOY / script.name, script)
            archive = root / 'release.tar.gz'
            with tarfile.open(archive, 'w:gz') as tar:
                for name, body in [('index.html', b'<script src="/assets/app-Ab12_cdE.js"></script>'),
                                   ('assets/app-Ab12_cdE.js', b'console.log(1)')]:
                    entry = tarfile.TarInfo(name)
                    entry.size = len(body)
                    tar.addfile(entry, io.BytesIO(body))
            command = ['bash', str(script), 'install', 'a' * 40, str(archive)]
            first = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            self.assertEqual((root / 'studio/current').readlink(), Path('releases/' + 'a' * 40))
            with (root / 'studio/.release.lock').open('w') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                blocked = subprocess.run(command, capture_output=True, text=True)
                self.assertNotEqual(blocked.returncode, 0)
                self.assertIn('another Studio release', blocked.stderr)
            invalid = subprocess.run(command, env=dict(os.environ, STUDIO_KEEP_RELEASES='0'), capture_output=True, text=True)
            self.assertNotEqual(invalid.returncode, 0)
            self.assertIn('positive', invalid.stderr)


if __name__ == '__main__':
    unittest.main()
