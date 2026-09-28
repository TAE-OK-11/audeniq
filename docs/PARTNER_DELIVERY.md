# 실제 DSP 전송 (F6 연동) — 계약 후 바로 켜는 방법

2026-09-28 추가 (migrations 0053–0055). 목표: DSP와 계약하면 **설정 파일 한 개 + 운영 명령 몇 줄**로
실제 전송이 시작되고, 코드는 수정하지 않아도 되는 상태. 계약서마다 다른 부분(주소, 계정, 폴더 이름,
상태 값 이름, 국내 피드 컬럼명)은 전부 설정으로 뺐다.

## 0. 공식 오픈 전 잠금 (현재 상태: 잠김)

실DSP 코드는 전부 구현돼 있지만 **공식 오픈 전까지는 실제로 동작하지 않는다.** 스위치는 하나,
`DSP_LIVE_TRANSMISSION=enabled` (worker·api 환경변수). 비어 있으면(기본) 잠김이며 두 겹으로 막는다:

1. **라우팅 잠금** — 온보딩·계약·go-live까지 끝난 DSP라도 라우팅 결과가 `PRE_LAUNCH_LOCKED`라서 전송 작업 자체가 만들어지지 않는다.
2. **전송선 잠금** — SFTP·S3·HTTP 클라이언트가 외부 호스트로는 DNS 조회·접속 전에 거부한다(`LIVE_TRANSMISSION_LOCKED`).
   운영자의 `partner probe`도 막힌다. 루프백(127.0.0.1 등 테스트 서버)만 허용.

잠긴 동안에도 되는 것: 설정 파일 점검(`config-check`), DPID/테스트 ERN·ACK/계약 기록, 스테이징(ERN 생성·검증),
로컬 MockDSP 샌드박스, 파트너 웹훅 **수신**(외부로 나가는 요청이 아님). `/ready`에 `"live_transmission": false`로 표시된다.
공식 오픈 때 `deploy/production.env`에 `DSP_LIVE_TRANSMISSION=enabled`를 넣고 worker·api를 재시작한다.

## 0-1. DSP별 전송 프로파일 (`crates/core/src/dsp_registry.rs`)

내부에서는 `D-1`…`D-11` 코드를 키로 쓰지만, 아티스트·담당자 화면과 검사 문구에는 항상 플랫폼 이름(`name_ko`)만 나간다.
전송 방식은 업계 관행 기준이며 계약서의 기술 부속서가 우선한다(설정 파일로 맞춘다).

| 플랫폼 | 형식 | 전송 | 방식 | 상업 모델(ERN Deal) | Merlin | 추가 정책 |
|---|---|---|---|---|---|---|
| 멜론·지니·벅스 | 전용 피드(JSON+CSV) | 계약 시 지정 | 피드 완료 마커 | 구독 | 불가 | 원작자 커버 동의서, 19금 표시, 작사·작곡 필수 |
| FLO | 전용 피드(JSON+CSV) | 계약 시 지정 | 피드 완료 마커 | 구독 | 가능 (Merlin의 국내 유일 파트너, 2022~) | 위와 같음 |
| Spotify | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | AI 정책, 24bit/44.1k로 서비스 |
| Apple Music | DDEX ERN 3.8.2* | Transporter* | release-by-release | 구독(스트리밍) | 가능 | AI 정책, 발매유형 불일치=차단, 작곡가 필수 |
| YouTube Music | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | Content ID(커버·샘플·리믹스 주의), 손실 압축 서비스 |
| Amazon Music | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | — |
| TIDAL | DDEX ERN 3.8.2 | SFTP | batch | 구독 | 가능 | 커버곡 이용허락, AI 정책 |
| Deezer | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | 작사·작곡 필수, 커버 4096px 이하, 16bit/44.1k 서비스, 14일 전 |
| Qobuz | DDEX ERN 3.8.2 | SFTP | release-by-release | 구독(주문형) | 불가 → 직계약 | — |
| Pandora (SiriusXM), SoundCloud, Audiomack, iHeartRadio | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | SoundCloud는 지문 인식(Content ID식) |
| Napster, KKBOX, LINE MUSIC | DDEX ERN 3.8.2 | SFTP | batch | 구독 | 가능 | — |
| Anghami, Boomplay, JioSaavn, AWA | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | 지역 서비스 |
| NetEase Cloud Music, Tencent Music (QQ뮤직·쿠거우·쿠워) | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | 중국 콘텐츠 심의, 발매 21일 전 |
| Meta (Facebook·Instagram), TikTok·CapCut (ByteDance), Snapchat | DDEX ERN 3.8.2 | SFTP | batch | 무료(광고) + 업로드 영상 클레임 | 가능 | 음원 라이브러리 제공 + 사용자 영상 지문 클레임, 커버·샘플 주의 |
| YouTube Content ID·Shorts | DDEX ERN 3.8.2 | SFTP | batch | 업로드 영상 클레임 | 가능 | YouTube Music(스트리밍)과 별도 선택 |
| Beatport | DDEX ERN 3.8.2 | SFTP | batch | 구독(Beatport Streaming) | 불가 → 직계약 | 일렉트로닉 장르만(그 외는 차단) |
| iTunes Store | DDEX ERN 3.8.2* | Transporter* | release-by-release | 다운로드 판매(가격대 Normal) | 가능 | Apple Music과 별도 선택, 작곡가 필수 |
| Claro Música (중남미) | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 불가 → 직계약 | — |
| Pretzel (스트리머용 음원) | DDEX ERN 3.8.2 | SFTP | batch | 구독 | 불가 → 직계약 | 방송(트위치 등)용 라이선스 |
| Triller | DDEX ERN 3.8.2 | SFTP | batch | 무료(광고) + 업로드 영상 클레임 | 불가 → 직계약 | 로열티 미지급 분쟁 이력 — 선지급·보증 확인 |
| TouchTunes (디지털 주크박스) | DDEX ERN 3.8.2 | SFTP | batch | 재생당 과금 | 불가 → 직계약 | 매장 공연(주크박스) |
| Yandex Music (러시아·CIS) | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | 제재·해외송금 제한 — 법무·정산 검토 후 |
| Kuaishou·Kwai·SnackVideo | DDEX ERN 3.8.2 | SFTP | batch | 무료(광고) + 업로드 영상 클레임 | 가능 | 중국 콘텐츠 심의, 21일 전, 지문 클레임 |
| JOOX (동남아·홍콩) | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 무료(광고) | 가능 | — |
| TREBEL | DDEX ERN 3.8.2 | SFTP | batch | 무료(광고) — 스트리밍 + 조건부 다운로드 | 가능 | — |
| Mixcloud, Twitch (DJ) | DDEX ERN 3.8.2 | SFTP | batch | 무료(광고) + 구독 — 사용자 믹스·웹캐스트 | 가능 | 지문 인식 |
| Peloton, Adaptr | DDEX ERN 3.8.2 | SFTP | batch | 구독 | 가능 | 피트니스·앱용 라이선스 (Peloton은 작곡가 필수) |
| Canva, Lickd, STYNGR | DDEX ERN 3.8.2 | SFTP | batch | 구독 + 건별 — 고객 콘텐츠·게임에 사용 | 가능 | Canva·Lickd는 지문 인식, Lickd 작곡가 필수 |

요청 목록 대조(2026-09-28): Instagram·Facebook은 Meta 한 피드, QQ Music·Kugou·Kuwo·WeSing은 Tencent Music 한 피드로 들어간다.
**Resso**(TikTok Music으로 바뀐 뒤 2024-11 종료)와 **Soundtrack by Twitch**(2022 종료)는 서비스가 없어 추가하지 않았다.

\* Apple은 보통 iTunes Package를 Transporter로 받는다. DDEX로 받는다는 계약이면 그대로, 아니면 전용 어댑터가 필요하다.
다운로드 판매 가격대는 기본 `Normal`이며 실제 도매가 등급은 계약서 값으로 바꾼다.

배급 준비(스테이징) 단계에서 DSP마다 위 조건을 검사한다: 커버 크기·정사각형, 무손실·샘플레이트·비트, 필수 크레딧, 장르,
발매일 여유, 음량 기준, 19금 표시, 가상 코드, ERN 버전, 그리고 신고 항목(커버·샘플·리믹스·AI)에 따른 플랫폼별 정책
(`DSP_CONTENT_ID_RISK`, `DSP_COVER_LICENSE_REQUIRED`, `DSP_KR_COVER_CONSENT`, `DSP_AI_POLICY`)과
서비스 음질 안내(`DSP_AUDIO_SERVED_DOWNSAMPLED`). 정책 항목은 경고(담당자 확인)이며 단독으로 전송을 막지 않는다.

**계약 경로 기본값 (0057·0059)**: Merlin이 라이선스하는 DSP는 전부 `MERLIN`으로 시작한다(Merlin 가입 계약 하나로 커버).
Merlin 파트너(2026-09 확인): Spotify, Apple Music·iTunes, YouTube(Music·Content ID), Amazon, TIDAL, Deezer, Pandora, SoundCloud,
Audiomack, Anghami, Boomplay, JioSaavn, KKBOX, LINE MUSIC, AWA, NetEase, Tencent, Napster, iHeartRadio, Meta, TikTok, Snap,
**FLO**, Yandex, Kuaishou, JOOX, TREBEL, Mixcloud, Twitch, Peloton, Canva, Lickd, Adaptr, STYNGR.
Merlin 딜이 없는 곳 — 멜론·지니·벅스, Qobuz, Beatport, Claro Música, Pretzel, Triller, TouchTunes — 은 `DIRECT`(직계약 대상).
넣지 않은 Merlin 관련: Triller(Merlin 계약이 미지급으로 끝남 → 직계약 대상 유지), Supernatural(새 회사로 재출시 중, 딜 재확인 필요),
AI 학습 라이선스(ElevenLabs·Udio — 권리자별 동의(옵트인) 방식이라 발매 전송 대상이 아님), Nina(Merlin Connect API 라이선스). Merlin의 파트너 목록은 바뀌므로
계약 시점에 Merlin이 준 목록과 대조해 `partner merlin-eligible`로 맞춘다. Spotify·Apple·YouTube·Amazon처럼 규모가 커지면
직계약이 유리한 곳은 `partner route D-n DIRECT`로 언제든 바꿀 수 있다(관리자 화면의 `planned_route`가 현재 계획).
NAVER VIBE는 2026-12-31 종료 발표로 넣지 않았다.

## 0-2. 직계약 / Merlin 선택 (migration 0056)

DSP마다 계약 경로를 고른다. 전송 자체는 같은 DSP 수신 서버로 가고, **계약 증빙만** 달라진다.

- `DIRECT` (기본): 그 DSP와의 직접 계약 (`partner contract D-5 계약번호`)
- `MERLIN`: Merlin 가입 계약이 DSP 계약을 대신 (`partner contract merlin 가입계약번호`). Merlin 딜이 있는 DSP만 선택 가능
  (위 Merlin 파트너 목록). DPID·엔드포인트·테스트 ERN/ACK 등 기술 온보딩은 동일하게 필요.

변경: `audeniq-admin --operator 이름 partner route D-5 MERLIN` 또는 스태프 ADMIN `POST /api/staff/dsps/{code}/route`
`{"route":"MERLIN","note":"사유"}`. Merlin 딜 목록이 바뀌면 `partner merlin-eligible D-11 true`.
어느 경로든 공식 오픈 전에는 잠금(0절) 때문에 실제로 라우팅되지 않는다.

## 1. 무엇이 구현됐나

| 구성 | 파일 | 내용 |
|---|---|---|
| 파트너 설정 | `crates/core/src/partner_config.rs` | `PARTNER_CONFIG_DIR/<partner_id>.json`. 비밀값은 `{"file": …}` / `{"env": …}` 참조만 허용 (본문에 키를 쓰면 로드 거부). DB에는 비밀이 들어가지 않는다 |
| 전송 계층 | `crates/core/src/transport.rs` | **SFTP** (OpenSSH `sftp -b -`, known_hosts 고정, 키 인증만, `.part` 업로드 후 rename), **S3 호환 버킷** (SigV4), 로컬 폴더(리허설·테스트 전용) |
| DDEX 어댑터 | `crates/core/src/partners/drop.rs` | ERN 3.8.2 + 음원/커버를 DDEX ERN Choreography 규칙대로 드롭: batch(`<batch>/<UPC>/…` + `BatchComplete_<batch>.xml`) 또는 release-by-release. **완료 마커는 항상 마지막**. ACK 파일 폴링 |
| 국내 피드 어댑터 | 같은 파일 (`adapter: "partner_spec"`) | 멜론·지니·FLO·벅스용: 음원·커버 + `manifest.json` + `metadata.csv`(UTF-8 BOM, 엑셀 호환) + 완료 마커. 19금 표시(`adult_only`), 작사·작곡·편곡, ISRC/UPC 포함. 컬럼명은 `field_map`으로 계약서에 맞춤 |
| REST API 어댑터 | `crates/core/src/partners/api.rs` | 생성 → 파일 업로드(스트리밍) → commit → 상태 조회. 인증: Bearer / API Key / OAuth2 client credentials / HMAC 서명. 경로·상태값 이름 전부 설정 |
| ACK 해석 | `crates/core/src/partners/ack.rs` | DDEX `FtpAcknowledgementMessage`류 XML, 파트너 결과 JSON. 모르는 상태값은 성공으로 보지 않고 실패 코드로 기록 |
| 웹훅 수신 | `crates/core/src/partner_hooks.rs` | `POST /api/partner-hooks/{partner_id}` — HMAC-SHA256 서명 + 타임스탬프(재전송 방지) 검증 → `execution.partner_inbox` → 워커 `delivery.ack` |
| 운영 명령 | `crates/core/src/partner_admin.rs`, `audeniq-admin partner …` | 설정 점검, 접속 확인, DPID, 테스트 ERN/ACK, 기능 플래그, 계약, go-live, 중단 |
| 유통사 계약 경로 | migration 0054 | 온보딩이 전부 끝나고 LIVE로 올린 파트너는 **모든 아티스트 조직에** 전송 경로가 열린다 |

### 중복 전송 방지 규칙 (가장 중요)

- 파트너 쪽 제출 ID(DDEX 폴더명 `<batch>/<UPC>` 등)를 **전송 시작 전에** attempt 행에 먼저 기록한다.
- 완료 마커(또는 API commit) **이전** 실패 → 파트너가 처리하지 않는 상태가 확실하므로 새 폴더로 자동 재시도.
- 완료 마커 **쓰는 도중** 끊김 → `SENT_UNKNOWN` (자동 재전송 금지). 대조(reconciliation)에서 기록해 둔 그 폴더의 ACK를 확인한다.
- 접속 자체 실패(DNS·거부·인증·호스트키) → 아무것도 안 올라갔으므로 재시도.

## 2. 계약 후 켜는 순서

아래 `D-5`는 예시. 코드는 `docs/DISTRIBUTION_STAGING.md` 표 참조.

1. **설정 파일 작성**: `deploy/partners/examples/`의 예시를 복사해 서버 `deploy/partners/D-5.json`에 두고
   계약서의 값으로 채운다. 비밀 파일은 `deploy/partners/secrets/`(권한 600). 둘 다 git에 올라가지 않는다(.gitignore).
   - SSH 호스트키: `ssh-keyscan -p 포트 호스트 > secrets/D-5.known_hosts` 후 **파트너가 알려준 지문과 대조**.
2. `audeniq-admin partner config-check` — 파일 문법, 비밀 참조가 전부 읽히는지.
3. `audeniq-admin --operator 이름 partner probe D-5` — 실제 접속·인증 (공식 오픈 전에는 잠금 때문에 `LIVE_TRANSMISSION_LOCKED`; 오픈 직후 실행). 성공하면 엔드포인트(자격증명 없는 주소)와 자격증명 종류가 온보딩에 기록된다.
4. `audeniq-admin --operator 이름 partner dpid D-5 PADPIDA…` — 계약서의 수신자 DDEX Party ID.
5. 유통사 송신 DPID: `deploy/production.env`의 `DDEX_SENDER_DPID` / `DDEX_SENDER_NAME` (DDEX에서 발급받은 AUDENIQ의
   Party ID). 모든 아티스트 조직의 메시지 송신자로 쓰인다. 자체 DPID로 보내는 레이블만 `identity.orgs.ddex_sender_dpid`를 따로 둔다.
   국내 피드(partner_spec/http_api) 설정이 있으면 스테이징의 `DSP_PARTNER_SPEC_PENDING` 차단도 자동으로 풀린다.
6. 파트너 테스트 환경에 시험 전송 → `partner test-ern D-5 파일.xml`, 파트너가 돌려준 ACK로 `partner test-ack D-5 ack.xml`.
7. `partner capabilities D-5 '{"send_or_publish":true,"inquire_submission":true,"parse_ack":true,"takedown":true}'` — 파트너 문서가 지원하는 것만.
8. `partner contract D-5 계약서번호` → `partner go-live D-5`. 빠진 항목이 있으면 거부되고 무엇이 빠졌는지 `partner status D-5`에 나온다.
9. `docker compose … up -d worker api` (설정은 5분 안에 자동 재로딩: `PARTNER_REGISTRY_TTL_SECS`).

문제가 생기면 즉시 `partner suspend D-5 --reason "사유"` — 새 전송이 멈추고(진행 중 전송은 마무리) 경로가 닫힌다.

## 3. 설정 필드 요약

```jsonc
{
  "partner_id": "D-5",               // 파일명과 같아야 함
  "adapter": "ddex" | "partner_spec" | "http_api",
  "transport": {"kind": "sftp" | "s3" | "local", ...},
  "ddex": {
    "choreography": "batch" | "release_by_release",
    "inbox_dir": "incoming",          // 우리가 올리는 폴더
    "ack_dir": "acknowledgements",    // 파트너가 ACK를 두는 폴더 (없으면 제출 폴더 안에서 찾음)
    "live_policy": "explicit" | "on_ack"  // on_ack: 성공 ACK를 서비스 개시로 간주
  },
  "partner_spec": {"formats": ["json","csv"], "field_map": {"isrc": "ISRC코드"}, "result_dir": "results",
                   "complete_marker": "{id}.complete"},
  "http_api": {"base_url": "https://…", "auth": {...}, "create_path": "/v1/releases", "status_map": {...}},
  "status_api": {...},                // 파일 드롭 파트너가 별도 상태 조회 API를 줄 때
  "webhook": {"secret": {"file": "…"}, "signature_header": "X-Signature", "timestamp_header": "X-Timestamp"}
}
```

웹훅 서명: `hex(HMAC_SHA256(secret, timestamp + "." + body))` (timestamp_header를 null로 두면 body만).
파트너 방식이 다르면 헤더 이름만 바꾸면 되고, 알고리즘이 다르면 `partners/http.rs::verify_webhook`만 수정한다.

## 4. 운영 중 확인

- 전송 상태: 스태프 화면의 배포(deliveries) 목록, 릴리스 상세의 `delivery_status_by_dsp` / `live_status_by_dsp`.
- 서비스 개시를 파트너가 알려주지 않는 경우: `POST /api/staff/deliveries/{package}/{code}/live`
  `{"partner_release_id": "파트너 쪽 ID/URL", "note": "카탈로그에서 확인"}` — DELIVERED 건만 LIVE로 기록.
- 웹훅 처리 결과: `execution.partner_inbox.result` (APPLIED / DUPLICATE_IGNORED / UNMATCHED / UNPARSEABLE…).
- 대조 케이스: `execution.reconciliation_cases` (SENT_UNKNOWN, MISSING_ACK, OVERDUE_LIVE, PARTNER_REJECTED).

## 5. 계약에 따라 손봐야 할 수 있는 곳 (코드 위치)

| 상황 | 할 일 |
|---|---|
| 폴더 구조·ACK 파일 이름이 다름 | 대부분 `inbox_dir`/`ack_dir`/`choreography`로 해결. 이름 규칙 자체가 다르면 `partners/drop.rs::ack_candidates` |
| ACK 상태값 이름이 다름 | `partners/ack.rs::classify_status`에 값 추가 |
| 국내 DSP 피드 형식 | `field_map`으로 컬럼명, 형식 자체가 다르면 `partners/manifest.rs` |
| Apple (D-6) | Apple은 보통 Transporter(iTunes Package)로 받는다. DDEX로 받는다는 계약이면 그대로 사용, 아니면 Transporter 전용 어댑터가 필요 (새 `AdapterKind`) |
| Aspera 전송 요구 | `transport.rs`에 `ascp` 실행 방식 전송 추가 (SFTP와 같은 인터페이스) |
| 공식 DDEX XSD 요구 | `ddex_xsd.rs`는 xmllint 사용. 파트너가 준 XSD로 교체 |

## 6. 검증

`crates/core/tests/partner_delivery.rs` (실제 파이프라인: 제출 → 1·2·3단계 → 전송):
DDEX batch 드롭(파일 해시·마커·ACK·takedown), ACK 반려, 서명 웹훅(위조·재전송·다른 릴리스 미영향),
국내 피드(manifest/CSV/결과 파일), REST API(503 재시도 → commit → LIVE), 온보딩 → 경로 개방 → 중단.
실제 SFTP 왕복은 `SFTP_TEST_*` 환경변수로 켜는 `transport::sftp_tests` (CI는 임시 sshd로 실행).
