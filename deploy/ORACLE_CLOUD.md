# Oracle Cloud(OCI)에서 운영하기

AWS로 하던 일을 Oracle Cloud로도 할 수 있다. 사업자를 섞어 써도 된다. 예를 들어 서버는 OCI에 두고, 업로드는 R2, 원본 보관은 AWS S3에 둘 수 있다.

| 하는 일 | AWS | Oracle Cloud | 설정 |
|---|---|---|---|
| 서버 | EC2 (Graviton4: `-graviton4` 이미지) | Compute **A4(권장)** → `-ampere-a4` 이미지. A1 → `-ampere-a1`, AMD E 시리즈 → `-x86-64-v3`, 그 밖 → 공용 이미지 | 아래 "서버 이미지" |
| 개인정보·계좌 키 보호(KMS) | AWS KMS | **OCI Vault** 키 | `audeniq-secrets kms --provider oci` ([KMS 문서](../docs/BACKEND_SECURITY_2026-10-03.md)) |
| 비밀값(시크릿 매니저) | Secrets Manager | **OCI Vault Secrets** | 매니페스트 `"provider": "oci"` ([시크릿 매니저](../docs/SECRET_MANAGER.md)) |
| 서버 자격(키 파일 없이) | EC2 인스턴스 역할(IMDSv2) | **instance principal** | 아래 "키 파일 없이 쓰기" |
| 업로드 파일 저장소 | S3 | **Object Storage**(S3 호환 API) | `S3_ENDPOINT=https://<네임스페이스>.compat.objectstorage.<리전>.oraclecloud.com` ([저장소 문서](../docs/STORAGE_PROVIDERS.md)) |
| 원본 문서 보관(WORM) | S3 Object Lock | Object Storage 보존 규칙(버킷 전체) | `ARCHIVE_S3_*` ([원본 보관](../docs/DOCUMENT_ARCHIVE.md)) |
| DB 백업·PITR 사본 | S3 | Object Storage | `deploy/archive.sh` |
| 파트너(DSP) S3 전송 | 파트너 S3 | 파트너가 OCI 버킷을 주면 그대로 | 파트너 설정 `transport.kind=s3`, `endpoint`·`region`에 OCI 값 |
| 화면(Studio·관리자) | CloudFront + S3 (`deploy/aws`) | (없음) Cloudflare Workers 그대로 사용 | 아래 "화면 배포" |

## 서버 이미지

| OCI 인스턴스 | 이미지 태그 | 컴파일 대상 | 비고 |
|---|---|---|---|
| **A4** (VM.Standard.A4.Flex, AmpereOne M) — 운영 기준 | `sha-<commit>-ampere-a4` | `ampere1a` (ARMv8.6) | Ampere 권장값(`-mcpu=ampere1a`, AmpereOne M = AC04_1) |
| A2 (AmpereOne) | `sha-<commit>-ampere-a1` 또는 공용 | `neoverse-n1` | A4 이미지는 쓰지 않는다(Ampere: ampere1a 코드는 AC03에서 보장되지 않음) |
| A1 (Ampere Altra) | `sha-<commit>-ampere-a1` | `neoverse-n1` | |
| AMD E4/E5/E6 | `sha-<commit>-x86-64-v3` | `x86-64-v3` | AVX2 |
| Intel X9 / 그 밖 | `sha-<commit>` (공용) | | |

- **A4 최적화**:
  - API, worker와 오디오 엔진(audeniq-qc), 이미지 도구(audeniq-photo)까지 Rust와 C/C++ 의존성을 모두 같은 대상(`ampere1a`)으로 빌드한다.
  - 그래서 업로드 변환·QC 분석의 디코딩·FFT에서 ARMv8.6 명령(BF16·I8MM·SHA3·내적 등)을 쓴다.
- **배포 전 확인**: `check-cpu.sh`는 모든 코어가 컴파일러가 스스로 쓸 수 있는 v8.6 기능(`bf16 i8mm sha3 sha512 frint ilrcpc asimddp atomics …`)을 가졌는지 확인한다. 하나라도 없으면 DB를 건드리기 전에 거절한다. Graviton3/4는 통과하고, Altra(A1)는 거절된다.
- **CircleCI(운영 배포)**: `audeniq-production` 컨텍스트에 `BACKEND_CPU_VARIANT=ampere-a4`를 넣는다. 그러면 main마다 A4 이미지를 따로 빌드하고, 승인하면 그 이미지를 배포한다.
  - CircleCI ARM 머신은 A4가 아니어서 새 명령이 없을 수 있다. 그래서 빌드한 바이너리는 QEMU(`-cpu max`)로 실행해 확인한다.
  - 값을 비우면 지금처럼 공용 이미지를 배포한다.
- **GitHub Actions**: 수동 실행에서 `cpu_variant=ampere-a4`를 고르거나 저장소 변수 `BACKEND_CPU_VARIANT=ampere-a4`를 둔다.

- `deploy.sh`는 DB를 건드리기 전에 서버 CPU를 확인한다(`check-cpu.sh`). 맞지 않는 최적화 이미지는 거절한다.
- 자동 배포에서 쓰려면 GitHub 저장소 변수 `BACKEND_CPU_VARIANT=ampere-a1`을 둔다.
- 자원 값(`WORKER_CPUS` 등)은 실제 코어 수에 맞춘다. **A4는 1 OCPU = 2코어**(A1은 1 OCPU = 1코어, AMD·Intel은 1 OCPU = 2 vCPU). 예: A4 2 OCPU = 4코어 → `WORKER_CPUS=2.0`, `API_CPUS=1.0`, `PG_CPUS=1.0`. QC 자동 확장(`QUEUE_QC_MAX_CONCURRENCY` 기본 vCPU×2)과 미디어 슬롯은 컨테이너에 보이는 CPU 수로 맞춰진다.

## 키 파일 없이 쓰기 (instance principal)

OCI 인스턴스에서는 `~/.oci/config` 없이 그 인스턴스 자신의 신원으로 Vault를 쓸 수 있다.

1. 동적 그룹을 만든다. 예: `audeniq-prod`, 규칙 `ANY {instance.id = 'ocid1.instance.oc1.ap-seoul-1.…'}`.
2. 정책을 둔다. 대상 compartment·키·시크릿으로 좁힌다.
   ```
   Allow dynamic-group audeniq-prod to use keys in compartment audeniq where target.key.id = '<키 OCID>'
   Allow dynamic-group audeniq-prod to read secret-bundles in compartment audeniq
   ```
   시크릿을 회전하는 쪽(사람·CI)에만 `use secrets` 권한을 따로 준다.
3. 서버에 `~/.oci/config`를 두지 않는다. 그러면 `audeniq-secrets`가 instance principal을 쓴다. 강제하려면 `OCI_CLI_AUTH=instance_principal`.

동작 방식:
- 인스턴스 메타데이터(`http://169.254.169.254/opc/v2`, `Authorization: Bearer Oracle`)에서 인스턴스 인증서와 리전을 받는다.
- 리전 Auth Service(`https://auth.<리전>.oraclecloud.com/v1/x509`)에서 세션 토큰으로 바꾼다.
- 토큰은 메모리에만 있는 새 RSA 키에 묶이고, 만료 5분 전에 다시 받는다. 디스크에는 아무것도 쓰지 않는다.
- API·worker 컨테이너에는 클라우드 자격을 주지 않는 원칙은 그대로다. `audeniq-secrets`는 host에서 돈다.

## 업로드 저장소 주의

- OCI Object Storage는 CORS 헤더가 고정이라 버킷별로 설정할 수 없다.
- `audeniq-admin storage probe --origin https://studio.audeniq.com`으로 브라우저 직접 업로드가 되는지 먼저 확인한다.
- 안 되면 업로드는 R2·S3에 두고, OCI는 보관·백업에 쓴다([저장소 문서](../docs/STORAGE_PROVIDERS.md)).

## 원본 보관 주의

- OCI 보존 규칙은 버킷 전체에 걸린다. 그래서 계약 10년, 약관 동의 5년, 백업 35일처럼 접두어마다 다른 기한을 줄 수 없다.
- 보관 버킷은 AWS S3 Object Lock이나 R2 버킷 잠금을 권장한다. OCI에 둔다면 기한과 파기 정책을 따로 정한다.

## 화면 배포

- 지금 화면은 Cloudflare Workers가 서빙하고, `/api`는 Cloudflare Tunnel로 서버에 넘긴다.
- 서버가 OCI에 있어도 그대로 동작한다. `cloudflared`는 ARM64 이미지도 있다.
- AWS용 CloudFront 구성(`deploy/aws`)에 해당하는 OCI 구성은 따로 두지 않았다. OCI에는 CloudFront 같은 엣지 함수 CDN이 없어서다.

## 참고

- [Oracle: Amazon S3 Compatibility API](https://docs.oracle.com/en-us/iaas/Content/Object/Tasks/s3compatibleapi.htm)
- [Oracle: Calling Services from an Instance (instance principals)](https://docs.oracle.com/en-us/iaas/Content/Identity/Tasks/callingservicesfrominstances.htm)
- Oracle SDK 구현(토큰 교환 형식의 근거): [oci-python-sdk `auth/federation_client.py`](https://github.com/oracle/oci-python-sdk/blob/master/src/oci/auth/federation_client.py)
