# F6 실전송 연결·점검 기록 (2026-09-28)

범위: DSP 실제 전송 경로 구현, 자동검사 보강, 백엔드 점검·최적화. 외부 계약이 필요한 부분은
"설정 파일 + 운영 명령"만 남기고 코드는 전부 구현했다. 운영 방법은 [PARTNER_DELIVERY.md](PARTNER_DELIVERY.md).

검증: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test -p audeniq-core`
전체 통과 (PostgreSQL 18, ffmpeg, xmllint, 실제 sshd로 SFTP 왕복 포함). Studio `bun run test` 61개 통과,
Admin/Studio `tsc --noEmit` 통과.

## 1. 발견해서 고친 버그 (운영 영향 큰 순)

| # | 문제 | 영향 | 수정 |
|---|---|---|---|
| 1 | CONTRACTED DSP는 어떤 경우에도 전송 경로가 열리지 않음: 조직별 `route_plans.enabled`/`dsp_endpoints.integration_status`가 CHECK로 비활성 고정, 게다가 DSP 계약을 아티스트 조직마다 요구 | 계약해도 실제 전송 불가 | 유통사 단위 계약 경로(0054 `execution.platform_contract_live`): 온보딩 완료 + LIVE 단계면 모든 조직에 경로 개방. 라우팅·2단계 적격성 둘 다 반영 |
| 2 | DDEX 송신자 DPID를 아티스트 조직마다 요구 | 유통사 DPID로는 실제 ERN이 생성되지 않음 | `DDEX_SENDER_DPID`(유통사) 기본값, 레이블 자체 DPID가 있으면 우선 |
| 3 | 국내 DSP(파트너 피드)도 DDEX 메시지를 요구 | 멜론·지니·FLO·벅스 전송이 항상 `EXECUTION_DDEX_MESSAGE_MISSING` | 파트너 피드 DSP는 ERN 없이 피드로 전송. 피드 설정이 있으면 스테이징의 "스펙 미정" 차단도 해제 |
| 4 | LIVE/테이크다운 웹훅 하나가 같은 파트너의 **모든** INGESTING 릴리스를 LIVE로 바꿈 | 다른 아티스트 릴리스 상태 오염 | 이벤트를 제출 ID/파트너 릴리스 ID로 한 건에만 연결, `execution.ack_events`로 중복 제거 |
| 5 | LIVE 이벤트가 attempt의 ACCEPTED 이벤트 ID를 덮어써 재전송된 ACCEPTED가 두 번 적용됨 | 상태 중복 적용 | 위 ack_events 테이블 |
| 6 | 상태 조회 API가 없는 파트너(ACK 파일만 주는 DDEX 파트너)는 폴링 자체가 Gated로 실패 | 전송 후 상태 추적 불가 | 제출 조회(inquire)로 폴링, 폴링 응답을 수신 증거로 기록(가짜 MISSING_ACK 방지), 폴링으로 발견한 반려는 대조 케이스 생성 |
| 7 | 어댑터 검증 실패 시 전송 작업이 LEASED로 방치 → 재시도 반복 후 DEAD_LETTER | 원인 안 보이는 막힘 | 즉시 FAILED + PARTNER_REJECTED 케이스 |
| 8 | 운영 이미지에 `xmllint` 없음 | 운영에서 DDEX XSD 검증 불가(`ERN_XSD_VALIDATOR_UNAVAILABLE`) | Dockerfile에 libxml2-utils, openssh-client 추가 |
| 9 | 업데이트/테이크다운이 빈 문서로 호출됨 | 실제 파트너에 보낼 내용 없음 | 원 메시지 스레드의 DDEX UpdateMessage / 딜 종료일 테이크다운 생성 |

## 2. 새로 구현한 것

- 파트너 설정(`partner_config.rs`), 전송 계층 SFTP/S3/로컬(`transport.rs`)
- DDEX ERN Choreography 어댑터, 국내 파트너 피드(JSON+CSV, 19금 표시), REST API 어댑터(`partners/`)
- 전송 전 제출 ID 기록 + 완료 마커 마지막 → 중복 전송 없는 재시도 규칙
- 서명 웹훅 수신 `POST /api/partner-hooks/{id}` (edge 통과 허용, HMAC+재전송 창)
- 운영 명령 `audeniq-admin partner config-check|status|probe|dpid|test-ern|test-ack|capabilities|contract|go-live|suspend`
- 스태프 LIVE 증거 기록 `POST /api/staff/deliveries/{package}/{code}/live`
- 예시 설정 `deploy/partners/examples/` (SFTP DDEX, S3 DDEX, 국내 피드, OAuth2 API)

## 3. 자동검사 보강 (1차 검사)

| 코드 | 판정 | 근거 |
|---|---|---|
| `TRACK_TITLE_EXPLICIT_MARKER` / `RELEASE_TITLE_STYLE`(표기 포함 시) | 보완 요청 | Spotify/Apple: Explicit은 플래그로만, 제목에 (Explicit)/(Clean)/(19금) 금지 |
| `TRACK_TITLE_STYLE` / `RELEASE_TITLE_STYLE` | 검토(경고) | 전부 대문자, 이모지, 홍보 문구(Out Now 등), 제목 속 feat. |
| `FIELD_LANGUAGE_INVALID` | 보완 요청 | ISO 639 / BCP 47 형식 아님 (준비 단계에서야 실패하던 것) |
| `AUDIO_CONTENT_SUSPECT` + "silence padding" | 검토 | 시작 5초·끝 15초 이상 무음 (Spotify silence padding 반려). QC 규칙 버전 3→4 |

Studio/Admin 보완 안내 문구(`corrections.ts`) 추가.

## 4. 최적화

- 어댑터 레지스트리: 전송/폴링/테이크다운 작업마다 새로 만들던 것을 프로세스 캐시(5분 TTL)로
- 대조(reconcile): 15분마다 **전체 조직**을 돌던 것을 진행 중 전송이 있는 조직만(0055), 전달 이력 재스캔 30일로 제한
- 인덱스: partner_message_id, (partner_id, partner_release_id), 열린 live_bindings / delivery_jobs 부분 인덱스
- REST 파일 업로드 스트리밍(마스터를 메모리에 올리지 않음), materialize에서 장르/레이블 한 번에 조회

## 5. 남은 외부 조건 (코드 아님)

DSP별 계약·계정·호스트·키·DPID, 국내 DSP 피드 컬럼 확정(설정으로 매핑), Apple이 DDEX를 받지 않으면
Transporter 어댑터, 공식 DDEX 인증. 계약서를 받으면 [PARTNER_DELIVERY.md](PARTNER_DELIVERY.md) 2절 순서대로.
