# 부가서비스 백엔드 안정화

기존 phase 1 구현을 이어서 점검했다. 기본 release/distribution 상태 머신과 외부 adapter 방식은 유지한다.

## 변경 파일

- `crates/core/src/addons/api.rs`, `jobs.rs`, `model.rs`, `workflow.rs`: 관리자 권한·worker 검증·우선권·기간·영상 상세 및 결과 처리.
- `migrations/0070_addon_stabilization.sql`, `deploy/grants.sql`: 후속 보정·priority trigger·제한된 관리자 권한 조회 함수와 runtime 권한.
- `crates/core/tests/addons.rs`, `crates/core/tests/fixtures/addon_0069_upgrade.sql`: 신규 7개 회귀 테스트 및 기존 데이터 업그레이드 fixture.
- `docs/ADDON_SERVICES.md`, `docs/ADDON_STABILIZATION.md`, `docs/ADDON_VALIDATION.md`: 변경 내용과 검증 결과.

## 수정한 오류

| 문제 | 수정 |
|---|---|
| 실제 `audeniq_api` 역할에서 관리자 ACL의 `FOR SHARE`가 직원 테이블 UPDATE 권한 부족으로 실패 | 기존 ACTIVE staff 조회·잠금을 좁은 `identity.lock_active_staff_role(uuid)` 함수로 수행. API의 직원 권한 변경은 계속 금지하고 PUBLIC 실행권한도 제거 |
| 우선권 취소 후 주문 priority가 남고, 실행 중이던 작업의 재시도는 우선권 변경을 반영하지 않음 | 주문 priority를 초기화. 작업의 재대기·실행 시작 때 현재 구매를 확인해 적용/복원. 실행 중인 작업은 변경하지 않고 관리자 URGENT 값은 보존 |
| 우선권 재적용이 실제 변경 없이 NORMAL → HIGH audit를 반복하고, 미래 job 상속에는 audit가 없음 | 실제 변경에만 audit 기록. INSERT의 AFTER trigger를 사용해 멱등 enqueue 충돌 시 존재하지 않는 job을 기록하지 않음 |
| PROFILE_PLUS 가격을 0으로 변경한 경우 유효기간이 NULL이라 완료 후 중복·후속 요청 규칙이 깨짐 | 무료 기간제 상품도 신청 시 기간 snapshot으로 활성화. PROFILE_PLUS는 신청일 + 12 calendar months이며 paid_at이나 결제 완료를 생성하지 않음 |
| 대상 권한은 유지하면서 주문 write ACL만 회수해도 worker가 실행됨 | worker가 대상 ACL과 주문 ACL을 모두 재확인. 권한이 없으면 NEEDS_INFO로 전환하고 provider 작업 생성 차단 |
| 제출 후 거절된 LRC/source video도 작업에 사용될 수 있음 | 작업 직전 해당 첨부 asset의 등록·upload 완료·hash/etag 상태를 재확인 |
| Basic Video 요청을 제거해도 상세 row가 남고, 요청하지 않은 영상 결과도 성공 응답 | 상세를 삭제하고 audit에 이전 요청을 보존. 존재하지 않는 영상 요청의 결과 등록은 트랜잭션 전체 거절 |

## Migration 0070

`0070_addon_stabilization.sql`은 0069를 수정하지 않는 후속 migration이다.

- 기존 priority trigger를 재대기/실행 시작에도 적용하고 자동 priority 변경 audit trigger를 추가한다.
- 무료 기간제 주문의 누락된 만료일을 **기존 submitted_at**과 기간 snapshot으로 보정한다. 재배포일을 기준으로 기간을 연장하지 않는다.
- 취소·거절·실패한 PRIORITY_DELIVERY 주문의 남은 priority를 초기화한다.
- 기존에 Basic Video 선택을 취소하고도 남은 상세를 정리하며 source/output asset은 보존한다.
- 데이터 보정도 기존 append-only audit에 이전/이후 값을 기록한다.
- FORCE RLS가 적용되는 일반 schema owner도 조직 간 보정을 수행하도록 migration 내부에서 staff scope를 설정하고 복원한다.
- 관리자 ACL 잠금 함수를 추가한다. schema owner로 migration 후 `deploy/grants.sql`을 적용한다. 새 권한은 해당 함수 EXECUTE와 선택 취소 시 필요한 lyric_video_requests DELETE뿐이다.

로컬 PostgreSQL 18의 빈 DB에 전체 migration 적용을 확인했다. 별도 0069 DB에 무료 완료 주문·취소된 우선권 주문·RUNNING 및 URGENT 작업을 생성해 0070을 적용했고, 기간·priority 보정, audit, 재대기 복원, URGENT 보존을 확인했다. 실제 API 역할에서 관리자 역할 조회가 성공하고 직원 테이블 UPDATE 권한은 false인 것도 확인했다.

## 회귀 테스트

`crates/core/tests/addons.rs`에 7개를 추가했다.

- `free_profile_plus_keeps_calendar_entitlement_and_follow_ups`
- `priority_retries_restore_cancelled_boost_and_preserve_operator_priority`
- `revoked_order_write_acl_stops_work_with_target_acl_intact`
- `lyrics_worker_rechecks_rejected_attachment`
- `removing_basic_video_request_clears_details_and_rejects_video_results`
- `runtime_api_can_operate_admin_workflow_without_staff_update_privilege`
- `forward_migration_repairs_existing_orders_under_forced_rls`

운영 역할 테스트는 관리자 목록·결제·승인·진행·가사 영상 선택 취소를 실제 API 역할로 실행한다. 직원 권한 변경 거절, 관리자 권한 회수 후 접근 차단, 진행 중인 권한 검사와 회수의 동시 실행도 확인한다.

## 검증 결과

최종 코드 `1be9154619e526fceeba43921c7ef7508755e32d`의 [Foundation CI](https://github.com/TAE-OK-11/audeniq/actions/runs/37007311088)가 모두 성공했다. 이후 변경은 결과 기록 문서뿐이다.

- PostgreSQL 백엔드 446/446 통과(기존 ignored 3개 제외), 부가서비스 31/31 및 신규 안정화 7/7 통과.
- 기존 release/distribution 회귀와 300건 혼합 발매 처리 통과.
- workspace fmt/clippy `-D warnings`, backend build, Compose 설정·기동·readiness 통과.
- backend benchmark, edge WASM 및 Workers dry-run bundle 통과.
- React 61개, Worker/D1 16개, 브라우저 smoke 통과.

운영 적용 순서는 schema owner migration → `deploy/grants.sql` → API/worker 코드 배포이다. 외부 provider와 PG는 기존 manual/준비 상태를 유지한다. 상태 전이·권리·승인·배급 처리 로직을 교체하지 않았으며, 기존 queue에 대한 영향은 priority 상속/복원과 해당 audit에 한정된다.
