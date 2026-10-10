# 배급 파이프라인 아키텍처 (Job 중심 설계)

작성 2026-09-29. 대상: `crates/core` 백엔드(API·worker)와 PostgreSQL. 목표 7가지에 대해 지금 코드가 어디까지 와 있는지,
무엇이 빠졌는지, 어떤 순서로 옮기는지를 정리한다. 각 단계는 따로 배포할 수 있고, 앞 단계 없이도 동작을 깨지 않는다.

## 1. 목표

1. 배급 파이프라인은 job 중심: 신청 → 검증 → 변환 → DSP별 보정 → 승인 → 전달 → 추적이 각각 독립 작업이다.
2. PostgreSQL은 source of truth + transactional outbox. 작업 실행기는 수평 확장된다.
3. DSP 어댑터는 코어와 분리한다. Spotify·Apple·Melon 규격 변경이 코어로 번지지 않는다.
4. 파일 처리는 업로드/R2 격리 → 검사 → 정규화 → deliverable 생성으로 단계가 나뉜다.
5. 모든 작업에 idempotency, retry policy, timeout, DLQ, audit trail이 기본으로 들어 있다.
6. workflow state는 1급 데이터다. 관리자 화면이 내부 상태를 해석하지 않는다.
7. 처음부터 observability: `release_id` 하나로 DB 변경·작업 큐·DSP 요청·파일 처리 로그를 전부 따라간다.

## 2. 지금 상태

| 목표 | 이미 있는 것 | 빠진 것 |
|---|---|---|
| 1. job 중심 | `operations.jobs` 큐 하나에 단계별 job: `asset.analyze`, `stage1`(검증), `stage2`(권리 심사), `prepare_release`(식별자·패키지·DDEX), `delivery.stage`(DSP별 스펙 확인·스테이징), `delivery.enqueue`/`send`, `delivery.poll`/`ack`/`mark_live`/`reconcile`/`takedown` | **변환(정규화)이 job이 아니다**: WAV·ALAC 등 → FLAC이 API 업로드 완료 요청 안에서 동기로, 프로세스당 1개씩 돈다 |
| 2. PG = SoT + outbox, 수평 확장 | 업무 데이터와 같은 트랜잭션에서 enqueue, `operations.outbox` + `outbox.record`, `FOR UPDATE SKIP LOCKED` 클레임, lease·heartbeat·fencing token, LISTEN/NOTIFY 깨우기(큐별) — worker 프로세스를 여러 개 띄워도 안전 | 이벤트마다 `outbox.record` job이 따로 생겨 큐 부하가 약 2배 (작은 문제) |
| 3. DSP 어댑터 분리 | 전송 계층은 데이터 기반: 파트너별 JSON(`PARTNER_CONFIG_DIR`)으로 DDEX SFTP/S3, HTTP API, JSON+CSV 피드를 코드 변경 없이 켬 (`docs/PARTNER_DELIVERY.md`). `FileTransport` trait | **DSP 규격이 코드다**: `dsp_registry.rs`의 `Dsp` enum + 정적 `REGISTRY`(커버 최소 px, 샘플레이트, lead days, 필수 크레딧 …)를 코어 13개 모듈이 직접 import. 규격 하나 바뀌면 코어 수정·재배포 |
| 4. 파일 단계 분리 | 격리 키 `quarantine/` → ETag 고정 복사 `registered/` → `asset.analyze`(해시·QC·지문) → `prepare_release`의 패키지 | 정규화가 API 안(목표 1과 같은 문제). 단계별 R2 권한 분리는 worker 읽기 전용 토큰까지만 |
| 5. job 기본 계약 | `idempotency_key` UNIQUE(같은 키·다른 내용이면 충돌), `max_attempts` + 지수 백오프(5·2ⁿ초, 최대 1시간), lease 만료 재수거, `DEAD_LETTER` + 릴리스에 드러내기(`surface_dead_letter`), claim·retry·dead-letter 감사 로그 | **kind별 정책이 없다**: 재시도 횟수·백오프·실행 제한시간이 모든 kind에 같다. 실행 제한시간은 없고 lease 만료(최대 `JOB_LEASE_SECONDS`)가 대신한다 |
| 6. workflow state 1급 | `config/states.json`의 축 4개(`application_pipeline_status`, `dsp_eligibility_status`, `delivery_job_status`, `dsp_live_status`)와 허용 전이, `catalog.releases.status` | **한 릴리스의 진행 상황이 여러 테이블에 흩어져 있다**: releases.status, check_results, delivery_staging, delivery_jobs, live_bindings, jobs. 스태프 화면(`staff.rs`)이 그걸 조합해 해석한다 |
| 7. observability | JSON 구조 로그, 요청별 `x-request-id`, `operations.audit_events`(resource_id), `execution.delivery_attempts` | **release_id로 묶이지 않는다**: 로그 39곳 중 release를 담는 건 2곳, tracing span 0개, `operations.jobs`에 release 컬럼이 없어 payload JSON을 뒤져야 한다 |

요약: 뼈대(큐·outbox·idempotency·DLQ·감사)는 이미 목표에 가깝다. 새로 세울 것은 **추적(7), workflow 데이터(6), DSP 규격의 데이터화(3), 정규화 job(1·4), kind별 정책(5)** 다섯 가지다.

## 3. 목표 설계

### 3.1 Workflow 모델 (목표 1·6)

릴리스 제출 한 번(= revision 하나)이 **run** 하나, 파이프라인의 각 단계가 **step** 하나다. DSP별 단계는 DSP마다 step이 따로 있다.

```
workflow.runs   (id, org_id, release_id, revision_id, kind[SUBMISSION|UPDATE|TAKEDOWN],
                 status[RUNNING|WAITING|SUCCEEDED|FAILED|CANCELLED], started_at, finished_at)
workflow.steps  (id, run_id, release_id, stage, partner_id NULL, status, attempt,
                 job_id NULL, waiting_on NULL, error_code NULL, detail jsonb,
                 started_at, finished_at, updated_at)
```

| stage | 하는 일 | 지금 job | partner별 |
|---|---|---|---|
| `INTAKE` | 제출 고정(스냅샷·동의·해시 확인) | API submit | |
| `VALIDATE` | 파일 QC + 메타데이터 규칙 | `stage1` | |
| `RIGHTS` | 권리·보호 아티스트 심사 | `stage2` | |
| `NORMALIZE` | 무손실 → FLAC 마스터 | (API 동기 → job으로, 3.4) | |
| `PREPARE` | 식별자·정규 패키지 | `prepare_release` | |
| `DSP_ADAPT` | DSP별 보정·deliverable 생성·스펙 확인 | `delivery.stage` | ✓ |
| `APPROVE` | 스태프/자동 승인 | staff API | ✓ |
| `DELIVER` | 전송 | `delivery.enqueue`/`send` | ✓ |
| `TRACK` | ACK·폴링·LIVE·정정 | `delivery.poll`/`ack`/`mark_live`/`reconcile` | ✓ |

step status: `PENDING → RUNNING → (SUCCEEDED | WAITING | CORRECTION_REQUIRED | FAILED | DEAD_LETTER | SKIPPED)`.
`WAITING`은 사람(스태프 승인, 아티스트 수정)이나 외부(DSP ACK)를 기다리는 상태이고 `waiting_on`에 무엇을 기다리는지 적는다.

규칙:
- step 상태는 **그 step을 실행한 job과 같은 트랜잭션**에서 바꾼다. 다음 step의 job도 같은 트랜잭션에서 enqueue한다(지금 방식 그대로, outbox 보장 유지).
- `catalog.releases.status`는 run/steps에서 파생되는 요약으로 남긴다(기존 API·Studio 호환). 전이 규칙은 `config/states.json` 그대로.
- 관리자 화면은 `workflow.steps`를 그대로 보여 준다. "무엇이, 어디서, 왜 멈췄나"가 한 테이블에 있다.

### 3.2 Job 계약 (목표 5)

`operations.job_policies`(kind 단위, 마이그레이션으로 시드, 운영 중 조정 가능):

```
kind, queue, max_attempts, backoff_base_secs, backoff_max_secs,
timeout_secs, retry_on[TRANSIENT|ALL], dead_letter_surface[bool]
```

- enqueue가 정책의 `queue`·`max_attempts`를 복사한다(행 단위 값이 이미 있으므로 스키마 변경은 작다).
- 실행기가 handler를 `timeout_secs`로 감싼다. 넘으면 그 시도는 `TIMEOUT`으로 실패 처리(재시도 대상). lease는 크래시 대비로만 남는다.
- 오류는 `Transient`(네트워크·DB·스토리지)와 `Permanent`(검증 실패·규격 위반)로 나눈다. Permanent는 재시도하지 않고 바로 step `CORRECTION_REQUIRED`/`FAILED`.
- DLQ: 지금의 `DEAD_LETTER` + `surface_dead_letter`를 step `DEAD_LETTER`로 일반화. 스태프가 원인 수정 후 "재시도"를 누르면 같은 idempotency 키로 다시 QUEUED(시도 횟수 초기화, 감사 기록).
- 감사: claim·재시도·타임아웃·DLQ·수동 재시도 모두 `audit_events`에 release_id와 함께.
- 오래된 job 정리는 **하지 않는다**: enqueue 중복 방지가 `idempotency_key` 행의 존재에 기대므로, 지우면 늦은 재시도(DSP 전송 등)가 두 번 돌 수 있다. 정리하려면 먼저 사용한 키를 따로 보관하는 테이블이 필요하다(별도 단계).

### 3.3 DSP 어댑터 경계 (목표 3)

코어는 **파트너 코드(`D-5`)와 어댑터 종류**만 안다. DSP별 지식은 전부 어댑터 쪽에 둔다.

```
core ──(CanonicalRelease v1: config/contracts/*.json)──▶ Adapter ──▶ Deliverable + Findings
                                                         ▲
                                      DSP spec (데이터, 버전 관리)
```

- **정규 입력**: `prepare_release`가 만드는 패키지(이미 `config/contracts/PreparationPackageV1.json` 등 JSON Schema로 고정)를 어댑터 입력 계약으로 삼는다. 버전 필드로 호환을 관리한다.
- **DSP 규격을 데이터로**: `dsp_registry.rs`의 `REGISTRY` 필드를 `execution.dsp_specs(partner_id, version, spec jsonb, effective_from)`로 옮긴다. 코어의 `Dsp` enum 참조(13개 모듈)는 `partner_id: String` + `DspSpec` 조회로 바꾼다. 규격 변경 = 새 버전 행 추가(재배포 없음), 이미 스테이징된 릴리스는 그때의 spec 버전을 기록해 재현 가능.
- **어댑터 인터페이스**: `trait DspAdapter { fn check(&CanonicalRelease, &DspSpec) -> Vec<Finding>; fn render(&CanonicalRelease, &DspSpec) -> Deliverable; }`. DDEX(ERN 3.8.2/4.x 프리셋), JSON+CSV 피드, HTTP API가 구현체다. 코드는 `crates/adapters`로 분리해 코어가 DSP 구현을 import하지 못하게 한다(의존 방향 core ← adapters 금지, adapters → contracts만).
- 전송(`FileTransport`, 파트너 JSON 설정)은 지금 구조를 유지한다.

### 3.4 파일 단계 (목표 4)

| 단계 | R2 키 | 실행 위치 | 권한 |
|---|---|---|---|
| 격리 | `quarantine/{org}/{asset}/{nonce}` | 브라우저 → presigned PUT | 1회용 URL |
| 고정 | `registered/{org}/{asset}/{uuid}` (원본, ETag 고정) | API | API 토큰 |
| 검사 | (읽기) | worker `asset.analyze` | 읽기 전용 |
| 정규화 | `normalized/{org}/{asset}/{uuid}.flac` | **worker `asset.normalize`** (지금은 API) | 정규화 전용 쓰기 prefix |
| deliverable | `deliverables/{package}/{partner}/…` | worker `DSP_ADAPT` | deliverable prefix 쓰기 |

- 업로드 완료는 격리 → 고정까지만 하고 바로 응답한다. 정규화는 job이라 API 동시성 1개 제약과 Studio의 `UPLOAD_BUSY` 재시도가 사라진다.
- 정규화 결과가 자산의 "배급용 마스터"가 된다. 원본(`registered/`)은 보관만 하고, 해시·스펙은 정규화 결과 기준으로 기록한다(지금 FLAC 변환 후와 같은 의미).
- 각 단계는 앞 단계의 해시/ETag를 입력으로 고정해, 중간에 객체가 바뀌면 `ASSET_OBJECT_DRIFT`로 멈춘다(지금 precheck와 같은 방식).

### 3.5 Observability (목표 7)

**상관관계 키**: `release_id`(+ `run_id`, `step_id`, `job_id`, `partner_id`).

- `operations.jobs`에 `release_id`, `run_id`, `step_id` 컬럼(인덱스 `(release_id, created_at)`). enqueue 시 채운다.
- 실행기가 job마다 tracing span을 연다: `job{kind, job_id, release_id, run_id, partner_id, attempt}`. handler 안의 모든 로그·ffmpeg 호출·DSP HTTP 요청이 그 필드를 자동으로 달고 나온다. API 요청 span에는 `request_id`와, 경로에서 알 수 있으면 `release_id`.
- DSP 요청: 기존 `execution.delivery_attempts`에 release_id·요청/응답 요약(상태 코드, 소요 시간, 파트너 메시지 id)을 남긴다.
- 파일 처리: 단계별(격리·고정·검사·정규화·deliverable) 이벤트를 audit에 release_id와 함께.
- **타임라인 조회**: `GET /api/staff/releases/{id}/timeline` — `audit_events`, `jobs`, `workflow.steps`, `check_results`, `delivery_attempts`, `ack_events`를 release_id로 시간순 합친다. 로그 수집기(Loki 등)에서도 같은 `release_id` 필드로 검색된다.
- 지표(나중): 큐별 대기 시간·처리 시간·DLQ 수를 `/metrics`(Prometheus 형식)로. OpenTelemetry 내보내기는 span 필드가 자리 잡은 뒤 붙인다.

## 4. 이행 순서

앞 단계가 뒤 단계의 기반이 되도록 배열했다. 각 단계는 PR 하나(또는 몇 개)이고, 기존 API·Studio·데이터와 호환된다.

| 단계 | 내용 | 위험 | 효과 |
|---|---|---|---|
| **P1 추적 기반** | jobs에 `release_id`/`run_id`(nullable, 기존 행은 payload에서 채움), job span, API span, 타임라인 조회 API | 낮음 (추가만) | release_id 하나로 전부 추적 (목표 7) |
| **P2 kind별 정책** | `operations.job_policies`, 실행 제한시간, Transient/Permanent 오류 구분, DLQ 수동 재시도 | 낮음~중간 | 목표 5 완성 |
| **P3 workflow 데이터** | `workflow.runs`/`steps` 이중 기록(기존 status와 나란히), 스태프 화면이 steps를 읽음 | 중간 | 목표 6, 관리자 화면 단순화 |
| **P4 정규화 job** | `asset.normalize` worker job, API는 격리→고정까지만 | 중간 (업로드 흐름 변경) | 목표 1·4, 업로드 대기 제거 |
| **P5 DSP 규격 데이터화** | `execution.dsp_specs` + 조회 계층, `Dsp` enum 의존 제거 | 중간 (13개 모듈) | 규격 변경이 재배포 없이 (목표 3) |
| **P6 어댑터 크레이트** | `crates/adapters` 분리, `DspAdapter` trait, 정규 입력 계약 버전 | 중간~큼 | 코어/DSP 완전 분리 (목표 3) |

## 5. 하지 않는 것

- **외부 큐/워크플로 엔진(Kafka, RabbitMQ, Temporal 등)**: 업무 데이터와 같은 트랜잭션의 enqueue(outbox 보장)가 지금 구조의 핵심이고, 이 규모에서 PostgreSQL 큐는 병목이 아니다. 수평 확장은 `SKIP LOCKED`로 이미 된다.
- **API·worker 프로세스 통합**: DB 역할·저장소 키·정산 키 분리와 메모리 격리가 보안 경계다.
- **오래된 job 삭제**: 3.2의 이유로, 사용 키 보관 테이블이 생기기 전까지.
