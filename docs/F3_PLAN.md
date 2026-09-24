# F3 — Stage 2 심사 (BLUEPRINT §5)

브랜치: `foundation/f3-stage2-review` (from `foundation/f2-presubmit-stage1` @ `3e640ad`)

## 목표

F2가 `park()`으로 보존한 `rights`/`stage2` job을 실제로 처리한다.
BLUEPRINT §5.9: 논리 모듈 5개를 **하나의 `stage2.review` durable job** 안에서
체크포인트와 함께 실행한다. 엔진별 바이너리·큐·잡을 만들지 않는다.

## 입력

job payload (F2가 enqueue): `{"revision_id", "validation_package_id", "package_hash"}`

## 모듈 (crates/core/src/review.rs)

| # | 영역 | BLUEPRINT | 핵심 로직 |
|---|------|-----------|-----------|
| 1 | `applicant_rights` | 2-0, 2-A/2-B | 라우터 경로 결정(본인/직계약레이블/대리·공동/미성년), grant_atom 체인 검사, 자동 PASS allowlist (a)(b)(c) |
| 2 | `catalog_match` | 2-C | ISRC/UPC 내부 매칭, SHA-256 활성 자산 대조(DUPLICATE_CLAIM), fingerprint는 REVIEW_ONLY 정책 기록(미구현 시 NOT_APPLICABLE) |
| 3 | `metadata_content` | 2-D, 2-E, 2-F | 크레딧 교차대조, 1-C metric_hash 재사용 콘텐츠 신호, 특수 플래그(COVER/REMIX/SAMPLE/AI) 증거 서류 요구 |
| 4 | `policy_integrity` | 2-G, 2-H | route_plans 기반 DSP eligibility(활성 계약 없으면 INELIGIBLE_NO_CONTRACT), 중복 신청·분쟁 대조 |
| 5 | `decision_review` | 2-I | 병합 판정, Verification Package 발행, override |

## 체크포인트·멱등

- 각 모듈은 `operations.check_results`에 `(revision_id, check_code)` 단위로 기록.
  check_results에 immutable 트리거가 있으므로 재실행 시 기존 행이 있으면 해당 모듈 스킵.
- job payload에 `checkpoint` 필드 추가(완료 모듈 목록). 크래시 후 재클레임 시 이어서 실행.

## 판정

- `PASS`: 필수 check 전부 PASS + allowlist 충족 → `distribution.verification_packages`
  발행(rights_epoch, approved_scope, commercial_split_snapshot ref 포함) →
  release `STAGE2_PASSED` → `distribution` 큐에 `prepare_release` job enqueue(F4, 당장은 park).
- `REVIEW_REQUIRED`: allowlist 제외 사유(외부 문서, 불명확 위임체인, 분쟁 OPEN 등) →
  release `STAGE2_REVIEW`, 환송 정보(reason_code, affected 목록) 기록.
- `CORRECTION_REQUIRED`: 수정 가능한 입력 문제 → release `STAGE2_CORRECTION`.
- 미실행·불명확·시스템 실패를 PASS로 처리하지 않는다(§5.8).

## 자동 PASS allowlist (§5.9)

(a) 본인 권리자 일반 발매 + 권리 진술·검사 무충돌 + 2-F 추가 허락 대상 없음.
(b) 사전 검증된 직계약 레이블의 유효 scope 내 발매(contract_revision ACTIVE).
(c) AUDENIQ 직접 생성 전자문서의 hash 검증 통과 + 전문가 검토 정책 활성.
제외: 외부 PDF/스캔, 불명확 공동권리·위임체인, 새 샘플·커버·AI 이용권·새 지역,
분쟁 OPEN, rights_epoch 변경 → REVIEW.

법률 검토 미완료 자동 승인 경로는 `auto_pass_allowed=false` → REVIEW.

## Override (2-I)

- `rights.review_overrides`: 원 check_results를 수정하지 않고 별도 행에
  원 상태/제안 상태/사유/행위자/제2승인자/만료 기록.
- 권리·금전 클래스 강제 PASS는 senior reviewer + 서로 다른 제2 승인자 필요. 서버가 강제.
- API: `POST /api/orgs/{org}/reviews/overrides`.

## commercial_split_snapshot

Verification Package body에 `payee_party_id`, `share_bps`, `contract_revision_id`,
`effective_model` 참조를 핀한다. 정산은 핀된 구간만 사용.

## Migration 0009

- `rights.grant_atoms`: party_id, target(release/track), right_type, territory_set,
  use_set, start_at, end_exclusive, exclusive, sublicensable, parent_grant_id,
  contract_revision_id, revoked_at. immutable 트리거.
- `rights.review_overrides`: 위 override 행. append-only(DELETE 거부).
- `catalog.releases`에 `rights_epoch bigint DEFAULT 0` 추가? → verification_packages에
  이미 rights_epoch 컬럼이 있음. release 단위 epoch 카운터는
  `rights.rights_epochs(org_id, release_id, epoch)` 별도 테이블로 관리.

## 테스트 (crates/core/tests/stage2.rs)

1. 본인 권리자 일반 발매 → PASS, verification package 발행, prepare_release enqueue.
2. 다른 조직의 동일 SHA 활성 자산 → DUPLICATE_CLAIM REVIEW.
3. COVER 플래그 + 증거 없음 → CORRECTION/REVIEW.
4. 활성 DSP route 없음 → eligibility INELIGIBLE_NO_CONTRACT, PASS는 유지(approved_scope 빈 집합).
5. override: 권리 클래스 강제 PASS에 승인자 1명 → 거부, 서로 다른 2명 → 승인.
6. 체크포인트 재개: 2개 모듈 후 크래시 시뮬레이션 → 재실행이 완료 모듈 스킵.

## F4로 미룸

- `prepare_release` 실제 처리(Stage 3).
- Chromaprint 실제 비교(2-C는 SHA/ISRC 매칭 + REVIEW_ONLY 정책).
- 외부 카탈로그 API 조회(UNKNOWN 구분만).
- OCR(외부 문서).
