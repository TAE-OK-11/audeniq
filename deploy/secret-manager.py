#!/usr/bin/env python3
"""Keep production secrets (DB passwords, service secrets, encryption keys) in
AWS Secrets Manager, GCP Secret Manager or OCI Vault and materialize them into
host tmpfs for docker compose.

A private JSON manifest maps each environment variable (or runtime key file) to
one pinned secret: a full AWS secret ARN, a GCP `projects/P/secrets/S` resource
or an OCI vault secret OCID. `materialize` reads every secret through the
official CLI (aws / gcloud / oci handle signing and TLS) and replaces
`secrets.env` and the key files only when every read succeeded. `put` stores a
new version of an existing secret from a private file. No secret value is ever
printed, logged or passed as a command argument. No cloud resources are
created here.

Manifest (0600, persistent storage):

    {"version": 1, "secrets": [
      {"env": "POSTGRES_PASSWORD", "provider": "aws",
       "secret_id": "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db-AbCdEf",
       "json_key": "password"},
      {"env": "EDGE_SERVICE_SECRET", "provider": "gcp",
       "secret_id": "projects/audeniq-prod/secrets/edge-service-secret"},
      {"file": "payout-keyring.json", "provider": "oci",
       "secret_id": "ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaexample"}
    ]}

`json_key` selects one field of a JSON secret (e.g. host / username / password
of one database secret). `version` pins a GCP version number, an AWS version
stage or an OCI stage; the default is the current version.
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

MAX_VALUE = 65536
ENV_NAME = re.compile(r'[A-Z][A-Z0-9_]{0,63}')
FILE_NAME = re.compile(r'[a-z0-9][a-z0-9._-]{0,63}')
# deploy.sh owns this line of production.env; a secret must never replace it.
RESERVED_ENV = {'AUDENIQ_IMAGE'}
AWS_SECRET = re.compile(
    r'arn:aws:secretsmanager:(?P<region>[a-z]{2}(?:-gov)?-[a-z]+-\d):\d{12}:secret:[A-Za-z0-9/_+=.@-]{1,512}-[A-Za-z0-9]{6}'
)
GCP_SECRET = re.compile(r'projects/(?P<project>[a-z][a-z0-9-]{4,28}[a-z0-9])/secrets/(?P<secret>[A-Za-z0-9_-]{1,255})')
OCI_SECRET = re.compile(r'ocid1\.vaultsecret\.oc1\.[a-z0-9-]{1,32}\.[a-z0-9]{1,128}')
VERSION = {
    'aws': re.compile(r'[A-Za-z0-9_-]{1,256}'),     # version stage, e.g. AWSCURRENT
    'gcp': re.compile(r'[1-9]\d{0,9}|latest'),      # version number
    'oci': re.compile(r'CURRENT|PENDING|PREVIOUS|LATEST'),
}
DEFAULT_VERSION = {'aws': 'AWSCURRENT', 'gcp': 'latest', 'oci': 'CURRENT'}


def validate_secret_id(provider, secret_id):
    pattern = {'aws': AWS_SECRET, 'gcp': GCP_SECRET, 'oci': OCI_SECRET}.get(provider)
    if pattern is None:
        raise ValueError('unsupported secret manager provider')
    match = pattern.fullmatch(secret_id) if isinstance(secret_id, str) else None
    if not match:
        raise ValueError(f'a pinned {provider} secret id is required (full ARN, projects/P/secrets/S or vault secret OCID)')
    return match


def validate_manifest(manifest):
    if not isinstance(manifest, dict) or set(manifest) != {'version', 'secrets'} or manifest['version'] != 1:
        raise ValueError('invalid secret manifest format')
    entries = manifest['secrets']
    if not isinstance(entries, list) or not 1 <= len(entries) <= 128:
        raise ValueError('a manifest lists 1..128 secrets')
    targets = set()
    for entry in entries:
        if not isinstance(entry, dict) or not set(entry) <= {'env', 'file', 'provider', 'secret_id', 'json_key', 'version'}:
            raise ValueError('invalid secret entry fields')
        if ('env' in entry) == ('file' in entry):
            raise ValueError('each secret has exactly one target: env or file')
        if 'env' in entry:
            if not isinstance(entry['env'], str) or not ENV_NAME.fullmatch(entry['env']) or entry['env'] in RESERVED_ENV:
                raise ValueError('invalid or reserved environment variable name')
            target = 'env:' + entry['env']
        else:
            if not isinstance(entry['file'], str) or not FILE_NAME.fullmatch(entry['file']) or entry['file'] == 'secrets.env':
                raise ValueError('invalid runtime file name')
            target = 'file:' + entry['file']
        if target in targets:
            raise ValueError('duplicate secret target')
        targets.add(target)
        provider = entry.get('provider')
        validate_secret_id(provider, entry.get('secret_id'))
        if 'json_key' in entry and not (isinstance(entry['json_key'], str) and re.fullmatch(r'[A-Za-z0-9_.-]{1,128}', entry['json_key'])):
            raise ValueError('invalid json_key')
        if 'version' in entry and not (isinstance(entry['version'], str) and VERSION[provider].fullmatch(entry['version'])):
            raise ValueError('invalid secret version for provider')
    return manifest


def private_read(path, limit=MAX_VALUE):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        m = os.fstat(fd)
        if not stat.S_ISREG(m.st_mode) or m.st_uid != os.geteuid() or m.st_mode & 0o077 or m.st_size > limit:
            raise ValueError('input must be a private file (0600 or stricter) within the size limit')
        return os.read(fd, limit + 1)
    finally:
        os.close(fd)


def secure_directory(path, gid=None):
    path = Path(path)
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.is_symlink() or path.stat().st_uid != os.geteuid():
        raise ValueError('runtime directory must be owned by this user and not a symlink')
    fs = subprocess.run(['stat', '-f', '-c', '%T', str(path)], check=True, capture_output=True, text=True).stdout.strip()
    if fs != 'tmpfs':
        raise ValueError('secret values must be written to tmpfs')
    if gid is None:
        os.chmod(path, 0o700)
    else:
        os.chown(path, os.geteuid(), gid)
        os.chmod(path, 0o750)
    return path


def atomic_write(path, data, gid=None):
    path = Path(path)
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.secret-', delete=False) as file:
        tmp = Path(file.name)
        try:
            os.fchmod(file.fileno(), 0o640 if gid is not None else 0o600)
            if gid is not None:
                os.fchown(file.fileno(), os.geteuid(), gid)
            file.write(data)
            file.flush()
            os.fsync(file.fileno())
            os.replace(tmp, path)
        finally:
            tmp.unlink(missing_ok=True)


def crc32c(data):
    crc = 0xffffffff
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ (0x82f63b78 if crc & 1 else 0)
    return crc ^ 0xffffffff


def run_cli(args, what):
    env = os.environ.copy()
    env.update({'AWS_MAX_ATTEMPTS': '3', 'AWS_RETRY_MODE': 'standard', 'CLOUDSDK_CORE_LOG_HTTP': 'false',
                'CLOUDSDK_CORE_VERBOSITY': 'error', 'OCI_CLI_SUPPRESS_FILE_PERMISSIONS_WARNING': 'True'})
    result = subprocess.run(args, env=env, capture_output=True, timeout=90)
    if result.returncode:
        # Never echo CLI output: it can carry the secret under debug settings.
        raise ValueError(f'{what} failed; check IAM, secret state and connectivity')
    if len(result.stdout) > 4 * MAX_VALUE:
        raise ValueError(f'{what} response exceeds limit')
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        raise ValueError(f'{what} returned an invalid response') from None


def with_request_file(scratch, request, call):
    # Values go through a 0600 tmpfs file, never through argv.
    with tempfile.NamedTemporaryFile(dir=scratch, prefix='.secret-req-', mode='w', delete=False) as file:
        name = file.name
        json.dump(request, file)
    try:
        return call('file://' + name)
    finally:
        Path(name).unlink(missing_ok=True)


def aws_fetch(secret_id, version, scratch):
    region = validate_secret_id('aws', secret_id)['region']
    response = with_request_file(scratch, {'SecretId': secret_id, 'VersionStage': version}, lambda request: run_cli([
        'aws', 'secretsmanager', 'get-secret-value', '--region', region,
        '--endpoint-url', f'https://secretsmanager.{region}.amazonaws.com',
        '--cli-input-json', request, '--output', 'json', '--no-cli-pager',
    ], 'AWS Secrets Manager read'))
    if response.get('ARN') != secret_id:
        raise ValueError('AWS returned an unexpected secret ARN')
    if isinstance(response.get('SecretString'), str):
        return response['SecretString'].encode()
    return base64.b64decode(response['SecretBinary'], validate=True)


def gcp_fetch(secret_id, version, scratch):
    match = validate_secret_id('gcp', secret_id)
    response = run_cli([
        'gcloud', 'secrets', 'versions', 'access', version, '--secret', match['secret'],
        '--project', match['project'], '--format', 'json', '--quiet',
    ], 'GCP Secret Manager read')
    # The project appears as its number in the response; the secret name must match.
    if not re.fullmatch(r'projects/[a-z0-9-]+/secrets/' + re.escape(match['secret']) + r'/versions/[1-9]\d*', response.get('name', '')):
        raise ValueError('GCP returned an unexpected secret version')
    payload = response.get('payload') or {}
    data = base64.b64decode(payload.get('data', ''), validate=True)
    if str(payload.get('dataCrc32c')) != str(crc32c(data)):
        raise ValueError('GCP secret integrity checksum mismatch')
    return data


def oci_fetch(secret_id, version, scratch):
    validate_secret_id('oci', secret_id)
    response = run_cli([
        'oci', 'secrets', 'secret-bundle', 'get', '--secret-id', secret_id, '--stage', version, '--output', 'json',
    ], 'OCI Vault secret read')
    data = response.get('data') if isinstance(response, dict) else None
    if not isinstance(data, dict) or data.get('secret-id') != secret_id:
        raise ValueError('OCI returned an unexpected secret')
    content = data.get('secret-bundle-content') or {}
    if content.get('content-type') != 'BASE64':
        raise ValueError('unsupported OCI secret content type')
    return base64.b64decode(content.get('content', ''), validate=True)


def aws_put(secret_id, value, scratch):
    region = validate_secret_id('aws', secret_id)['region']
    try:
        request = {'SecretId': secret_id, 'SecretString': value.decode('utf-8')}
    except UnicodeDecodeError:
        request = {'SecretId': secret_id, 'SecretBinary': base64.b64encode(value).decode()}
    response = with_request_file(scratch, request, lambda request: run_cli([
        'aws', 'secretsmanager', 'put-secret-value', '--region', region,
        '--endpoint-url', f'https://secretsmanager.{region}.amazonaws.com',
        '--cli-input-json', request, '--output', 'json', '--no-cli-pager',
    ], 'AWS Secrets Manager write'))
    if response.get('ARN') != secret_id:
        raise ValueError('AWS returned an unexpected secret ARN')


def gcp_put(secret_id, value, scratch):
    match = validate_secret_id('gcp', secret_id)
    with tempfile.NamedTemporaryFile(dir=scratch, prefix='.secret-data-', delete=False) as file:
        name = file.name
        file.write(value)
    try:
        response = run_cli([
            'gcloud', 'secrets', 'versions', 'add', match['secret'], '--project', match['project'],
            '--data-file', name, '--format', 'json', '--quiet',
        ], 'GCP Secret Manager write')
    finally:
        Path(name).unlink(missing_ok=True)
    if not re.fullmatch(r'projects/[a-z0-9-]+/secrets/' + re.escape(match['secret']) + r'/versions/[1-9]\d*', response.get('name', '')):
        raise ValueError('GCP returned an unexpected secret version')


def oci_put(secret_id, value, scratch):
    validate_secret_id('oci', secret_id)
    response = with_request_file(scratch, {
        'secretId': secret_id, 'secretContentContent': base64.b64encode(value).decode(), 'secretContentStage': 'CURRENT',
    }, lambda request: run_cli(['oci', 'vault', 'secret', 'update-base64', '--from-json', request, '--output', 'json'],
                               'OCI Vault secret write'))
    data = response.get('data') if isinstance(response, dict) else None
    if not isinstance(data, dict) or data.get('id') != secret_id:
        raise ValueError('OCI returned an unexpected secret')


FETCH = {'aws': aws_fetch, 'gcp': gcp_fetch, 'oci': oci_fetch}
PUT = {'aws': aws_put, 'gcp': gcp_put, 'oci': oci_put}


def resolve(entry, scratch):
    provider = entry['provider']
    raw = FETCH[provider](entry['secret_id'], entry.get('version', DEFAULT_VERSION[provider]), scratch)
    if len(raw) > MAX_VALUE:
        raise ValueError('secret value exceeds limit')
    if 'json_key' in entry:
        try:
            document = json.loads(raw)
        except (json.JSONDecodeError, UnicodeDecodeError):
            document = None
        value = document.get(entry['json_key']) if isinstance(document, dict) else None
        if value is None or isinstance(value, (dict, list, bool)):
            raise ValueError('secret has no such JSON field')
        raw = str(value).encode()
    if 'env' in entry:
        text = raw.decode('utf-8')
        # Written single-quoted: compose reads it literally ($, #, " and spaces
        # included). A quote or line break could not round-trip; refuse it.
        if not text or any(c in text for c in "'\n\r\0"):
            raise ValueError('an environment secret must be one non-empty line without single quotes')
        return text
    return raw


def materialize(manifest, scratch, gid, env_dir=None):
    validate_manifest(manifest)
    env_lines, files = [], {}
    # Every secret resolves before anything is replaced: no half-updated runtime.
    for entry in manifest['secrets']:
        value = resolve(entry, scratch)
        if 'env' in entry:
            env_lines.append(f"{entry['env']}='{value}'")
        else:
            files[entry['file']] = value
    for name, value in files.items():
        atomic_write(Path(scratch) / name, value, gid)
    # secrets.env is read by deploy.sh/compose on the host only: owner 0600,
    # in a directory that is not mounted into any container.
    atomic_write(Path(env_dir or scratch) / 'secrets.env', ('\n'.join(env_lines) + '\n').encode() if env_lines else b'')
    return len(env_lines), len(files)


def put(manifest, target, value_file, scratch):
    validate_manifest(manifest)
    matches = [e for e in manifest['secrets'] if e.get('env') == target or e.get('file') == target]
    if len(matches) != 1:
        raise ValueError('target is not in the manifest')
    entry = matches[0]
    if 'json_key' in entry:
        raise ValueError('a JSON secret is updated as a whole in its provider console or CLI')
    value = private_read(value_file)
    if len(value) > MAX_VALUE:
        raise ValueError('secret value exceeds limit')
    if 'env' in entry:
        text = value.decode('utf-8').rstrip('\n')
        if not text or any(c in text for c in "'\n\r\0"):
            raise ValueError('an environment secret must be one non-empty line without single quotes')
        value = text.encode()
    PUT[entry['provider']](entry['secret_id'], value, scratch)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('command', choices=['check', 'materialize', 'put'])
    parser.add_argument('--manifest', required=True, type=Path, help='private JSON manifest of pinned secrets')
    parser.add_argument('--runtime-dir', type=Path, default=Path('/dev/shm/audeniq-secrets'),
                        help='tmpfs directory for key files (mounted read-only into the API)')
    parser.add_argument('--env-dir', type=Path, default=Path('/dev/shm/audeniq-env'),
                        help='host-only tmpfs directory for secrets.env (SECRETS_ENV_FILE)')
    parser.add_argument('--gid', type=int, default=10001, help='API container group for key files')
    parser.add_argument('--target', help='put: the env name or file name of the manifest entry')
    parser.add_argument('--value-file', type=Path, help='put: private 0600 file holding the new value')
    args = parser.parse_args()
    try:
        manifest = validate_manifest(json.loads(private_read(args.manifest)))
        if args.command == 'check':
            print(f'secret manifest valid; {len(manifest["secrets"])} secrets')
            return
        scratch = secure_directory(args.runtime_dir, args.gid)
        if args.command == 'materialize':
            env_dir = secure_directory(args.env_dir)
            if env_dir.resolve() == scratch.resolve():
                raise ValueError('secrets.env must not share the directory mounted into the API')
            envs, files = materialize(manifest, scratch, args.gid, env_dir)
            print(f'secrets materialized; {envs} environment values, {files} files')
        else:
            if not args.target or not args.value_file:
                raise ValueError('put needs --target and --value-file')
            put(manifest, args.target, args.value_file, scratch)
            print(f'secret {args.target} stored as a new version; run materialize to use it')
    except (ValueError, KeyError, TypeError, OSError, UnicodeDecodeError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        # Exception details can contain secret values. Return only a status.
        parser.exit(1, f'secret operation failed ({type(error).__name__}); no secret value was printed.\n')


if __name__ == '__main__':
    main()
