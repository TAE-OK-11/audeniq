# Studio 연동 및 GitHub 리뷰 반영 보고서

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
