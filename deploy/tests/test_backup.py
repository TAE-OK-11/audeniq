import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

DEPLOY = Path(__file__).resolve().parents[1]


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
