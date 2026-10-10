# F2 Pre-submit + Stage 1 — 구현 계획 (구현 반영)

BLUEPRINT §§3–4 구현. 법률 검토(§23.1)는 스킵 → 0-B/0-C의 미성년·전자서명 자동 경로는 전부
`REVIEW_REQUIRED` 게이트로 고정. F2 완료 조건: "청소년 예외/서류 검토 GATE, 변경 없는 오디오 재분석 0회".

## 상태 전이 (config/states.json 축 그대로)

`DRAFT → SUBMITTED → STAGE1_RUNNING → STAGE1_PASSED | STAGE1_CORRECTION`
재제출은 새 revision 발행, `current_revision_id`만 이동 (구 revision은 불변 유지).
Stage 1 통과 시 같은 트랜잭션에서 `rights` 큐에 `stage2` job enqueue (Stage 2 오케스트레이터 인계).

## DDL

F1 마이그레이션에 이미 있음: `application_revisions`, `consent_packages`,
`operations.check_results`, `assets.qc_status`. F2에서 추가:

- migration 0007: `distribution.validation_packages(id, org_id, revision_id, body jsonb,
  package_hash, rule_version)` — Stage 1 Validation Package(MUST) 저장.
- migration 0008: `catalog.application_revisions.idempotency_key text NOT NULL`
  + `UNIQUE(release_id, idempotency_key)` — submit 멱등 키.

## 모듈

### `crates/core/src/submission.rs`

| 함수 | 단계 | 동작 |
|---|---|---|
| `presubmit(s, actor, org, release)` | 0-A~0-D | 게이트 평가만 (읽기). 계정(ACTIVE 멤버십·사용자 상태), 업로드 admit(에셋 REGISTERED), 미성년 경로 감지 → `gates[]` 반환 |
| `create_consent(s, actor, org, release, input)` | 0-C | 미성년 경로 포함 시 `Error::Gated(MINORITY_REVIEW_REQUIRED)`. 그 외 동의 패키지 생성 (policy_version="v1-self"). scope = 실질 revision body(consent hash 제외, row_version 제외) |
| `submit(s, actor, org, release, input)` | 1-A | 게이트 서버 재확인 → 불변 revision 발행 → `qc` 큐에 `stage1` job enqueue. 멱등: (1) 동일 idempotency key → 최초 revision으로 수렴, (2) 동일 key+변경된 body → `IDEMPOTENCY_KEY_REUSED` 422, (3) 동일 body → 현 revision 수렴. 동시 더블클릭은 `ON CONFLICT DO NOTHING`으로 수렴 |
| `run_stage1(pool, storage, revision_id)` | 1-B~1-D | 워커 진입점. Validation Package가 이미 있으면 재실행 없이 기존 요약 반환 (멱등 완료) |

### `crates/core/src/qc.rs`

- `QC_RULE_VERSION = "1"`
- `probe(path)` — ffprobe JSON 파싱. 안전 제한: `AUDENIQ_QC_PROBE_TIMEOUT_SECS`
  (기본 30초) 초과 시 프로세스 kill → `TECHNICAL_RETRY`, stdout 8MiB 상한 초과 시 실패.
- 매직바이트 검증 (WAV/FLAC/MP3, PNG/JPEG)
- `check_audio` / `check_image` → `Vec<CheckOutcome{check_code, status, detail}>`
- **변경점 캐시**: `result_hash = sha256(check_code || rule_version || asset_sha256 || metric_hash)`.
  동일 `result_hash`의 기존 `check_results`가 있으면 ffprobe 재실행 없이 상태 복사
  (새 revision 행은 그대로 적재 → 감사 추적 유지, 재분석 0회).
- **에셋별 격리**: `analyze_asset` — `head()` 크기 검사(`AUDENIQ_QC_MAX_BYTES`, 기본 512MiB)
  초과 시 다운로드 없이 해당 에셋의 check만 `TECHNICAL_RETRY`. 스토리지/IO 실패도
  에셋 단위로 격리 → 전체 run 중단 없음. 임시파일명은 UUID로 워커 간 충돌 방지.

### 1-B 필드 검증 (check_code)

`FIELD_TITLE_MISSING`, `FIELD_RELEASE_DATE_MISSING`, `FIELD_RELEASE_TYPE_MISMATCH`(트랙 수),
`TRACK_ORDER_GAP`, `CREDIT_MISSING`(트랙별 권리자 크레딧), `ISRC_FORMAT_INVALID`,
`ASSET_MISSING`, `ASSET_NOT_ADMITTED`.

### 1-C 파일 QC (check_code)

`SHA256_MISMATCH`(BLOCKED), `AUDIO_MAGIC_MISMATCH`, `AUDIO_PROBE_FAILED`(TECHNICAL_RETRY),
`AUDIO_TOO_SHORT`(30초 미만 → CORRECTION_REQUIRED), `AUDIO_SAMPLE_RATE_LOW`(44100 미만),
`AUDIO_CHANNEL_INVALID`, `IMAGE_MAGIC_MISMATCH`, `IMAGE_PROBE_FAILED`(TECHNICAL_RETRY),
`IMAGE_TOO_SMALL`(3000px 미만 → CORRECTION_REQUIRED). short-circuit 검사는
`NOT_APPLICABLE`/`TECHNICAL_RETRY`로 명시 기록 (누락 없음).

### 1-D 집계

- 전원 PASS/NOT_APPLICABLE → `STAGE1_PASSED` + Validation Package 발행 +
  `rights`/`stage2` job enqueue (동일 트랜잭션, `idempotency_key='stage2:{revision_id}'`)
- CORRECTION_REQUIRED 존재 → `STAGE1_CORRECTION` (+ 보완 항목 API 노출, 메일은 F2 범위 밖)
- TECHNICAL_RETRY 존재 → checks 저장 후 job 재시도 (지수 백오프는 jobs.run_at로, 캐시로 재분석 비용 0)
- BLOCKED 존재 → `STAGE1_CORRECTION` + 에셋 `qc_status=BLOCKED` (제출 자체는 막지 않고 보완 경로)

## 워커

`audeniq-worker`가 `qc` 큐의 `kind="stage1"` 처리. 에셋 바이트는 스토리지에서 내려받아
임시 파일로 ffprobe 실행 (운영 워커 호스트에 ffprobe 필요). 실패 시 `TECHNICAL_RETRY`.
성공 시 `rights` 큐에 `stage2` job이 이미 enqueue되어 있음 (Stage 2 구현은 F3).

## API

- `POST /api/orgs/{org}/releases/{id}/presubmit` — 게이트 평가
- `POST /api/orgs/{org}/releases/{id}/consents` — 동의 패키지 생성
- `POST /api/orgs/{org}/releases/{id}/submit` — 기존 `Error::Gated` 스텁 교체
- `GET /api/orgs/{org}/releases/{id}/submission` — revision + checks + 상태 조회

## 테스트

- `qc` 단위 테스트: ffmpeg로 픽스처 생성 (정상 WAV, 손상 파일, 저해상도 이미지),
  hang하는 analyzer에 대한 probe 타임아웃 테스트.
- 통합 테스트(PG) 9개: presubmit 게이트, submit 멱등 키(재시도 수렴/키 재사용 거부),
  stage1 전체 흐름(Stage 2 enqueue 검증 포함), 변경점 캐시(2회 실행 시 재다운로드 0회),
  손상 오디오 보완 경로, 초대형 에셋 TECHNICAL_RETRY, 미성년 게이트.
- 법률 경로: 미성년 포함 시 `MINORITY_REVIEW_REQUIRED` 게이트 확인.

## 설계 결정 (구현 중 확정)

1. **consent scope = 실질 revision body** (consent_package_hash 제외, row_version 제외).
   row_version을 scope에 넣으면 첫 submit이 row_version을 올려 재시도·재제출의
   consent가 항상 무효가 되는 근본 버그. 실질 변경(트랙/크레딧/에셋/드래프트)만 scope 변경.
2. **Stage 2 enqueue는 pass-path와 동일 트랜잭션.** Validation Package 없이 stage2 job이
   생기거나, job 없이 package만 생기는 중간 상태 불가. `ON CONFLICT DO NOTHING`으로
   재실행 시 중복 enqueue 방지.
3. **Storage 실패 ≠ job 실패.** 에셋 단위 `TECHNICAL_RETRY`로 격리하고 job을 재큐잉.
   캐시가 있어 재시도 비용은 0에 수렴.
4. **Stage 2 handoff는 park로 보존.** `execute()`가 `stage2`를 만나면
   `UNIMPLEMENTED_JOB_KIND` DLQ 대신 `operations::park()`으로 QUEUED에 되돌림
   (attempts 리셋, 5분 후 재클레임 가능). lease 만료 sweeper에서도 `stage2`는
   DLQ 대상에서 제외하고 재클레임 가능. F3가 구현하기 전까지 handoff가 소실되지 않음.
