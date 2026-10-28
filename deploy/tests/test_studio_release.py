"""Local Studio release checks; no Docker daemon or credentials."""
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

DEPLOY = Path(__file__).resolve().parents[1]


class StudioReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        shutil.copy(DEPLOY / 'studio-release.sh', self.root)

    def run_release(self, *args):
        return subprocess.run(['bash', str(self.root / 'studio-release.sh'), *args],
                              capture_output=True, text=True)

    def archive(self, unsafe=False):
        archive = self.root / 'studio.tar.gz'
        with tarfile.open(archive, 'w:gz') as tar:
            if unsafe:
                link = tarfile.TarInfo('bad-link')
                link.type = tarfile.SYMTYPE
                link.linkname = '/tmp'
                tar.addfile(link)
            files = [('index.html', b'<script src="/assets/app.js"></script>'),
                     ('assets/app.js', b'void 0;')]
            if unsafe:
                files += [(f'assets/padding-{i}.js', b'') for i in range(4000)]
            for name, content in files:
                info = tarfile.TarInfo(name)
                info.size = len(content)
                tar.addfile(info, io.BytesIO(content))
        return str(archive)

    def test_valid_archive_installs(self):
        result = self.run_release('install', 'abcdef0', self.archive())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(os.readlink(self.root / 'studio/current'), 'releases/abcdef0')

    def test_large_archive_with_link_is_rejected_before_extraction(self):
        result = self.run_release('install', 'abcdef0', self.archive(unsafe=True))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('링크나 특수 파일', result.stderr)
        self.assertFalse((self.root / 'studio/current').exists())
        self.assertFalse((self.root / 'studio/releases/.abcdef0.tmp').exists())

    def test_corrupt_archive_has_clear_error(self):
        archive = self.root / 'broken.tar.gz'
        archive.write_text('not an archive')
        result = self.run_release('install', 'abcdef0', str(archive))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('cannot read archive metadata', result.stderr)

    def test_missing_or_invalid_bun_pin_fails_before_docker(self):
        app = self.root / 'source/web/studio/app'
        app.mkdir(parents=True)
        (app / 'package.json').write_text('{}')
        for version in [None, '', '1.2.3; touch /tmp/should-not-run']:
            with self.subTest(version=version):
                if version is not None:
                    (app.parent / '.bun-version').write_text(version)
                result = self.run_release('build', 'abcdef0', str(self.root / 'source'))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('.bun-version', result.stderr)
                self.assertNotIn('화면 빌드', result.stdout)


if __name__ == '__main__':
    unittest.main()
