# F4 Stage 3 — Codex(Astra)용 프롬프트

> 아래 블록을 그대로 복사해서 Codex에 붙여넣으세요.

```
# F4 Stage 3 — Astra 담당분

## 컨텍스트
- 저장소: TAE-OK-11/audeniq (Rust 백엔드, PostgreSQL + SQLx, Axum)
- 브랜치 `foundation/f3-stage2-review`에서 분기. 작업 브랜치: `foundation/f4-stage3-distribution`
  (이미 존재하면 pull 후 이어서 작업)
- F3까지 완료: Stage 2 권리 심사가 끝나면 Verification Package가 발행되고,
  `prepare_release` job이 `distribution` 큐에 들어감
- 스펙: `docs/BLUEPRINT.md` §5 (Stage 3). F4 인수 기준은
  "partner-neutral fixture, XML/메타/파일/권리 preflight 성공"

## 네 담당 (아래는 Muse가 하므로 건드리지 마)
1. `crates/core/src/ern.rs` (신규): `generate_ern(canonical: &CanonicalRelease) -> Result<String>`
   - 순수 함수. DB 접근 금지. 입력→ERN XML 문자열 출력
   - partner-neutral: 실제 DSP 비공개 명세 없이 synthetic fixture로 개발
2. 식별자 관리: ISRC/UPC 형식 검증 + 내부 할당대장 테이블. 실제 발급기관 연동 없음(발급 OFF)
3. Route plan: Verification Package의 승인 DSP 집합 → DSP별 제출 항목 매핑 함수
4. Preflight: XML 정합 / 메타데이터 필수값 / 파일 존재 / 권리 일치 4종 체크

Muse 담당(건드리지 말 것): Canonical Release 스냅샷, 패키지 고정(불변+hash),
`prepare_release` 워커 배선, migration 0010.

## 인터페이스 계약
- `CanonicalRelease` 구조체는 Muse가 `crates/core/src/distribution.rs`에 정의 중.
  네 작업 시작 시점에 없으면 `crates/core/src/review.rs`의 `VerificationPackage`와
  `DspScope`를 보고 네 쪽에 임시 구조체로 시작하고 나중에 합친다.
- 네 마이그레이션은 `migrations/0011_*.sql`부터 사용 (0010은 Muse 것).
- `docs/F4_HANDOFF_ASTRA.md`에 전체 분할표와 체크리스트가 있다.

## 먼저 읽을 것
- `docs/BLUEPRINT.md` §5
- `docs/F3_PLAN.md`
- `crates/core/src/review.rs` (VerificationPackage, DspScope 구조체)
- `crates/core/src/operations.rs` (`prepare_release` park 부분)
- `migrations/0009_stage2_review.sql` (마이그레이션 컨벤션: RLS, 불변 트리거 등)

## 규칙
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` 무경고
- `DATABASE_URL=postgres://f2test:f2test-local-dev-only@localhost/audeniq_f2 cargo test --workspace` 전부 통과
- 중국 기업 관련 서비스 의존 금지. ERN은 synthetic fixture만 사용
- 상태 보고는 실제 실행 결과로만. 추측 금지

## 완료 기준
- [ ] partner-neutral fixture 3종 이상으로 ERN 생성 단위 테스트 통과
- [ ] XML/메타/파일/권리 preflight 테스트 통과
- [ ] 식별자 검증 테스트 통과
- [ ] 위 fmt/clippy/test 전부 통과 후 `foundation/f4-stage3-distribution`에 푸시
```
