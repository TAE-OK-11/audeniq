# 시크릿 매니저 (AWS · GCP · Oracle Cloud)

운영 비밀(DB 비밀번호, 서비스 비밀값, 스토리지 키, 계좌 암호화 키·키링)을 서버의 `production.env` 평문 대신 클라우드 시크릿 매니저에 보관한다. 세 공급자를 지원하며 한 매니페스트에서 섞어 쓸 수 있다.

도구는 Rust 바이너리 `audeniq-secrets`의 `secrets` 명령이다(소스: `crates/core/src/host_secrets/store.rs`, `crates/core/src/cloud`). Python이나 클라우드 CLI 없이 각 REST API를 직접 서명해 호출한다. 받는 방법과 자격 증명 순서는 [KMS 문서](BACKEND_SECURITY_2026-10-03.md#kms-키-준비와-회전)와 같다.

| 공급자 | 서비스 | 시크릿 ID 형식 (고정) | 인증 |
|---|---|---|---|
| `aws` | AWS Secrets Manager | 전체 ARN `arn:aws:secretsmanager:<region>:<account>:secret:<name>-<6자>` | SigV4: 환경변수 → `~/.aws/credentials` → IMDSv2 역할 |
| `gcp` | GCP Secret Manager | `projects/<project-id>/secrets/<secret>` | OAuth2: 서비스 계정 키/ADC 파일 → GCE 메타데이터 |
| `oci` | OCI Vault Secrets | `ocid1.vaultsecret.oc1.<region>.<id>` | 요청 서명: `~/.oci/config` API 키 → (설정 파일이 없으면) OCI 인스턴스의 instance principal. region은 config 또는 인스턴스 메타데이터의 값 |

이름이나 별칭이 아닌 정확한 ID만 받는다. 응답의 ARN, 시크릿 이름, OCID가 고정한 값과 다르면 거절한다. GCP는 응답 CRC32C를 검사하고, 쓰기 요청에도 CRC32C를 붙인다. TLS 1.2 이상, 인증서 검증, redirect 거절, 응답 256 KiB 제한을 적용한다. 이 도구는 시크릿, 볼트, IAM을 만들지 않으므로 시크릿은 운영자가 먼저 만든다.

## 명령

```bash
audeniq-secrets secrets check       --manifest /opt/audeniq/secrets-manifest.json
audeniq-secrets secrets materialize --manifest /opt/audeniq/secrets-manifest.json
audeniq-secrets secrets put         --manifest /opt/audeniq/secrets-manifest.json --target API_DB_PASSWORD --value-file /dev/shm/new-value
```

- `check`: 매니페스트 형식만 검사한다. 네트워크를 쓰지 않는다.
- `materialize`: 모든 시크릿을 먼저 읽는다. 하나라도 실패하면 아무 파일도 바꾸지 않는다. 다 성공했을 때만 아래 두 곳에 원자적으로 쓴다.
  - 환경변수 항목: `--env-dir`(기본 `/dev/shm/audeniq-env`)의 `secrets.env`. host 전용 tmpfs이고 디렉터리는 0700, 파일은 0600이며 어떤 컨테이너에도 마운트하지 않는다.
  - 파일 항목: `--runtime-dir`(기본 `/dev/shm/audeniq-secrets`)에 쓴다. `--gid` 그룹(기본 10001)이 읽을 수 있는 0640이며, API가 읽기 전용으로 마운트한다.
  - 두 디렉터리가 같으면 거절한다. tmpfs가 아니어도 거절한다.
- `put`: 기존 시크릿에 새 버전을 올린다. 값은 0600 파일에서만 읽고, 명령 인자로 받지 않는다. 환경변수 항목은 끝의 줄바꿈 하나를 떼고 저장한다.
- 매니페스트와 값 파일은 이 사용자 소유 0600이어야 한다. 비밀값, 자격, 응답 본문은 출력하거나 로그에 남기지 않는다.

`secrets.env`의 값은 작은따옴표로 감싸므로 compose가 그대로 읽는다(`$`, `#`, `"`, 공백 포함). 작은따옴표나 줄바꿈이 든 값은 거절한다. DB 비밀번호는 `DATABASE_URL`에 들어가므로 URL-safe 값을 쓴다.

## 매니페스트

`/opt/audeniq/secrets-manifest.json`, 0600. 비밀값은 들어 있지 않지만 어떤 시크릿을 쓰는지 드러나므로 비공개로 둔다.

```json
{"version": 1, "secrets": [
  {"env": "POSTGRES_PASSWORD", "provider": "aws",
   "secret_id": "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db-owner-AbCdEf",
   "json_key": "password"},
  {"env": "API_DB_PASSWORD", "provider": "gcp", "secret_id": "projects/audeniq-prod/secrets/api-db-password"},
  {"env": "WORKER_DB_PASSWORD", "provider": "gcp", "secret_id": "projects/audeniq-prod/secrets/worker-db-password", "version": "3"},
  {"env": "EDGE_SERVICE_SECRET", "provider": "oci", "secret_id": "ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaa.example1"},
  {"env": "S3_SECRET_ACCESS_KEY", "provider": "oci", "secret_id": "ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaa.example2"},
  {"file": "payout-keyring.json", "provider": "aws",
   "secret_id": "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/payout-keyring-GhIjKl"}
]}
```

- `env`: compose 환경변수 이름(대문자, 숫자, `_`). `AUDENIQ_IMAGE`는 `deploy.sh`가 관리하므로 쓸 수 없다.
- `file`: 런타임 키 폴더에 쓸 파일 이름. `secrets.env`는 쓸 수 없다.
- `json_key`: JSON 시크릿에서 문자열이나 숫자 필드 하나를 고른다. 예를 들어 DB 시크릿 하나에 `host`, `port`, `username`, `password`를 함께 두고 항목마다 나눠 쓸 수 있다. 이런 시크릿은 `put` 대신 공급자에서 JSON 전체를 갱신한다.
- `version`: 버전을 고정한다. 공급자별 값과 기본값은 다음과 같다.

  | 공급자 | 지정 값 | 기본값 |
  |---|---|---|
  | AWS | 버전 스테이지 | `AWSCURRENT` |
  | GCP | 버전 번호 | `latest` |
  | OCI | `CURRENT`, `PENDING`, `PREVIOUS`, `LATEST` | `CURRENT` |

현재 compose는 DB 호스트(`postgres`/`pgbouncer` 컨테이너)와 DB 사용자 이름을 고정해 쓴다. 그래서 지금 시크릿으로 바꿀 수 있는 DB 값은 비밀번호다. 외부 DB로 옮길 때는 compose가 읽는 변수(예: `DB_HOST`)를 추가한 뒤 같은 방식으로 연결한다.

## 운영 절차

1. 공급자 콘솔이나 API로 시크릿을 만든다. 값은 바로 넣거나, 빈 시크릿을 만든 뒤 `put`으로 넣는다.
2. 매니페스트를 만들고 `check`로 검사한다.
3. `materialize`로 tmpfs에 받는다. 재부팅할 때마다, compose를 시작하기 전에 실행한다. systemd 등에서 실행 순서를 정해 둔다.
4. `production.env`에 `SECRETS_ENV_FILE=/dev/shm/audeniq-env/secrets.env`를 설정한다.
   - `deploy.sh`는 이 파일을 `production.env` 뒤에 읽으므로 같은 이름의 값을 덮는다.
   - 파일이 없거나, tmpfs가 아니거나, 배포 사용자 소유 0600이 아니거나, `AUDENIQ_IMAGE`를 담고 있으면 compose를 실행하기 전에 멈춘다.
   - 전환이 확인되면 `production.env`에서 해당 비밀값을 지운다.
5. 키링을 시크릿으로 받는다면 `PAYOUT_KEYRING_FILE=/run/audeniq-keys/payout-keyring.json`을 설정한다(KMS-off 로컬 키링 모드). KMS 모드(`audeniq-secrets kms materialize`)와 파일 이름이 같으므로 둘 중 하나만 쓴다. 보호 수준이 다르다는 점에 유의한다. KMS로 감싼 키링은 키를 한 번 더 암호화하지만, 시크릿 매니저는 감싸지 않은 키를 공급자 접근 권한으로만 보호한다.
6. 회전은 `put` → `materialize` → `./deploy.sh <현재 이미지>` 순서로 한다. DB 비밀번호를 바꿀 때는 DB 역할의 비밀번호도 같은 값이 되어야 한다. compose의 grants 단계가 `API_DB_PASSWORD`/`WORKER_DB_PASSWORD`로 역할 비밀번호를 설정한다.

## 권한 (최소 범위)

| 공급자 | 서버 (`materialize`) | 회전 담당 (`put`) |
|---|---|---|
| AWS | 해당 ARN의 `secretsmanager:GetSecretValue` (KMS CMK로 암호화했다면 그 키의 `kms:Decrypt`) | `secretsmanager:PutSecretValue` |
| GCP | 시크릿별 `roles/secretmanager.secretAccessor` | `roles/secretmanager.secretVersionAdder` |
| OCI | 대상 시크릿·compartment로 제한한 `read secret-bundles` (instance principal이면 동적 그룹에 부여) | 시크릿 내용 갱신 권한 (`use secrets` 수준) |

API와 worker 컨테이너에는 클라우드 자격을 주지 않는다. 자격 파일은 host 관리자만 읽게 두고(0600), 정책 문법은 적용 전에 각 공급자 문서로 확인한다.

## 검증 범위

다음 Rust 테스트가 확인한다.

- **`host_secrets::store`**: 매니페스트 검증, 실패 시 기존 파일 보존, 따옴표·줄바꿈 거절, `json_key`, compose가 값을 글자 그대로 읽는지, `put`
- **`cloud::*`**: ARN·리소스·OCID 고정, GCP 응답·CRC 검사, 서명
- **`tests/secrets_cli.rs`**: 바이너리 입력 검사와 값 비노출, `deploy.sh`의 env 파일 순서와 거절 조건

클라우드 호출은 테스트 대역으로 확인했다. 실제 공급자·IAM 연결은 운영 환경에서 확인해야 한다.
