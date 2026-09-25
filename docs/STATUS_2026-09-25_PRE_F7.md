# F0~F9 상태 기록 (2026-09-25) — pre-F7 전수 감사

2026-09-25 하루 동안 F7 진입 전 F0~F6를 실제 코드·테스트 기준으로 전수 점검하고,
계약 없이 가능한 항목은 전부 수정한 기록. 각 F별로 "오늘 뭘 어떤 식으로 했는지"와
"뭐가 남았는지"를 분리해서 적는다.

검증 기준: `cargo fmt --all --check` 통과, `cargo clippy --workspace --all-targets -- -D warnings` 통과,
`cargo test --workspace` **125개 전부 통과** (신규 7개 포함), 실패 0.
CI run [36097605861](https://github.com/TAE-OK-11/audeniq/actions/runs/36097605861) — **success** (2026-09-25 확인).

관련 커밋 (main): `89cce75` (F3 grant FK 병합), `4d2b59d` (pre-F7 audit 병합), `8346e8f` (문서).

---

## F0 — 설계 동결: 완료

- BLUEPRINT v1.4 기준 동결 상태 유지. 오늘 변경 없음.
- **남은 것:** 없음.

## F1 — Foundation: 완료

오늘 한 일:

- `delivery_enabled` 활성화 플래그의 변경 권한을 schema owner / platform operator로 고정
  (`crates/core/tests/foundation.rs`에 테스트 2개 추가).
  `audeniq_api`와 `audeniq_worker`가 `execution.adapter_profiles`의
  `delivery_enabled`를 UPDATE하면 `42501`이어야 함을 테스트로 못 박음.
  API/worker가 임의로 DSP를 켜고 끄는 경로를 DB 권한 레벨에서 차단.
- **남은 것:** 없음 (계약 없이 가능한 범위).

## F2 — Pre-submit + Stage1: 완료

- 오늘 직접 건드린 것 없음. 기존 게이트·QC·보완 플로우 그대로 통과.
- **남은 것:** 없음.

## F3 — Stage2 (권리 검증): 완료

오늘 한 일:

1. **grant FK tenant 경계** (migration `0021`, main `89cce75`).
   `rights.grant_atoms.parent_grant_id`와 `contract_revision_id`가 전역 id만 참조해서
   org A의 grant가 org B의 grant/contract revision을 부모로 삼을 수 있던 구멍을 막음.
   두 FK를 복합 `(org_id, id)` FK로 교체하고, `review.rs`의 parent-chain 조회도
   `org_id` 스코프로 제한 (defense in depth). `rights_tenant` 테스트 4개 추가.
2. **checkpoint 의미 정정** (`crates/core/src/review.rs`).
   기존 주석은 "완료 모듈을 재실행하지 않는다"고 했지만 실제 코드는 네 모듈을 매번 실행했음.
   동작은 유지하고 문서를 실제에 맞춤: crash가 verification package 고정 *전*이면
   모듈 재실행 (읽기 기반이라 안전하고, crash 이후 rights/catalog 변경을 반영),
   고정 *후*면 pinned package 즉시 반환. checkpoint는 skip gate가 아니라
   idempotent audit trail. 중복 저장을 `record_check_result` helper로 분리하고
   동일 결과 재기록 시 같은 row를 반환하는 단위 테스트 추가.
3. **rights epoch auto-bump** (오늘 이른 작업): override/grant INSERT 시 epoch 증가,
   stage2 pin 이후 bump, fact 테이블 append-only 유지. CI success 확인됨
   (run 36093955569, 36093964764).
- **남은 것:** 없음 (계약 없이 가능한 범위).

## F4 — Stage3 (패키지·DDEX 생성): 완료 (단, 공식 XSD 검증 제외)

오늘 한 일:

1. **DDEX retry-count RLS 수정** (`crates/core/src/distribution.rs`).
   `READY_FOR_DELIVERY` idempotent 경로의 `ddex_messages` COUNT가 일반 pool로 실행돼,
   FORCE RLS 테이블에서 `app.org_id` 없이 0을 반환할 수 있었음 (실제 row가 있어도
   "없는 것"처럼 보여 중복 생성을 유발할 수 있는 버그).
   idempotent 조회 row에 `org_id`를 포함시키고, 짧은 transaction 안에서
   `set_config('app.org_id', ...)` 후 count를 조회하도록 수정.
2. **contributor role 매핑** (`crates/core/src/ddex_ern.rs`).
   credit role을 DDEX `ContributorRole` allowed-value subset으로 매핑
   (composer/songwriter→Composer, mixer→Mixer, feat.→FeaturedArtist 등),
   매핑 안 되는 값은 generic `Contributor`로 fallback — 생성 XML에 허용값만 나가도록.
   기존 테스트의 `<Role>COMPOSER</Role>` 기대값을 `<Role>Composer</Role>`로 갱신.
3. **well-formed XML guard** (테스트): `quick-xml`(dev-dep)로 3개 fixture의 생성 ERN
   전체를 파싱해 루트 `ern:NewReleaseMessage`와 태그 균형을 확인.
4. **공식 XSD 확보 시도 → 실패, 미완료로 명시.**
   ERN 3.8.2 공식 schema를 신뢰 가능한 경로에서 구할 수 없음을 확인:
   `service.ddex.net/xml/ern/382/*`는 404 HTML,
   daddykev/ddex-workbench 저장소·PyPI wheel에도 XSD 없음
   (로컬에 받아둔 `/tmp/ddex-xsd` 파일들은 전부 404 HTML로 확인).
   공식 XSD validation은 F6 전제 조건으로 미완료 유지. 공개 제3자 API에
   production XML 검증을 의존시키는 방식은 쓰지 않음.

- **남은 것:**
  - ~~`run_prepare_release` retry가 기존 DDEX row 수를 정상 반환함을 증명하는 integration test (코드 수정은 완료, 테스트 미작성)~~ → **2026-09-25 완료** (`prepare_release_retry_reports_ddex_count_under_rls`; fix 되돌리면 실패 확인).
  - 공식 XSD 기반 schema validation — 신뢰 가능한 XSD 확보 또는 첫 파트너 계약 후.

## F5 — MockDSP + Execution (실제 전송 인터페이스): 완료

오늘 한 일:

1. **worker RLS end-to-end 테스트** (`crates/core/tests/execution.rs::dsp_worker_role_rls_delivery`).
   non-owner `audeniq_worker` 역할 + deploy grants 적용 상태에서
   E-0 enqueue → claim → E-2/E-3 send 전체를 수행하고, 다른 org의 `delivery_jobs`는
   보이지 않음을 검증. production 런타임 형태 그대로 테스트.
   부수 발견: FORCE RLS는 테이블 owner에게도 WITH CHECK를 적용하므로,
   org context 없이 직접 INSERT하면 owner라도 `42501` — fail-closed 의도대로 동작.
2. **partner별 DDEX artifact routing** (`execution.rs::materialize`).
   전송 문서는 `(package_id, dsp_id)`의 `distribution.ddex_messages` row를 우선 사용
   (sha256 검증 포함). DDEX row가 없고 transport가 mock이 아니면
   `EXECUTION_DDEX_MESSAGE_MISSING`으로 wire call 전에 fail-closed —
   합성 preparation envelope이 실제 partner wire에 나가는 경로를 원천 차단.
   테스트 2개: routing 선호 / fail-closed. MockDsp는 수신한 전송 문서의
   `ern_sha256`을 기록하도록 확장해 routing을 관찰 가능하게 함.
- **남은 것:** 없음 (계약 없이 가능한 범위). DSP-01~12 기존 계약 테스트 전부 통과 유지.

## F6 — 실제 첫 파트너: 미완료 (외부 blocker)

- 계약·프로파일·명세·라이선스·샌드박스·실제 production credentials·runbook이 없어
  실제 파트너 연동을 "완료"로 표현할 수 없음. 오늘 작업에서도 완료로 표기하지 않음.
- 오늘 F6를 향해 깔아둔 기반:
  - partner/DSP별 DDEX interchange artifact를 `ddex_messages`에 저장·선택하는 구조
  - artifact 없으면 실제 전송 fail-closed
  - `ddex_sender_dpid` / `ddex_recipient_dpid` DPID 필드
  - contributor role AVS 매핑
- **남은 것 (전부 계약 전제):**
  - DSP 직접 계약, 기술 담당·테스트 환경 확보
  - partner profile, DDEX 명세 준수 확인, 라이선스
  - 공식 XSD 기반 schema validation, DDEX 인증
  - 실제 테스트 릴리스 접수·라이브 확인, runbook
  - ~~명시적 activation 모델 (`MOCK` vs `CONTRACTED`): 현재 `delivery_enabled`는
    operator 전용으로 잠갔지만, 상용 partner가 계약 경로를 우회하지 못하도록
    non-mock profile에 contract/endpoint/profile readiness를 강제하는 구조는
    계약 없이도 DB/code 레벨에서 구현 가능 — 다음 작업 후보.~~ → **2026-09-25 완료** (migration 0022 + Stage 2/E-0/materialize 3중 게이트 + 테스트 2개).

## F7 — Finance: 부분 완료

- generic finance ledger는 병합되어 있음. 오늘 작업 없음.
- **남은 것:** BLUEPRINT 의미의 F7 = **첫 파트너 실제 보고서 파서 + 실보고 대사**.
  실제 파트너 보고서가 없으므로 미완료. `docs/F7_PLAN.md` 참조.
  (과거에 "F7 완료"라고 말한 적 있으나, 코드상 ledger 병합과 BLUEPRINT의
  실보고 파서·대사 완료는 다른 것 — 정정함.)

## F8 — 운영 베타: 미착수

- **남은 것:** 자원/온콜·복원·fallback·보안·실제 발매 일일 한도, GO/NO-GO 표의
  전부 증거 링크 확보.

## F9 — DSP 확장: 미착수

- **남은 것:** 둘째 DSP/업스트림 어댑터를 기존 계약 테스트(DSP-01~12)로 추가,
  기존 카탈로그/정산·접근권한 회귀 없음 확인.

---

## 계약 없이 추가로 가능한 다음 작업 — 2026-09-25 갱신

1. ~~`run_prepare_release` retry의 DDEX row-count RLS integration test 작성~~ → **완료**
   (`prepare_release_retry_reports_ddex_count_under_rls`; fix 되돌리면 실패 확인).
2. ~~`delivery_enabled` 명시적 activation 모델 (`MOCK` | `CONTRACTED`) 설계·구현~~ → **완료**:
   - migration `0022`: `execution.adapter_profiles.activation_kind` (`MOCK`|`CONTRACTED`, default `MOCK`).
   - Stage 2: `MOCK`만 `delivery_enabled`으로 eligible에 합류. `CONTRACTED` + 계약 경로 없음 → `INELIGIBLE_NO_CONTRACT`로 감사 trail에 명시.
   - E-0: route plan에 있어도 `CONTRACTED`는 계약 경로(route enabled + endpoint ACTIVE + 미해지 계약 revision) 없으면 job 생성 안 함 (defense in depth).
   - materialize: `CONTRACTED`는 DDEX artifact 없으면 transport 무관하게 fail-closed (합성 envelope이 상용 파트너 wire에 나갈 수 없음).
   - 테스트 2개: `stage2_contracted_profile_not_eligible_without_contract`, `contracted_profile_cannot_enqueue_without_contract`.
   - 참고: F1 스키마가 route `enabled`와 endpoint `ACTIVE`를 CHECK로 원천 차단하므로, 현재 스키마에서 상용 전송은 구조적으로 불가능 — 이 모델은 F6에서 그 CHECK가 풀릴 때를 대비한 것.
3. 남은 계약-프리 작업: 공식 XSD 확보 전까지 F6 schema validation은 blocker 유지.
   그 다음은 태영 결정에 따라 F7 보고서 파서용 synthetic fixture 설계 또는 F8 운영 항목 선행 가능.
