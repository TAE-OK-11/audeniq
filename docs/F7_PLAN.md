# F7 Finance — 원장 코어 (파트너 독립분)

BLUEPRINT §8, INV-09. 파서·자동 매칭은 F6 소관이므로 제외.
브랜치: `foundation/f7-finance-ledger` (F4와 독립, `f3-stage2-review`에서 분기).

## 스키마 (migration 0011)

| 테이블 | 성격 |
|---|---|
| `finance.ledger_transactions` | 이중기입 거래 1건=1통화, FX·수수료·계약·세금 버전 핀, `reversal_of` 참조 |
| `finance.ledger_entries` | 차변/대변 행, 금액 NUMERIC ≥ 0, 당사자·ISRC·split 스냅샷 연결 |
| `finance.commercial_split_snapshots` | 수취인·지분(bps, 합 10000), append-only (`UNIQUE(org,release,valid_from)`, "greatest valid_from ≤ t"가 유효) |
| `finance.payout_orders` | 수동 승인 기본, `idempotency_key` 유니크, `SUBMITTED_UNKNOWN` 상태 보유 |
| `finance.finance_holds` | scope(ISRC/RELEASE/PARTY) 한정 hold, `active` 플래그로 해제 이력 유지 |
| `finance.royalty_reports` / `report_lines` / `royalty_match_candidates` | F0 데이터 계약대로 테이블만 생성, 파서·매칭 로직 없음 |

원장·스냅샷·보고서·행은 immutable 트리거(UPDATE/DELETE 거부). 지급 주문·hold·매칭 후보는 상태 라이프사이클이 있어 mutable.

## `finance.rs` 핵심 규칙

- `post_transaction`: 1통화 강제, ΣDEBIT=ΣCREDIT (rust_decimal 정확 일치), 음수 거부, 빈 거래 거부. `match_status`가 `AUTO`가 아니면 거부 — 모호한 매칭은 원장에 못 올라감 (INV-09).
- `reverse_transaction`: 원본 불변. side를 뒤집은 상쇄 거래를 `reversal_of`로 연결. 중복 역분개 거부.
- `apply_split_snapshot` / `effective_split`: 과거 구간 재계산 금지. 계약 변경은 새 `valid_from` 행.
- `create_payout_order`: `auto_payout_enabled=false`, 동일 idempotency_key는 기존 주문 id 반환 (중복 지급 0건). hold된 당사자는 생성 거부.
- `approve_payout_order` → `mark_payout_submitted` → `record_bank_result`: 은행 응답 불명확 시 `SUBMITTED_UNKNOWN`으로 고정, 자동 재송금 경로 없음 (재시도는 상태 전이 거부).
- `place_hold` / `release_hold` / `is_payable`: hold는 정확히 하나의 scope에만 적용, 관련 없는 발매로 확대 금지.

## 테스트 (`tests/finance.rs`, 10개)

정상 전기·차대 불일치 거부·음수 거부·복수통화 혼합 거부·모호 매칭 전기 금지·역분개(원본 불변+중복 거부)·split append-only(과거 구간 유지·합 10000 강제)·지급 멱등·SUBMITTED_UNKNOWN 비재시도·hold 범위 한정.

## 남은 것 (F6)

DSP 보고서 파서, `royalty_match_candidates` 자동 매칭, 실보고 샘플 대사, 은행 API 연동(현재 수동).
