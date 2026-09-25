# AUDENIQ Foundation 구현·검증 보고서

기준: docs/BLUEPRINT.md FINAL 1.0. 작업 브랜치: foundation/f0-f1. 운영 배포 및 main 병합은 수행하지 않았다.

## ① 실제 구현 결과

새 백엔드는 Rust로 구현했다. Cargo workspace의 audeniq-core가 Axum API, Tokio Worker, SQLx migrator 세 실행 파일과 도메인/인증/저장소/작업 엔진을 공유한다. audeniq-edge는 Rust/WASM Cloudflare BFF이다. 기존 홈페이지·Studio·설문 소스는 web 아래 보존했다. 새 Python 애플리케이션이나 런타임 의존성은 없다.

주요 파일:
- crates/core/src/{api,auth,catalog,uploads,storage,operations}.rs: 실제 API와 기반 업무.
- crates/core/src/{domain,contracts,packages}.rs 및 config: 상태, FreshnessGuard, 후속 권리·정산 구조, §10 패키지.
- migrations/0001_foundation.sql, 0002_state_contract.sql: 초기 DB 및 전이 계약.
- crates/core/tests/foundation.rs: 실제 PostgreSQL·HTTP Router·Mock ObjectStore 통합 테스트.
- compose.yaml, deploy/Dockerfile, bootstrap.sql, grants.sql: 개발 실행 및 역할 분리.
- .github/workflows/foundation.yml: Rust/PostgreSQL/WASM 검사와 Docker Compose 기동 검사.
- docs/API.md, DATA_MODEL.md, deploy/PRODUCTION.md: 실제 API, ERD/전체 엔티티 대응표, 배포 전 연결 검증 절차.

인증 감사 로그가 응답 request ID와 다르게 기록되던 문제를 수정하고 회귀 테스트를 추가했다.

## ② 실행 및 테스트

실행 명령:
```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked -p audeniq-core --bins
cargo test --locked -p audeniq-core -- --test-threads=2
cargo build --locked -p audeniq-edge --target wasm32-unknown-unknown
docker compose up --build -d
```

최초 Foundation CI 35833285808에서 Rust/API/Worker/WASM 빌드, clippy, 단위 16개와 PostgreSQL 통합 9개가 통과했다. 후속 검증에는 제한된 런타임 DB 역할, 상태 전이표 일치, verification/snapshot 불변성과 인증 감사 request ID 검사를 추가했다. 최종 실행 증거는 아래 검증 기록으로 확정한다.

테스트는 disposable PostgreSQL 17에서 SQLx가 생성한 별도 DB로 실행한다. R2 테스트는 Mock ObjectStore를 사용한다. 가짜 QC/DSP/정산 성공 결과는 테스트하지 않는다. Compose CI의 down -v는 해당 CI가 생성한 개발 볼륨에만 적용된다.

## ③ 데이터베이스

6개 스키마, 31개 테이블을 생성한다. rights에는 계약·계약 revision 참조 테이블이 있으며 finance 실행 테이블은 후속이다. 상세 목록 및 전체 보고서 엔티티의 구현/후속 구분은 DATA_MODEL.md에 있다.

PK, 조직 복합 FK, 유일성/검사 제약, 조회/선점 인덱스, 불변 트리거 및 상태 전이/row_version 방어를 적용했다. submitted revision과 snapshot은 직접 UPDATE/DELETE할 수 없다. 마이그레이션 재실행과 잘못된 참조/중복 데이터 거부를 통합 테스트로 검사한다.

마이그레이션 owner와 API/Worker 역할을 분리했다. 일반 런타임은 감사 수정·삭제·TRUNCATE, 제출 revision 생성 권한이 없다. Worker는 회원 비밀번호 테이블에 접근할 수 없다.

## ④ API

세부 요청·응답은 API.md 참조. 실제 엔드포인트:
- 회원 등록/로그인/로그아웃, 현재 사용자/조직 조회, 조직 생성.
- 조직 멤버십과 리소스 ACL 변경.
- 아티스트/레이블/발매 초안 생성·조회·수정·보관, 트랙 연결.
- 직접 업로드 권한 발급·완료 확인, asset 메타데이터 조회.
- health/ready 및 명시적으로 비활성인 submit(501).

모든 요청은 서비스 경계 인증을 통과해야 한다. 사용자 보호 API는 별도로 Rust 서버 세션을 검증한다. JSON 요청, UUID 내부 ID, row_version 낙관적 잠금, 64KiB 본문 제한을 사용한다. 성공 응답은 JSON이고 도메인 오류는 error.code이다.

## ⑤ 보안·무결성

Argon2id, 원문 대신 해시된 세션/CSRF 토큰, 만료/폐기, HttpOnly/SameSite 쿠키 및 운영 Secure 설정, 정확한 Origin 검증, DB 인증 제한을 적용했다. 브라우저 제공 사용자 ID 헤더는 인증 근거로 사용하지 않는다.

ACTIVE 멤버십과 ACL을 매 요청 재평가한다. 타 조직 발매/파일/아티스트/레이블 접근·연결 거부, 철회 후 접근 거부, ACL 불일치를 테스트한다.

객체 키·asset ID는 서버가 생성한다. 서명 권한은 대상 PUT/크기/MIME/nonce/기한에 묶인다. HEAD 검증 뒤 별도의 비공개 등록 키로 조건부 복사하여 아직 유효한 업로드 URL로 등록 객체를 덮어쓰지 못하게 한다. ETag를 SHA-256으로 취급하지 않으며 QC는 PENDING이다.

SKIP LOCKED, lease token/만료, 제한 재시도, DLQ, 멱등성으로 작업을 관리한다. 오래된 Worker 완료는 거부된다. 업무 변경·감사·Outbox·job은 같은 DB 트랜잭션이다. receipt 중복 방지와 rollback 원자성을 검증한다.

## ⑥ 미완료·미검증

- 실제 R2 계정에서 PUT/HEAD/조건부 복사/CORS를 검증하지 않았다. 현재 검증은 Mock이며 운영 연동 성공을 주장하지 않는다.
- Workers VPC Service/Tunnel 실제 연결과 계정 설정은 미검증이다. Rust/WASM 컴파일 성공은 네트워크 연결 증거가 아니다.
- Pre-submit 동의/계약/미성년자 요건, 최종 제출, 전체 QC, 권리 심사, DDEX, DSP 전송/접수/LIVE, 로열티·은행 지급은 비활성이다.
- 관리자 Access JWT와 자체 관리자 권한을 결합한 업무 API는 미구현·비공개다.
- 후속 권리/배급/정산 엔티티에는 구조 타입과 의존 계약만 준비한 부분이 있다. 계약·route·package 저장 구조는 추가 구현했으며, 실제 승인 흐름·delivery_jobs·원장 영속화는 후속이다.
- 트랙 교체·보관·크레딧 편집, 카탈로그/세션 페이지네이션, 세션 폐기·비밀번호 변경, 업로드 취소는 추가 구현했다. 고아 업로드 정리/보존 정책, MFA와 비밀번호 분실 복구는 후속이다.
- 실제 2vCPU/4GB 동시 부하, 백업·복원, 운영 비밀 회전/관측 경보는 검증하지 않았다. 고객 운영 출시는 NO-GO이다.

## ⑦ 다음 개발 단계

1. Pre-submit 계약·동의 정책과 미성년자 증거 요건을 확정하고 불변 ConsentPackage를 발행한다.
2. 실제 R2 및 VPC/Tunnel을 별도 테스트 계정에서 검증한다. Stage 1은 격리 객체의 실제 SHA-256·포맷·이미지·음원 메타데이터를 검사한다.
3. 제출 트랜잭션에서 검증된 consent/assets를 핀한 신규 Application Revision과 Stage 1 job/Outbox를 함께 생성한다. 현재 제출 DB/API 게이트는 검증된 흐름을 추가할 때 해제한다.
4. 보완·새 revision·STALE 결과 처리, 정책버전 및 취소/재시도를 연결한다. 권리 승인이나 DSP 상태는 Stage 1 성공으로 변경하지 않는다.
5. 이후 Stage 2 권리 증거·심사, Stage 3 snapshot/package, 계약된 경로의 실행기를 순차 구현한다.

## 최초 Foundation 검증 기록 — 2026-09-23

검증한 코드 커밋: `76382712e75eb7e876e65a6ab991afe14fc4ccbc`.
[GitHub Actions 35858084863](https://github.com/TAE-OK-11/audeniq/actions/runs/35858084863)의 두 job이 모두 success이다.

| 검증 | 실제 결과 |
|---|---|
| cargo fmt / clippy -D warnings | 통과 |
| API / Worker / migrator 빌드 | 통과 |
| Rust 단위 테스트 | 17 passed, 0 failed, 0 ignored |
| 실제 PostgreSQL 통합 테스트 | 11 passed, 0 failed, 0 ignored |
| Rust/WASM edge 빌드 | 통과 |
| Docker release 이미지 빌드 | 통과 |
| 신규 Compose DB 마이그레이션 / grants | 모두 종료 코드 0 |
| Compose API / Worker 기동 | 실행 확인, API /ready HTTP 성공 |

통합 테스트 이름:
- auth_audit_uses_server_request_id
- auth_sessions_csrf_origin_and_gates
- immutable_revisions_state_and_compare_and_swap
- auth_rate_limit_and_db_token_hashes
- migration_constraints_and_reapplication
- late_revision_check_never_advances_current_release
- organization_acl_and_revocation
- queue_claim_crash_fencing_retry_dead_letter
- outbox_atomic_rollback_and_idempotency
- runtime_roles_enforce_foundation_boundary
- upload_binding_expiry_duplicate_and_freeze

최종 문서 커밋은 실행 코드·마이그레이션·테스트를 변경하지 않는다. Compose readiness는 프로세스/DB 기동 증거이며 실제 R2/VPC나 고객 부하 검증으로 확대 해석하지 않는다.

## 추가 구현 — Foundation 확장

새 애플리케이션 및 테스트 코드는 계속 Rust로 작성했다. SQL 마이그레이션 0003–0006을 추가했으며 기존 마이그레이션은 수정하지 않았다.

| 영역 | 추가된 실제 기능 |
|---|---|
| 카탈로그 | 트랙 metadata/asset 교체, 트랙 soft archive, 크레딧 전체 교체/조회, release row_version 경합 방어 |
| 조회 | 카탈로그·세션 UUID 커서 페이지네이션, 페이지마다 현재 ACL 검사 |
| Pre-submit 준비 | 읽기 전용 체크리스트 API; 동의/미성년자/QC 게이트는 계속 비활성 |
| 계정 | 세션 목록·개별 폐기·전체 로그아웃, 현재 비밀번호 확인 후 변경, 모든 세션 폐기 |
| 인증 동시성 | 로그인 중 비밀번호가 바뀌면 구 비밀번호 검증 결과로 세션 발급 거부 |
| 업로드 | 세션 상태 조회, 취소, 중복 취소 멱등성, 취소 이후 늦은 완료 요청 거부 |
| 배급 데이터 계약 | 계약/revision/endpoint/route/package 5개 테이블과 불변·FK·profile·유일성 제약, 멱등 package 메타데이터 저장 |
| 실행 경계 | 경로 enabled=false, endpoint INTEGRATION_PENDING, API/Worker의 신규 배급·계약 쓰기 권한 없음 |

새 통합 테스트는 초안 편집·크레딧·preflight, 페이지네이션·ACL 철회, 세션/비밀번호 회전, 불변 계약/경로/package, 업로드 취소를 다룬다. 기존 runtime role 테스트에도 크레딧 교체와 세션 조회를 추가했다.

검증 중 발생한 Rust 포맷 차이는 rustfmt 결과를 적용해 수정한다. CI는 포맷 차이가 있어도 컴파일/테스트 결과를 함께 수집하지만, 마지막 포맷 게이트가 실패하므로 포맷 오류를 성공으로 표시하지 않는다. 최종 실행 결과는 아래에 기록한다.

## 확장 최종 검증 기록 — 2026-09-24

검증 코드: `e753c64001c89b62e55f6b17e737b28aa3710f8b`.
[GitHub Actions 35936749958](https://github.com/TAE-OK-11/audeniq/actions/runs/35936749958)에서 rust-postgres와 compose-smoke가 모두 **success**이다.

| 실행 검사 | 결과 |
|---|---|
| cargo fmt --all + git diff --exit-code | 통과, 포맷 차이 없음 |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 통과 |
| cargo build --locked -p audeniq-core --bins | API/Worker/migrator 통과 |
| cargo test --locked -p audeniq-core -- --test-threads=2 | 단위 17개 + PostgreSQL 통합 16개 통과, 실패/무시 0개 |
| cargo build --locked -p audeniq-edge --target wasm32-unknown-unknown | 통과 |
| docker compose up --build -d | release 이미지 빌드, 마이그레이션 0001–0006 및 runtime grants 적용, API/Worker 기동 성공 |
| /ready 서비스 인증 HTTP 검사 | 통과 |

추가된 5개 통합 테스트:
- draft_tracks_credits_archive_and_preflight
- cursor_pagination_rechecks_acl_and_rejects_invalid_limits
- session_inventory_revoke_and_password_rotation
- immutable_contract_route_package_lineage
- upload_cancel_blocks_late_completion_and_is_idempotent

최종 실행 로그에서 migration/grants 컨테이너는 각각 exit 0, Worker는 Up, API readiness 요청은 성공했다. 최초 Foundation 대비 코드/설정/문서 18개 파일을 변경했고 기존 web 파일과 마이그레이션 0001/0002는 변경하지 않았다.

이 기록을 추가하는 문서 커밋은 실행 코드·마이그레이션·테스트를 변경하지 않는다. 실제 R2/VPC 연결, 법적 동의/서명 검증, 전체 Stage 1 QC, DSP 송출 및 정산 실행은 여전히 완료·검증된 것으로 표시하지 않는다.
