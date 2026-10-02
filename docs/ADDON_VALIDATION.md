# Add-on validation

Base: origin/main b16a52b (2026-10-02 fetch). Branch: feat/addon-services-phase1.

* 별도 PostgreSQL 18 테스트 DB에서 전체 전방 migration 적용 확인.
* cargo check --locked -j 1 -p audeniq-core --test addons 통과.
* 로컬 RAM 약 1GiB에서 test code generation과 전체 lib-test clippy가 OOM/SIGKILL로 종료됐다. 통과로 간주하지 않는다.
* cargo fmt/clippy, PostgreSQL 통합 테스트 및 기존 배급 회귀 suite의 최종 결과는 검증 종료 후 갱신한다.
