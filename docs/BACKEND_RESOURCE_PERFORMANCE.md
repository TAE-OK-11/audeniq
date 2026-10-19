# Backend CPU·RAM 최적화 — 2026-10-04

## 적용한 변경

- `domain::validate_package`는 내장된 네 종류의 JSON 스키마를 프로세스에서
  각각 한 번만 컴파일한다. 요청마다 스키마 JSON을 파싱하고 validator를
  만드는 비용을 없앴다. 캐시에는 고정 스키마만 들어가며 패키지 본문,
  검증 결과, 사용자 권한은 캐시하지 않는다.
- Consent/Verification의 자체 해시를 검증할 때 JSON 전체를 복제하지 않는다.
  원본 객체를 빌려 최상위 해시 필드만 제외하고 직렬화한다. 키 순서,
  중첩 데이터, SHA-256 입력 바이트와 검증 규칙은 기존과 같다.
- 회원가입·로그인·비밀번호 변경의 실행 슬롯을 `spawn_blocking` 작업이
  직접 소유한다. 요청이 취소돼도 진행 중인 Argon2가 끝날 때까지 슬롯을
  유지해 기본 동시 실행 한도 2개를 지킨다. 비밀번호 변경은 검증과 새
  해시 생성 사이에서도 같은 슬롯을 사용한다. Argon2 비용·랜덤 salt와
  비밀번호/세션 재검증은 유지한다.
- worker의 빈 큐 조회 최대 간격은 2초에서 5초로 늘렸다. 각 루프의
  정상 상태 claim 시도는 초당 0.5회에서 0.2회로 **60% 감소**한다.
  이는 조회 횟수 계산이며 프로세스 CPU 감소율 측정이 아니다.
  알림 future를 실행 슬롯/DB 조회 전에 만들어, 빈 조회와 대기 사이의
  `notify_waiters` 알림도 받는다. 신규 작업의 LISTEN/NOTIFY 경로는 즉시
  깨우며, 알림 연결이 끊기거나 미래 시각에 예약된 재시도는 최대 5초
  간격의 polling으로 찾는다. 실행 슬롯 경쟁·DB 지연은 별도로 더해질 수 있다.

이 변경은 아키텍처별 명령어를 추가하지 않아 기존 x86-64/ARM64 빌드에
공통으로 적용된다. 미디어 경로의 앞선 최적화와 측정은
[AUDIO_QC_PERFORMANCE.md](AUDIO_QC_PERFORMANCE.md)에 있다.

## 로컬 측정

Linux x86-64, Rust 1.98.1에서 실제 `domain.rs`를 `rustc -O`로 컴파일했다.
기준은 `b3bcdfe`의 알고리즘이며 새 벤치마크의 reference 구현에도 보관했다.
전체 라이브러리 빌드가 이 호스트의 메모리 한도를 넘어서, 동일한 벤치마크
소스를 실제 domain 모듈과 연결한 격리 harness를 사용했다. 의존성은 두
경로에 같은 캐시의 최적화된 빌드를 사용했다. 운영 release/fat LTO 또는
Graviton4 실기 측정은 아니다.

각 fixture를 미리 만들고 두 경로를 워밍업한 뒤, 실행 순서를 번갈아 5회
측정한 CPU 시간 중앙값이다. 한 회의 반복 수는 아래 표와 같다. CPU 시간은
`getrusage`의 user+system이며 측정 중 다른 빌드·테스트를 실행하지 않았다.

| 패키지 입력 | 반복/회 | 검증 CPU µs, 기존 → 변경 | CPU 감소 | 할당 횟수/검증, 기존 → 변경 | 임시 할당 payload 피크 bytes, 기존 → 변경 |
|---|---:|---:|---:|---:|---:|
| Consent: signer/ref 각각 1개 | 500 | 28.054 → 1.298 | 95.4% | 325 → 1 | 23842 → 64 |
| Consent: signer/ref 각각 1000개 | 100 | 241.210 → 145.450 | 39.7% | 2323 → 1 | 155340 → 64 |
| Verification: 네 ID/ref 배열 각각 16개 | 500 | 33.396 → 5.464 | 83.6% | 398 → 1 | 31625 → 64 |
| Verification: 네 ID/ref 배열 각각 2048개 | 30 | 818.033 → 515.667 | 37.0% | 8526 → 1 | 584329 → 64 |

할당 추적은 단일 스레드에서 System allocator를 감싼 별도 측정이며,
타이밍 중에는 추적하지 않는다. **payload 피크는 RSS가 아니다.** 입력
fixture, 이미 준비된 캐시, allocator 관리 비용, 스택, 다른 프로세스는
제외한다. 운영 API/worker의 할당기(jemalloc)나 전체 서비스 RAM 감소율을 나타내지
않는다. 표의 남은 64바이트 할당은 해시의 hex 문자열이다.

네 validator를 모두 준비하면 총 37609바이트의 할당 payload가 캐시에
남는다. 초기화 중 피크는 59105바이트였다. 이 작은 상주 비용을 추가해
반복 컴파일과 큰 패키지의 임시 복사를 없앤다. 작은 단일 요청에서 전체
프로세스 RSS까지 줄어든다는 의미는 아니다. SHA-256 sink 자체는 기존
스트리밍 구현을 유지했다.

## Allocator (2026-10-19)

API와 worker의 전역 할당기는 jemalloc(`tikv-jemallocator`)이다. 실제 `audeniq-api`(release,
thin LTO)를 PostgreSQL에 붙여 같은 부하를 주고 비교했다. API 2코어, 부하 생성기(oha) 2코어로 나누고
매 회차 새 프로세스로 시작했다.

부하: MCP `tools/list`(약 54 KB JSON) 동시 32개 20초 → MCP 도구 호출 동시 32개 15초 →
`/api/notices`(공지 40건, DB) 동시 32개 15초 → 로그인(Argon2, 해시당 19 MiB) 동시 8개 20초 → 30초 유휴.

1차(7종, `LD_PRELOAD`, 2회 평균)에서 glibc는 유휴 후 168 MB를 반환하지 않았고 snmalloc은 3배 느렸다.
jemalloc도 preload로는 3배 느리게 나왔지만, 크레이트로 직접 링크하면 아래처럼 정상이었다
(preload 측정의 문제). rpmalloc은 Rust 크레이트가 수년째 갱신되지 않아 제외했다.

2차(상위 3종, 직접 링크, 3회, 순서를 돌려 가며):

| | mimalloc v3 | mimalloc v2 | **jemalloc** |
|---|---|---|---|
| 시작 RSS | 43 MB | 53 MB | **23 MB** |
| 부하 후 30초 유휴 RSS | 58 MB | 114 MB | **31 MB** |
| 최대 RSS | 175 MB | 166 MB | **122 MB** |
| MCP tools/list req/s (p99) | 1,272 (47 ms) | 1,336 (45 ms) | 1,228 (48 ms) |
| MCP 도구 호출 req/s (p99) | 7,595 (8.8 ms) | 7,944 (8.4 ms) | 8,212 (8.2 ms) |
| 공지 조회 req/s (p99) | 1,221 (53 ms) | 1,395 (36 ms) | 1,740 (27 ms) |
| 로그인/s (p99) | 95 (117 ms) | 95 (119 ms) | 71 (140 ms) |

jemalloc은 메모리를 30~50% 덜 쓰고 DB가 섞인 요청은 더 빠르다. Argon2 로그인 처리량은 25% 낮지만
로그인은 IP·계정당 횟수 제한이 있어 이 처리량에 닿지 않는다. ARM 이미지는 64 KiB 페이지로 빌드한다
(`deploy/build-target.sh`, 4/16/64 KiB 커널 모두에서 실행).

## 검증과 재현

- 실제 domain, packages, auth, password_work, worker poll 소스와 기존
  config/error 테스트를 격리 harness에서 실행: **28개 통과**.
  테스트는 네 패키지의 정상/비정상 입력, 변조 후 재검증, 스키마 재사용,
  대형·한글·escape·중첩 JSON의 해시 바이트 일치, 취소된 blocking 작업의
  슬롯 보유, 연속 작업의 슬롯 재사용과 panic 후 해제를 확인한다.
  worker는 조회 중 알림, 같은 큐의 복수 waiter, 알림 없는 polling을 검사한다.
- auth endpoint 전체를 실제 의존성과 최소 AppState/audit 연결 stub으로
  타입 검사했고, 반환 future의 `Send`도 확인했다. DB 경로는 실행하지 않았다.
- 실제 worker binary와 테스트의 타입 검사는 기존 core metadata에 연결해
  통과했다. 새 전체 core 빌드 성공을 대신하는 검사는 아니다.
- 변경 모듈·테스트와 벤치마크의 Clippy `-D warnings` 통과. 격리 harness의
  기존 생성 상태 enum에만 acronym lint를 제외했다.
- x86/ARM 네이티브 CI에 worker binary 회귀 테스트를 추가했다. 기존
  라이브러리 테스트에 새 domain/password 테스트도 포함된다.
- 전체 `cargo check`는 incremental 제거·단일 컴파일러 스레드로 재시도해도
  OOM killer의 SIGKILL로 완료하지 못했다. 전체 서비스/DB 통합 테스트,
  네이티브 ARM CI 실행, 운영 CPU·RSS 및 Graviton4 실기 측정은 미실행이다.

충분한 메모리가 있는 개발/CI 환경의 명령:

```sh
cargo test --locked -p audeniq-core --lib domain::
cargo test --locked -p audeniq-core --lib packages::
cargo test --locked -p audeniq-core --lib auth::
cargo test --locked -p audeniq-core --lib password_work::
cargo test --locked -p audeniq-core --bin audeniq-worker
cargo clippy --locked -p audeniq-core --all-targets -- -D warnings
cargo bench --locked -p audeniq-core --bench backend_resources
```

Graviton4에서는 같은 입력·같은 자원 제한으로 공용 ARM 빌드와
`neoverse-v2` 빌드를 각각 측정한다. CPU 옵션과 서버 설정은
[GRAVITON4.md](../deploy/GRAVITON4.md)를 따른다. 이 벤치마크는 패키지 경로의
비용을 분리하며 API 처리량·DB 부하·전체 RSS는 운영 부하 시험으로 확인한다.

# 2026-10-10 추가 — async 런타임 블로킹 제거·유휴 DB 왕복 감소

- 업로드 완료(`uploads::prepare`): 변환된 파일(최대 512 MiB)의 SHA-256을
  async 스레드가 아니라 이미 있던 `spawn_blocking` 안에서 계산한다. 1 vCPU
  API에서 큰 마스터 하나가 해시되는 동안 다른 요청이 모두 멈추던 문제를 없앤다.
- 배급 준비(`distribution::persist_ddex_messages`)와 `partner test-ern`: DSP마다
  도는 xmllint XSD 검증을 블로킹 풀에서 실행한다
  (`ddex_xsd::validate_ern_382_xml_blocking`). 열린 트랜잭션을 잡은 채 런타임
  스레드를 막지 않는다. xmllint 종료 대기는 50 ms 고정 대신 2 ms부터 두 배씩
  (최대 50 ms) 늘려, 검증마다 평균 ~25 ms의 불필요한 대기를 줄인다.
- 목록 응답의 인라인 이미지(`upload_safety::visible_inline_files`): data: URL마다
  SHA-256을 두 번 계산하던 것을 한 번으로 줄였다. 순회 순서·판정은 동일하다.
- worker 만료 lease 회수: 큐마다 5초마다 BEGIN·UPDATE·claim·COMMIT(4왕복)을
  돌던 것을, 먼저 `jobs_expired_lease` 인덱스로 존재만 확인해 없으면 일반
  claim(총 2왕복)으로 끝낸다. 만료 건이 있을 때의 동작은 같다.
- 점검 게이트(`site_content::gate`): 요청마다 캐시된 점검 행을 깊은 복사하던
  것을 `Arc` 공유로 바꾸고, 예정된 점검이 없으면 상태 JSON을 만들지 않는다.

검증: `cargo clippy -p audeniq-core --all-targets -D warnings`, lib 테스트 238개,
`foundation`·`distribution`·`portal`·`partner_onboarding`·`stage2`·
`sandbox_regressions` 통합 테스트(PostgreSQL, audeniq-qc/photo 포함)를 변경 전후로
돌려 실패 목록이 동일함(ClamAV가 필요한 업로드 VERIFIED 테스트만 이 환경에서 실패)을 확인했다.
