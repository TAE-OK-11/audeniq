# Audio QC 최적화 — 2026-10-03

Stage 1 worker는 `qc::check_audio_with_fingerprint`로 QC와 지문을 한 번의
FFmpeg 디코딩에서 계산한다. QC v6과 fingerprint v2는 유지한다.

- 일반 PCM WAV와 native FLAC은 Rust 헤더 분석기로 기술 정보를 읽어
  FFprobe 실행을 없앤다. FLAC은 STREAMINFO까지 42바이트만 읽고, WAV는
  고정 크기 버퍼와 최대 4096개 chunk 탐색을 사용한다. 애매한 헤더나
  확장 형식은 기존 sandbox FFprobe로 처리한다. 헤더 분석 후에도 전체
  디코딩·무결성·loudness·true peak 검사는 수행한다.
- 지문 구간의 PCM을 쌓지 않고, 2048샘플의 겹치는 프레임으로 즉시 FFT를
  계산한다. 작업당 최대 90초 × 11025Hz × 4바이트 = 약 3.8MiB의 PCM
  보관을 없앤다. 구간마다 별도의 에너지 이력을 사용한다.
- 전체 FFT 에너지 행렬과 구간마다 만드는 스레드 풀을 없앤다. 이전·현재
  에너지와 FFT scratch만 사용하며, 병렬 처리는 기존 asset 동시 실행에 맡긴다.
- tap 버퍼와 head/middle/tail 구간이 겹치는 부분만 변환한다. 구간 사이의
  샘플은 읽어서 버리고, FFmpeg의 연속 리샘플링은 유지한다.
- PCM 측정의 샘플별 나눗셈을 없애고, 잡음 검사에서 에너지 배열의 복사를
  없앤다. 반복 구간 검사에서는 오차가 한도를 넘은 lag를 즉시 제외한다.
- QC와 지문 디코더의 FFmpeg 필터·코덱 스레드는 1개로 지정한다.
  OpenMP·BLAS의 암묵적 스레드 수와 glibc malloc arena 수도 제한한다.

기존 `check_audio_with_fp_tap` API는 PCM 구간이 필요한 호출자를 위해 유지한다.
QC 임계값·해시 입력·구간 좌표·FFT 연산 순서는 바꾸지 않는다.

## 실패 처리와 자원 경계

- 미디어 실행 슬롯을 앨범마다 따로 생성하지 않고 프로세스 전체에서 공유한다.
  기본값은 사용 가능한 CPU 수(1–8)이며, `AUDENIQ_QC_ASSET_PARALLELISM`도
  1–8로 제한한다. 전체 작업을 강제로 1개씩 실행하는 모드는 아니다.
  다운로드부터 분석이 끝날 때까지 슬롯을 유지한다. 시작된 `spawn_blocking`
  작업은 async 취소 후에도 자신의 슬롯을 보유하므로 중복 디코더가 누적되지 않는다.
- 외부 분석기의 파이프는 nonblocking read와 같은 deadline을 사용한다.
  자식이 stdout을 닫고 멈추거나, 하위 프로세스가 파이프를 계속 보유해도
  무기한 `wait`/`join`하지 않는다. Drop에서 프로세스 그룹을 종료하고
  자식을 회수한 뒤 reader를 합류시킨다. 부모 사망 시에도 자식을 종료한다.
- reader는 `thread::Builder`의 실패 가능한 생성과 256KiB 스택을 사용한다.
  스레드 생성 실패는 분석 오류로 전달한다.
- NaN·무한대·비정상 채널 수·샘플레이트는 할당/실행 전에 거절한다.
  PCM 작업량은 선언 길이 + max(1초, 1%)이며 최대 4시간으로 제한한다.
  4시간 초과나 작업량 초과는 `Unavailable`로 처리한다. 시간 제한은 기존
  길이 비례 예산(최대 1시간)을 유지하고, probe 설정은 최대 300초로 제한한다.
- audio parser의 주소 공간 상한은 기본 768MiB다.
  `AUDENIQ_AUDIO_PARSER_MEMORY_MIB`는 512–1024MiB 범위이며, 더 낮은
  상속 rlimit은 그대로 존중한다. **주소 공간은 실제 RAM/RSS와 다르다.**
  이 환경의 FFmpeg는 공유 라이브러리·스택 매핑 때문에 256MiB 상한에서
  정상 파일도 실패하여 그 값을 기본값으로 쓰지 않았다.
- 디코더의 메모리/스레드 부족, signal 종료, 실행/로더 실패는 기술 재시도로
  분류한다. 분석 환경의 실패 때문에 정상 파일에 재업로드를 요구하지 않는다.

Rust 헤더 분석기를 새 경로로 사용하고, 전체 디코딩·EBU R128·연속 리샘플링은
현재 FFmpeg를 유지했다. 엔진을 교체하려면 이 측정 값과 저장된 지문의 호환성을
검증해야 한다. 다른 엔진이 더 빠르다는 비교 결과는 이번 변경에 포함하지 않았다.

## 로컬 측정

기준 소스는 `b3bcdfe`, 비교 대상은 이 변경의 작업 트리다. Linux x86-64,
1 vCPU 환경에서 같은 의존성으로 소스 모듈을 `rustc -O`로 빌드했다.
기준 경로는 `check_audio_with_fp_tap` 다음에 `fingerprint_from_segments`를
호출하고, 새 경로는 `check_audio_with_fingerprint`를 호출했다.

각 파일에 대해 순서를 번갈아 5회씩 실행한 중앙값이다. CPU 시간은
`getrusage`의 user+system 시간이며, 총 CPU는 분석 프로세스와 자식 프로세스의
합이다. RSS는 `/proc`를 5ms 간격으로 읽은 피크다. 실행 중 다른 빌드·테스트는
하지 않았다.

| 입력 FLAC | 총 CPU 초, 기존 → 변경 | 분석 프로세스 RSS KiB, 기존 → 변경 | FFmpeg 포함 RSS 합계 KiB, 기존 → 변경 |
|---|---:|---:|---:|
| 210초, 48kHz, stereo, 16bit | 1.2563 → 1.1183 | 7504 → 4464 | 69076 → 66004 |
| 210초, 192kHz, stereo, 24bit | 2.3546 → 2.2453 | 7572 → 4436 | 78824 → 75804 |
| 90.001초, 44.1kHz, mono, 16bit | 0.4450 → 0.3718 | 7572 → 4452 | 66608 → 63268 |

독립 분석 프로세스의 RSS는 약 40–41%, FFmpeg를 포함한 RSS 합계는 약 4–5%,
총 CPU 시간은 약 5–16% 줄었다. Rust 분석 프로세스 자체의 CPU 시간은
약 5–13% 줄었다. 이 수치는 기본 allocator를 쓰는 격리된 분석 실행의 결과이며,
DB·Tokio·캐시가 있는 실제 worker 전체의 메모리 감소율을 뜻하지 않는다.
RSS 합계는 공유 페이지도 프로세스마다 센다.

세 입력의 모든 QC 코드·상태·input hash·기술 정보·지문 바이트의 SHA-256이
기준 버전과 같았다. 성공 probe detail만 엔진에 독립적인 `audio probe ok`로
바뀌었고 다른 detail은 같았다. 이 1 vCPU 환경에서 총 스레드 피크는 두 버전
모두 13개였다. 전체 loudness/true peak 디코딩 비용은 계속 필요하다.

## 검증

- 변경 모듈의 실제 소스를 격리된 Rust test harness로 실행: **57개 통과**.
  부분 읽기·Interrupted·겹치는 구간·잘린 EOF·짧은 구간·FFT 프레임 경계와
  실제 FFmpeg 연속 PCM/지문의 일치를 포함한다. 헤더 정보와 FFprobe 비교,
  1000개 변형 FLAC 헤더, 비정상 메타데이터, 자식/하위 프로세스 deadline,
  취소된 blocking 작업의 슬롯 보유, 자식의 MemoryError와 상속 상한도 검사한다.
- 반복 lag의 조기 종료는 전체 오차를 계산하는 기준 구현과 비교했다.
- 분석 프로세스와 자식에 768MiB 주소 공간 hard limit을 상속시켜
  210초/192kHz/stereo/24bit FLAC의 전체 QC·지문 처리 성공을 확인했다.
- `cargo check --locked --offline -p audeniq-core --lib -j 1` 통과.
- 위 소스 모듈과 테스트의 Clippy `-D warnings` 통과.
- `cargo fmt --all -- --check`, `git diff --check` 통과.
- 전체 라이브러리 Clippy는 호스트 메모리 부족으로 종료되어 완료하지 못했다.
  커널 로그에서 OOM killer가 `clippy-driver`를 종료한 것을 확인했다.
  전체 서비스·DB 통합 테스트와 운영 부하 시험은 이 측정에 포함하지 않았다.

일반 개발 환경에서 관련 테스트를 실행하는 명령:

```sh
cargo test --locked -p audeniq-core --lib qc::tests -- --test-threads=1
cargo test --locked -p audeniq-core --lib fingerprint::tests -- --test-threads=1
cargo test --locked -p audeniq-core --lib analyzer_process::tests -- --test-threads=1
cargo test --locked -p audeniq-core --lib media_budget::tests -- --test-threads=1
cargo test --locked -p audeniq-core --lib parser_sandbox::tests -- --test-threads=1
cargo clippy --locked -p audeniq-core --lib -- -D warnings
```
