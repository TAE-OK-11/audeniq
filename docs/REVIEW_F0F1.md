# AUDENIQ 백엔드 파운데이션(F0/F1) 코드 리뷰

- 대상: `TAE-OK-11/audeniq` 브랜치 `foundation/f0-f1` (HEAD `d4ee95a`, 2026-09-24)
- 범위: 정밀 리뷰 — `crates/core/src/{auth,api,uploads,operations,storage,contracts}.rs`, `migrations/0001,0002.sql`, `deploy/grants.sql`, `compose.yaml` / 스킴 — `catalog.rs`, `drafts.rs`, `domain.rs`, `packages.rs`, `artifacts.rs`, `crates/edge`, `tests/foundation.rs`, 문서
- 방식: 읽기 전용. 로컬 클론 기준 정적 분석 + 문서/테스트 대조

## 총평

파운데이션 목표(인증·조직/ACL·초안·업로드·큐·감사라는 **안전한 뼈대**)에 대해서는 설계-코드-테스트-문서가 일관되게 맞물려 있다. 보안 모델의 핵심 주장(매 요청 ACL 재평가, 토큰 해시 저장, 업로드 바인딩, lease fencing, 불변 감사)은 코드에서 실제로 강제됨을 확인했다. **Critical 없음.**

다만 Major 3건은 각각 "F2 통합 시 반드시 터질 함정"(M1), "비용 증폭 가능한 순서 결함"(M2), "rate limit 누락"(M3)이다. 모두 작은 패치로 해결 가능하다.

**판정: 파운데이션 기준 A- (우수). 머지 전 아래 Major 3건 중 M2/M3는 패치 권장, M1은 F2 킥오프 조건으로 기록.**

---

## Critical

없음. 파운데이션의 보안 모델을 깨는 치명적 결함은 발견하지 못했다.

---

## Major

### M1. `record_check`·`store_package`가 런타임 DB 역할로 실행 불가 — F2 통합 함정 (latent)

**문제:** 다음 단계(F2)에서 워커/API가 호출하게 될 두 함수가 현재 `grants.sql`의 권한으로는 실행 불가능하다. 지금은 프로덕션 코드에서 호출되지 않는 데드코드라 장애가 아니지만, F2에서 그대로 연결하면 즉시 `42501 permission denied`가 발생한다.

**근거:**
- `crates/core/src/operations.rs:195-223` — `record_check()`가 `operations.check_results`에 INSERT. 호출자는 테스트뿐(`tests/foundation.rs:730`)
- `crates/core/src/artifacts.rs:14-37` — `store_package()`가 `distribution.route_plans`·`distribution.packages` 접근. 호출자는 테스트뿐(`tests/foundation.rs:1251-1261`). 주석에 "No Foundation API or worker handler exposes this owner-only repository" 명시
- `deploy/grants.sql` — `check_results` 언급 0건. `distribution`·`rights` 스키마에 대한 GRANT 전무. `audeniq_worker`는 `operations` 스키마 USAGE만 보유 → `catalog.*` 접근도 불가

**수정 제안:**
1. F2 킥오프 전 `grants.sql`에 최소 권한 설계 반영 (`check_results` INSERT를 worker에, `distribution.*`는 용도 확정 후)
2. `tests/foundation.rs`의 `runtime_roles_enforce_foundation_boundary` 테스트에 `record_check` 호출 시 42501이 아니라 정상 동작함을 검증하는 케이스를 F2에서 추가 (지금은 실패 케이스만 있음)
3. 또는 당장 `operations.rs:194`의 "F2 will add its authorized orchestrator" 주석에 "grants.sql 권한 추가 필요"를 명시

### M2. `complete()`가 만료 최종 확인보다 S3 `freeze`(복사)를 먼저 수행 → orphan 객체 생성

**문제:** 업로드 완료 흐름에서 실제 객체 복사(`freeze`: quarantine → registered 키)가 만료 여부 최종 판정보다 먼저 일어난다. 만료된 세션으로 `complete`를 호출하면 `Conflict`를 반환하지만, 이미 `registered/{org}/{asset}/…` 키에 객체가 복사된 뒤다. `catalog.assets` 행은 `UPLOADING` 상태로 남고, orphan 정리는 후속 과제로 미뤄져 있어(이 보고서의 "고아 업로드 정리/보존 정책은 후속"과 일치) 스토리지 누적이 발생한다.

**근거:** `crates/core/src/uploads.rs:81-140`
- `:112` — `s.storage.freeze(&key, &stable, &meta.etag).await?` (네트워크 I/O, 복사 실행)
- `:122` — `UPDATE … SET status='COMPLETED' … WHERE id=$1 AND expires_at>clock_timestamp()` (만료 재확인은 여기서야 일어남)

**수정 제안:** `:112` 이전에 `clock_timestamp()` 기준 만료 검사를 한 번 더 수행 (최종 원자적 `UPDATE … WHERE expires_at>…`는 그대로 유지 — TOCTOU 방어는 유지됨). 장기적으로 orphan 스캔/보존 정책 워커를 F-후속 로드맵에 명시.

### M3. `complete()`에 rate limit이 없음

**문제:** `issue()`에는 사용자당 60회 제한이 있지만(`uploads.rs:42`), `complete()`에는 rate limit이 없다. 한 번의 `complete` 호출은 DB 트랜잭션 + 최대 3회의 S3 네트워크 호출(`head` → `freeze` → `head`)을 수행하고, 그 동안 `FOR UPDATE OF u,a` 행 잠금을 점유한다(`uploads.rs:99`). M2와 결합하면 만료 세션에 대한 반복 호출로 S3 복사 연산 비용을 증폭시킬 수 있다. (주: `COMPLETED` 상태의 중복 호출은 S3 호출 없이 early-return되므로(`:104-108`), 증폭 경로는 만료/미완료 세션에 한정)

**근거:** `crates/core/src/uploads.rs:81-140` (rate 호출 없음), `:42` (issue의 rate와 대조)

**수정 제안:** `complete()` 진입 시 `auth::rate(&s.pool, &format!("upload-complete:{}", a.user), 60).await?;` 추가.

---

## Minor

### m1. `/health`·`/ready`도 서비스 시크릿을 요구

**문제:** `boundary` 미들웨어(`crates/core/src/api.rs:87-113`)가 전 경로에 적용되어 헬스체크 경로도 `x-audeniq-service` 없으면 403. 오케스트레이터/로드밸런서의 시크릿 없는 헬스체크가 실패한다. 두 엔드포인트는 민감 정보를 반환하지 않는다(`{"service":"audeniq-api","operational_release":false}`, DB 도달성).

**수정 제안:** `/health`, `/ready`는 시크릿 검사 면제 (단, `x-request-id`·보안 헤더 부여는 유지).

### m2. `complete()`가 S3 I/O 동안 DB 행 잠금을 점유 → `cancel()`이 500으로 실패 가능

**문제:** `complete()`는 `:99`에서 `FOR UPDATE OF u,a`로 잠금을 잡은 뒤 `:103-116`에서 수 초~수십 초의 S3 I/O를 수행한다. 같은 세션에 대한 `cancel()`(`uploads.rs:176-224`, `:190-195`에서 `FOR UPDATE`)은 `lock_timeout='3s'`(`database.rs:connect`) 초과 시 `55P03` → `Error::Database` → 500 INTERNAL_ERROR로 반환된다. 잠금 순서는 일치해 데드락은 없으나, 느린 S3 상황에서 취소 UX가 500이 된다.

**수정 제안:** (a) `cancel()` 경로의 lock-timeout 에러를 409(CONFLICT)로 매핑하거나, (b) 문서에 "진행 중인 complete와 cancel 경합 시 재시도" 명시. 잠금 순서 자체는 올바르므로 구조 변경 불필요.

### m3. Argon2 세마포어(2개)를 DB 트랜잭션 전체 구간에 점유

**문제:** `password_slots: Semaphore::new(2)`(`api.rs:38`)인데, `login`/`register`에서 `_permit`이 함수 스코프 전체(DB 트랜잭션 포함) 유지된다(`auth.rs` login/register). 동시 로그인 3개째부터 직렬화되며, 각자가 풀(6개)의 커넥션도 점유한다. 데드락은 아니지만(2 < 6) 처리량 병목.

**수정 제안:** `password_hash()`/`verify()` 호출 직후 `drop(_permit)`으로 스코프 축소.

### m4. `identity.auth_limits`에 정리 로직 없음

**문제:** `auth::rate()`(`auth.rs:138`)가 버킷을 INSERT/UPSERT만 하고 만료 버킷을 삭제하지 않는다. 무한 누적.

**수정 제안:** 워커나 주기 작업에 `DELETE FROM identity.auth_limits WHERE window_start < now() - interval '1 day'` 추가.

### m5. `member()`가 임의의 기존 사용자를 멤버로 추가 가능

**문제:** `catalog::member()`(`catalog.rs:321`)는 대상 `user_id`의 존재·ACTIVE 여부를 확인하지 않는다. (없는 사용자는 FK 위반 → 409로 처리되지만, 비활성 사용자는 추가 가능.) 초대 플로우 없이 OWNER가 UUID만 알면 추가된다.

**수정 제안:** `identity.users`에서 `status='ACTIVE'` 확인을 INSERT 전에 추가. (UUID 추측 불가라 실질 위험은 낮음 — Minor)

### m6. `submit()` 핸들러의 데드 트랜잭션

**문제:** `api.rs:332-341` — 트랜잭션 시작 → `authorize` → `Err(Error::Gated)`. 롤백되므로 무해하지만 불필요한 DB 라운드트립 2회.

**수정 제안:** `actor` 확인 후 바로 `Err(Error::Gated)` 반환.

### m7. 쿠키 값 trim 누락

**문제:** `auth::actor()`(`auth.rs:66`) — 키는 `trim()`하지만 값은 trim하지 않는다. `…=abc␣` 형태면 길이 65로 거부. 강건성 nit.

**수정 제안:** `val.trim()` 적용.

### m8. `domain::transition()`이 호출마다 `states.json` 파싱

**문제:** `domain.rs:8-19` — `include_str!`는 컴파일타임이지만 `serde_json::from_str`은 호출마다 실행. 현재 프로덕션 호출자는 없고(테스트만; 실제 게이트는 DB 트리거 `guard_pipeline`), F2에서 핫패스에 오르면 낭비.

**수정 제안:** `OnceLock<Value>`로 캐싱. (지금은 Nit)

### m9. `get()`/`list()`가 `to_jsonb(t)`로 내부 컬럼 전체 노출

**문제:** `catalog.rs:get/list` — `row_version` 등 내부 필드가 API 응답에 그대로 포함. 민감 정보는 없으나 명시적 컬럼 선택이 원칙상 깔끔.

**수정 제안:** Nit. 당장 수정 불필요, F2 API 정리 시 반영.

---

## 확인된 강점 (리뷰 중 검증됨)

- **매 요청 ACL 재평가 강제됨**: `auth::membership()`·`authorize()`가 `FOR SHARE` 행 잠금과 함께 매번 DB에서 재확인. 역할 캐시 없음. 철회 레이스가 설계상 차단됨 (`auth.rs:86-118`)
- **토큰은 해시만 저장**: 세션/CSRF 모두 SHA-256 다이제스트 저장, 비교는 상수시간 (`auth.rs:33-54, 92-99`)
- **로그인 타이밍 공격 완화**: 존재하지 않는 계정에도 dummy hash로 Argon2 검증 수행 (`auth.rs:217-236`)
- **비밀번호 변경 시**: 전 세션 폐기 + 변경 직전 자격증명 버전 재확인 (`auth.rs:480-530`)
- **업로드 바인딩 실효성**: presigned URL에 `content-length`·`content-type`·`x-amz-meta-upload-nonce` 서명 → 서명 불일치 시 S3가 거부. `HEAD` 검증 후 ETag 조건부 복사(`x-amz-copy-source-if-match`)로 TOCTOU 차단 (`storage.rs:104-135, 196-226`, `uploads.rs:103-120`)
- **큐 fencing**: `SKIP LOCKED` 선점, `lock_token`+`lease_until` 이중 검사, 만료된 워커의 완료 거부, DLQ, `event_receipts` 멱등성 (`operations.rs:62-192`)
- **Outbox 원자성**: `event()`+`enqueue()`가 호출자 트랜잭션 안에서 실행 — 비즈니스 쓰기와 같은 tx (`operations.rs:17-60`, 호출부 `uploads.rs:133-140`)
- **DB가 최종 게이트**: 불변 트리거(`0001:188-193`), 파이프라인 전이 트리거(`0002:32-42`, F2까지 전부 차단), `row_version` 방어, `resources.kind` CHECK 제약
- **grants.sql ↔ 코드 일치**: `runtime_roles_enforce_foundation_boundary` 테스트가 실제 역할로 42501까지 검증 (`tests/foundation.rs:748-843`). `record_check`/`store_package`의 권한 부재도 이 테스트에서 의도적으로 확인됨
- **에러 처리**: `23505/23503/23514` → 409 매핑, 그 외 DB 에러는 상세 미노출 (`error.rs:36-52`). 프로덕션 경로에 `unwrap`/`expect` 없음 (있는 것은 테스트·스타트업 fail-fast·`HMAC accepts any key`뿐)
- **운영 하드닝**: Dockerfile non-root(10001), read-only, cap_drop ALL (`deploy/Dockerfile`), compose loopback 바인딩, `statement_timeout`/`lock_timeout` (`database.rs`)
- **Edge BFF**: 헤더 allowlist(서비스 신원 헤더 전파 차단), 64KiB 바디 제한, `/api/admin` 차단 (`crates/edge/src/lib.rs`)
- **문서 정직성**: README·IMPLEMENTATION_REPORT가 "미검증/비활성"을 명시. `docs/API.md`의 27개 엔드포인트가 라우터와 일치. 상태 계약의 Rust↔DB 드리프트 테스트 존재 (`domain.rs:drift_tests`)

## 테스트 갭

- `member`/`acl` 동시 변경 레이스: `FOR SHARE`/`FOR UPDATE`로 보호되나 테스트 없음
- `auth_limits` 누적/정리: 미테스트 (m4와 연결)
- `complete` 만료-orphan 경로: 만료 시 `Conflict`는 테스트되나 orphan 객체 생성 여부는 미검증 (M2와 연결)
- 실제 R2/VPC 연결, 부하·복원·장애: 미검증 — 문서에 명시되어 있으므로 정직한 범위

## 다음 액션 제안

1. **M2+M3 패치** (작음): `complete()`에 만료 사전확인 + rate limit 추가 후 f0-f1에 커밋
2. **m1, m3 패치** (작음): `/health`·`/ready` 시크릿 면제, `password_slots` 스코프 축소
3. **f0-f1 → main 머지**: PR 생성. 리뷰어는 없어도 셀프 체크리스트(위 Major/Minor)로 1차 검토 기록 남기기
4. **F2 킥오프 조건으로 기록**: M1 — `grants.sql`에 `check_results`·`distribution`·`rights` 권한 설계 반영 없이 `record_check`/`store_package` 연결 금지
5. **m4**: `auth_limits` 정리 작업을 F2 운영 항목에 추가
