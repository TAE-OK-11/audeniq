#!/usr/bin/env python3
"""Protect payout AES keys with AWS KMS or GCP Cloud KMS; materialize into tmpfs.

AWS CLI handles SigV4; GCP uses ADC from gcloud and a pinned, verified HTTPS API.
The API receives no cloud credentials. No cloud resources are created here.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import secrets
import ssl
import stat
import subprocess
import tempfile
import urllib.request


def key_region(arn):
    if not isinstance(arn, str):
        raise ValueError('a pinned AWS KMS key ARN is required')
    match = re.fullmatch(r'arn:aws:kms:([a-z]{2}-[a-z]+-\d):\d{12}:key/[0-9a-f-]{36}', arn)
    if not match:
        raise ValueError('a pinned AWS symmetric KMS key ARN is required (no alias)')
    return match[1]


def context(version):
    # CloudTrail records this context. Never put account numbers or names here.
    return {'service': 'audeniq', 'purpose': 'payout-account-key', 'version': str(version)}


def gcp_key_name(name):
    if not isinstance(name, str) or not re.fullmatch(
        r'projects/[A-Za-z0-9_-]{1,63}/locations/[a-z0-9-]{1,63}/'
        r'keyRings/[A-Za-z0-9_-]{1,63}/cryptoKeys/[A-Za-z0-9_-]{1,63}', name
    ):
        raise ValueError('a full GCP CryptoKey resource name is required')
    return name


def validate_key_id(provider, key_id):
    if provider == 'aws':
        key_region(key_id)
    elif provider == 'gcp':
        gcp_key_name(key_id)
    else:
        raise ValueError('unsupported KMS provider')


def crc32c(data):
    # Castagnoli polynomial; no dependency is needed for these small AES keys.
    crc = 0xffffffff
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ (0x82f63b78 if crc & 1 else 0)
    return crc ^ 0xffffffff


def verify_crc(data, value):
    if not isinstance(value, (str, int)) or isinstance(value, bool):
        raise ValueError('missing GCP integrity checksum')
    if str(value) != str(crc32c(data)):
        raise ValueError('GCP integrity checksum mismatch')


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, new_url):
        return None


def gcp_access_token():
    env = os.environ.copy()
    env['CLOUDSDK_CORE_LOG_HTTP'] = 'false'
    env['CLOUDSDK_CORE_VERBOSITY'] = 'warning'
    result = subprocess.run(
        ['gcloud', 'auth', 'application-default', 'print-access-token', '--quiet'],
        env=env, capture_output=True, timeout=90,
    )
    if result.returncode or len(result.stdout) > 16384:
        raise ValueError('GCP ADC authentication failed')
    token = result.stdout.decode('ascii').strip()
    if not re.fullmatch(r'[A-Za-z0-9._~+/-]{1,16384}={0,2}', token):
        raise ValueError('invalid GCP access token')
    return token


def gcp_request(operation, key_id, payload):
    gcp_key_name(key_id)
    if operation not in ('encrypt', 'decrypt'):
        raise ValueError('unsupported GCP operation')
    url = f'https://cloudkms.googleapis.com/v1/{key_id}:{operation}'
    tls = ssl.create_default_context()
    tls.minimum_version = ssl.TLSVersion.TLSv1_2
    opener = urllib.request.build_opener(urllib.request.HTTPSHandler(context=tls), NoRedirect())
    headers = {'Content-Type': 'application/json'}
    quota_project = os.environ.get('GOOGLE_CLOUD_QUOTA_PROJECT')
    if quota_project:
        if not re.fullmatch(r'[A-Za-z0-9_-]{1,63}', quota_project):
            raise ValueError('invalid GCP quota project')
        headers['X-Goog-User-Project'] = quota_project
    headers['Authorization'] = 'Bearer ' + gcp_access_token()
    request = urllib.request.Request(url, data=json.dumps(payload).encode(), headers=headers, method='POST')
    with opener.open(request, timeout=90) as response:
        if response.geturl() != url:
            raise ValueError('unexpected GCP response URL')
        raw = response.read(65537)
    if len(raw) > 65536:
        raise ValueError('GCP response exceeds limit')
    result = json.loads(raw)
    if not isinstance(result, dict):
        raise ValueError('invalid GCP response')
    return result


def gcp_kms(operation, key_id, data, version):
    aad = json.dumps(context(version), sort_keys=True, separators=(',', ':')).encode()
    field = 'plaintext' if operation == 'encrypt' else 'ciphertext'
    response = gcp_request(operation, key_id, {
        field: base64.b64encode(data).decode(), field + 'Crc32c': str(crc32c(data)),
        'additionalAuthenticatedData': base64.b64encode(aad).decode(),
        'additionalAuthenticatedDataCrc32c': str(crc32c(aad)),
    })
    if operation == 'encrypt':
        if not re.fullmatch(re.escape(key_id) + r'/cryptoKeyVersions/[1-9]\d*', response.get('name', '')):
            raise ValueError('GCP returned an unexpected key resource')
        if response.get('verifiedPlaintextCrc32c') is not True or response.get('verifiedAdditionalAuthenticatedDataCrc32c') is not True:
            raise ValueError('GCP did not verify request integrity')
        field = 'ciphertext'
    else:
        field = 'plaintext'
    result = base64.b64decode(response[field], validate=True)
    verify_crc(result, response.get(field + 'Crc32c'))
    return result


def secure_directory(path, uid=0, gid=0):
    path = Path(path)
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.is_symlink() or path.stat().st_uid != os.geteuid():
        raise ValueError('runtime directory must be owned by this user and not a symlink')
    fs = subprocess.run(['stat', '-f', '-c', '%T', str(path)], check=True, capture_output=True, text=True).stdout.strip()
    if fs != 'tmpfs':
        raise ValueError('plaintext key material must be written to tmpfs')
    os.chown(path, uid, gid)
    os.chmod(path, 0o750 if gid else 0o700)
    return path


def private_read(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        m = os.fstat(fd)
        if not stat.S_ISREG(m.st_mode) or m.st_uid != os.geteuid() or m.st_mode & 0o077 or m.st_size > 65536:
            raise ValueError('input must be a private file (0600 or stricter), at most 64 KiB')
        return os.read(fd, 65537)
    finally:
        os.close(fd)


def atomic_write(path, data, gid=None):
    path = Path(path)
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.keyring-', delete=False) as file:
        tmp = Path(file.name)
        try:
            os.fchmod(file.fileno(), 0o640 if gid else 0o600)
            if gid is not None:
                os.fchown(file.fileno(), os.geteuid(), gid)
            file.write(json.dumps(data).encode())
            file.flush()
            os.fsync(file.fileno())
            os.replace(tmp, path)
        finally:
            tmp.unlink(missing_ok=True)


def aws_kms(operation, arn, request, scratch):
    region = key_region(arn)
    # Requests can include the legacy key. Never use shell arguments for it.
    with tempfile.NamedTemporaryFile(dir=scratch, prefix='.kms-', mode='w', delete=False) as file:
        name = file.name
        json.dump(request, file)
    try:
        env = os.environ.copy()
        env['AWS_MAX_ATTEMPTS'] = '3'
        env['AWS_RETRY_MODE'] = 'standard'
        result = subprocess.run([
            'aws', 'kms', operation, '--region', region,
            '--endpoint-url', f'https://kms.{region}.amazonaws.com',
            '--cli-input-json', 'file://' + name, '--output', 'json', '--no-cli-pager',
        ], env=env, capture_output=True, timeout=90)
        if result.returncode:
            # Do not echo SDK output; it may contain request data under debug config.
            raise ValueError(f'KMS {operation} failed; check IAM, key state and connectivity')
        if len(result.stdout) > 65536:
            raise ValueError('KMS response exceeds limit')
        response = json.loads(result.stdout)
        if response.get('KeyId') != arn:
            raise ValueError('KMS returned an unexpected key ARN')
        return response
    finally:
        Path(name).unlink(missing_ok=True)


def validate_bundle(bundle):
    if not isinstance(bundle, dict) or set(bundle) != {'version', 'active_version', 'keys'} or type(bundle['version']) is not int or bundle['version'] not in (1, 2):
        raise ValueError('invalid wrapped keyring format')
    keys = bundle['keys']
    if type(bundle['active_version']) is not int or not isinstance(keys, dict) or not 1 <= len(keys) <= 64 or str(bundle['active_version']) not in keys:
        raise ValueError('missing active key or too many versions')
    for version, key in keys.items():
        if not re.fullmatch(r'[1-9]\d{0,4}', version) or int(version) > 32767:
            raise ValueError('invalid key version')
        fields = {'key_id', 'ciphertext'} if bundle['version'] == 1 else {'provider', 'key_id', 'ciphertext'}
        if not isinstance(key, dict) or set(key) != fields:
            raise ValueError('invalid wrapped key fields')
        validate_key_id(key.get('provider', 'aws'), key['key_id'])
        if not 1 <= len(base64.b64decode(key['ciphertext'], validate=True)) <= 6144:
            raise ValueError('invalid KMS ciphertext')
    if len(json.dumps(bundle).encode()) > 65536:
        raise ValueError('wrapped bundle exceeds private input limit')


def upgrade_bundle(bundle):
    validate_bundle(bundle)
    return {'version': 2, 'active_version': bundle['active_version'], 'keys': {
        version: dict(key, provider=key.get('provider', 'aws')) for version, key in bundle['keys'].items()
    }}


def wrap_key(provider, key_id, version, plaintext, scratch):
    validate_key_id(provider, key_id)
    if len(plaintext) != 32:
        raise ValueError('key must be AES-256')
    if provider == 'aws':
        response = aws_kms('encrypt', key_id, {
            'KeyId': key_id, 'Plaintext': base64.b64encode(plaintext).decode(),
            'EncryptionAlgorithm': 'SYMMETRIC_DEFAULT', 'EncryptionContext': context(version),
        }, scratch)
        ciphertext = response['CiphertextBlob']
    else:
        ciphertext = base64.b64encode(gcp_kms('encrypt', key_id, plaintext, version)).decode()
    return {'provider': provider, 'key_id': key_id, 'ciphertext': ciphertext}


def generate_key(provider, key_id, version, scratch):
    validate_key_id(provider, key_id)
    if provider == 'gcp':
        return wrap_key(provider, key_id, version, secrets.token_bytes(32), scratch)
    response = aws_kms('generate-data-key-without-plaintext', key_id, {
        'KeyId': key_id, 'KeySpec': 'AES_256', 'EncryptionContext': context(version),
    }, scratch)
    return {'provider': provider, 'key_id': key_id, 'ciphertext': response['CiphertextBlob']}


def unwrap_key(key, version, scratch):
    provider = key.get('provider', 'aws')
    validate_key_id(provider, key['key_id'])
    if provider == 'aws':
        response = aws_kms('decrypt', key['key_id'], {
            'KeyId': key['key_id'], 'CiphertextBlob': key['ciphertext'],
            'EncryptionAlgorithm': 'SYMMETRIC_DEFAULT', 'EncryptionContext': context(version),
        }, scratch)
        plaintext = base64.b64decode(response['Plaintext'], validate=True)
    else:
        plaintext = gcp_kms('decrypt', key['key_id'], base64.b64decode(key['ciphertext'], validate=True), version)
    if len(plaintext) != 32:
        raise ValueError('KMS key is not AES-256')
    return plaintext


def rewrap_bundle(bundle, provider, key_id, scratch):
    source = upgrade_bundle(bundle)
    validate_key_id(provider, key_id)
    # Preserve each AES key/version; changing the KMS requires no DB rewrite.
    keys = {version: wrap_key(provider, key_id, version, unwrap_key(key, version, scratch), scratch)
            for version, key in source['keys'].items()}
    result = dict(source, keys=keys)
    validate_bundle(result)
    return result


def materialize(bundle, scratch, destination, gid):
    validate_bundle(bundle)
    keys = {}
    for version, key in bundle['keys'].items():
        keys[version] = unwrap_key(key, version, scratch).hex()
    # All versions must decrypt successfully before replacing a usable keyring.
    atomic_write(destination, {'version': 1, 'active_version': bundle['active_version'], 'keys': keys}, gid)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['wrap-legacy', 'generate', 'materialize', 'rewrap'])
    parser.add_argument('--bundle', required=True, type=Path, help='private, KMS-wrapped JSON on persistent storage')
    parser.add_argument('--provider', choices=['aws', 'gcp'], help='KMS provider for new wrapping (default: aws)')
    parser.add_argument('--key-id', help='existing symmetric AWS key ARN or GCP CryptoKey resource name')
    parser.add_argument('--legacy-key-file', type=Path, help='private 64-hex key file, supplied only for migration')
    parser.add_argument('--runtime-dir', type=Path, default=Path('/dev/shm/audeniq-secrets'))
    parser.add_argument('--gid', type=int, default=10001, help='API container group; directory owner remains root')
    args = parser.parse_args()
    try:
        scratch = secure_directory(args.runtime_dir, os.geteuid(), args.gid)
        bundle = upgrade_bundle(json.loads(private_read(args.bundle))) if args.bundle.exists() else {'version': 2, 'active_version': 0, 'keys': {}}
        if args.command == 'materialize':
            if args.provider is not None or args.key_id is not None:
                raise ValueError('materialize reads providers from the bundle; use rewrap to switch')
            materialize(bundle, scratch, scratch / 'payout-keyring.json', args.gid)
        else:
            provider = args.provider or 'aws'
            validate_key_id(provider, args.key_id or '')
            if args.command == 'rewrap':
                bundle = rewrap_bundle(bundle, provider, args.key_id, scratch)
                atomic_write(args.bundle, bundle)
                print(f'KMS keyring rewrap completed; active version {bundle["active_version"]}')
                return
            if args.command == 'wrap-legacy':
                if bundle['keys'] or not args.legacy_key_file:
                    raise ValueError('legacy import requires an empty bundle and a private key file')
                version = 1
                key = bytes.fromhex(private_read(args.legacy_key_file).decode().strip())
                if len(key) != 32:
                    raise ValueError('legacy key must be 32 bytes')
                entry = wrap_key(provider, args.key_id, version, key, scratch)
            else:
                version = max((int(v) for v in bundle['keys']), default=0) + 1
                if version > 32767 or len(bundle['keys']) >= 64:
                    raise ValueError('keyring version limit reached')
                entry = generate_key(provider, args.key_id, version, scratch)
            bundle['keys'][str(version)] = entry
            bundle['active_version'] = version
            validate_bundle(bundle)
            atomic_write(args.bundle, bundle)
        print(f'KMS keyring {args.command} completed; active version {bundle["active_version"]}')
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        # Key/JSON exception details can contain secrets. Return only an operation status.
        parser.exit(1, f'KMS keyring operation failed ({type(error).__name__}); no key material was printed.\n')


if __name__ == '__main__':
    main()
