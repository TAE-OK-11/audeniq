# F4 Stage 3 — Astra 협업 핸드오프

> 이 문서를 Astra에게 그대로 전달하면 F4 작업을 바로 시작할 수 있다.
> 작성일: 2026-09-25 / 작성자: Muse

## 1. 현재 상태

- 저장소: `TAE-OK-11/audeniq` (로컬: `/home/hatch/audeniq-f2`)
- 브랜치: `foundation/f3-stage2-review` (F3 완료, CI success — run 36068842959)
- F0(설계)·F1(Foundation)·F2(Pre-submit+Stage1)·F3(Stage2 권리 심사) 완료.
- F4 브랜치는 아직 없음. `foundation/f4-stage3-distribution` 로 새로 파서 작업할 것.

## 2. F4 범위 (BLUEPRINT §5, F4 행)

Stage 3의 3-A…3-I는 **논리 단계**이며, `prepare_release` 1개 durable job 안의
체크포인트로 구현한다. 별도 큐/바이너리를 만들지 않는다 (`distribution` 큐 사용 중).

구현 대상:
1. **Canonical Release** — 발매 확정용 정규 릴리스 스냅샷 (메타데이터·트랙·권리 핀 고정)
2. **식별자 관리** — ISRC/UPC 검증·할당 기록. **발급 OFF**: 실제 발급 기관 연동 없이 기존 식별자 검증 + 내부 할당대장만
3. **Route plan** — DSP별 라우팅 계획 (어떤 DSP에 무엇을 보낼지, Verification Package의 승인 DSP 집합 기반)
4. **DDEX/독자 포맷 생성** — ERN 메시지 생성. **partner-neutral**: 상대 비공개 명세 없이 synthetic fixture로 개발
5. **패키지 고정** — 제출 패키지 불변 고정 + sha256 hash 기록

인수 기준: partner-neutral fixture, XML/메타/파일/권리 preflight 성공.

## 3. 작업 분할 (병렬 가능)

| 담당 | 범위 | 산출물 |
|------|------|--------|
| Muse (나) | 1 Canonical Release + 5 패키지 고정 + `prepare_release` 워커 배선 | migration **0010**, `crates/core/src/distribution.rs`, `operations.rs` 연결, 통합 테스트 |
| Astra | 2 식별자 관리 + 3 route plan + 4 DDEX ERN 생성 | `crates/core/src/ern.rs` (순수 함수), 식별자 대장 테이블, fixture 기반 단위 테스트, 마이그레이션 **0011부터** |

경계: Astra의 ERN 생성기는 **순수 함수**로 만든다 — 입력(Canonical Release JSON) → 출력(ERN XML String).
DB 접근 없이, Muse가 워커에서 호출하는 형태. 이렇게 하면 서로 블로킹 없이 병렬 개발 가능.

## 4. Astra가 먼저 읽을 파일

- `docs/BLUEPRINT.md` §5 (Stage 3), §22는 참고만
- `docs/F3_PLAN.md` — Stage 2 출력물(Verification Package) 형식
- `crates/core/src/review.rs` — `VerificationPackage`, `DspScope` 구조체 (F4 입력)
- `crates/core/src/operations.rs` — `prepare_release` park 위치 (현재 230행 근처)
- `migrations/0009_stage2_review.sql` — 마이그레이션 네이밍/컨벤션

## 5. 공통 컨벤션 (반드시)

- 마이그레이션: `migrations/0010_*.sql` 부터 순번. DDL은 F0 BLUEPRINT 컨벤션 준수
  (RLS, `updated_at` 트리거, 불변 테이블은 변경 거부 트리거).
- Rust: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` 무경고.
- 테스트: `DATABASE_URL=postgres://f2test:f2test-local-dev-only@localhost/audeniq_f2 cargo test --workspace` 전부 통과.
- 외부 서비스(중국 기업 관련 포함) 신규 의존 금지. ERN은 synthetic fixture만 사용.
- 상태 보고는 실제 실행 결과로만. 추측 금지.

## 6. Astra 산출물 체크리스트

- [ ] `crates/core/src/ern.rs`: `generate_ern(canonical: &CanonicalRelease) -> Result<String>` + 단위 테스트 (fixture 3종 이상)
- [ ] 식별자 대장 migration + ISRC/UPC 형식 검증 함수 + 테스트
- [ ] route plan: 승인 DSP 집합 → DSP별 제출 항목 매핑 함수 + 테스트
- [ ] preflight: XML 스키마 정합·메타데이터 필수값·파일 존재·권리 일치 4종 체크 + 테스트
- [ ] `cargo fmt` / `clippy -D warnings` / `cargo test` 통과 후 PR을 `foundation/f4-stage3-distribution` 로
