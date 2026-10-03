import base64
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import ssl
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('kms_providers', Path(__file__).resolve().parents[1] / 'kms-keyring.py')
kms = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kms)
ARN = 'arn:aws:kms:ap-northeast-1:123456789012:key/12345678-1234-1234-1234-123456789abc'
GCP = 'projects/audeniq-prod/locations/asia-northeast3/keyRings/backend/cryptoKeys/payout'


class FakeCloud:
    """API-contract stub binding the encrypted AES key to its provider and AAD."""
    def __init__(self):
        self.records = {}
        self.calls = []

    def aws(self, operation, key_id, request, scratch):
        self.calls.append(('aws', operation, dict(request)))
        if operation == 'decrypt':
            data, aad = self.records[('aws', key_id, request['CiphertextBlob'])]
            if aad != request['EncryptionContext']:
                raise ValueError('wrong context')
            return {'Plaintext': base64.b64encode(data).decode()}
        data = b'A' * 32 if operation == 'generate-data-key-without-plaintext' else base64.b64decode(request['Plaintext'])
        ciphertext = base64.b64encode(b'aws-' + bytes([len(self.records)])).decode()
        self.records[('aws', key_id, ciphertext)] = data, request['EncryptionContext']
        return {'KeyId': key_id, 'CiphertextBlob': ciphertext}

    def gcp(self, operation, key_id, request):
        self.calls.append(('gcp', operation, dict(request)))
        aad = base64.b64decode(request['additionalAuthenticatedData'])
        kms.verify_crc(aad, request['additionalAuthenticatedDataCrc32c'])
        if operation == 'decrypt':
            kms.verify_crc(base64.b64decode(request['ciphertext']), request['ciphertextCrc32c'])
            data, original_aad = self.records[('gcp', key_id, request['ciphertext'])]
            if aad != original_aad:
                raise ValueError('wrong AAD')
            return {'plaintext': base64.b64encode(data).decode(), 'plaintextCrc32c': str(kms.crc32c(data))}
        data = base64.b64decode(request['plaintext'])
        kms.verify_crc(data, request['plaintextCrc32c'])
        ciphertext = b'gcp-' + bytes([len(self.records)])
        encoded = base64.b64encode(ciphertext).decode()
        self.records[('gcp', key_id, encoded)] = data, aad
        return {'name': key_id + '/cryptoKeyVersions/7', 'ciphertext': encoded,
                'ciphertextCrc32c': str(kms.crc32c(ciphertext)),
                'verifiedPlaintextCrc32c': True, 'verifiedAdditionalAuthenticatedDataCrc32c': True}


class ProviderTests(unittest.TestCase):
    def setUp(self):
        self.cloud = FakeCloud()
        self.aws = patch.object(kms, 'aws_kms', side_effect=self.cloud.aws)
        self.gcp = patch.object(kms, 'gcp_request', side_effect=self.cloud.gcp)
        self.aws.start(); self.gcp.start()
        self.addCleanup(self.aws.stop); self.addCleanup(self.gcp.stop)

    def legacy(self):
        key = kms.wrap_key('aws', ARN, 1, b'L' * 32, '/dev/shm')
        del key['provider']
        return {'version': 1, 'active_version': 1, 'keys': {'1': key}}

    def test_legacy_aws_bundle_and_mixed_provider_materialization(self):
        old = self.legacy()
        upgraded = kms.upgrade_bundle(old)
        self.assertEqual(old['version'], 1)
        self.assertEqual(upgraded['keys']['1']['provider'], 'aws')
        with patch.object(kms.secrets, 'token_bytes', return_value=b'G' * 32) as random:
            upgraded['keys']['2'] = kms.generate_key('gcp', GCP, 2, '/dev/shm')
        random.assert_called_once_with(32)
        upgraded['active_version'] = 2
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            destination = Path(directory) / 'keyring.json'
            kms.materialize(upgraded, directory, destination, os.getegid())
            result = json.loads(destination.read_text())
            self.assertEqual(result, {'version': 1, 'active_version': 2,
                                      'keys': {'1': (b'L' * 32).hex(), '2': (b'G' * 32).hex()}})
        self.assertEqual([c[:2] for c in self.cloud.calls[-2:]], [('aws', 'decrypt'), ('gcp', 'decrypt')])

    def test_rewrap_between_clouds_preserves_aes_material_versions_and_source(self):
        source = self.legacy()
        original = json.dumps(source)
        gcp = kms.rewrap_bundle(source, 'gcp', GCP, '/dev/shm')
        self.assertEqual(gcp['active_version'], 1)
        self.assertEqual(kms.unwrap_key(gcp['keys']['1'], 1, '/dev/shm'), b'L' * 32)
        aws = kms.rewrap_bundle(gcp, 'aws', ARN, '/dev/shm')
        self.assertEqual(kms.unwrap_key(aws['keys']['1'], 1, '/dev/shm'), b'L' * 32)
        self.assertEqual(json.dumps(source), original)

    def test_key_version_aad_mismatch_refuses_decryption(self):
        for provider, key_id in [('aws', ARN), ('gcp', GCP)]:
            with self.subTest(provider=provider):
                key = kms.wrap_key(provider, key_id, 1, b'K' * 32, '/dev/shm')
                with self.assertRaises(ValueError):
                    kms.unwrap_key(key, 2, '/dev/shm')

    def test_aws_generation_keeps_without_plaintext_operation(self):
        key = kms.generate_key('aws', ARN, 3, '/dev/shm')
        self.assertEqual(key['provider'], 'aws')
        provider, operation, request = self.cloud.calls[-1]
        self.assertEqual(operation, 'generate-data-key-without-plaintext')
        self.assertNotIn('Plaintext', request)

    def cli(self, command, directory, *options):
        argv = ['kms-keyring.py', command, '--bundle', str(Path(directory) / 'wrapped.json'),
                '--runtime-dir', directory, '--gid', str(os.getegid()), *options]
        with patch('sys.argv', argv), redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            kms.main()

    def test_cli_gcp_generate_and_provider_neutral_materialize(self):
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            self.cli('generate', directory, '--provider', 'gcp', '--key-id', GCP)
            wrapped = json.loads((Path(directory) / 'wrapped.json').read_text())
            self.assertEqual(wrapped['version'], 2)
            self.assertEqual(wrapped['keys']['1']['provider'], 'gcp')
            self.cli('materialize', directory)
            plaintext = json.loads((Path(directory) / 'payout-keyring.json').read_text())
            self.assertEqual(len(bytes.fromhex(plaintext['keys']['1'])), 32)
            self.assertEqual((Path(directory) / 'wrapped.json').stat().st_mode & 0o777, 0o600)

    def test_cli_default_aws_and_gcp_legacy_import(self):
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            self.cli('generate', directory, '--key-id', ARN)
            self.assertEqual(json.loads((Path(directory) / 'wrapped.json').read_text())['keys']['1']['provider'], 'aws')
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            path = Path(directory) / 'legacy'; path.write_text('ab' * 32); path.chmod(0o600)
            self.cli('wrap-legacy', directory, '--provider', 'gcp', '--key-id', GCP, '--legacy-key-file', str(path))
            self.cli('materialize', directory)
            self.assertEqual(json.loads((Path(directory) / 'payout-keyring.json').read_text())['keys']['1'], 'ab' * 32)

    def test_cli_rewrap_failure_preserves_existing_bundle(self):
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            self.cli('generate', directory, '--key-id', ARN)
            path = Path(directory) / 'wrapped.json'; original = path.read_bytes()
            with patch.object(kms, 'gcp_request', side_effect=ValueError('KMS unavailable')):
                with self.assertRaises(SystemExit) as error:
                    self.cli('rewrap', directory, '--provider', 'gcp', '--key-id', GCP)
                self.assertEqual(error.exception.code, 1)
            self.assertEqual(path.read_bytes(), original)
            self.cli('materialize', directory)

    def test_mixed_cloud_failure_preserves_existing_runtime_keyring(self):
        bundle = kms.upgrade_bundle(self.legacy())
        bundle['keys']['2'] = kms.generate_key('gcp', GCP, 2, '/dev/shm')
        bundle['active_version'] = 2
        with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
            destination = Path(directory) / 'keyring.json'; destination.write_bytes(b'previous keyring')
            with patch.object(kms, 'gcp_request', return_value={'plaintext': base64.b64encode(b'X' * 32).decode(), 'plaintextCrc32c': '0'}):
                with self.assertRaises(ValueError):
                    kms.materialize(bundle, directory, destination, os.getegid())
            self.assertEqual(destination.read_bytes(), b'previous keyring')
            self.assertEqual(list(Path(directory).glob('.keyring-*')), [])


class GcpTransportTests(unittest.TestCase):
    def test_crc32c_known_vectors(self):
        self.assertEqual(kms.crc32c(b''), 0)
        self.assertEqual(kms.crc32c(b'123456789'), 0xe3069283)

    def test_adc_uses_captured_output_and_rejects_invalid_tokens(self):
        def command(args, **options):
            self.assertEqual(args, ['gcloud', 'auth', 'application-default', 'print-access-token', '--quiet'])
            self.assertEqual(options['env']['CLOUDSDK_CORE_LOG_HTTP'], 'false')
            self.assertTrue(options['capture_output'])
            return subprocess.CompletedProcess(args, 0, b'fake-access-token\n')
        with patch.object(kms.subprocess, 'run', side_effect=command):
            self.assertEqual(kms.gcp_access_token(), 'fake-access-token')
        for output, status in [(b'valid\ninvalid', 0), (b'secret-token', 1), (b'a' * 16385, 0)]:
            with patch.object(kms.subprocess, 'run', return_value=subprocess.CompletedProcess([], status, output)):
                with self.assertRaises(ValueError):
                    kms.gcp_access_token()

    def test_pinned_verified_tls_no_redirect_and_bounded_response(self):
        url = 'https://cloudkms.googleapis.com/v1/' + GCP + ':encrypt'
        class Response(io.BytesIO):
            def geturl(self): return url
        class Opener:
            def open(inner, request, timeout):
                self.assertEqual(request.full_url, url)
                self.assertEqual(request.get_method(), 'POST')
                self.assertEqual(request.get_header('Authorization'), 'Bearer fake-token')
                self.assertEqual(json.loads(request.data), {'plaintext': 'YQ=='})
                return Response(b'{"name":"result"}')
        with patch.object(kms, 'gcp_access_token', return_value='fake-token'), patch.object(kms.urllib.request, 'build_opener', return_value=Opener()) as builder:
            self.assertEqual(kms.gcp_request('encrypt', GCP, {'plaintext': 'YQ=='}), {'name': 'result'})
            https, redirect = builder.call_args.args
            self.assertEqual(https._context.minimum_version, ssl.TLSVersion.TLSv1_2)
            self.assertEqual(https._context.verify_mode, ssl.CERT_REQUIRED)
            self.assertTrue(https._context.check_hostname)
            self.assertIsNone(redirect.redirect_request(None, None, 302, '', {}, 'http://evil.test'))
        class TooLarge:
            def open(self, *args, **kwargs): return Response(b'a' * 65537)
        with patch.object(kms, 'gcp_access_token', return_value='fake-token'), patch.object(kms.urllib.request, 'build_opener', return_value=TooLarge()):
            with self.assertRaises(ValueError): kms.gcp_request('encrypt', GCP, {})

    def test_encrypt_rejects_wrong_resource_missing_checksums_and_bad_integrity(self):
        valid = {'name': GCP + '/cryptoKeyVersions/1', 'ciphertext': 'YQ==',
                 'ciphertextCrc32c': str(kms.crc32c(b'a')), 'verifiedPlaintextCrc32c': True,
                 'verifiedAdditionalAuthenticatedDataCrc32c': True}
        variants = [dict(valid, name=GCP + '-other/cryptoKeyVersions/1'), dict(valid, ciphertextCrc32c='0'),
                    dict(valid, verifiedPlaintextCrc32c=False), dict(valid, verifiedAdditionalAuthenticatedDataCrc32c=False)]
        missing = dict(valid); del missing['ciphertextCrc32c']; variants.append(missing)
        for response in variants:
            with patch.object(kms, 'gcp_request', return_value=response), self.assertRaises(ValueError):
                kms.gcp_kms('encrypt', GCP, b'K' * 32, 1)

    def test_quota_project_header_and_invalid_project_refusal(self):
        class Response(io.BytesIO):
            def geturl(self): return 'https://cloudkms.googleapis.com/v1/' + GCP + ':decrypt'
        class Opener:
            def open(inner, request, timeout):
                self.assertEqual(request.get_header('X-goog-user-project'), 'audeniq-quota')
                return Response(b'{}')
        with patch.dict(os.environ, {'GOOGLE_CLOUD_QUOTA_PROJECT': 'audeniq-quota'}), patch.object(kms, 'gcp_access_token', return_value='fake-token'), patch.object(kms.urllib.request, 'build_opener', return_value=Opener()):
            kms.gcp_request('decrypt', GCP, {})
        with patch.dict(os.environ, {'GOOGLE_CLOUD_QUOTA_PROJECT': 'bad\nproject'}), patch.object(kms, 'gcp_access_token') as token:
            with self.assertRaises(ValueError): kms.gcp_request('decrypt', GCP, {})
            token.assert_not_called()

    def test_invalid_provider_key_resources_and_plaintext_bundles_refuse_before_network(self):
        entry = {'provider': 'gcp', 'key_id': GCP, 'ciphertext': 'YQ=='}
        for key_id in ['http://kms.test', GCP + '/cryptoKeyVersions/1', GCP + '?q=1', GCP.replace('payout', '../payout')]:
            with patch.object(kms, 'gcp_access_token') as token, self.assertRaises(ValueError):
                kms.gcp_request('encrypt', key_id, {})
            token.assert_not_called()
        for invalid in [dict(entry, provider='other'), dict(entry, plaintext='11' * 32), dict(entry, provider='aws')]:
            with self.assertRaises(ValueError):
                kms.validate_bundle({'version': 2, 'active_version': 1, 'keys': {'1': invalid}})
        with self.assertRaises(ValueError):
            kms.validate_bundle({'version': 1, 'active_version': 1, 'keys': {'1': entry}})


if __name__ == '__main__':
    unittest.main()
