# x86-64 / ARM64와 AWS Graviton4 백엔드

API, worker, migrate, admin을 모두 Linux amd64와 arm64로 빌드한다.
32비트 ARM은 지원 대상이 아니다. PostgreSQL·PgBouncer와 외부 분석기는
대상 아키텍처의 Debian 패키지/이미지를 사용한다. `CLOUDFLARED_IMAGE`의
고정 digest도 ARM64를 포함하는 manifest 또는 ARM64 이미지여야 한다.
x86 전용 digest를 복사해 사용하지 않는다.
운영 커널은 Landlock ABI 3 이상과 seccomp를 제공해야 한다.
배포 스크립트가 실제 컨테이너에서 parser 샌드박스를 검사한다.

## 이미지 선택

| 서버 | 커밋 태그 | Rust CPU / 네이티브 컴파일러 옵션 |
|---|---|---|
| 일반 x86-64 | `sha-<commit>` | `x86-64` / `-march=x86-64` |
| 일반 ARM64, Graviton2/3 | `sha-<commit>` | `generic` / `-march=armv8-a` |
| AMD/Intel AVX2 | `sha-<commit>-x86-64-v3` | `x86-64-v3` / `-march=x86-64-v3` |
| AWS Graviton4 | `sha-<commit>-graviton4` | `neoverse-v2` / `-mcpu=neoverse-v2` |
| Oracle Cloud A4 (AmpereOne M) | `sha-<commit>-ampere-a4` | `ampere1a` / `-mcpu=ampere1a` |
| Oracle Cloud Ampere A1, AWS Graviton2 이상 | `sha-<commit>-ampere-a1` | `neoverse-n1` / `-mcpu=neoverse-n1` |

공용 태그는 Docker가 아키텍처를 자동으로 선택한다. `main`, `latest`에도
같은 CPU 접미사를 사용할 수 있다. 운영에서는 커밋 태그를 받아 digest로
고정한다. Backend image의 수동 배포는 `cpu_variant=graviton4`, 자동 배포는
저장소 변수 `BACKEND_CPU_VARIANT=graviton4`를 사용한다.

CPU 기준선 확인은 샌드박스 검사와 DB 마이그레이션보다 먼저 실행한다.
Graviton4 최적화 이미지는 모든 노출된 코어가 Arm Neoverse V2
(implementer `0x41`, part `0xd4f`)인지와 커널의 LSE/SVE/SVE2 등 지원을
확인한다. CPU 정보가 감춰져 있거나 다른 ARM CPU라면 공용 이미지를 사용한다.
Ampere A1 이미지(`neoverse-n1`)는 모든 코어가 ARMv8.2 원자 연산·내적·RCpc
(`atomics asimddp lrcpc`)를 지원하는지만 본다. 그래서 Ampere Altra, AmpereOne, Graviton2/3/4에서 모두 실행된다.

## Graviton4 최적화

`neoverse-v2`는 Rust의 LSE 원자 연산과 해당 CPU의 벡터 명령·명령 스케줄링을
활성화한다. C/C++로 빌드되는 AWS-LC, jemalloc, 압축 코덱에도 같은 CPU
설정을 적용한다. 의존성 빌드와 최종 빌드가 동일한 옵션을 사용한다.
release 프로필은 기존의 opt-level 3, fat LTO, codegen-units 1을 유지한다.
`native`를 지정하지 않아 CI 러너 CPU가 배포 CPU 기준선에 섞이지 않는다.
Cargo의 target triple을 명시해 빌드 스크립트와 proc macro는 러너의 기본
CPU로 컴파일한다. Graviton4가 아닌 ARM 러너에서도 최적화 코드를 빌드할 수 있다.

기존 RustFFT의 기본 NEON 기능과 `FftPlanner`를 유지한다. FFT 알고리즘,
저장 지문 포맷, QC 임계값을 변경하지 않는다. 미디어 처리는 `audeniq-music`이 맡으며
같은 `TARGET_CPU`로 빌드되고, AVX2/NEON 커널은 실행 시 CPU 검출로 선택한다.

참고: [AWS Rust/LSE 가이드](https://github.com/aws/aws-graviton-getting-started/blob/main/rust.md),
[AWS Graviton4 컴파일러 옵션](https://github.com/aws/aws-graviton-getting-started/blob/main/c-c%2B%2B.md),
[RustFFT NEON](https://github.com/ejmahler/RustFFT#arm-aarch64-neon-support).

## 서버 자원 설정

이미지만 바꾸어도 CPU별 코드가 적용된다. 여러 코어를 활용하려면 기존
1 vCPU용 Compose 제한을 `production.env`에서 서버 크기에 맞게 올린다.
아래는 **4 vCPU / 16 GiB, DB·API·worker가 같은 서버에 있는 경우의 시작값**이다.
실제 처리량 보장은 아니며 동시 QC의 RSS와 API 지연 시간을 측정해 조정한다.

```dotenv
PG_CPUS=1.0
PG_MEM_LIMIT=2g
PG_SHARED_BUFFERS=512MB
PG_EFFECTIVE_CACHE_SIZE=2GB
API_CPUS=1.0
API_MEM_LIMIT=768m
WORKER_CPUS=2.0
WORKER_MEM_LIMIT=3g
WORKER_MAX_IN_FLIGHT=6
DATABASE_MAX_CONNECTIONS=12
QUEUE_QC_CONCURRENCY=2
AUDENIQ_QC_ASSET_PARALLELISM=2
AUDENIQ_AUDIO_PARSER_MEMORY_MIB=768
```

`AUDENIQ_QC_ASSET_PARALLELISM`은 API와 worker **각 프로세스**의 미디어 실행
슬롯(1–8)이다. 빈 값은 사용 가능한 CPU 수를 사용한다. 앨범별 병렬 처리를
늘려도 프로세스 전체 슬롯을 공유하므로 디코더가 무제한으로 늘지 않는다.
`QUEUE_QC_CONCURRENCY`는 동시에 처리할 큐 작업 수이며 미디어 슬롯과 구분한다.
`WORKER_MAX_IN_FLIGHT`는 QC 이외의 큐 작업까지 포함한다. audeniq-music qc 작업은
단일 스레드이며, 서로 다른 음원 작업을 병렬 실행한다.
parser의 주소 공간 상한은 RAM/RSS와 다르다.

## 로컬 빌드

Docker Buildx와 해당 CPU의 네이티브 빌더 또는 QEMU가 필요하다.
운영 CI는 x86/ARM 네이티브 러너를 각각 사용한다.

```sh
# 공용 이미지: 양쪽 아키텍처를 같은 manifest로 게시
docker buildx build --platform linux/amd64,linux/arm64 \
  -f deploy/Dockerfile -t YOUR_REGISTRY/audeniq:portable --push .

# Graviton4 전용 ARM64 이미지
docker buildx build --platform linux/arm64 -f deploy/Dockerfile \
  --build-arg TARGET_CPU=neoverse-v2 \
  -t YOUR_REGISTRY/audeniq:graviton4 --push .

# CPU 선택·배포 전 검사 회귀 테스트
python3 -m unittest discover -s deploy/tests -p test_cpu_compatibility.py -v
```

CPU 최적화 이미지의 QEMU 검사는 명령어 호환성과 CLI 실행만 확인한다.
실제 Graviton4 서버에서는 배포 전 `audeniq-admin upload-sandbox-check`,
DB 마이그레이션, API readiness, 음원 QC·지문 결과, 같은 입력으로 공용/최적화
이미지의 처리 시간·CPU 시간·RSS를 확인한다. 두 이미지에 같은 자원 제한을
사용한다. 현재 변경에는 Graviton4 실기 성능 향상률 측정이 포함되지 않는다.

패키지 검증·로그인·유휴 worker의 CPU/RAM 최적화와 재현 가능한 벤치마크는
[BACKEND_RESOURCE_PERFORMANCE.md](../docs/BACKEND_RESOURCE_PERFORMANCE.md)에 있다.
계좌 암호화·KMS 키 준비·인증 해시의 개선과 CI 벤치마크는
[CRYPTO_RESOURCE_PERFORMANCE.md](../docs/CRYPTO_RESOURCE_PERFORMANCE.md)에 있다.

## 이번 변경의 로컬 검증

- 배포 Python 테스트 44개 통과(CPU·빌드 옵션 회귀 14개 포함).
- 세 워크플로의 Actionlint, 배포 셸의 ShellCheck, Compose 기본값과
  위 4 vCPU 설정의 렌더링 검사 통과.
- 실제 `parser_sandbox.rs`를 ARM64 `generic`·`neoverse-v2` 대상으로
  크로스 컴파일하고 AArch64 ELF object 생성을 확인했다. 로컬의 일치하는
  ARM sysroot와 Rust 1.97.1을 사용했으며 운영 CI는 기존 1.98.1을 유지한다.
- `neoverse-v2` 원자 연산 코드에서 LSE `ldaddal` 명령 생성을 확인했다.
- 실제 Cargo 실행으로 x86 host 빌드 스크립트와 ARM64 최적화 대상 코드를
  분리하고, host 스크립트 실행과 target crate 컴파일 성공을 확인했다.

로컬 머신은 x86이며 전체 Docker 빌드에 필요한 디스크·메모리가 부족하다.
전체 빌드·DB 회귀와 x86/ARM 컨테이너 검증은
[Backend portability](https://github.com/TAE-OK-11/audeniq/actions/workflows/backend-portability.yml),
[Foundation](https://github.com/TAE-OK-11/audeniq/actions/workflows/foundation.yml),
[Backend image](https://github.com/TAE-OK-11/audeniq/actions/workflows/backend-image.yml)에서
실행한다. Backend image의 수동 검증 실행은 `deploy=false`를 사용한다.
Graviton4 실기 성능 측정은 별도로 필요하다.
