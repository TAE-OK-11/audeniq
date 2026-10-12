import base64
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

DEPLOY = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('secret_manager', DEPLOY / 'secret-manager.py')
sm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sm)

AWS = 'arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db-AbCdEf'
GCP = 'projects/audeniq-prod/secrets/edge-service-secret'
OCI = 'ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexamplesecret0123'


def manifest(*entries):
    return {'version': 1, 'secrets': list(entries)}


class ManifestTests(unittest.TestCase):
    def test_valid_manifest_with_all_providers(self):
        sm.validate_manifest(manifest(
            {'env': 'POSTGRES_PASSWORD', 'provider': 'aws', 'secret_id': AWS, 'json_key': 'password'},
            {'env': 'EDGE_SERVICE_SECRET', 'provider': 'gcp', 'secret_id': GCP, 'version': '3'},
            {'file': 'payout-keyring.json', 'provider': 'oci', 'secret_id': OCI, 'version': 'CURRENT'},
        ))

    def test_rejects_unpinned_ids_reserved_names_and_ambiguous_targets(self):
        bad = [
            {'env': 'A', 'provider': 'aws', 'secret_id': 'audeniq/db'},                       # name, not ARN
            {'env': 'A', 'provider': 'aws', 'secret_id': AWS.replace('secretsmanager', 'kms')},
            {'env': 'A', 'provider': 'gcp', 'secret_id': GCP + '/versions/1'},
            {'env': 'A', 'provider': 'oci', 'secret_id': OCI.replace('vaultsecret', 'key')},
            {'env': 'A', 'provider': 'azure', 'secret_id': AWS},
            {'env': 'AUDENIQ_IMAGE', 'provider': 'aws', 'secret_id': AWS},
            {'env': 'lower', 'provider': 'aws', 'secret_id': AWS},
            {'env': 'A', 'file': 'x', 'provider': 'aws', 'secret_id': AWS},
            {'provider': 'aws', 'secret_id': AWS},
            {'file': '../escape', 'provider': 'aws', 'secret_id': AWS},
            {'file': 'secrets.env', 'provider': 'aws', 'secret_id': AWS},
            {'env': 'A', 'provider': 'gcp', 'secret_id': GCP, 'version': 'AWSCURRENT'},
            {'env': 'A', 'provider': 'aws', 'secret_id': AWS, 'extra': 1},
        ]
        for entry in bad:
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                sm.validate_manifest(manifest(entry))
        dup = {'env': 'A', 'provider': 'aws', 'secret_id': AWS}
        with self.assertRaises(ValueError):
            sm.validate_manifest(manifest(dup, dict(dup, provider='gcp', secret_id=GCP)))


class MaterializeTests(unittest.TestCase):
    def setUp(self):
        self.values = {
            AWS: json.dumps({'host': '10.0.0.5', 'username': 'audeniq_owner', 'password': 'p$w"d#x y'}).encode(),
            GCP: b'edge-secret-value',
            OCI: b'{"version":1,"active_version":1,"keys":{"1":"' + b'ab' * 32 + b'"}}',
        }
        def fetch(secret_id, version, scratch):
            value = self.values[secret_id]
            if isinstance(value, Exception):
                raise value
            return value
        self.patch = patch.dict(sm.FETCH, {'aws': fetch, 'gcp': fetch, 'oci': fetch})
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.manifest = manifest(
            {'env': 'DB_HOST', 'provider': 'aws', 'secret_id': AWS, 'json_key': 'host'},
            {'env': 'POSTGRES_PASSWORD', 'provider': 'aws', 'secret_id': AWS, 'json_key': 'password'},
            {'env': 'EDGE_SERVICE_SECRET', 'provider': 'gcp', 'secret_id': GCP},
            {'file': 'payout-keyring.json', 'provider': 'oci', 'secret_id': OCI},
        )

    def test_env_file_is_literal_for_compose_and_key_file_is_group_readable(self):
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            self.assertEqual(sm.materialize(self.manifest, directory, os.getegid()), (3, 1))
            env = Path(directory) / 'secrets.env'
            self.assertEqual(env.read_text(), "DB_HOST='10.0.0.5'\nPOSTGRES_PASSWORD='p$w\"d#x y'\nEDGE_SERVICE_SECRET='edge-secret-value'\n")
            self.assertEqual(env.stat().st_mode & 0o777, 0o600)
            key = Path(directory) / 'payout-keyring.json'
            self.assertEqual(key.read_bytes(), self.values[OCI])
            self.assertEqual(key.stat().st_mode & 0o777, 0o640)
            if shutil.which('docker'):
                compose = Path(directory) / 'c.yaml'
                compose.write_text('services:\n  x:\n    image: busybox\n    environment:\n      P: ${POSTGRES_PASSWORD}\n')
                out = subprocess.run(['docker', 'compose', '-f', str(compose), '--env-file', str(env), 'config', '--format', 'json'],
                                     capture_output=True, text=True)
                if out.returncode == 0:
                    # `config` re-escapes a literal $ as $$; no interpolation happened.
                    self.assertEqual(json.loads(out.stdout)['services']['x']['environment']['P'], 'p$$w"d#x y')

    def test_one_failed_secret_replaces_nothing(self):
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            env = Path(directory) / 'secrets.env'; env.write_text('previous')
            self.values[GCP] = ValueError('denied')
            with self.assertRaises(ValueError):
                sm.materialize(self.manifest, directory, os.getegid())
            self.assertEqual(env.read_text(), 'previous')
            self.assertFalse((Path(directory) / 'payout-keyring.json').exists())
            self.assertEqual(list(Path(directory).glob('.secret-*')), [])

    def test_unrepresentable_or_missing_values_refuse(self):
        for value, entry in [
            (b"has'quote", {'env': 'A', 'provider': 'gcp', 'secret_id': GCP}),
            (b'two\nlines', {'env': 'A', 'provider': 'gcp', 'secret_id': GCP}),
            (b'', {'env': 'A', 'provider': 'gcp', 'secret_id': GCP}),
            (b'not json', {'env': 'A', 'provider': 'gcp', 'secret_id': GCP, 'json_key': 'password'}),
            (b'{"password": {"nested": 1}}', {'env': 'A', 'provider': 'gcp', 'secret_id': GCP, 'json_key': 'password'}),
            (b'{"user": "x"}', {'env': 'A', 'provider': 'gcp', 'secret_id': GCP, 'json_key': 'password'}),
        ]:
            self.values[GCP] = value
            with self.subTest(value=value), tempfile.TemporaryDirectory(dir='/dev/shm') as directory, self.assertRaises(ValueError):
                sm.materialize(manifest(entry), directory, os.getegid())


class CliTests(unittest.TestCase):
    """Each provider: official CLI, no secret in argv, pinned id checked in the response."""
    def capture(self, stdout, call, returncode=0):
        seen = []
        def run(args, **options):
            entry = {'args': args}
            for arg in args:
                if arg.startswith('file://'):
                    path = Path(arg.removeprefix('file://'))
                    entry['request'] = json.loads(path.read_text())
                    entry['mode'] = path.stat().st_mode & 0o777
            if '--data-file' in args:
                path = Path(args[args.index('--data-file') + 1])
                entry['data'] = path.read_bytes()
                entry['mode'] = path.stat().st_mode & 0o777
            seen.append(entry)
            self.assertTrue(options['capture_output'])
            return subprocess.CompletedProcess(args, returncode, stdout)
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory, patch.object(sm.subprocess, 'run', side_effect=run):
            try:
                return call(directory), seen
            finally:
                self.assertEqual(list(Path(directory).iterdir()), [])

    def test_aws_read_pins_region_endpoint_and_arn(self):
        out = json.dumps({'ARN': AWS, 'SecretString': 'v1'}).encode()
        value, seen = self.capture(out, lambda d: sm.aws_fetch(AWS, 'AWSCURRENT', d))
        self.assertEqual(value, b'v1')
        args = seen[0]['args']
        self.assertEqual(args[:3], ['aws', 'secretsmanager', 'get-secret-value'])
        self.assertIn('https://secretsmanager.ap-northeast-2.amazonaws.com', args)
        self.assertEqual(seen[0]['request'], {'SecretId': AWS, 'VersionStage': 'AWSCURRENT'})
        binary = json.dumps({'ARN': AWS, 'SecretBinary': base64.b64encode(b'\x00\x01').decode()}).encode()
        self.assertEqual(self.capture(binary, lambda d: sm.aws_fetch(AWS, 'AWSCURRENT', d))[0], b'\x00\x01')
        with self.assertRaises(ValueError):
            self.capture(json.dumps({'ARN': AWS + 'x', 'SecretString': 'v'}).encode(), lambda d: sm.aws_fetch(AWS, 'AWSCURRENT', d))

    def test_aws_put_sends_value_by_file_only(self):
        _, seen = self.capture(json.dumps({'ARN': AWS}).encode(), lambda d: sm.aws_put(AWS, b'new-secret', d))
        self.assertNotIn('new-secret', ' '.join(seen[0]['args']))
        self.assertEqual(seen[0]['request'], {'SecretId': AWS, 'SecretString': 'new-secret'})
        self.assertEqual(seen[0]['mode'], 0o600)
        _, seen = self.capture(json.dumps({'ARN': AWS}).encode(), lambda d: sm.aws_put(AWS, b'\xff\x00', d))
        self.assertEqual(seen[0]['request']['SecretBinary'], base64.b64encode(b'\xff\x00').decode())

    def test_gcp_read_checks_secret_name_and_crc32c(self):
        data = b'edge'
        ok = {'name': 'projects/123456/secrets/edge-service-secret/versions/4',
              'payload': {'data': base64.b64encode(data).decode(), 'dataCrc32c': str(sm.crc32c(data))}}
        value, seen = self.capture(json.dumps(ok).encode(), lambda d: sm.gcp_fetch(GCP, 'latest', d))
        self.assertEqual(value, data)
        self.assertEqual(seen[0]['args'][:5], ['gcloud', 'secrets', 'versions', 'access', 'latest'])
        self.assertIn('audeniq-prod', seen[0]['args'])
        for bad in [dict(ok, name='projects/1/secrets/other/versions/4'),
                    dict(ok, payload=dict(ok['payload'], dataCrc32c='1')),
                    dict(ok, payload={'data': ok['payload']['data']})]:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                self.capture(json.dumps(bad).encode(), lambda d: sm.gcp_fetch(GCP, 'latest', d))

    def test_gcp_put_uses_private_data_file(self):
        out = json.dumps({'name': 'projects/1/secrets/edge-service-secret/versions/5'}).encode()
        _, seen = self.capture(out, lambda d: sm.gcp_put(GCP, b'rotated', d))
        self.assertEqual((seen[0]['data'], seen[0]['mode']), (b'rotated', 0o600))
        self.assertNotIn('rotated', ' '.join(seen[0]['args']))

    def test_oci_read_checks_secret_id_and_content_type(self):
        ok = {'data': {'secret-id': OCI, 'secret-bundle-content': {'content-type': 'BASE64', 'content': base64.b64encode(b'k').decode()}}}
        value, seen = self.capture(json.dumps(ok).encode(), lambda d: sm.oci_fetch(OCI, 'CURRENT', d))
        self.assertEqual(value, b'k')
        self.assertEqual(seen[0]['args'][:4], ['oci', 'secrets', 'secret-bundle', 'get'])
        for bad in [{'data': dict(ok['data'], **{'secret-id': OCI + 'x'})},
                    {'data': dict(ok['data'], **{'secret-bundle-content': {'content-type': 'TEXT', 'content': 'k'}})},
                    {'data': []}]:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                self.capture(json.dumps(bad).encode(), lambda d: sm.oci_fetch(OCI, 'CURRENT', d))

    def test_oci_put_sends_base64_value_by_file_only(self):
        _, seen = self.capture(json.dumps({'data': {'id': OCI}}).encode(), lambda d: sm.oci_put(OCI, b'rotated', d))
        self.assertEqual(seen[0]['args'][:4], ['oci', 'vault', 'secret', 'update-base64'])
        self.assertEqual(seen[0]['request']['secretContentContent'], base64.b64encode(b'rotated').decode())
        self.assertNotIn(base64.b64encode(b'rotated').decode(), ' '.join(seen[0]['args']))

    def test_cli_failure_and_oversized_output_never_echo(self):
        for stdout, code in [(b'secret-in-debug-output', 1), (b'a' * (4 * sm.MAX_VALUE + 1), 0), (b'not json', 0)]:
            with self.subTest(code=code):
                with self.assertRaises(ValueError) as error:
                    self.capture(stdout, lambda d: sm.gcp_fetch(GCP, 'latest', d), code)
                self.assertNotIn('secret-in-debug-output', str(error.exception))


class CommandTests(unittest.TestCase):
    def cli(self, directory, *options):
        argv = ['secret-manager.py', *options, '--manifest', str(Path(directory) / 'manifest.json'),
                '--runtime-dir', str(Path(directory) / 'run'), '--env-dir', str(Path(directory) / 'env'),
                '--gid', str(os.getegid())]
        out, err = io.StringIO(), io.StringIO()
        with patch('sys.argv', argv), redirect_stdout(out), redirect_stderr(err):
            try:
                sm.main()
                code = 0
            except SystemExit as exit:
                code = exit.code
        return code, out.getvalue() + err.getvalue()

    def test_put_then_materialize_and_failures_print_no_value(self):
        store = {}
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            path = Path(directory) / 'manifest.json'
            path.write_text(json.dumps(manifest({'env': 'API_DB_PASSWORD', 'provider': 'oci', 'secret_id': OCI})))
            path.chmod(0o600)
            value = Path(directory) / 'value'; value.write_text('fresh-password\n'); value.chmod(0o600)
            with patch.dict(sm.PUT, {'oci': lambda sid, v, s: store.__setitem__(sid, v)}), \
                 patch.dict(sm.FETCH, {'oci': lambda sid, ver, s: store[sid]}):
                self.assertEqual(self.cli(directory, 'put', '--target', 'API_DB_PASSWORD', '--value-file', str(value))[0], 0)
                self.assertEqual(store[OCI], b'fresh-password')
                code, output = self.cli(directory, 'materialize')
                self.assertEqual(code, 0, output)
                self.assertNotIn('fresh-password', output)
                self.assertEqual((Path(directory) / 'env/secrets.env').read_text(), "API_DB_PASSWORD='fresh-password'\n")
                self.assertEqual((Path(directory) / 'env').stat().st_mode & 0o777, 0o700)
                self.assertFalse((Path(directory) / 'run/secrets.env').exists())
                value.chmod(0o644)
                code, output = self.cli(directory, 'put', '--target', 'API_DB_PASSWORD', '--value-file', str(value))
                self.assertEqual(code, 1)
                self.assertNotIn('fresh-password', output)
            path.chmod(0o644)
            self.assertEqual(self.cli(directory, 'check')[0], 1)


class DeployIntegrationTests(unittest.TestCase):
    def probe(self, secrets_line, prepare=None):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory(dir='/dev/shm') as shm:
            task = Path(directory)
            shutil.copy(DEPLOY / 'deploy.sh', task)
            (task / 'pgbouncer').mkdir(); (task / 'pgbouncer/Dockerfile').touch()
            (task / 'compose.production.yaml').touch(); (task / 'check-cpu.sh').touch()
            secrets = Path(shm) / 'secrets.env'
            (task / 'production.env').write_text('AUDENIQ_IMAGE=ghcr.io/test/a@sha256:x\n' + secrets_line.format(secrets=secrets))
            if prepare:
                prepare(secrets)
            (task / 'docker').write_text('#!/usr/bin/env python3\nimport os,sys,json\n'
                                         "open(os.environ['LOG'],'a').write(json.dumps(sys.argv[1:])+'\\n')\n"
                                         "sys.exit(1 if sys.argv[1:3]==['volume','inspect'] else 0)\n")
            (task / 'docker').chmod(0o755)
            log = task / 'log'
            result = subprocess.run(['bash', str(task / 'deploy.sh'), '--status'], capture_output=True, text=True,
                                    env=dict(os.environ, PATH=f'{task}{os.pathsep}{os.environ["PATH"]}', LOG=str(log)))
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return result, [c for c in calls if c[:1] == ['compose']], secrets

    def test_without_secret_manager_compose_reads_only_production_env(self):
        result, compose, _ = self.probe('')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(compose[0].count('--env-file'), 1)

    def test_materialized_secrets_are_added_after_production_env(self):
        def ready(path):
            path.write_text("POSTGRES_PASSWORD='x'\n"); path.chmod(0o600)
        result, compose, secrets = self.probe('SECRETS_ENV_FILE={secrets}\n', ready)
        self.assertEqual(result.returncode, 0, result.stderr)
        files = [compose[0][i + 1] for i, a in enumerate(compose[0]) if a == '--env-file']
        self.assertEqual(files, ['production.env', str(secrets)])

    def test_missing_open_or_image_overriding_secrets_refuse_before_compose(self):
        def loose(path):
            path.write_text("A='x'\n"); path.chmod(0o644)
        def image(path):
            path.write_text("AUDENIQ_IMAGE='ghcr.io/evil'\n"); path.chmod(0o600)
        for prepare in [None, loose, image]:
            with self.subTest(prepare=prepare):
                result, compose, _ = self.probe('SECRETS_ENV_FILE={secrets}\n', prepare)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(compose, [])


if __name__ == '__main__':
    unittest.main()
