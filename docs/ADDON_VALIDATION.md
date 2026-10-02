# Add-on validation

Base: origin/main `b16a52b` (2026-10-02 fetch). Branch: `feat/addon-services-phase1`.
검증 대상 코드: `6c4efe57afc3d02b3fc5e72c9cec27a20c190bc5`.
CI: [Foundation 36999241941](https://github.com/TAE-OK-11/audeniq/actions/runs/36999241941).
전체 workflow와 rust-postgres / compose-smoke job 모두 성공했다. 이후 검증 결과를 기록하는 문서 변경은 실행 코드와 테스트를 변경하지 않는다.

| 검사 | 결과 |
|---|---|
| 전체 migration, PostgreSQL 18 별도 테스트 DB | 통과 |
| cargo fmt --all / git diff --exit-code | 통과 |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 통과 |
| cargo build --locked -p audeniq-core --bins | 통과 |
| cargo nextest run --locked -p audeniq-core --test-threads 4 --no-fail-fast | 439 / 439 통과, 기존 ignored 3개 제외; 25개 test binary, 546.448초 |
| Compose 설정·기동·readiness 및 분석기 확인 | 통과 |
| backend review benchmarks | 통과; 별도 benchmark 테스트 1개 통과 |
| edge WASM·Workers bundle | 통과; bundle은 dry-run으로 검증 |
| React Studio / Worker·D1 / 브라우저 검사 | 61개 / 16개 통과 및 signup·reload·profile·release draft·404·logout smoke 통과 |

부가서비스 테스트 24개(23개 PostgreSQL 통합 테스트 + LRC/업로드 규칙 단위 테스트 1개)를 추가했고 전부 통과했다.

| 필수 시나리오 | crates/core/tests/addons.rs의 대응 테스트 |
|---|---|
| 1. 무료 상품 신청 | free_order_catalog_and_admin_acl |
| 2. 유료 신청 → PAYMENT_REQUIRED | paid_order_requires_payment_and_cannot_be_approved |
| 3. 결제 후 진행 | paid_order_requires_payment_and_cannot_be_approved |
| 4. 불가능한 전이 차단 | illegal_transition_is_rejected_by_api_and_database |
| 5. 타 조직 / 대상 ACL 차단 | cross_org_and_resource_acl_are_enforced |
| 6. 가격 변경 후 snapshot 보존 | catalog_version_change_preserves_order_snapshot |
| 7. 동일 key 중복 / 동시 요청 | idempotency_retry_concurrency_and_payload_conflict |
| 8. PROFILE_PLUS 중복 / 기간 내 후속 요청 | profile_plus_active_and_completed_valid_period_prevent_duplicates |
| 9. MV_GLOBAL_ONLY 증빙 필수 | mv_global_only_needs_verified_evidence |
| 10. 증빙 승인 후 전달 | mv_evidence_approval_gates_distribution_and_completion |
| 11. 수정 2회 및 초과 시 관리자 검토 | lyric_video_two_revisions_then_manual_review |
| 12. 기존·미래 queue priority / aging | priority_updates_queued_and_future_jobs_and_ages_normal_work |
| 13. 취소 후 작업 차단 | cancelled_order_jobs_cannot_create_external_work |
| 14. outbox receipt 재실행 멱등성 | outbox_receipt_retry_deduplicates_jobs |
| 15. 관리자 override audit | manual_payment_assignment_override_and_refund_have_audit |

추가 검증은 보완 메시지·재제출, MV 증빙 만료·ACL 회수, AUDENIQ 심의→증빙→전달, 일시적 실패와 DLQ, migration 순서, 모든 서비스의 manual 결과·완료, 실제 signed LRC/MV 업로드, runtime role의 RLS를 포함한다.

기존 release/distribution 회귀 suite는 전체 nextest 실행에 포함한다. S3 서명, SFTP 왕복, 권리·승인·freshness, delivery/reconciliation, 300건 혼합 release 처리도 함께 검증한다. 기본 실행에서 제외되는 기존 ignored 테스트 3개는 `sandbox_full_distribution_run`, `sandbox_adversarial_submissions`, `benchmark_prepared_matching`이다. 앞의 2개는 별도 sandbox 연결을 요구하며, benchmark는 CI 후속 단계에서 별도 실행한다.

기존 stage2 fixture는 UPC를 수정할 때 row_version을 올리고, 오디오 지문 비교를 위한 고정 seed의 broadband 성분을 사용하도록 보정했다. 기존 순수 정현파 fixture의 MP3 재인코딩 long-overlap BER는 0.357490, 보정 fixture는 0.023954(기준 0.10)이었다. 제품 지문 알고리즘·심사 기준은 변경하지 않았다.

로컬 RAM 약 1GiB에서는 전체 test code generation / lib-test clippy 및 마지막 cargo check가 OOM/SIGKILL로 종료됐다. 해당 실행은 통과로 간주하지 않으며, 최종 판정은 위의 CI 결과를 사용한다. 테스트용 로컬 DB는 운영과 분리했고 검증 후 컨테이너를 정지했다.
