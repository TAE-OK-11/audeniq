import base64
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

DEPLOY = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('kms_keyring', DEPLOY / 'kms-keyring.py')
kms = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kms)
ARN = 'arn:aws:kms:ap-northeast-1:123456789012:key/12345678-1234-1234-1234-123456789abc'


class KeyringTests(unittest.TestCase):
    def test_private_bundle_write_keeps_the_current_users_group(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'wrapped.json'
            kms.atomic_write(destination, {'ciphertext': 'YQ=='})
            self.assertEqual(destination.stat().st_uid, os.geteuid())
            self.assertEqual(destination.stat().st_gid, os.getegid())
            self.assertEqual(destination.stat().st_mode & 0o777, 0o600)

    def test_private_inputs_and_tls_pinned_sdk_request(self):
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            path = Path(directory) / 'legacy'
            path.write_text('11' * 32)
            path.chmod(0o644)
            with self.assertRaises(ValueError):
                kms.private_read(path)
            path.chmod(0o600)
            self.assertEqual(kms.private_read(path), b'11' * 32)
            secret = base64.b64encode(bytes.fromhex('11' * 32)).decode()
            def sdk(command, **options):
                self.assertIn('https://kms.ap-northeast-1.amazonaws.com', command)
                self.assertNotIn('--no-verify-ssl', command)
                self.assertNotIn(secret, ' '.join(command))
                request_path = Path(command[command.index('--cli-input-json') + 1][7:])
                self.assertEqual(json.loads(request_path.read_text())['Plaintext'], secret)
                self.assertEqual(request_path.stat().st_mode & 0o077, 0)
                return subprocess.CompletedProcess(command, 0, json.dumps({'KeyId': ARN, 'CiphertextBlob': 'YQ=='}).encode())
            with patch.object(kms.subprocess, 'run', side_effect=sdk):
                kms.aws_kms('encrypt', ARN, {'Plaintext': secret}, directory)
            self.assertEqual(list(Path(directory).glob('.kms-*')), [])

    def test_materialization_is_atomic_and_context_contains_no_personal_data(self):
        bundle = {'version': 1, 'active_version': 2, 'keys': {
            '1': {'key_id': ARN, 'ciphertext': 'YQ=='}, '2': {'key_id': ARN, 'ciphertext': 'Yg=='}}}
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            destination = Path(directory) / 'keyring.json'
            destination.write_text('previous usable keyring')
            def sdk(operation, arn, request, scratch):
                self.assertEqual(set(request['EncryptionContext']), {'service', 'purpose', 'version'})
                if request['CiphertextBlob'] == 'Yg==':
                    raise ValueError('unavailable KMS')
                return {'Plaintext': base64.b64encode(b'1' * 32).decode()}
            with patch.object(kms, 'aws_kms', side_effect=sdk), self.assertRaises(ValueError):
                kms.materialize(bundle, directory, destination, os.getegid())
            self.assertEqual(destination.read_text(), 'previous usable keyring')
            with patch.object(kms, 'aws_kms', return_value={'Plaintext': base64.b64encode(b'2' * 32).decode()}):
                kms.materialize(bundle, directory, destination, os.getegid())
            self.assertEqual(json.loads(destination.read_text())['active_version'], 2)
            self.assertEqual(destination.stat().st_gid, os.getegid())
            self.assertEqual(destination.stat().st_mode & 0o777, 0o640 if os.getegid() else 0o600)

    def test_rejects_aliases_plaintext_bundles_and_non_tmpfs(self):
        for invalid in ['alias/audeniq', 'http://kms.example', ARN.replace(':key/', ':alias/')]:
            with self.assertRaises(ValueError):
                kms.key_region(invalid)
        with self.assertRaises(ValueError):
            kms.validate_bundle({'version': 1, 'active_version': 1, 'keys': {'1': {'key': '11' * 32}}})
        with tempfile.TemporaryDirectory(dir=Path.home()) as directory:
            with self.assertRaises(ValueError):
                kms.secure_directory(directory, os.geteuid(), os.getegid())


@unittest.skipUnless(shutil.which('age') and shutil.which('age-keygen'), 'age binaries required')
class BackupTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        identity = self.root / 'identity'
        subprocess.run(['age-keygen', '-o', str(identity)], check=True, capture_output=True, text=True)
        recipient = subprocess.run(['age-keygen', '-y', str(identity)], check=True, capture_output=True, text=True).stdout
        recipients = self.root / 'recipients'; recipients.write_text(recipient)
        self.payload = b'PGDMP-test-personal-data-secret@example.test-account-1234567890'
        (self.root / 'payload').write_bytes(self.payload)
        docker = self.root / 'docker'
        docker.write_text('''#!/usr/bin/env python3
import os,sys
from pathlib import Path
root=Path(os.environ['TEST_ROOT'])
with (root/'calls').open('a') as f: f.write(' '.join(sys.argv[1:])+'\\n')
if 'pg_dump' in sys.argv:
 sys.stdout.buffer.write((root/'payload').read_bytes())
 sys.exit(int(os.environ.get('DUMP_FAIL','0')))
if 'pg_restore' in sys.argv:
 assert sys.stdin.buffer.read()==(root/'payload').read_bytes()
 sys.exit(0)
if 'psql' in sys.argv: print('0')
''')
        docker.chmod(0o700)
        self.env = dict(os.environ, PATH=str(self.root) + ':' + os.environ['PATH'], TEST_ROOT=str(self.root),
                        BACKUP_DIR=str(self.root / 'backups'), BACKUP_RECIPIENTS_FILE=str(recipients),
                        BACKUP_IDENTITY_FILE=str(identity), CONTAINER='isolated-test-container')

    def run_script(self, name, *args, **env):
        return subprocess.run(['bash', str(DEPLOY / name), *args], env=dict(self.env, **env), capture_output=True)

    def test_encrypted_backup_roundtrip_and_verify_do_not_restore(self):
        self.assertEqual(self.run_script('backup.sh').returncode, 0)
        backups = list((self.root / 'backups').glob('*.dump.age'))
        self.assertEqual(len(backups), 1)
        self.assertNotIn(self.payload, backups[0].read_bytes())
        plaintext = subprocess.run(['age', '-d', '-i', self.env['BACKUP_IDENTITY_FILE'], str(backups[0])], check=True, capture_output=True).stdout
        self.assertEqual(plaintext, self.payload)
        self.assertEqual(self.run_script('restore-backup.sh', str(backups[0]), '--verify').returncode, 0)
        self.assertNotIn('psql', (self.root / 'calls').read_text())

    def test_dump_failure_leaves_no_complete_or_partial_dump(self):
        self.assertNotEqual(self.run_script('backup.sh', DUMP_FAIL='42').returncode, 0)
        self.assertEqual(list((self.root / 'backups').iterdir()), [])

    def test_invalid_recipient_refuses_before_database_access(self):
        self.assertNotEqual(self.run_script('backup.sh', BACKUP_RECIPIENTS_FILE=str(self.root / 'missing')).returncode, 0)
        self.assertFalse((self.root / 'calls').exists())

    def test_corrupt_backup_and_production_restore_are_rejected(self):
        bad = self.root / 'bad.dump.age'; bad.write_bytes(b'corrupt')
        self.assertNotEqual(self.run_script('restore-backup.sh', str(bad), '--restore-empty').returncode, 0)
        self.assertEqual(self.run_script('backup.sh').returncode, 0)
        backup = next((self.root / 'backups').glob('*.dump.age'))
        self.assertNotEqual(self.run_script('restore-backup.sh', str(backup), '--restore-empty', TARGET_DB='audeniq_prod').returncode, 0)
        self.assertNotIn('psql', (self.root / 'calls').read_text())


if __name__ == '__main__':
    unittest.main()
