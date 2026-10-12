# 정산 수집·입금 증거·지급 반송 구현

기준: main a91be4f8178b0d1d629b9e47b72c4b1604eeb759.

## 변경된 동작

외부 업체 규격 없이 구현할 수 있는 정산 입력과 반송 업무를 연결했다.
실제 DSP별 보고서 파서, 은행 통신, 신원·계좌 검증을 완료한 변경은 아니다.

| 부분 | 구현 |
|---|---|
| 정산 CSV | 인용부호 안의 쉼표·줄바꿈·이스케이프·BOM 지원. 잘못된 인용부호, 열 수 불일치, 중복 헤더, 누락 금액, 잘못된 통화 거절 |
| 입력 예산 | 최대 16 MiB, 헤더 포함 100,001개 레코드, 128열 |
| 환수·정정 | 음수 수량/금액을 원천 보고서에 보존. 원장의 금액은 기존대로 비음수이며 이 입력으로 원장을 자동 작성하지 않음 |
| 원본 | source hash와 원본 CSV를 immutable raw_ref에 함께 보존 |
| 통화 | 한 보고서에 한 정산 통화. 통화가 다른 행은 import 거절 |
| 중복 import | UNIQUE + ON CONFLICT로 동시 중복도 원자적으로 거절 |
| 매칭 | ISRC 후보가 정확히 트랙 한 개일 때만 AUTO. 여러 후보는 AMBIGUOUS 및 후보 증거 생성 |
| 매칭 상태 | 미매칭/모호한 행이 남으면 PARTIALLY_MATCHED. 재호출 시 후보 중복 생성 방지, POSTED 보고서 rematch 거절 |
| 입금 | 보고서와 별도로 금액·통화·은행 참조·증빙·담당자를 append-only 기록. 동일 참조의 상충 결과 거절 |
| 지급 반송 | 실제 반환액만 ROYALTY_PAYABLE에 복원. 부분/전체 반환, 중복 증빙, 초과 반환 거절 |
| 반송 수수료 | 기본적으로 수수료 유지. 최종 전체 반환 시 명시적으로 --refund-fee를 선택한 경우만 환불. 계약별 정책을 확인 후 선택 |
| 지급 참조 | 기존 은행 거래번호와 다른 결과 통지를 거절. SETTLED 뒤 일반 returned는 반환액/참조가 있는 새 경로를 요구 |

MATCHED는 카탈로그 매칭 상태다. 입금 기록만으로 보고서를 POSTED로
바꾸거나 지급가능액을 생성하지 않는다. 기간별 계약 split, 수금 공제,
환전/세무, 확정 마감이 다음 작업이다.

## 실행 경로

모든 아래 변경은 schema owner용 audeniq-admin CLI에서 수행하며,
활성 ADMIN/OPERATOR 직원 이메일을 --operator로 지정해야 한다.
새 테이블에 API/worker runtime의 일반 쓰기 권한을 부여하지 않았다.

```bash
audeniq-admin --operator STAFF_EMAIL royalty import ORG_UUID DSP_CODE 2026-08-01 2026-08-31 USD NORMALIZED_CSV
audeniq-admin --operator STAFF_EMAIL royalty match ORG_UUID REPORT_UUID
audeniq-admin --operator STAFF_EMAIL royalty receipt ORG_UUID REPORT_UUID BANK_REFERENCE 123.45 USD EVIDENCE_JSON_FILE
audeniq-admin --operator STAFF_EMAIL royalty show ORG_UUID REPORT_UUID

audeniq-admin --operator STAFF_EMAIL payout return ORDER_UUID BANK_RETURN_REFERENCE 1000 EVIDENCE_JSON_FILE
audeniq-admin --operator STAFF_EMAIL payout return ORDER_UUID BANK_RETURN_REFERENCE 1000 EVIDENCE_JSON_FILE --refund-fee
```

EVIDENCE_JSON_FILE에는 빈 객체가 아닌 실제 은행 명세/증빙의 내부 참조를 넣는다.
표준 CSV 헤더: isrc,dsp_track_id,quantity,gross_amount,currency.
추가 열은 원본 증거에 보존한다. 외부 DSP의 실제 원본은 규격별로 변환한
후 import해야 하며, 이 변환기를 DSP별로 구현한 것으로 표시하지 않는다.

부분 반환 중 주문 상태는 SETTLED로 유지하고 반환 합계는 payout_returns에서
조회한다. 최종 net_amount 전액 반환 시 RETURNED로 바뀐다. 원 지급 분개를
변경하거나 전체 요청액을 임의 복원하지 않는다. 은행 수수료 차감으로 일부만
돌아왔다면 실제 입금액만 입력한다. 재지급은 별도 새 신청/주문으로 처리한다.

## DB 이행

0106 migration(원래 0090, main 병합 때 번호 변경)은 음수 원천 조정과 AMBIGUOUS/PARTIALLY_MATCHED를 허용하고
royalty_receipts, payout_returns를 추가한다. 입금/반송 기록은 변경·삭제를
거절하며, 조직 경계의 FK와 은행 참조 유일성을 적용한다.

기존 MATCHED 보고서 중 UNMATCHED 행이 남은 것은 PARTIALLY_MATCHED로 수정한다.
POSTED 장부와 과거 AUTO 매칭은 자동 변경하지 않는다. 이미 여러 후보에서
잘못 AUTO 처리된 과거 자료는 운영 검토가 필요하다.

## 남은 연결

- 실제 공급자 인증/SMS·예금주 검증·은행 통신: 공급자 선정, 규격, 시험 계정 필요.
- 실제 DSP 전송과 보고서 파서: 파트너 규격/정산 표본 필요.
- 서명 계약→split→원장 확정, 원천 수금 공제·기간별 배분, 세무/환전 계산.
- 고객/직원의 웹 API와 화면. 이번 신규 정산·반송 업무 진입점은 운영 CLI다.
- 권리 승인/미성년·대표권, 보류 금액, 발매 후 수정/중단/이전, 개인정보 보존·탈퇴.
- 퍼블리싱과 별도 부가서비스.

이 변경만으로 전체 잔여 구현이나 출시 준비가 완료되지 않는다.

## 로컬 검증

- Rust 1.98.1, cargo check --locked -p audeniq-core --lib --bins --tests 통과.
- cargo test --locked -p audeniq-core --lib royalty: 7개 통과.
- cargo clippy --locked -p audeniq-core --all-targets -- -D warnings 통과
  (아래와 같은 로컬 빌드 설정 사용).
- 변경 Rust 파일의 rustfmt 검사 및 git diff --check 통과.
- PostgreSQL 문법 파서로 새 migration과 소스의 정적 SQL 53개 문법 검사 통과.
- 최초 기본 dev 의존성 빌드에서 linker 오류가 발생해, 로컬 검증은
  CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_CODEGEN_UNITS=1,
  --config 'profile.dev.package."*".opt-level=0' 설정을 사용했다.
  이 환경 설정을 저장소의 빌드 정책 변경으로 커밋하지 않았다.
- PostgreSQL 통합 테스트는 실행하지 못했다. 테스트 DB를 비특권 사용자로
  실행하는 데 필요한 UID/GID 전환이 현재 환경에서 허용되지 않는다.
  SQL 문법 검사와 컴파일 통과가 DB 이행/동시성/잔액 검증의 대체는 아니다.
- 실제 은행/DSP 호출·송금·배급·운영 DB 이행은 수행하지 않았다.
