# 암호화·해시 자원 최적화 — 2026-10-04

## 개선한 경로

DB에서 현재 암호화되는 계좌번호 필드(`portal.payout_accounts.account_cipher`),
AWS/GCP KMS 키 준비, 비밀번호와 서비스/세션 토큰 인증 경로를 다룬다.
계좌 암호문 형식·키 버전·조직/필드/header AAD, AES-256-GCM의 랜덤 nonce와
인증 tag, Argon2id의 메모리·반복 비용을 유지한다.

| 경로 | 기존 반복 비용 | 변경 |
|---|---|---|
| 계좌 암복호화 | 호출마다 AES/GHASH 키 스케줄 생성 | 키링 생성 시 버전마다 한 번 준비하고 공유 |
| 계좌 암호화 | 출력 Vec 확장, 별도 ciphertext Vec와 AAD Vec | 정확한 크기의 출력 버퍼 하나에 암호화, AAD는 고정 스택 배열 |
| 계좌 복호화 | AAD Vec, plaintext Vec, plaintext String 복사 | 한 버퍼에 복호화하고 같은 할당을 Zeroizing String으로 이동 |
| 서비스 인증 | 요청마다 양쪽 SHA-256과 두 Vec 할당 | 설정된 서비스 비밀의 digest는 AppState 생성 시 한 번 계산; 제공 값은 스택 digest로 constant-time 비교 |
| 로그인 세션 생성 | 새 토큰을 CSRF·DB 바인딩용으로 두 번 해시 | 한 번 계산한 같은 digest 재사용 |
| owner 재암호화 배치 | 최대 100행, 행마다 UPDATE와 audit INSERT | 잠긴 100행의 UPDATE와 개별 감사 기록을 한 SQL 문으로 묶음 |
| KMS materialize/rewrap | 키 버전별 직렬 네트워크/CLI 호출 | 기본 동시 2개, 1–4 범위로 제한된 병렬 처리 |

100행의 재암호화에서 쓰기·감사용 SQL 호출은 **200회에서 1회**로 줄었다.
owner 확인·행 선택·트랜잭션 시작/종료는 별도로 유지한다. 이는 호출 횟수이며
DB CPU나 실제 처리 시간의 감소율 측정이 아니다. 암호화된 출력과 ID 배열만
최대 100행까지 모으므로 일괄 처리용 메모리는 그 범위 안에서 늘어난다.
평문 계좌번호는 각 반복이 끝날 때 지운다.

## 키·평문 수명과 실패 처리

- 준비된 cipher는 기존 키링 수명 동안 유지한다. 버전 수는 기존 최대 64개다.
  AES/GHASH 상태가 Drop에서 zeroize되도록 production 컴파일에서도 trait
  조건을 강제한다. 원래 AES 키 바이트/hex 입력은 준비 후 지운다.
  준비된 스케줄은 raw 32바이트 키보다 크므로 작은 상주 메모리 비용을 추가한다.
- 암호화 실패 시 입력이 들어 있는 버퍼를 zeroize한다. 인증 실패·잘린 입력·
  잘못된 UTF-8 경로의 복호화 버퍼도 zeroize한다. 평문 결과는 기존
  `Zeroizing<String>`을 유지한다. 이전 nonce+ciphertext 형식도 복호화한다.
- 재암호화의 `FOR UPDATE SKIP LOCKED`와 100행 제한을 유지한다.
  계좌별 감사 event/request UUID도 유지한다. 인증 또는 감사 INSERT가 실패하면
  배치 전체가 rollback된다. 모든 기록이 성공한 뒤에만 commit한다.
- KMS는 기존처럼 host 준비 단계에서 사용한다. API는 tmpfs 키링을 읽어
  시작하며 계좌 요청에서 KMS를 호출하지 않는다. 새 요청별 키 캐시나
  자격 증명 캐시는 추가하지 않았다. 키 회전·서비스 비밀 변경 후에는
  기존 운영 절차대로 새 AppState/프로세스를 시작한다.
- KMS ARN/resource·EncryptionContext/AAD·TLS·CRC32C 검사, 호출별 timeout과
  AWS retry 설정은 유지한다. 모든 병렬 작업이 완료되고 모든 키 버전이
  성공해야 파일을 원자적으로 교체한다. 실패 시 기존 파일을 유지한다.

KMS 동시 실행을 늘리면 네트워크 대기 시간이 겹치지만 AWS CLI 프로세스의
순간 RAM 사용량도 늘 수 있다. 메모리가 작은 서버에서는 다음처럼 직렬
모드를 사용한다. 전체 KMS 호출 수는 버전 수에 따라 동일하다.

```sh
python3 deploy/kms-keyring.py materialize \
  --bundle /opt/audeniq/payout-wrapped-keyring.json --kms-concurrency 1
```

현재 이메일·일반 프로필은 기존 권한/접근 기록으로 보호하는 DB 컬럼이다.
이 코드에서 전체 DB 볼륨의 EBS/RDS KMS 연결이나 그 컬럼들의 암호화를
구성하지 않는다. DB 백업은 기존 `pg_dump | age` 암호화 스트림을 유지한다.
공급자 선택·키 회전·복원 절차는
[BACKEND_SECURITY_2026-10-03.md](BACKEND_SECURITY_2026-10-03.md)에 있다.

## 검증과 벤치마크

- 실제 auth/domain/packages/payout_keys/password_work/worker poll 소스를
  연결한 로컬 격리 Rust 테스트 **36개 통과**. 이전 암호문과의 양방향 호환,
  독립적인 이전 API로 계산한 ciphertext/tag 바이트 일치, 중첩 JSON 해시,
  키 버전·조직·모든 암호문 바이트 변조, 잘린 입력, 잘못된 UTF-8,
  공유 cipher의 동시 사용, Argon2 성공/실패·랜덤 salt를 포함한다.
- KMS 공급자/병렬 작업 테스트 **19개**, 키링·실제 age 백업 테스트 **8개**
  로컬 통과. 네트워크는 테스트 대역이며 실제 IAM/KMS 성능 측정은 아니다.
  GitHub Actions의 KMS·백업·업로드/업데이트 테스트도 **49개 통과**했다.
- PostgreSQL 회귀는 105행을 100/5/0행으로 처리하는 경계, 행별 감사 기록,
  인증·감사 실패의 원자성, 다른 트랜잭션이 잠근 행의 건너뛰기를 검사한다.
  전체 Cargo 빌드·Clippy·DB 통합 검증은 GitHub Actions에서 실행한다.
- `Backend portability`는 x86-64와 ARM64 네이티브 러너에서 바이너리·회귀
  테스트와 `backend_resources`, `crypto_resources` 벤치마크를 실행한다.
  로그는 `backend-resource-benchmarks-X64/ARM64` 아티팩트로 저장한다.
  CI dev 프로필 비교는 운영 release/fat LTO 및 Graviton4 실기 결과와 구분한다.

```sh
cargo test --locked -p audeniq-core --lib payout_keys::
cargo test --locked -p audeniq-core --lib auth::
cargo test --locked -p audeniq-core --test crypto_performance
cargo bench --locked -p audeniq-core --bench crypto_resources
python3 -m unittest discover -s deploy/tests -p 'test_kms*.py' -v
```

벤치마크는 합성 키·계좌 문자열만 사용하며, 두 경로를 같은 의존성으로
워밍업하고 순서를 번갈아 5회 측정한다. CPU는 user+system 중앙값이고,
할당 payload는 System allocator에서 별도로 추적한다. 입력·준비된 cipher·
allocator 관리 비용을 제외하므로 RSS와 같지 않다. Argon2 비용을 낮춰
얻는 속도 비교는 포함하지 않는다. 이전 패키지·미디어 측정은
[BACKEND_RESOURCE_PERFORMANCE.md](BACKEND_RESOURCE_PERFORMANCE.md),
[AUDIO_QC_PERFORMANCE.md](AUDIO_QC_PERFORMANCE.md)에 있다.

참고: [AES-GCM in-place API](https://docs.rs/aes-gcm/0.11.1/aes_gcm/),
[Argon2 메모리 비용 API](https://docs.rs/argon2/0.6.0/argon2/struct.Argon2.html).

## 네이티브 CI 측정 결과

[2026-10-04 Backend portability 실행](https://github.com/TAE-OK-11/audeniq/actions/runs/37171628590)은
x86-64·ARM64 각각 Rust 회귀 296개(ignored 1개 제외), CPU/KMS Python 테스트
33개를 통과했다. 실제 PostgreSQL의 배치 경계·감사 실패/변조 rollback·잠긴 행
검사도 포함한다. 아래 값은 같은 dev 프로필과 의존성에서 순서를 번갈아
5회 비교한 CPU 중앙값이다. 운영 release/Graviton4 서버 측정과 구분한다.

| 경로 | x86-64 CPU µs 기존 → 변경 | ARM64 CPU µs 기존 → 변경 | 할당 횟수 기존 → 변경 | 순간 payload bytes 기존 → 변경 |
|---|---:|---:|---:|---:|
| 암호화 14자 | 14.076 → 8.969 | 15.472 → 7.245 | 7 → 1 | 134 → 50 |
| 복호화 14자 | 13.862 → 8.655 | 15.740 → 7.272 | 4 → 1 | 122 → 14 |
| 암호화 20자 | 19.643 → 14.507 | 17.662 → 9.426 | 7 → 1 | 140 → 56 |
| 복호화 20자 | 19.422 → 14.244 | 18.015 → 9.484 | 4 → 1 | 128 → 20 |
| secret 일치 비교 | 1.330 → 1.245 | 2.115 → 1.998 | 2 → 0 | 64 → 0 |

이 계좌 fixture에서 CPU 감소는 x86-64 26–38%, ARM64 47–54%다.
프로세스 전체 RSS 또는 API 지연 감소율은 아니다. 준비된 cipher는 양쪽
아키텍처에서 버전당 1,104바이트였고, raw 32바이트 키보다 크다.
키 64개일 때 cipher 상태만 약 69 KiB이며 BTreeMap 관리 비용은 별도다.
KMS 및 DB의 네트워크/전체 처리 시간은 이 마이크로벤치마크에서 측정하지 않는다.
