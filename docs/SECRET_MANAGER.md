# 시크릿 매니저 (AWS · GCP · Oracle Cloud)

DB 비밀번호, 서비스 비밀값, 계좌 암호화 키 같은 운영 비밀을 서버의 `production.env` 평문 대신 클라우드 시크릿 매니저에 보관한다. 세 공급자를 지원하며 한 매니페스트에서 섞어 쓸 수 있다.

| 공급자 | 서비스 | 시크릿 ID 형식 (고정) | 호출 도구 |
|---|---|---|---|
| `aws` | AWS Secrets Manager | 전체 ARN `arn:aws:secretsmanager:<region>:<account>:secret:<name>-<6자>` | `aws` CLI (SigV4, 리전 엔드포인트 고정) |
| `gcp` | GCP Secret Manager | `projects/<project-id>/secrets/<secret>` | `gcloud` CLI |
| `oci` | OCI Vault Secrets | `ocid1.vaultsecret.oc1.<region>.<id>` | `oci` CLI (API 키 또는 `OCI_CLI_AUTH=instance_principal`) |

이름이나 별칭이 아닌 정확한 ID만 받는다. 응답의 ARN / 시크릿 이름 / OCID가 고정한 값과 다르면 거절한다. GCP는 응답 CRC32C를 검사한다. 이 도구는 시크릿·볼트·IAM을 만들지 않는다. 시크릿은 운영자가 먼저 만든다.

## 동작

- `materialize`: 매니페스트의 모든 시크릿을 읽는다. 하나라도 실패하면 기존 파일을 바꾸지 않는다. 모두 성공했을 때만 원자적으로 쓴다.
  - 환경변수 항목은 `/dev/shm/audeniq-env/secrets.env`에 쓴다. host 전용 tmpfs이고 디렉터리는 0700, 파일은 0600이며 어떤 컨테이너에도 마운트하지 않는다.
  - 파일 항목(예: 계좌 키링)은 `/dev/shm/audeniq-secrets/<file>`에 쓴다. 그룹 10001, 0640이며 API가 읽기 전용으로 마운트한다.
- `put`: 이미 있는 시크릿에 새 버전을 올린다. 값은 0600 파일에서만 읽고 명령 인자에 넣지 않는다. 요청 파일은 tmpfs에 만들고 바로 지운다.
- `check`: 매니페스트 형식만 검사하며 네트워크를 쓰지 않는다.
- 어떤 경우에도 비밀값을 출력·로그에 남기지 않는다. 실패 시 예외 종류만 출력한다.

`secrets.env`의 값은 작은따옴표로 감싸 compose가 그대로 읽는다(`$`, `#`, `"`, 공백 포함). 작은따옴표·줄바꿈이 든 값은 거절한다. DB 비밀번호는 `DATABASE_URL`에 들어가므로 URL-safe 값을 쓴다.

## 매니페스트

`/opt/audeniq/secrets-manifest.json`, 0600. 비밀값은 들어 있지 않지만 어떤 시크릿을 쓰는지 드러나므로 비공개로 둔다.

```json
{"version": 1, "secrets": [
  {"env": "POSTGRES_PASSWORD", "provider": "aws",
   "secret_id": "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/db-owner-AbCdEf",
   "json_key": "password"},
  {"env": "API_DB_PASSWORD", "provider": "gcp", "secret_id": "projects/audeniq-prod/secrets/api-db-password"},
  {"env": "WORKER_DB_PASSWORD", "provider": "gcp", "secret_id": "projects/audeniq-prod/secrets/worker-db-password", "version": "3"},
  {"env": "EDGE_SERVICE_SECRET", "provider": "oci", "secret_id": "ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexample"},
  {"env": "S3_SECRET_ACCESS_KEY", "provider": "oci", "secret_id": "ocid1.vaultsecret.oc1.ap-seoul-1.amaaaaaaexample2"},
  {"file": "payout-keyring.json", "provider": "aws",
   "secret_id": "arn:aws:secretsmanager:ap-northeast-2:123456789012:secret:audeniq/payout-keyring-GhIjKl"}
]}
```

- `env`: compose 환경변수 이름. `AUDENIQ_IMAGE`는 `deploy.sh`가 관리하므로 쓸 수 없다.
- `file`: 런타임 키 폴더에 쓸 파일 이름.
- `json_key`: JSON 시크릿의 한 필드를 고른다. 예를 들어 DB 시크릿 하나에 `host` / `username` / `password`를 함께 두고 항목별로 나눠 쓸 수 있다.
- `version`: 버전 고정. AWS는 버전 스테이지(기본 `AWSCURRENT`), GCP는 버전 번호(기본 `latest`), OCI는 스테이지(기본 `CURRENT`)다.

현재 compose는 DB 호스트(`postgres`/`pgbouncer` 컨테이너)와 DB 사용자 이름을 고정해 쓰므로, 시크릿으로 바꿀 수 있는 DB 값은 비밀번호다. 외부 DB로 옮길 때는 compose가 읽는 변수(`DB_HOST` 등)를 추가한 뒤 같은 방식으로 매핑한다.

## 운영 절차

1. 공급자 콘솔이나 CLI로 시크릿을 만들고 현재 값을 넣는다. 또는 빈 시크릿을 만든 뒤 아래 `put`으로 값을 넣는다.
2. 매니페스트를 작성하고 검사한다.

   ```bash
   python3 deploy/secret-manager.py check --manifest /opt/audeniq/secrets-manifest.json
   ```

3. tmpfs로 받는다(재부팅 후마다, compose 시작 전).

   ```bash
   python3 deploy/secret-manager.py materialize --manifest /opt/audeniq/secrets-manifest.json
   ```

4. `production.env`에 `SECRETS_ENV_FILE=/dev/shm/audeniq-env/secrets.env`를 설정한다. `deploy.sh`는 이 파일이 없거나, tmpfs가 아니거나, 배포 사용자 소유 0600이 아니거나, `AUDENIQ_IMAGE`를 담고 있으면 compose 실행 전에 멈춘다. 이 파일은 `production.env` 뒤에 읽으므로 같은 이름의 값을 덮는다. 전환이 확인되면 `production.env`에서 해당 비밀값을 지운다.
5. 키링을 시크릿으로 받는 경우 `PAYOUT_KEYRING_FILE=/run/audeniq-keys/payout-keyring.json`을 설정한다(KMS-off 로컬 키링 모드). KMS 모드(`kms-keyring.py materialize`)와 같은 파일 이름을 쓰므로 둘 중 하나만 사용한다. KMS로 감싼 키링이 더 강한 보호이며, 시크릿 매니저는 감싸지 않은 키를 공급자 접근 권한으로 보호한다.
6. 회전: 새 값을 0600 파일로 만든 뒤 아래를 실행한다.

   ```bash
   python3 deploy/secret-manager.py put --manifest /opt/audeniq/secrets-manifest.json --target API_DB_PASSWORD --value-file /dev/shm/new-value
   python3 deploy/secret-manager.py materialize --manifest /opt/audeniq/secrets-manifest.json
   ./deploy.sh <현재 이미지>
   ```

   DB 비밀번호 회전은 DB 역할의 비밀번호도 같은 값으로 바꿔야 한다(grants 단계가 `API_DB_PASSWORD`/`WORKER_DB_PASSWORD`로 역할 비밀번호를 설정한다). `json_key` 항목은 JSON 전체를 공급자 쪽에서 갱신한다.

## 권한 (최소 범위)

| 공급자 | 서버(materialize) | 회전 담당(put) |
|---|---|---|
| AWS | 해당 ARN의 `secretsmanager:GetSecretValue` (KMS CMK로 암호화했다면 그 키의 `kms:Decrypt`) | `secretsmanager:PutSecretValue` |
| GCP | 시크릿별 `roles/secretmanager.secretAccessor` | `roles/secretmanager.secretVersionAdder` |
| OCI | `read secret-bundles` (대상 시크릿·compartment로 제한) | `use secrets` 수준의 시크릿 갱신 권한 |

OCI 서버에서는 instance principal(동적 그룹)을 권장한다. 그 외 host에서는 각 CLI의 자격 파일을 host 관리자만 읽게 둔다. API·worker 컨테이너에는 클라우드 자격을 주지 않는다. 정책 문법은 적용 전 각 공급자 문서로 확인한다.

## 검증 범위

`deploy/tests/test_secret_manager.py`(CI `kms-backup` 작업에서 실행)가 다음을 확인한다.

- 매니페스트 검증
- 세 공급자 CLI 호출: 값이 명령 인자에 없음, 요청 파일 0600·삭제, 고정 ID 검사, GCP CRC32C
- 실패 시 기존 파일 보존
- compose가 값을 글자 그대로 읽음
- `deploy.sh`의 env 파일 순서와 거절 조건

클라우드 호출은 테스트 대역이다. 실제 공급자·IAM 연결은 운영 환경에서 확인해야 한다.
