# Studio 연동 및 GitHub 리뷰 반영 보고서

> 2026-09-26: 이 보고서의 Rust/WASM Studio(`crates/studio`, `studio-pack`, `audeniq-dev-web`, `audeniq-browser-smoke`)는 React Studio(`web/studio/app`)로 대체되어 삭제됐어요. 현재 절차는 `docs/STUDIO_DEPLOYMENT.md`를 보세요.

## ① 실제 구현 결과

- `crates/studio`: Rust/WASM 브라우저 앱. 회원 등록·로그인·로그아웃, 조직 선택, 카탈로그 페이지네이션, 아티스트·레이블·발매 초안 생성/수정/보관, 트랙 추가/보관, WAV/FLAC R2 직접 업로드, 제출 준비 상태 조회, 세션 폐기.
- `studio-pack`: 기존 Studio의 CSS·폰트 정의·로고를 재사용하는 배포 산출물 생성. 원본 시제품 HTML/JS와 랜딩·설문 파일은 유지. 기존 신청 마법사 전체 화면을 그대로 연결한 것은 아니며, Foundation용 연결 화면을 별도 제공한다.
- `crates/edge`: 동일 Worker에서 정적 파일과 `/api/*` 처리. API 요청은 서비스 시크릿을 추가해 PRIVATE_API VPC binding으로 전달. `/api/admin*` 차단 유지.
- `audeniq-dev-web`: 개발 전용 localhost:5173 게이트웨이. 운영 이미지에 포함하지 않음.
- `deploy/compose.production.yaml`: 독립 운영 프로젝트·DB 볼륨, 공개 포트 없는 API/PostgreSQL, 제한된 DB 역할, Tunnel 토큰 파일, 리소스 제한.
- GitHub 리뷰 3건을 확인했고 원본을 보존했다. 수정/유보 근거는 `REVIEW_RESPONSE.md`, 보존·비용 산정 기준은 `OPERATIONS_RETENTION.md`에 기록했다.

## ② 실행 및 테스트

최종 실행 결과와 검증 커밋은 검증 완료 후 아래에 기록한다. 검증에 사용되는 명령:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked -p audeniq-core --bins
cargo test --locked -p audeniq-core -- --test-threads=2
cargo test --locked -p audeniq-studio
cargo clippy --locked -p audeniq-studio --lib --target wasm32-unknown-unknown -- -D warnings
cargo build --locked -p audeniq-studio --lib --release --target wasm32-unknown-unknown
cargo build --locked -p audeniq-edge --target wasm32-unknown-unknown
cargo run --locked -p audeniq-studio --bin studio-pack
wasm-bindgen --target web --out-dir web/studio/dist/pkg target/wasm32-unknown-unknown/release/audeniq_studio.wasm
# crates/edge에서 실행: 실제 배포 없음
npx --yes wrangler@4.137.0 deploy --dry-run --outdir /tmp/audeniq-edge-bundle
# 일회용 CI DB/개발 서버/ChromeDriver에서 실행
target/debug/audeniq-browser-smoke
```

Docker 개발 환경은 `docker compose up --build -d` 후 서비스 시크릿을 포함한 `/ready` 요청으로 검증한다. 운영 Compose는 설정 파싱 및 공개 포트 없음만 검사하며 운영 서비스를 시작하지 않는다. 현재 작업 환경에는 Cargo/Docker가 없어 GitHub Actions의 격리된 러너에서 실행한다.

### 최종 검증 결과 (2026-09-24)

- 검증 커밋: `6913166` (브랜치 `foundation/f0-f1`)
- GitHub Actions run: [36010757220](https://github.com/TAE-OK-11/audeniq/actions/runs/36010757220) — 2026-09-24T14:09:40Z ~ 14:20:09Z
- `compose-smoke`: **success**
- `rust-postgres`: **success** (fmt, clippy `-D warnings`, build, 단위·PostgreSQL 통합 테스트, WASM/edge 빌드, 브라우저 스모크 전부 통과)
- 위 커밋 이전에는 `cargo fmt` 미적용 1건과 rust-cache 도입 후 `cargo install` 재설치 거부 2건으로 실패했으나 모두 수정 후 녹색 확인.

## ③ 데이터베이스

기존 마이그레이션 0001~0006 및 6개 스키마·31개 테이블을 유지했다. 이번 변경에 신규 마이그레이션은 없다. 세션/CSRF 원문은 DB에 저장하지 않고 해시만 유지한다. 비밀번호 검증·해싱을 행 잠금 밖에서 수행한 후 같은 비밀번호 버전인지 잠금 안에서 재확인한다. 비활성 사용자의 ACTIVE 멤버십 추가를 거부한다. 크레딧 당사자 검증은 중복 제거 후 한 번의 ANY 조회로 처리한다.

## ④ API

새 엔드포인트 `POST /api/auth/csrf`: 세션 쿠키 + 정확한 Origin 필수, `{}` 요청 → `{ "csrf_token": "..." }`. 세션별 HMAC 기반 토큰이라 새로고침과 여러 탭에서 안정적으로 복구하며 브라우저 저장소에 저장하지 않는다. 쓰기 요청은 이후 X-CSRF-Token을 포함한다.

기존 회원·조직·카탈로그·세션·업로드 API를 연결했다. 전체 요청/응답은 `API.md`. 업로드 완료는 사용자별 15분당 60회 제한하고 만료 시 스토리지 호출 전에 거부한다. 잠금 시간 초과·직렬화/교착 충돌은 409로 처리한다. 최종 제출 API의 501 비활성 처리를 유지한다.

## ⑤ 보안

조직 멤버십과 resource_acl은 Rust API가 매 요청 검증한다. Workers가 사용자 신원을 대신 확정하지 않는다. 서비스 비밀키는 서버/Worker secret에만 존재한다. 연결 화면은 localStorage 시제품 상태를 사용하지 않으며 제출·권리·배급 성공을 만들지 않는다. 업로드 파일은 브라우저→R2로 직접 전송하며 콘텐츠 유형·nonce만 전달하고 Content-Length는 브라우저가 설정한다. 페이지 메타데이터를 HTML escape하고 CSP를 적용한다. 감사·상태 변경·Outbox의 동일 트랜잭션은 유지한다.

## ⑥ 미완료·미검증

- 실제 NHN 서버, Cloudflare VPC/Tunnel, 배포된 Workers binding, R2 계정/CORS/서명 URL의 실제 브라우저 연동은 아직 검증하지 않았다.
- 전체 기존 신청 마법사·프로필·지원·권리·정산 UI 연결은 미완료다. 현재 레이블 당사자/트랙 아티스트 선택에는 내부 ID 입력이 필요하다.
- 업로드 완료 중 DB 행 잠금은 유지된다. 네트워크 IO를 분리하는 durable completion lease, 고아 객체 정리, auth_limits 보존 작업, 큐 sweep/upsert 최적화는 남아 있다. 업로드 복사 직후 만료되는 경계에서 고아 객체 가능성이 남는다.
- F2 package/check 저장소의 runtime 권한은 의도적으로 열지 않았다. 후속 구현 시 최소 권한과 runtime-role 테스트를 함께 추가해야 한다.
- 법적 동의(성인 self-consent)·미성년 동의 게이트·음원 QC는 F2에서 활성화했다(브랜치 `foundation/f2-presubmit-stage1`). 미성년·전자서명 자동 경로는 법률 검토 전까지 `MINORITY_REVIEW_REQUIRED` 게이트로 고정. DDEX/DSP 전송, 권리 자동심사, 로열티 확정·지급은 미활성.
- 실제 운영 배포·병합·DNS/방화벽/계정 변경·유료 서비스 개설은 하지 않았다.

## ⑦ 다음 단계

법률 검토를 반영한 Pre-submit 동의/계약 및 미성년 동의 요건, 불변 신청 revision 생성, 실제 콘텐츠 체크섬/음원 QC, 작업자 freshness 검증을 연결한다. 기존 신청 마법사의 화면을 구현된 API 범위부터 순차 통합하고 ID 입력을 권한 필터가 적용된 선택 UI로 개선한다. 운영 전에는 실제 R2/VPC/서버 연결, 업로드 취소·복사 경합, 백업 복원 및 부하 검증이 필요하다.

---

## F2 Pre-submit + Stage 1 (브랜치 `foundation/f2-presubmit-stage1`)

BLUEPRINT §§3–4 구현. 법률 검토(§23.1) 스킵 → 미성년·전자서명 자동 경로는 전부
`MINORITY_REVIEW_REQUIRED` 게이트로 고정. 상세 설계는 `docs/F2_PLAN.md`.

### 구현 범위

- `POST /api/orgs/{org}/releases/{id}/presubmit` — 0-A(계정)·0-D(업로드 admit)·0-B/0-C(미성년 경로 감지) 게이트 평가.
- `POST /api/orgs/{org}/releases/{id}/consents` — 성인 self-consent 패키지 생성. 미성년 경로 포함 시 `MINORITY_REVIEW_REQUIRED`로 차단.
- `POST /api/orgs/{org}/releases/{id}/submit` — 게이트 서버 재확인 → 불변 revision 발행 → `qc` 큐에 `stage1` job enqueue. idempotency key: 동일 key+동일 body는 최초 revision으로 수렴, 동일 key+변경 body는 `IDEMPOTENCY_KEY_REUSED` 422.
- Stage 1 워커: 1-B 필드 검증(8개 check) + 1-C 파일 QC(오디오 6·이미지 4 check, FFprobe 기반) + 변경점 캐시(result_hash, 재분석 0회) + 1-D Validation Package 발행.
- Stage 1 통과 시 같은 트랜잭션에서 `rights` 큐에 `stage2` job enqueue. `execute()`가 `stage2`를 만나면 `operations::park()`으로 QUEUED에 보존(F3 구현 전까지 DLQ 방지, attempts 리셋).
- `GET /api/orgs/{org}/releases/{id}/submission` — revision + checks + 상태 조회.
- 마이그레이션 0007(`distribution.validation_packages`), 0008(`application_revisions.idempotency_key` + unique).

### 로컬 검증 결과 (2026-09-25)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과
  - `audeniq-core` lib 26개 (qc 단위 테스트 포함)
  - `foundation` 18개 (F1 회귀 없음; migration 0008 추가로 raw INSERT 3건에 `idempotency_key` 명시)
  - `stage1` 10개 (presubmit 게이트, submit 멱등 2, stage1 전체 흐름, 변경점 캐시, 손상 오디오 보완, 초대형 에셋 TECHNICAL_RETRY, 미성년 게이트, stage2 park)
- GitHub Actions CI 결과는 푸시 후 아래에 기록한다.

### 최종 검증 결과 (2026-09-25)

- 검증 커밋: `25cdc9e` (브랜치 `foundation/f2-presubmit-stage1`)
- GitHub Actions run: [36022926027](https://github.com/TAE-OK-11/audeniq/actions/runs/36022926027) — `completed/success`
- `compose-smoke`: **success**
- `rust-postgres`: **success** (fmt, clippy `-D warnings`, build, 단위·PostgreSQL 통합 테스트, WASM/edge 빌드, 브라우저 스모크 전부 통과)
- 첫 푸시(`f06831d`)의 run 36022203984는 `rust-postgres` 실패: qc 단위 테스트가 픽스처 생성에 `ffmpeg`를 쓰는데 러너에 없어서 5개 실패. 워크플로우에 `ffmpeg` 설치 단계 추가로 해결.

---

## F3 — Stage 2 리뷰(실제 처리기) (2026-09-25)

BLUEPRINT §5 구현. F2에서 park 처리하던 `stage2` job이 이제 실제 심사 로직을 실행한다.
상세 설계는 `docs/F3_PLAN.md`.

### 구현 범위

- `POST /api/orgs/{org}/reviews/overrides` — 심사 override 기록. rights/money class 강제 PASS는 senior reviewer(OWNER) + 서로 다른 두 번째 승인자(활성 멤버) 필수. `role`은 API가 JWT에서 파생하며 클라이언트가 임의 지정 불가.
- Stage 2 워커(`rights` 큐, `review::run_stage2`): 5개 논리 모듈 체크포인트 재개 + lease fencing으로 STAGE2_PASSED/STAGE2_REVIEW/STAGE2_CORRECTION 전이.
- Verification Package: revision revision_id의 체크 결과 전체 스냅샷, `verification_packages`에 sha256 hash 기록, immutable 트리거로 변경 불가.
- DSP 수신 가능 scope: 계약·endpoint이 활성일 때만 도출. 활성 DSP 계약이 없으면 scope은 비어 있고 Stage 2 자체 PASS는 가능하다.
- duplicate SHA/ISRC 감지: 타 조직의 활성 릴리스와 동일 asset sha256 또는 ISRC가 있으면 S2_DUP_MATCH → STAGE2_REVIEW.
- commercial split snapshot: 계약 split 근거가 없을 때는 균등 분배가 아니라 `S2_COMMERCIAL_SPLIT` REVIEW_REQUIRED로 보류.
- 마이그레이션 0009(`rights.grant_atoms`, `rights.review_overrides`, `rights.rights_epochs`, `distribution.verification_packages`, 체크포인트 컬럼 + enum/상태값 확장).

### 로컬 검증 결과 (2026-09-25)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과
  - `audeniq-core` lib 26개
  - `foundation` 18개
  - `stage1` 10개 (기존 park 테스트는 `stage2_job_is_executed_not_parked`로 변경 — F3에서 stage2는 실제 실행되어 SUCCEEDED + STAGE2_PASSED)
  - `stage2` 5개 (self rights-holder PASS, 타 조직 duplicate REVIEW, lease loss, two-person override, seniority 매핑)
- GitHub Actions CI 결과는 푸시 후 아래에 기록한다.

### 최종 검증 결과 (2026-09-25)

- 검증 커밋: `bf513a4` (브랜치 `foundation/f3-stage2-review`)
- GitHub Actions run: [36068842959](https://github.com/TAE-OK-11/audeniq/actions/runs/36068842959) — `completed/success`
- `compose-smoke`: **success**
- `rust-postgres`: **success** (fmt, clippy `-D warnings`, build, 단위·PostgreSQL 통합 테스트 전부 통과)

---

## F4 — Stage 3 준비 (Muse 담당분) (2026-09-25)

BLUEPRINT §6의 3-A(Finalizer)/3-D(Canonical Model)/3-F(Package) — Muse 담당분.
3-B(식별자)·3-C(Route)·ERN·3-G(일정)·3-H(Preflight)는 Astra 담당(migrations 0011+).

### 구현 범위

- 마이그레이션 0010: `distribution.canonical_releases` (verification_package 1:1 불변 스냅샷, `UNIQUE(verification_package_id)`), `distribution.distribution_packages` (content-addressed 패키지, `status` 기본 `PREPARED` — Astra 단계가 확장, 불변 트리거).
- `crates/core/src/distribution.rs` (신규):
  - `CanonicalRelease` — release/트랙/아티스트/asset sha256/크레딧 스냅샷 + Stage 2 핀 4종(`verification_package_hash`, `approved_dsp_ids`, `rule_version`, `rights_epoch`).
  - `build_canonical(pool, verification_package_id)` — 읽기 전용 스냅샷 생성. verification package decision이 PASS가 아니면 거부.
  - `freeze_package(pool, canonical)` — JSON 직렬화→sha256→`distribution_packages` 기록. 동일 스냅샷이면 기존 행 반환(멱등). 패키지 body의 `identifier_refs`/`route_id`/`dsp_packages`/`preflight_ref`는 Astra 단계를 위한 빈 플레이스홀더.
  - `run_prepare_release(pool, job)` — lease fencing + 멱등 재개. rights epoch가 Stage 2 핀과 달라지면 `STAGE3_CORRECTION`으로 두고 `stage2` job을 재enqueue(2차 환송). 정상 시 `STAGE2_PASSED → STAGE3_PREPARING → READY_FOR_DELIVERY`.
- `operations.rs`: `prepare_release` park를 실제 핸들러로 교체.
- `crates/core/tests/distribution.rs`: happy path(전체 파이프라인에서 패키지 hash 존재 확인), freeze 멱등 2개.
- `crates/core/tests/stage2.rs`: 기존 park 기대 테스트를 F4 실행 동작으로 갱신.

### 로컬 검증 결과 (2026-09-25)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과 (lib 26, foundation 18, stage1 10, stage2 5, distribution 2)
- 참고: 테스트 중 VM이 교체되어 PostgreSQL 16을 재설치하고 `f2test` 롤/DB를 AGENTS.md 절차대로 복구함.

---

## F4 — Astra 병합·연동 (2026-09-25)

`origin/validation/f4-astra-preparation`을 `foundation/f4-stage3-distribution`에 병합(충돌 없음) 후, Astra 모듈을 `prepare_release` 워커에 실제로 연동.

### 병합 내용 (Astra)

- `crates/core/src/`: `ern.rs`(synthetic ERN 생성·검증), `identifiers.rs`(식별자 ledger, `record_existing`), `preflight.rs`(XML/metadata/files/rights 4종 독립 체크), `preparation_model.rs`(`PreparedRelease`/`VerificationPackage`), `route_plan.rs`(DSP scope 검증·제출 계획).
- 마이그레이션 0011: `distribution.identifier_assignments` (append-only, 불변 트리거, 강제 RLS, UPC-A 체크섬·ISRC 형식 SQL 검증).
- 테스트: `stage3_identifiers.rs`(동시성·멱등·충돌·RLS), `stage3_preparation.rs`(synthetic 픽스처 3종·XSD).
- `.github/workflows/f4-preparation.yml`: F4 acceptance 워크플로.

### 연동 내용 (Muse)

- 마이그레이션 0012: `catalog.releases.upc`, `artwork_asset_id`, `CanonicalRelease.schema_version = 2` (UPC·아트워크·트랙 오디오 object key 포함).
- `PreparedRelease::from_canonical(pool, snapshot_id, canonical)` — canonical 스냅샷 + catalog DB에서 UPC·아트워크·오디오 asset·ISRC·발매 메타데이터를 읽어 준비 모델 생성. 누락 시 값을 만들어내지 않고 policy gate로 fail-closed.
- `run_prepare_release` 파이프라인: canonical → freeze → `from_canonical` → synthetic ERN 생성 → 4종 preflight → route plan → preflight 통과 시에만 `READY_FOR_DELIVERY`. ERN XML sha256·preflight 결과·제출 수를 audit reason에 기록.
- 마이그레이션 0013: `distribution.preparation_artifacts` (package당 1행, append-only, 불변 트리거) — ERN sha256·preflight 리포트·route plan 영속화. frozen package body는 canonical 스냅샷 그대로 불변 유지.
- 식별자 ledger 연동: `READY_FOR_DELIVERY` 커밋과 같은 트랜잭션에서 UPC + 전 트랙 ISRC를 `record_existing`으로 기록 (`app.org_id` 세팅). exact-target 재시도는 멱등, cross-target 충돌은 `IDENTIFIER_CONFLICT`로 즉시 DEAD_LETTER (재시도 무의미).
- `CheckStatus`/`PreflightReport`에 `Serialize` 추가 (artifact JSON 저장용). uuid `v5` feature (synthetic route contract 결정적 ID).
- 테스트 수정:
  - `distribution.rs`: FileStore가 content-type을 실제처럼 반환하도록 수정, 준비 보충물(UPC·아트워크·ISRC·메타데이터) 헬퍼화, artifact 행·ledger 기록 검증, 식별자 충돌 시 DEAD_LETTER 신규 테스트.
  - `stage2.rs`: happy-path 끝에 `prepare_release`가 도는 부분에 보충물 추가 (새 파이프라인이 fail-closed라 필요).
  - `stage3_identifiers.rs`: Astra CI가 postgres 슈퍼유저라 RLS가 우회됐던 테스트를 수정 — `app.org_id` 명시 세팅, 트리거/CHECK 검증은 RLS 통과 후 실제 행에 닿도록, `SET ROLE`용 롤 멤버십 부여.

### 로컬 검증 결과 (2026-09-25)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과 (lib 29, distribution 3, foundation 18, stage1 10, stage2 5, stage3_identifiers 2, stage3_preparation 8 — 총 75)
- 참고: 테스트 중 `/tmp`(tmpfs 512M)가 ffmpeg 산출물로 가득 차 stage2 테스트가 실패했으나, 코드 문제가 아니라 디스크 문제였음. `/tmp/audeniq-*` 정리 후 전부 통과.

### CI 검증 결과 (2026-09-25)

- 검증 커밋: `0236829` (브랜치 `foundation/f4-stage3-distribution`)
- F4 preparation acceptance run: [36077276805](https://github.com/TAE-OK-11/audeniq/actions/runs/36077276805) — **success** (6m24s)
- Foundation run: [36077276749](https://github.com/TAE-OK-11/audeniq/actions/runs/36077276749) — **success** (16m15s)

---

## F7 — Finance ledger 병합 (2026-09-25)

`origin/foundation/f7-finance-ledger`(`c8101ac`, Codex)를 `foundation/f4-stage3-distribution`에 병합.

### 병합 내용

- `crates/core/src/finance.rs` (신규): 이중기입 원장 코어 — `post_transaction`(1통화·ΣDEBIT=ΣCREDIT·음수 거부·모호 매칭 금지), `reverse_transaction`(역분개, 원본 불변), `apply_split_snapshot`/`effective_split`(append-only), `create_payout_order`(idempotency_key 멱등·hold 당사자 거부), `approve_payout_order`→`mark_payout_submitted`→`record_bank_result`(`SUBMITTED_UNKNOWN` 비재시도), `place_hold`/`release_hold`/`is_payable`(scope 한정).
- 마이그레이션: 원 브랜치의 `0011_finance_ledger.sql`은 F4의 0011/0012/0013과 번호가 겹쳐 `0014_finance_ledger.sql`로 재번호화. identity·catalog만 참조하므로 의존성상 안전.
- 충돌 해결: `lib.rs`에 `finance` + `identifiers` 모듈 둘 다 유지. `Cargo.toml`은 F7의 `rust_decimal`(sqlx `rust_decimal` feature 포함)을 병합하되 F4의 uuid `v5` feature 유지.
- `crates/core/tests/finance.rs`: 10개 테스트 전부 통과.

### 로컬 검증 결과 (2026-09-25)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과 (lib 29, distribution 3, finance 10, foundation 18, stage1 10, stage2 5, stage3_identifiers 2, stage3_preparation 8 — 총 85)

### CI 검증 결과 (2026-09-25)

- 검증 커밋: `3a88498` (브랜치 `foundation/f4-stage3-distribution`)
- F4 preparation acceptance run: [36080175477](https://github.com/TAE-OK-11/audeniq/actions/runs/36080175477) — **success** (7m26s)
- Foundation run: [36080175476](https://github.com/TAE-OK-11/audeniq/actions/runs/36080175476) — **success** (14m16s)
- 참고: 병합 직후 push(`585151f`)에서는 CI의 `cargo fmt --all`이 `lib.rs` 모듈 순서(`finance`가 `identifiers`보다 먼저)를 지적해 acceptance가 실패했음. 로컬 rustfmt 컴포넌트가 빠져 있어 사전에 못 잡은 것. `3a88498`에서 수정 후 전부 녹색.

---

## F5 — MockDSP + Distribution Execution (2026-09-25)

BLUEPRINT §6의 E-0~E-5 송출 실행 파이프라인. 브랜치 `foundation/f5-mockdsp-execution` (`7341f20`에서 분기). 상업 DSP 연동(F6)은 실제 파트너 계약·명세·샌드박스·credentials가 필요한 단계이므로, F5는 로컬 MockDSP만을 상대로 실행 계층의 내구성·멱등·fail-closed를 검증한다.

### 구현 범위

- 마이그레이션 0015: `execution` 스키마 —
  - `delivery_jobs` (QUEUED→LEASED→SENDING→DELIVERED/FAILED/AWAITING_RECONCILIATION, lease fencing, 조직별 RLS)
  - `delivery_attempts` (송출 시도 1행 = 1회 wire call; `idempotency_key` UNIQUE가 중복 송출 방지. outcome은 `IN_FLIGHT→terminal` 상태머신으로 UPDATE 허용하되 wire fact(id·key·request hash·attempt_no)는 불변, `ACCEPTED`/`REJECTED`는 종단 — blanket immutable 트리거를 컬럼 제한 가드 `guard_attempt_mutation()`으로 교체)
  - `live_bindings` (파트너측 lifecycle: DELIVERED ≠ LIVE 분리)
  - `reconciliation_cases` (E-5 인간 개입 큐)
  - `adapter_profiles` (로컬 MockDSP 프로파일; DSP/route 매핑)
- 마이그레이션 0016: `distribution.preparation_artifacts.ern_xml TEXT` — preparation 단계가 생성한 실제 ERN XML을 hash와 함께 저장. E-2는 저장된 XML 바이트를 읽고 SHA-256을 검증 (synthetic comment 아님). 누락 시 `EXECUTION_ERN_MISSING`으로 fail-closed.
- 마이그레이션 0017: `rights.rights_epochs`의 F3 blanket immutable 트리거를 단조 증가 가드로 교체 — epoch는 write-once가 아니라 버전 카운터여야 E-1의 rights-drift guard가 실제로 동작함. (grant/override 변경 시 자동 bump는 F3 후속 과제)
- 마이그레이션 0018: `operations.jobs` queue CHECK에 `'delivery'` 추가 (wire-call용 독립 워커 풀).
- `crates/core/src/execution.rs` (신규): `DspAdapter` async trait + capability flags + registry. E-0 enqueue/claim/lease, E-1 release/rights epoch/finance hold/profile freshness 게이트, E-2 package/preflight/files materialization (catalog.assets 핀 + storage 바이트 SHA-256 재검증, ERN XML 검증), E-3 durable attempt + wire call (송출 전 attempt/idempotency key 커밋, 외부 호출은 PG 트랜잭션 밖), E-4 ACK·polling·duplicate webhook 차단·LIVE/TAKEDOWN, E-5 reconciliation + unknown resolution (명시적 inquiry만, 재송출 없음).
- `crates/core/src/mockdsp.rs` (신규): ACCEPT/REJECT/TIMEOUT/UNKNOWN/duplicate webhook/delayed live, update/takedown, idempotency key별 호출 기록·inquiry. `Arc<Mutex>` 내부라 clone 시 상태 공유.
- `crates/core/src/operations.rs`: `delivery.enqueue` → 파트너별 `delivery.send` fan-out, `delivery.send`(lease 후 E-0~E-3), `delivery.poll`, `delivery.reconcile`, `delivery.takedown`. dispatcher는 프로세스 공용 MockDsp(`OnceLock`)를 사용해 send→poll→takedown 상태가 이어짐. RLS 테이블 조회는 `delivery_job_status` 등 org 인증 헬퍼 경유.
- `crates/core/src/review.rs`: Stage 2 DSP eligibility가 `execution.adapter_profiles`의 `delivery_enabled=true`인 dsp_id도 읽도록 확장 (execution RLS용 `app.org_id` 설정).
- `crates/core/tests/execution.rs` (신규): DSP-01~12 + operations dispatcher 시나리오, 13개 테스트.

### 로컬 검증 결과 (2026-09-25)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과 (lib 29, distribution 3, execution 13, finance 10, foundation 18, stage1 10, stage2 5, stage3_identifiers 2, stage3_preparation 8 — 총 99)
- 참고: 첫 시도에서 VM 교체로 PostgreSQL이 내려가 distribution 3개가 `PoolTimedOut`으로 실패했으나 인프라 문제였음. AGENTS.md 절차대로 postgres 재시작·`f2test` 롤/DB·`audeniq_api`/`audeniq_worker` 롤 및 `WITH SET TRUE` 부여 후 재실행해 전부 통과.
- 디버깅 중 수정한 근본 문제:
  - E-2가 canonical body의 `/audio/object_key`를 찾았으나 실제 구조는 `asset_object_key`/`asset_id` — catalog.assets 핀 + storage 바이트 SHA-256 재검증으로 교체
  - `delivery_attempts` 불변 트리거와 `IN_FLIGHT→terminal` UPDATE 충돌 — 컬럼 제한 가드로 교체
  - `RETURNING 1` (INT4)을 i64로 디코딩 — i32로 수정
  - `poll_live` 폴백 쿼리의 RLS 미인증 — org 인증 트랜잭션으로 수정
  - `rights_epochs` epoch 증가 불가 — 단조 증가 가드로 교체 (0017)
  - dispatcher의 `delivery.send` 상태 조회 RLS 차단 — `delivery_job_status` 헬퍼 추가
  - MockDSP `DelayedLive`가 `inquire_submission` 경로에서 poll을 카운트하지 않음 — 양쪽 endpoint 모두 카운트하도록 수정

### CI 검증 결과 (2026-09-25)

- 검증 커밋: `fd70814` (브랜치 `foundation/f5-mockdsp-execution`)
- Foundation run: [36086474881](https://github.com/TAE-OK-11/audeniq/actions/runs/36086474881) — **success** (compose-smoke + rust-postgres 전부 통과)
- 참고: 첫 push(`47472cc`)에서는 clippy 수정 후 `cargo fmt`를 다시 안 돌려 let-chain 포맷에서 CI가 실패했음. `fd70814`에서 수정 후 녹색.

## F5.5 — Durable delivery handoff + DDEX ERN 3.8.2 (2026-09-25)

### Durable READY_FOR_DELIVERY handoff (브랜치 `foundation/delivery-handoff`, main 병합 `388b13b`)

- Stage 3의 `READY_FOR_DELIVERY` 전환 + preparation artifact 저장 + `delivery.enqueue` 생성이 **같은 PostgreSQL 트랜잭션**에서 커밋됨. 크래시해도 상태와 큐가 어긋나지 않음.
- job payload: `{"package_id": ...}`, idempotency key: `delivery.enqueue:{package_id}`, pinned revision ID 사용.
- `deploy/grants.sql`에 distribution/finance/execution/rights worker 권한 보완. API 역할은 pipeline schema 쓰기 권한 없음(유지).
- 근본 수정: `dsp_ops_dispatcher_end_to_end` 테스트가 수동 enqueue를 시도해 `Conflict`로 실패 — handoff가 이미 같은 idempotency key로 넣었기 때문. 테스트를 새 설계에 맞게 수정(수동 enqueue 제거, QUEUED 상태의 `delivery.enqueue` 존재를 assert).

### DDEX ERN 3.8.2 빌더 (브랜치 `foundation/ddex-ern-382`, main 병합 `a0d3898`)

- `daddykev/stardust-distro`(MIT, commit `7080368`)의 ERN 3.8.2 구조를 참고해 Rust로 새로 작성: `crates/core/src/ddex_ern.rs` + `crates/core/tests/ddex_ern.rs` (6개 테스트).
- 결정론적 `NewReleaseMessage` 생성: Initial/Update/Takedown, `http://ddex.net/xml/ern/382`, 파일명 `UPC_DD_TTT.ext`, 트랙/ISRC/artwork/P-C line/deal/contributor, WAV/FLAC/MP3·JPEG/PNG 매핑, SHA-256 사용(Stardust의 MD5 미사용), XML escaping·결정론적 정렬.
- 미연결·미검증 명시: 아직 실제 pipeline에 연결되지 않음(Stage 3 preparation은 synthetic ERN 유지), 공식 XSD 검증 없음, contributor role allowed-value 매핑 후속, DDEX 인증 주장 없음. Stardust README의 성능·"production ready" 주장은 독립 검증하지 않음.
- F6 레퍼런스 메모: stardust의 `deliverViaFTP/SFTP/S3/API/Azure` + 재시도 스케줄(5min/15min/1hr)은 F6 실제 DSP 연동 설계 시 참고. F6은 실제 파트너 계약·명세·샌드박스·credentials 없이는 완료로 보지 않음.

### DDEX pipeline wiring (브랜치 `foundation/ddex-wiring`, main 병합 `744bbdc`)

- `prepare_release`가 frozen route plan의 DSP마다 실제 ERN 3.8.2 `NewReleaseMessage`를 생성해 `distribution.ddex_messages`에 저장. `READY_FOR_DELIVERY` 전환 + preparation artifact + `delivery.enqueue`와 **같은 트랜잭션**에서 커밋.
- 내부 synthetic ERN은 preflight integrity envelope으로 유지. `ddex_messages`는 별도 interchange artifact. 송출선(wire transmission)은 F6.
- DPID는 파트너 온보딩 데이터(F6): sender DPID(`identity.orgs.ddex_sender_dpid`)나 recipient DPID(`execution.adapter_profiles.ddex_recipient_dpid`)가 없으면 해당 DSP에 row를 만들지 않음 — 식별자를 발명하지 않음.
- `ddex_messages`: `(package_id, dsp_id)` PK, org RLS + FORCE RLS. migration 0019는 MockDSP profile에만 명백한 테스트 ID(`TESTDPID-MOCKDSP-0001`)를 seed.
- `PrepareSummary.ddex_messages` 카운트 추가(실제 insert 기준, idempotent retry는 기존 row 수 반환).
- 테스트 2개 추가: DPID 설정 시 ERN row 1개(namespace/DPID/UPC/ISRC/sha256 검증), DPID 미설정 시 0개. FORCE RLS 때문에 테스트는 `app.org_id`를 세팅한 커넥션으로 조회.
- Foundation run: [36091055565](https://github.com/TAE-OK-11/audeniq/actions/runs/36091055565) — **success** (compose-smoke + rust-postgres 전부 통과)

### 로컬 검증 결과 (2026-09-25, main `cc8b5df`)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과 (lib 29, ddex_ern 6, distribution 3, execution 13, finance 10, foundation 19, stage1 10, stage2 5, stage3_identifiers 2, stage3_preparation 8, 기타 1 — 총 106)

---

## F3 후속 — rights epoch 자동 bump (2026-09-25)

`rights_epochs`는 E-1 rights-drift guard(preparation + execution)가 읽는 버전 카운터였지만, 수동 UPDATE 외에는 증가시킬 경로가 없었다. 브랜치 `foundation/rights-epoch-auto-bump` (main 병합 `13f237b`).

### 구현 범위

- migration 0020: `rights.grant_atoms` / `rights.review_overrides`에 AFTER INSERT 트리거 추가. 새 rights fact row가 닿는 릴리스의 epoch를 자동 증가.
  - grant `target_kind='RELEASE'` → target_id가 곧 release_id
  - grant `target_kind='TRACK'` → `catalog.tracks`에서 release_id resolve
  - override → `catalog.application_revisions`의 release_id resolve
- 미확인 track을 가리키는 grant, org 밖 revision을 가리키는 override는 쓰기 자체를 실패 (fail-closed, 조용히 skip하지 않음).
- bump는 SECURITY DEFINER 함수 + search_path 고정: epoch 쓰기는 rights 쓰기의 필수 파생 효과이므로, runtime role(`audeniq_worker`는 INSERT만 보유)에 추가 직접 권한 불필요.
- rights fact 테이블은 append-only 유지 (F3 immutable 트리거 그대로, UPDATE/DELETE 거부). INSERT가 유일한 변경점이므로 트리거 커버리지가 완전함.
- 0017 단조 증가 트리거와 호환: `INSERT ... ON CONFLICT DO UPDATE SET epoch = epoch + 1` (첫 rights 쓰기는 1로 seed).

### 로컬 검증 결과 (2026-09-25, main `13f237b`)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과, 총 **114개** (기존 108 + 신규 6), 실패 0
  - 신규 `rights_epoch` 6개: override/grant INSERT 시 0→1→2 증가, RELEASE/TRACK 타깃 resolve, 미확인 track fail-closed, stage2 pin 이후 bump, fact 테이블 append-only 유지
- 환경 메모: 테스트 도중 VM 교체로 PostgreSQL 16이 내려가 `PoolTimedOut` 발생. AGENTS.md 절차대로 재설치·클러스터 시작·`f2test`/`audeniq_f2`/`audeniq_api`/`audeniq_worker` 재생성 후 전체 테스트 재실행 통과. 실패 원인은 코드가 아니라 DB 다운이었음.

### GitHub Actions

- run [36093955569](https://github.com/TAE-OK-11/audeniq/actions/runs/36093955569) (main 병합) — **success**
- run [36093964764](https://github.com/TAE-OK-11/audeniq/actions/runs/36093964764) (문서) — **success**

---

## F3 후속 — grant FK tenant 경계 (2026-09-25)

`rights.grant_atoms.parent_grant_id`와 `.contract_revision_id`가 전역 id만 참조해서, org A의 grant가 org B의 grant/contract revision을 부모로 삼을 수 있었다. 브랜치 `foundation/grant-fk-tenant-boundary` (main 병합 `89cce75`).

### 구현 범위

- migration 0021: 두 FK를 복합 `(org_id, id)` FK로 교체 (`grant_atoms_parent_org_fkey`, `grant_atoms_contract_rev_org_fkey`). FK 타깃용으로 `contract_revisions`에 `UNIQUE(org_id,id)` 추가 (`grant_atoms`는 0009부터 보유).
- 기존 단일 컬럼 FK는 제약 컬럼 기준으로 찾아 drop하는 DO 블록으로 제거 (자동 생성된 제약명 추측 안 함). 기존 row는 새 제약으로 전체 검증 — 이미 있는 cross-org 링크가 있으면 migration이 실패함.
- `review.rs`의 parent chain walk도 `org_id` 스코프로 수정 (defense in depth).

### 로컬 검증 결과 (2026-09-25, main `89cce75`)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과, 총 **118개** (기존 114 + 신규 4), 실패 0
  - 신규 `rights_tenant` 4개: 동일 org parent/contract 허용, 타 org parent/contract는 FK violation으로 거부

### GitHub Actions

- run [36096180495](https://github.com/TAE-OK-11/audeniq/actions/runs/36096180495) (main 병합 `89cce75`) — **success** (2026-09-25 확인)
- run [36096187165](https://github.com/TAE-OK-11/audeniq/actions/runs/36096187165) (문서 `015b5dc`) — **success** (2026-09-25 확인)

---

## Pre-F7 감사 — 미완료 항목 전수 점검·수정 (2026-09-25)

F7 진입 전 F0~F6를 실제 코드·테스트 기준으로 전수 점검하고, 계약 없이 가능한 항목은 전부 수정. 브랜치 `foundation/pre-f7-audit-fixes` (main 병합 `4d2b59d`).

### 구현 범위

- **DDEX retry-count RLS 수정** (`distribution.rs`): `READY_FOR_DELIVERY` idempotent 경로의 `ddex_messages` COUNT가 일반 pool로 실행돼, FORCE RLS 테이블에서 `app.org_id` 없이 0을 반환할 수 있었다. idempotent 조회 row에 `org_id`를 포함시키고 짧은 transaction에서 `set_config('app.org_id',...)` 후 count 조회.
- **Stage 2 checkpoint 의미 정정** (`review.rs`): 기존 주석은 "완료 모듈 재실행 안 함"이었으나 실제로는 네 모듈을 매번 실행 (check_results는 중복 저장 방지용). 동작은 유지하고 문서를 실제에 맞춤 — pre-package retry는 모듈 재실행(읽기 기반, crash 이후 변경 반영), post-package retry는 pinned package 즉시 반환. checkpoint는 skip gate가 아니라 idempotent audit trail. 중복 저장을 `record_check_result`로 분리 + 단위 테스트.
- **`delivery_enabled` 경계 고정** (`foundation.rs`): 활성화 플래그(`execution.adapter_profiles.delivery_enabled`)는 schema owner/platform operator만 변경 가능 — `audeniq_api`·`audeniq_worker`의 UPDATE는 `42501`이어야 한다는 테스트 2개 추가. MockDSP 경로는 stage3 합성 ERN fallback으로 유지, 상용 partner 계약 우회는 별도 명시적 activation 모델 필요 (미구현).
- **worker RLS end-to-end 테스트** (`execution.rs::dsp_worker_role_rls_delivery`): non-owner `audeniq_worker` 역할로 E-0 enqueue → claim → send 전체를 수행하고, 타 org `delivery_jobs`가 보이지 않음을 검증. 부수 발견: FORCE RLS는 owner에게도 WITH CHECK를 적용 — org context 없이 직접 INSERT하면 42501.
- **partner별 DDEX artifact routing** (`execution.rs::materialize`): 전송 문서는 `(package_id, dsp_id)`의 `ddex_messages` row를 우선 사용 (sha256 검증). DDEX row가 없고 transport가 mock이 아니면 `EXECUTION_DDEX_MESSAGE_MISSING`으로 fail-closed — 합성 preparation envelope이 실제 partner wire에 나가는 일을 원천 차단. 테스트 2개 (routing 선호 / fail-closed), MockDsp는 받은 문서의 ern_sha256을 기록.
- **contributor role 매핑** (`ddex_ern.rs`): credit role을 DDEX `ContributorRole` allowed-value subset으로 매핑, 미매핑은 generic `Contributor` fallback. 기존 `<Role>COMPOSER</Role>` 테스트를 `<Role>Composer</Role>`로 갱신.
- **ERN well-formed guard** (`ddex_ern.rs` 테스트): quick-xml(dev-dep)로 3개 fixture 전체 파싱, 루트 `ern:NewReleaseMessage` + 태그 균형 확인.
- **공식 XSD 미확보 명시**: ERN 3.8.2 공식 schema는 신뢰 가능한 경로에서 확보 불가 확인 — `service.ddex.net/xml/ern/382/*`는 404 HTML, ddex-workbench 저장소·PyPI wheel에도 XSD 없음 (`/tmp/ddex-xsd`의 파일들은 404 HTML로 확인). 공식 XSD validation은 F6 전제 조건으로 미완료 유지.

### 로컬 검증 결과 (2026-09-25, main `4d2b59d`)

- `cargo fmt --all --check`: 통과
- `cargo clippy --workspace --all-targets -- -D warnings`: 통과
- `cargo test --workspace`: 전부 통과, 총 **125개** (기존 118 + 신규 7), 실패 0
  - 신규: `checkpoint_dedupes_identical_results`, `dsp_worker_role_rls_delivery`, `dsp_routing_prefers_partner_ddex_message`, `dsp_routing_fail_closed_without_ddex_message`, `contributor_role_maps_studio_roles_to_avs`, `contributor_role_unknown_falls_back_to_generic`, `ddex_ern_output_is_well_formed_xml`

### GitHub Actions

- run [36097605861](https://github.com/TAE-OK-11/audeniq/actions/runs/36097605861) (main 병합 `4d2b59d`) — **success** (2026-09-25 확인).
