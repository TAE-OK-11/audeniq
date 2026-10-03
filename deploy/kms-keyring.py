#!/usr/bin/env python3
"""Protect payout AES keys with AWS KMS; materialize only into Linux tmpfs.

AWS CLI handles SigV4 and certificate validation. The API needs no AWS credentials.
No resources or paid keys are created by this script. Specify an existing key ARN.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile


def key_region(arn):
    match = re.fullmatch(r'arn:aws:kms:([a-z]{2}-[a-z]+-\d):\d{12}:key/[0-9a-f-]{36}', arn)
    if not match:
        raise ValueError('a pinned AWS symmetric KMS key ARN is required (no alias)')
    return match[1]


def context(version):
    # CloudTrail records this context. Never put account numbers or names here.
    return {'service': 'audeniq', 'purpose': 'payout-account-key', 'version': str(version)}


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


def atomic_write(path, data, gid=0):
    path = Path(path)
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.keyring-', delete=False) as file:
        tmp = Path(file.name)
        try:
            os.fchmod(file.fileno(), 0o640 if gid else 0o600)
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
    if set(bundle) != {'version', 'active_version', 'keys'} or bundle['version'] != 1:
        raise ValueError('invalid wrapped keyring format')
    keys = bundle['keys']
    if not isinstance(keys, dict) or not 1 <= len(keys) <= 64 or str(bundle['active_version']) not in keys:
        raise ValueError('missing active key or too many versions')
    for version, key in keys.items():
        if not re.fullmatch(r'[1-9]\d{0,4}', version) or int(version) > 32767:
            raise ValueError('invalid key version')
        if set(key) != {'key_id', 'ciphertext'}:
            raise ValueError('invalid wrapped key fields')
        key_region(key['key_id'])
        if not 1 <= len(base64.b64decode(key['ciphertext'], validate=True)) <= 6144:
            raise ValueError('invalid KMS ciphertext')


def materialize(bundle, scratch, destination, gid):
    validate_bundle(bundle)
    keys = {}
    for version, key in bundle['keys'].items():
        response = aws_kms('decrypt', key['key_id'], {
            'KeyId': key['key_id'], 'CiphertextBlob': key['ciphertext'],
            'EncryptionAlgorithm': 'SYMMETRIC_DEFAULT', 'EncryptionContext': context(version),
        }, scratch)
        plaintext = base64.b64decode(response['Plaintext'], validate=True)
        if len(plaintext) != 32:
            raise ValueError('KMS key is not AES-256')
        keys[version] = plaintext.hex()
    # All versions must decrypt successfully before replacing a usable keyring.
    atomic_write(destination, {'version': 1, 'active_version': bundle['active_version'], 'keys': keys}, gid)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['wrap-legacy', 'generate', 'materialize'])
    parser.add_argument('--bundle', required=True, type=Path, help='private, KMS-wrapped JSON on persistent storage')
    parser.add_argument('--key-id', help='existing symmetric AWS KMS key ARN')
    parser.add_argument('--legacy-key-file', type=Path, help='private 64-hex key file, supplied only for migration')
    parser.add_argument('--runtime-dir', type=Path, default=Path('/dev/shm/audeniq-secrets'))
    parser.add_argument('--gid', type=int, default=10001, help='API container group; directory owner remains root')
    args = parser.parse_args()
    try:
        scratch = secure_directory(args.runtime_dir, os.geteuid(), args.gid)
        bundle = json.loads(private_read(args.bundle)) if args.bundle.exists() else {'version': 1, 'active_version': 0, 'keys': {}}
        if bundle['keys']:
            validate_bundle(bundle)
        if args.command == 'materialize':
            materialize(bundle, scratch, scratch / 'payout-keyring.json', args.gid)
        else:
            key_region(args.key_id or '')
            if args.command == 'wrap-legacy':
                if bundle['keys'] or not args.legacy_key_file:
                    raise ValueError('legacy import requires an empty bundle and a private key file')
                version = 1
                key = bytes.fromhex(private_read(args.legacy_key_file).decode().strip())
                if len(key) != 32:
                    raise ValueError('legacy key must be 32 bytes')
                response = aws_kms('encrypt', args.key_id, {
                    'KeyId': args.key_id, 'Plaintext': base64.b64encode(key).decode(),
                    'EncryptionAlgorithm': 'SYMMETRIC_DEFAULT', 'EncryptionContext': context(version),
                }, scratch)
            else:
                version = max((int(v) for v in bundle['keys']), default=0) + 1
                if version > 32767 or len(bundle['keys']) >= 64:
                    raise ValueError('keyring version limit reached')
                response = aws_kms('generate-data-key-without-plaintext', args.key_id, {
                    'KeyId': args.key_id, 'KeySpec': 'AES_256', 'EncryptionContext': context(version),
                }, scratch)
            bundle['keys'][str(version)] = {'key_id': args.key_id, 'ciphertext': response['CiphertextBlob']}
            bundle['active_version'] = version
            validate_bundle(bundle)
            atomic_write(args.bundle, bundle)
        print(f'KMS keyring {args.command} completed; active version {bundle["active_version"]}')
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        # Key/JSON exception details can contain secrets. Return only an operation status.
        parser.exit(1, f'KMS keyring operation failed ({type(error).__name__}); no key material was printed.\n')


if __name__ == '__main__':
    main()
