# AUDENIQ 파운데이션 코드 품질·효율 리뷰

- 대상: `TAE-OK-11/audeniq` 브랜치 `foundation/f0-f1` (코드 기준, 리뷰 문서 커밋 제외)
- 방식: 직접 정독 (3,321줄). 보안 리뷰(REVIEW.md)와 별개로 코드 품질·효율에 집중
- 범위: `crates/core/src/*.rs`, `crates/core/src/bin/*`, `crates/edge/src/lib.rs`

## 총평

**잘 짜여있는가: A- / 효율적인가: B+**

일관된 에러 처리(`Result` 전파, prod 코드에 `unwrap` 없음), 전부 바인드 파라미터인 SQL, N+1 없음, 커서 페이지네이션, 의도적인 락 전략(`FOR SHARE`/`FOR UPDATE` 구분), `statement_timeout`/`lock_timeout` 가드레일 — 파운데이션치고 기본기가 단단하다.

효율은 "지금 규모에서는 충분, 트래픽이 붙기 전에 손볼 곳이 몇 군데" 수준. 요청당 쿼리 수가 많은 편인데, 그건 감사 로그+outbox를 모든 쓰기에 강제하는 설계의 고정 비용이라 의도된 트레이드오프다.

---

## 잘된 점 (효율 관점)

1. **N+1 없음.** 발매 조회는 tracks+credits를 단일 쿼리(`jsonb_agg` 서브쿼리)로, preflight는 2쿼리, 목록은 단일 쿼리+`EXISTS`로 처리.
2. **커서 페이지네이션.** `OFFSET` 없이 `id > $after` 키셋 방식.
3. **Argon2 격리.** `spawn_blocking` + 세마포어(2)로 비동기 런타임 블로킹 없음. 로그인 실패 시 더미 해시 검증으로 타이밍 공격 완화.
4. **큐 클레임 정석.** `FOR UPDATE SKIP LOCKED` + lease fencing + 만료 시 데드레터.
5. **DB 가드레일.** 연결마다 `statement_timeout='15s'`, `lock_timeout='3s'`.
6. **보수적 풀 사이즈.** API 6, worker 4, migrator 1.

---

## 효율 이슈 (심각도순)

### E1. `complete()`가 S3 왕복 동안 행락 점유 — `uploads.rs:complete`
`SELECT ... FOR UPDATE OF u,a`로 락을 잡은 뒤 S3 `head`→`freeze`→`head` 3회 왕복(수 초)을 수행한다. 그 사이 `cancel()`의 `FOR UPDATE`가 블로킹되고 `lock_timeout` 3초 후 500이 된다.
**수정:** `UPDATE ... SET status='COMPLETING' WHERE status='ISSUED'`로 원자적 선점 후 트랜잭션을 끝내고, S3 IO를 락 없이 수행, 마지막에 만료 재확인+`COMPLETED` 확정. 락-IO 결합을 끊는 정석 패턴.

### E2. `replace_credits()` 크레딧당 쿼리 — `drafts.rs:replace_credits`
크레딧 존재 확인을 루프로 `EXISTS` 쿼리 → 최대 100회 왕복.
**수정:** `SELECT count(*) FROM identity.parties WHERE org_id=$1 AND id = ANY($2)` 단일 쿼리로 개수 비교.

### E3. `claim()`의 sweep이 매 폴링마다 실행 — `operations.rs:claim`
데드레터 sweep `UPDATE`를 폴링(500ms)마다 쓰기 트랜잭션으로 실행. 대부분 no-op인데도 매번 트랜잭션+스캔 비용.
**수정:** sweep를 별도 주기로 분리(예: 30초마다 또는 N번째 폴링마다).

### E4. `change_password()`가 락 건 채로 Argon2 — `auth.rs:change_password`
`users` 행에 `FOR UPDATE`를 건 상태에서 `verify()`(spawn_blocking, 수백 ms)를 수행. 락 점유 시간이 CPU 작업에 묶인다.
**수정:** 락 전에 verify를 먼저 하거나, `FOR SHARE`+업데이트 직전 재확인.

### E5. 요청당 인증 쿼리 3회 — `auth.rs`
쓰기 요청마다 `actor()` 1회 + `authorize()` 내부 `membership()` 1회 + ACL 1회 = 3 왕복. membership과 ACL 검사를 단일 쿼리(JOIN)로 합칠 수 있다.

### E6. 읽기 전용인데 트랜잭션을 여는 핸들러 — `uploads.rs:get/status`
단일 `SELECT`를 위해 `pool.begin()`→`commit` (BEGIN/COMMIT 왕복 2회 추가). 풀에서 직접 조회로 변경.

### E7. `issue()`의 불필요한 시간 조회 — `uploads.rs:issue`
`SELECT now()+interval '15 minutes'`를 별도 왕복. Rust의 `chrono::Utc::now() + Duration`으로 대체.

### E8. 멱등 upsert의 no-op 쓰기 — `operations.rs:event/enqueue`
`ON CONFLICT DO UPDATE SET idempotency_key=EXCLUDED.idempotency_key`는 실질적 변경이 없어도 튜플을 새로 써서 bloat 유발. `DO NOTHING` + 충돌 시 `SELECT` 폴백.

### E9. (latent) 매 호출 JSON 파싱/스키마 재컴파일 — `domain.rs`
`transition()`은 호출마다 `states.json` 전체를 다시 파싱하고, `validate_package()`는 스키마 JSON 파싱+`validator_for` 컴파일을 매번 수행. 현재 F2 데드코드라 실제 영향은 없으나, 연결 전에 `OnceLock` 정적 캐싱 필요.

### E10. worker 폴링 지연 — `bin/audeniq-worker.rs`
유휴 시 500ms sleep이라 작업 수신까지 최대 ~0.5s+α 지연. 지금은 문제없고, 실시간성이 필요해지면 `LISTEN/NOTIFY` 고려.

---

## 코드 품질 이슈

### Q1. `submit()`의 데드 트랜잭션 — `api.rs:submit`
트랜잭션을 열고 `authorize` 후 무조건 `Err(Error::Gated)` 반환. 트랜잭션이 불필요하고 롤백만 발생. 정리 권장.

### Q2. 세마포어 점유 범위 과다 — `auth.rs:register/change_password`
`password_slots`가 Argon2 해싱 구간이 아니라 트랜잭션 전체를 감싼다. 세마포어 목적(Argon2 동시 실행 2개 제한)과 달리 등록/변경 처리량까지 직렬화됨. 해싱 구간으로 범위 축소 권장.

### Q3. `cancel()`의 중복 조회 — `uploads.rs:cancel`
세션 조회가 2회(`SELECT asset_id`, `SELECT status ... FOR UPDATE`). 하나로 합칠 수 있음.

### Q4. `to_jsonb(t)`의 내부 컬럼 노출 — `catalog.rs:list/get`
행 전체를 직렬화해서 `org_id`, `row_version` 등 내부 컬럼이 API 응답에 그대로 나감. 명시적 컬럼 화이트리스트 권장 (효율보다 API 위생 문제).

### Q5. `rate()`의 요청당 DB upsert — `auth.rs:rate`
global 버킷(`login:global` 등)은 공격 상황에서 단일 행에 쓰기가 직렬화됨. 속도 제한의 의도된 감속이라 허용 가능하나, 규모가 커지면 인메모리 슬라이딩 윈도우 병행 고려.

---

## 참고: 쓰기 요청 1회의 쿼리 예산

`actor` 1 + `authorize` 2 + 본작업 2~4 + `audit` 1 + `event` 2 = **약 8~10 쿼리**. 감사·outbox를 모든 쓰기에 강제하는 설계의 고정 비용이다. 파운데이션 규모에선 문제없고, 트래픽이 붙으면 audit/event 경로를 비동기화하는 것을 고려.

## 판정

- **잘 짜여있는가: A-.** 에러 처리·락 전략·쿼리 패턴이 일관되고 의도가 코드에 드러남. `unwrap` 남용 없음.
- **효율적인가: B+.** 지금은 충분. E1/E2는 F2 전에 패치 권장, E3~E8은 트래픽 전 최적화 후보, E9/E10은 latent/향후 과제.
