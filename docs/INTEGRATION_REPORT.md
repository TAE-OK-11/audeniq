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
