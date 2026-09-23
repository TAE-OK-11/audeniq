---
title: "AUDENIQ 백엔드·자체 음원 배급 시스템 최종 개발보고서"
subtitle: "자체 배급 코어 · DSP 실제 연동 · 권리 검증 · 로열티 정산 · 상용 운영"
author: "AUDENIQ | Engineering Blueprint"
date: "2026-09-23 (KST)"
lang: ko-KR
---

# 문서 상태 및 적용 범위

**문서 버전:** FINAL 1.0 (v1.4 통합 + 실제 배급/연동 실행 설계) · **기준:** AUDENIQ 백엔드 개발계획안 v1.3 및 2026-09-23 「통합 전항목 지적서」. 기존 제품 구조와 FIX-001~043의 안전 불변식은 유지하고, 구조적 OPEN/PARTIAL 항목의 구현 기본값을 본문에 추가한다. 문서상 수치·런북의 확정은 배포·부하 시험·복원 시험의 완료를 의미하지 않는다.

**본 문서의 적용 방식:** 「확정」은 AUDENIQ의 현재 제품·기술 설계 방향, 「구현 기본값」은 코딩 시 채택할 상세 설계, 「GATE」는 외부 계약·법률 검토·실제 연동이 확보되기 전까지 기능을 비활성화하는 항목, 「검증 필요」는 부하·보안·복원 시험으로 확인할 항목을 뜻한다. 보고서의 심각도는 검토자가 분류한 설계 위험도이며, 이미 발생한 사고의 기록이 아니다.

## 최종 개발 범위와 문서 읽는 방법

**이 문서는 기존 v1.4의 1~13장을 그대로 기준선으로 채택하고, 14~23장에 자체 배급 엔진의 구현·실제 DSP 연동·수신 확인·라이브 추적·발매 후 관리·로열티 정산·파트너 인증·출시 기준을 추가한 통합 개발보고서다.** 기존 기술 선택과 보안·권리·금전 불변식은 변경하지 않는다. 다만 '문서 설계 확정'과 '실제 계약/연동/운영 검증 완료'는 별개다. 본문 14~23장은 **실제 개발 목표 및 단계별 인수기준**이며 이미 운영 중인 기능의 설명이 아니다.

**이 보고서의 단일 구현 SoT:** §2.4(상태), §2.5(기본값), §6.1(환송/FreshnessGuard), §10(패키지), §12.0(상업 운영 게이트). 과거 v1.4의 역사적 수정사항·중복 부록은 본 최종본에서 제외했지만 참조 원본에는 그대로 존재한다.

## v1.1 → v1.2 주요 변경

| 변경 영역 | v1.2 적용 기준 |
|:---|:---|
| 신청 전 단계 | `0-A Account Gate` → `0-B Party & Minority` → `0-C Consent Capture` → `0-D Upload Admit` 추가. 동의와 입구 업로드 검증을 신청 이전으로 이동. |
| 데이터 경계 | `application_revision` / 불변 `release_snapshot` / 불변 `distribution_package` 분리; 출처·버전·해시를 단계별로 고정. |
| 권리 및 금융 | `grant_atom` 기반 배급 가능 범위, 계약 철회 즉시 hold, 지급 대상·분배 지분의 유효 구간 스냅샷. |
| 배급 | Stage 3은 준비 전용, 별도 Execution `E-0..E-5`; 준비·전송·실제 라이브 상태 분리. |
| 운영 | 최초 모듈형 모놀리스, 큐·outbox 단일 트랜잭션, VPC 대체 경로, 서버 자원 제한·복원 시험. |
| 위험한 자동화 차단 | 미검증 서류 자동 PASS 금지, 알 수 없는 DSP 전송 결과 재전송 금지, 번호 발급·자동 지급·미계약 경로 기본 OFF. |

## v1.2 → v1.3 주요 변경 (운영 보완)

| 항목 | v1.3 개발 기준 | 상태 |
|:---|:---|:---|
| 서버 프로파일 | 2 vCPU / 4GB는 개발·내부 테스트 전용. 실제 아티스트 접수 전 4 vCPU / 8GB / 160GB SSD 통합형 또는 코어·QC 물리 분리형을 준비한다. | 설계 기준; 성능 미검증 |
| 자원 예산 | OS·Docker·PostgreSQL·API·Worker 자식 프로세스·Tunnel·백업의 피크/상한·디스크 쿼터를 별도 계산한다. | 초기 설정안 |
| 큐·백업 | 무거운 QC와 무거운 백업을 상호 배제하되 WAL 아카이빙은 상시 유지하고, API/DB에는 자원 우선권을 둔다. | 구현·시험 필요 |
| 연결·복구 | DB 풀 예산, 임시 경보값, RPO/RTO 목표, 복원 합격 기준 및 DLQ 책임자를 정한다. | 운영 준비 게이트 |
| 네트워크 | Workers VPC+Tunnel 주경로 유지; Access 대체 경로는 기본 OFF, 수동 승인·시험·재차단 런북 명시. | 장애 전환 연습 필요 |
| 출시 판단 | 문서의 용량 추정은 실측으로 대체. 피크·복원·경로 장애 시험 통과 전 일반 고객 대상 운영 불가. | NO-GO 기본값 |

**문서 간 구분:** 기존 FIX-001~043 부록은 출처와 변경 이력만 기록한 참고 자료이며 **구현 규격이 아니다**. 이번 운영 검토의 K01~K12는 별도 부록 D의 운영 위험/조치로 추적한다. 두 검토의 수치는 실측이 아닌 가정 또는 초기 권고값으로 표시한다.

## v1.3 → v1.4 통합 지적 수용 결과

**수용:** 상태값 단일 SoT, 누락 엔티티·ACL, 4축 상태·전이 계약, L01–L20 구현 기본값, 권리 범위·분배 구간, 환송표, 패키지 스키마, 중복 측정 소유권, 최소 D1 게시 엔티티, 초기 실행 단위 경량화, 운영 단계와 개발 단계의 분리. 원본 기술검토가 이미 문서상 FIXED로 표시한 안전장치는 변경하지 않고 **구현·시험 항목으로 추적**한다.

**조건부 수용:** 2인 관리자 승인은 권리·금전 관련 override 및 위험한 연결 전환에 적용하되, 실제로 승인자 2명이 없으면 **그 행위를 보류**한다. `4 vCPU / 8GB / 160GB`는 검토 시작 사양이지 성능 보증 최소치가 아니다. 오디오 핑거프린트·외부 OCR은 필요 조건과 허용 범위가 충족된 작업에서만 실행한다.

**보류·미채택:** 미계약 Merlin/LIMBO/개별 DSP 어댑터의 실제 송출, 발급 권한 없는 ISRC·UPC 자체 발급, 자동 은행 지급, 외부 L2/L3 OCR 기본 사용, 초기 gRPC/Hyperdrive/PgBouncer/전체 OTel 스택, 엔진별 마이크로서비스·작업 큐, 과도한 CMS·artifact 전체 그래프. **미채택 이유:** 지금 구현할 권한이나 검증 근거가 없거나 초기 운영 복잡성 대비 이익이 확인되지 않았다. 해당 기능은 각각의 GATE 통과 뒤 재검토한다.

**완료 상태 구분:** 아래는 *문서 설계의 채택*을 의미한다. 실제 DDL 마이그레이션·서버 설정·샌드박스 테스트·법률 자문·운영 실측은 별도 작업이며, 이 문서 작성만으로 완료되었다고 표시하지 않는다. 통합 지적서의 위험 등급은 검토자의 주장으로 보존하고, 구현 우선순위와 실제 서비스 준비 여부는 각 GATE에서 확인한다.

## 유지되는 제품 원칙

AUDENIQ은 국내 인디 아티스트·소형 레이블 대상 배급·권리·정산 인프라를 자체 운영한다. 핵심 카탈로그, QC, 권리 기록, DSP별 배급 상태, 정산 원장은 AUDENIQ의 기준 데이터로 유지한다. Direct / Merlin / LIMBO는 **조건이 확인된 경우에만 활성화되는 독립 배급 경로**이며 외부 업스트림은 AUDENIQ 고객·권리 데이터를 대신 소유하는 것으로 취급하지 않는다. 이미 검증된 레이블 권한과 본인 권리자 진술은 필요한 범위에서 재사용하고, 권리 침해·서류 진정성·실제 금융 지급의 중대한 판단을 AI 또는 핑거프린트 결과만으로 확정하지 않는다.

# 1. 실행 아키텍처와 시스템 경계

## 1.1 확정 스택과 초기 배포

| 계층 | 기술 및 역할 | 개발 시 유의점 |
|:---|:---|:---|
| UI | 홍보사이트 기존 유지; 업무용 React + TypeScript + Vite; 관리자 별도 앱 | 아티스트·레이블 공통 UI와 관리 전용 진입 분리. |
| Edge | Cloudflare Workers Static Assets, 사용자 요청 BFF | 업무 원장은 Workers나 D1에서 작성하지 않음. |
| 내부 연결 | 주경로 Workers VPC Service + Cloudflare Tunnel | 원본 Rust API의 일반 인터넷 인바운드 차단; 서비스 신원 확인 필수. |
| 대체 연결 | 기본 OFF: Tunnel public hostname + Cloudflare Access service token 또는 검증된 mTLS | 장애 전환은 문서화된 운영 승인·설정으로만 수행. |
| API | Rust / Axum / Tokio / SQLx / PostgreSQL 17 계열 | 최초에는 단일 `audeniq-api` 실행 파일·모듈형 모놀리스. |
| 비동기 작업 | 동일 Rust 코드베이스의 `audeniq-worker`, PostgreSQL queues / outbox | 실행 파일 분리는 CPU/메모리·장애 격리를 위한 것. |
| 내부 서버 간 | 서버를 실제로 분리할 때 gRPC / Protobuf / Tonic + Prost | 초기 동일 호스트·프로세스 작업 전달에 불필요한 gRPC 강제 금지. |
| 공개 콘텐츠 | 관리자 PostgreSQL 원본 → 승인된 게시 job → D1 읽기 복제본 | D1은 FAQ·공지·이벤트 공개 조회에 한정, 정산·권리의 원본 DB 금지. |
| 원본 파일 | R2 비공개 객체; 브라우저에서 직접 업로드 | 파일 바이트를 API 서버로 중계하지 않음. |
| 서버 | Debian 13, Docker Compose, AMD64 / ARM64 이미지 | 2 vCPU / 4GB는 개발·내부 테스트용. 실사용 전 4 vCPU / 8GB / 160GB 이상 또는 코어·QC 물리 분리 + GO 게이트. |

**Hyperdrive:** 초기에는 도입하지 않는다. Cloudflare Workers에서 PostgreSQL에 직접 연결하지 않고, Rust API만 PostgreSQL에 접속한다. 물리적 다중 서버 분리 전 gRPC IDL·코드 생성 CI도 시작하지 않는다.

통신 흐름은 `React → Workers HTTPS/JSON → Workers VPC Service → Tunnel → Rust API → PostgreSQL`이다. 사용자가 직접 R2로 음원·이미지·문서를 전송할 때는 **Rust가 업로드 대상 키에 한정된 권한을 발급**한다. API와 Worker가 같은 코드베이스라도 권한과 프로세스는 분리한다. 추후 QC·배급·정산 서버가 별도 호스트로 이전되면 서비스 간 gRPC를 도입하되 비동기 외부 송출은 durable job으로 관리한다.

## 1.2 원본 서버·관리자 보안

- Workers VPC는 **연결 경로**이지 사용자·Worker 신원 검증의 대체재가 아니다. 원본 API는 사설 경로만 듣고, Worker 호출 서비스 인증, Rust 세션 검사, acting-org 리소스 인가를 각각 수행한다.
- VPC가 정상 작동하지 않을 때에는 요청 실패를 5xx로 표시하고 회로차단·상태 알림을 적용한다. 대체 호스트명/Access 토큰 경로는 평상시 비활성화하며 전환·복구·재차단 런북을 둔다. 공용 Cloudflare IP가 AUDENIQ 전용 Worker라는 증거가 되는 것은 아니다.
- 관리자 웹앱과 관리자 API는 별도 Access 정책 + AUDENIQ 별도 관리자 인증/MFA로 보호한다. 관리자 요청의 Access JWT는 Worker 및/또는 원본 Rust에서 서명·issuer·audience·만료 등 검증을 완료해야 한다. `workers.dev`, 프리뷰 URL, 원본 주소를 통한 우회를 차단한다.
- Rust가 인증 세션의 권위자이다. 쿠키는 HttpOnly/Secure 및 필요한 범위의 도메인 제한을 사용하고 관리자용 쿠키와 사용자용 쿠키를 구분한다. Origin/Fetch Metadata·CSRF 방어는 변경형 API 통합 테스트의 필수 조건이다.

## 1.3 실행 환경별 서버 사양 및 피크 자원 예산

### 1.3.1 사양별 사용 범위

| 프로파일 | 기준 하드웨어 | 허용 범위·출시 결정 |
|:---|:---|:---|
| LAB | **2 vCPU / 4GB RAM / SSD 80GB 이상** | 개발·스테이징·내부 dogfood만. 접수 속도 제한, 미디어 작업 1개, 백업 중 QC 중지. 실아티스트·실발매 일반 운영의 승인 사양이 아님. |
| PROD-A | **4 vCPU / 8GB RAM / SSD 160GB 이상** | 코어/API/PG/Worker 통합 운영의 **최초 검증 대상 최소 제안값**. 피크·복원·트래픽 시험과 12장 GO 기준 통과 시에만 베타 개시. 사양 자체만으로 GO 아님. |
| PROD-B | 코어 **2–4 vCPU / 4–8GB / SSD 160GB** + QC **2–4 vCPU / 4–8GB / SSD 100GB 이상** | QC/미디어를 별도 물리 호스트로 분리. 코어의 무거운 FFmpeg 작업은 0개. 각 호스트 별 피크·복원·네트워크 시험이 필요. |

LAB의 PostgreSQL `shared_buffers` 출발값은 128–256MiB이다. 초기 QC 동시 실행은 1개이며, 모든 프로파일의 실제 용량·발매/일 처리량은 트랙 길이·포맷·외부 호출·부하 시험 전 **미검증**으로 둔다. 실행 중인 서버는 항상 `MemAvailable`, cgroup RSS/peak, swap in/out, API p95, DB 대기와 WAL 지연을 측정한다.

### 1.3.2 4GB 단일 호스트 금지 사유 (검토 보고서의 추정, 실측 아님)

| 피크 상태 | 운영 검토에서 제시한 예상 합계 | 해석 |
|:---|:---|:---|
| Idle | 약 1.4–2.1GB | 커널·Docker·DB·API·Worker·cloudflared 등 기준값의 대략적인 합. |
| QC 피크·백업 OFF | 약 3.2–4.5GB | OS/페이지 캐시가 추가되면 4GB에서 OOM/스왑 경합 위험. |
| QC + 무거운 백업 + API 피크 | 약 4.5–5.5GB 이상 | 4GB 물리 메모리의 운영 상한을 넘어서는 시나리오. |

**운영 규칙:** `QC_HEAVY`, `BACKUP_HEAVY`, 수동 대량 유지보수 작업은 동일 코어 호스트에서 동시에 시작하지 않는다. PostgreSQL의 정상 autovacuum이나 WAL 아카이빙을 단순 중단하지 않는다. 백업은 보존·복구 우선이며, 무거운 백업 전에 QC 신규 선점을 막고 진행 중 자식 작업의 종료/체크포인트를 확인한다. API·DB의 메모리 보호를 위해 과부하 시 QC부터 일시 중지한다.

### 1.3.3 PROD-A 시험용 cgroup 메모리 상한 (KiB/MiB를 명시)

| 컨테이너/작업 그룹 | 최초 시험 상한 | 주의사항 |
|:---|---:|:---|
| PostgreSQL | 2,048MiB | PG 프로세스·연결·autovacuum 합산. `shared_buffers` 최초 256MiB, `work_mem`은 부하 시험으로 보수 설정. 제한 초과 시 DB가 OOM될 수 있으므로 모니터링·부하 조정 필수. |
| audeniq-api | 768MiB | HTTP 요청 본문·동시성·SQLx 풀을 제한. |
| audeniq-worker (FFmpeg/fpcalc 자식 포함) | 1,536MiB | FFmpeg가 컨테이너 밖에서 실행되어 상한을 우회하지 않도록 동일 cgroup 내 실행. 미디어 자식 메모리 추가 상한 768MiB부터 검증. |
| cloudflared × 2 | 각 256MiB, 합계 512MiB | 동일 호스트의 복제는 *프로세스 장애* 대비이며 호스트 장애 대비 아님. |
| 무거운 백업 별도 그룹 | 512MiB | QC와 실행상 상호배제. WAL 아카이빙은 별도의 필수 경량 업무로 지속. |
| 관측·보조 그룹 | 256MiB | exporter 및 운영 에이전트. |
| **상한 합계** | **5,632MiB** | 약 8GiB 기준으로 OS·Docker·커널·페이지 캐시에 약 2,560MiB 남기는 *설계 산술*. 실제 제공 RAM/호스트 측 오버헤드 확인 필수. |

이 값은 최초 부하 시험용 예산이며 성능 보장값이 아니다. `mem_limit`(또는 동등한 Compose 리소스 제한), CPU 제한/가중치, 자식 프로세스 실행 위치를 실제 배포 파일에 반영한다. 컨테이너 메모리 제한은 서비스 종료를 유발할 수도 있으므로 OOM이 발생하지 않는지 테스트하고 필요시 QC 분리 또는 상향한다. LAB에는 이 8GB용 예산을 그대로 적용하지 않고, LAB 전용의 더 작은 제한을 별도 시험으로 확정한다.

### 1.3.4 디스크·임시 파일·로그 예산

| PROD-A SSD 총 160GB 논리 예산 (초기 제안) | GB 한도·예약 | 구현 규칙 |
|:---|---:|:---|
| OS·Docker 이미지·journald | 최대 24GB | Docker 로그 rotation (`max-size=100m`, `max-file=3` 등), journald `SystemMaxUse=500M`부터 시험. |
| PostgreSQL 데이터 + WAL 헤드룸 | 최대 80GB | 실제 PG 데이터·WAL 증가율 분리 계측; archive 적체 시 신규 QC 정지/운영 알림. 데이터 급증 시 별도 볼륨 증설. |
| QC 임시 파일 | 최대 16GB | **작업별/호스트별 쿼터**. 트랙 단위 처리; 앨범 전체 동시 다운로드 금지. 종료·예외·재시작 시 임시 파일 회수. |
| 백업 임시 스풀 | 최대 16GB | 외부 백업 저장소 전송, 로컬 대용량 풀 백업 파일의 장기 보관 금지. |
| 안전 여유 | 최소 24GB | 파일시스템 잔여 15% 또는 10GB 미만이면 critical 및 신규 고비용 작업 중지. |

총합은 **24+80+16+16+24 = 160GB**의 계획 예산이다. 볼륨/파티션은 루트(`/`), PG 데이터·WAL, QC 임시, 백업 스풀의 사용량을 각각 관측 가능하도록 설계하며 `inode` 고갈도 경보 대상이다. WAL `max_wal_size`는 보관 상한이 아니므로 archiver 실패, replication slot, 미완료 multipart, 로그 증가를 별도 추적한다. 임시 파일·객체 삭제 작업은 멱등적으로 구현하고 사용자 원본 R2 파일은 청소 대상에서 제외한다.

### 1.3.5 스왑 및 DB 연결 예산

- **스왑:** PROD-A 최초 방침은 **512MiB swap + `vm.swappiness=1`**, LAB도 별도 명시. 스왑은 부하 흡수·장애 방지 보증이 아니다. 실제 `swap in/out`가 30초 이상 지속되거나 `SwapUsed >64MiB`가 지속하면 QC를 정지하고 장애 원인을 확인한다. 스왑 OFF 대안은 OOM 시험 후 선택한다. `panic_on_oom`을 사용해 호스트를 패닉시키지 않는다.
- **DB:** PROD-A 첫 시험값 `max_connections=40`, API SQLx pool **최대 6**, 일반/미디어 Worker 합 **최대 4**, 운영/관측 연결 **최대 4**, `superuser_reserved_connections` **최소 3**을 유지한다. 합계 6+4+4+3=17로 40 미만이지만, 실제 추가 API replica·백업·마이그레이션·외부 툴 연결까지 합산하여 배포마다 다시 검증한다. 사용 연결이 최대치 80%를 넘으면 경보 및 신규 QC 선점을 중지한다. 초기에는 PgBouncer를 필수로 추가하지 않는다.
- **CPU와 큐:** interactive/finance/distribution 우선권을 보호한다. R2→QC 다운로드는 **Rust QC Worker가 제한된 버퍼로** 수행하며 Cloudflare Workers가 음원 전체를 중계하지 않는다.

### 1.3.6 수치 기반 QC 분리·증설 트리거

**실사용 중 아래 조건 중 하나라도 충족되면 QC를 중지/감속하고 별도 호스트 분리 또는 상향을 변경 작업으로 연다:** `MemAvailable <300MiB` 60초, QC 큐 건수 >20 지속 30분, QC 대기 p95 >4시간, QC 부하와 연계된 API p95 >2초 반복, 주간 제출량이 실측 안전 상한의 70%를 2주 연속 초과, QC 임시 파일 쿼터 초과 1회, 복원 시험에서 QC와의 자원 경합 발생. 기준 위반이 단발성인지/영구적으로 분리가 필요한지는 원인·실측에 따라 판단하되, 미해결 상태에서 더 많은 고객을 받지 않는다.

# 2. 데이터 모델, 권한, 변하지 않는 규칙

## 2.1 PostgreSQL 도메인별 최소 엔티티

| 영역 | 엔티티(초기 기준) |
|:---|:---|
| `identity` | users, sessions, mfa_factors, orgs, memberships, **resource_acl**, parties, party_roles, payees, delegations, **account_trust_tiers** |
| `catalog` | artists, labels, releases, tracks, credits, assets, application_revisions, **consent_packages**(참조 및 정책버전), **publication_revisions** |
| `rights` | right_claims, contracts, contract_revisions, consents, grant_atoms, disputes, legal_representative_records, **commercial_split_plans**, **commercial_split_lines** |
| `distribution` | release_snapshots, **verification_packages**, route_plans, packages(=distribution_packages), delivery_jobs, dsp_endpoints, external_ids, **identifier_pool**, live_bindings, migration_cases, **match_candidates**(카탈로그 중복 후보) |
| `finance` | royalty_reports, report_lines, commercial_split_snapshots(분배 조건 고정 참조), **royalty_match_candidates**(보고서 매칭 후보), ledger_transactions, ledger_entries, payout_orders, finance_holds |
| `operations` | jobs, outbox, check_results, correction_requests, audit_events, admin_overrides, domain_holds, **review_tasks**, **publication_outbox_receipts** |

**테넌시:** 업무 소유 주체는 `org_id`로 표현한다. 개인 아티스트도 개인 조직 컨텍스트를 보유할 수 있으며 user_id는 신청 행위의 주체/감사 용도이지 카탈로그 접근 권한을 직접 부여하지 않는다. Rust API는 `acting_org_id`에 대한 현재 ACTIVE membership 및 `resource_acl(resource_type,resource_id,org_id,party_id,action,valid_range,revoked_at)`에 근거한 접근을 검사한다. `acting_org_id`는 인증된 소속에서만 선택할 수 있고 요청 본문의 타인 ID로 권한을 확장하지 않는다. 퇴사/REVOKED membership은 즉시 ACL 기반 작업을 차단한다. 관련 FK의 임의 CASCADE 삭제를 금지하고 감사·정산 참조에는 `ON DELETE RESTRICT`를 우선 적용한다. 모든 목록 API는 타 조직 자료가 0건이어야 하며, 멤버십 정지·철회는 큐에 있는 민감 업무 권한을 다시 평가하도록 한다.

**party ≠ payee:** 실연자·원반권리자·서명자·법정대리인·정산 수취인은 한 인물일 수도, 다른 법인일 수도 있다. 해당 역할과 법적 신분은 `party_id` 기반으로 명시적으로 연결한다. 계약 만료나 탈퇴가 발생해도 원장 참조는 삭제하지 않는다. identity/catalog/rights는 목적·보존 정책에 따라 soft-delete 및 병합 참조(`merge_into_id`)를 사용하고 불변 snapshot·package·ledger는 임의 삭제·덮어쓰기를 허용하지 않는다. 실제 삭제·보존의 적법한 적용은 별도 법률·개인정보 정책 GATE이다.

## 2.2 단계별 불변 자료 계층

| 자료 | 생성 시점과 변경 규칙 | 다음 계층 참조 |
|:---|:---|:---|
| `draft` | 사용자가 수정하는 작성 중 문서 | 최종 제출 시 신규 revision 생성. |
| `application_revision` | 제출 시 확정된 신청서·파일 참조·동의 버전 | 제출된 각 revision의 내용은 변경하지 않고 보완은 새로운 revision으로 생성. |
| `verification_package` | Stage 2 통과한 범위와 검증 결과를 package_hash로 고정 | 승인 범위만 Stage 3 입력 가능. |
| `release_snapshot` | Stage 3 시작 시 승인된 메타·권리범위·asset SHA·정책값을 바이트 불변 복사 | 생성 후 UPDATE 금지; 수정은 새 snapshot. |
| `distribution_package` | `(snapshot_id,dsp_id,adapter/profile_version,package_hash)`로 고정 | 새 내용은 새 package_id; 전송 job은 해시를 핀. |

**원본 보고서 해석 메모:** 보고서 FIX-003의 “application_revision: mutable 계열”은 초안/새 revision을 생성할 수 있다는 뜻으로 구현하되, 제출 완료된 개별 revision의 본문을 수정하지 않는 방식으로 정한다. 보고서가 단일 revision 본문의 사후 수정 허용을 명확히 승인한 것은 아니며, 이 구현 기본값은 기존 계획의 버전 고정 원칙과 안전성을 위해 채택한 해석이다.

모든 검사/job에는 `pinned_revision_id`와 유효 규칙·파일·계약 해시를 기록한다. 이전 revision의 QC 결과가 늦게 도착했을 때 현재 신청서와 핀이 다르면 `STALE`로 기록하고 새 revision을 통과시키지 않는다. optimistic `row_version`을 이용해 보완과 검증 결과의 경합을 제어한다.

## 2.3 시스템 공통 불변조건 및 원자성

1. Stage 3 snapshot은 해당 신청 revision의 Stage 2 승인 범위에서만 생성한다. 권리 보류·철회가 있으면 새 송출 job을 만들 수 없다.
2. 신청서 상태 전이와 후속 `jobs` 및 `outbox` 기록은 **동일 PostgreSQL 트랜잭션**으로 커밋한다. 두 곳을 독립 호출하는 dual-write를 금지한다.
3. `(release_id,dsp_id,활성 경로)`에는 중복 ACTIVE 경로가 없어야 한다. 배급사 이전은 별도 마이그레이션 사건으로 관리한다.
4. 원장의 각 transaction은 동등한 통화·회계 단위 기준으로 `Σdebit = Σcredit`이어야 하고 수정 대신 역거래·조정거래를 기록한다.
5. 타 org 조회 결과 0건, snapshot UPDATE 실패, 구 revision 결과로 다음 단계 진입 불가, 중복 번호·중복 지급 불가를 통합 테스트로 강제한다.
6. 검증 결과, 배급 준비, 실제 전송 접수, 라이브 확인, 지급 완료는 서로 다른 상태이다. `READY_FOR_DELIVERY ≠ ACCEPTED ≠ LIVE`.

권리 계약 철회 API의 트랜잭션은 hold를 기록하고 아직 외부 송출을 시작하지 않은 queued/leased 작업을 취소 대상으로 표시한 뒤 outbox 신호를 생성한다. leased Worker는 전송 바이트를 내보내기 전 hold를 재검증한다. **이미 외부로 전송 중이거나 외부가 접수한 작업을 DB 롤백만으로 되돌릴 수는 없으므로** 그 경우 상태조회·철회·테이크다운 등 별도 보상 절차로 관리한다.

## 2.4 상태 단일 SoT · 네 개의 직교 상태축 (P-C013/015, FIX-019)

**이 절만이 상태 이름·의미·전이의 구현 기준이다.** 부록 A/B의 `PREPARATION_READY`, `STAGE1_*`, `STAGE2_*`, `STAGE3_*`, `NOT_SENT` 등은 역사적 예시이며 구현 enum으로 사용하지 않는다. `READY_FOR_DELIVERY`는 준비 완료만 의미한다. 검증 PASS, DSP ACCEPTED, LIVE, 수익 확정은 서로 별개의 상태다.

| 상태축 / 테이블 | 단일 허용 값(정확한 문자열) | 유일한 쓰기 권위 |
|:---|:---|:---|
| `application_pipeline_status` / releases | `DRAFT`, `SUBMITTED`, `STAGE1_RUNNING`, `STAGE1_CORRECTION`, `STAGE1_PASSED`, `STAGE2_RUNNING`, `STAGE2_REVIEW`, `STAGE2_CORRECTION`, `STAGE2_PASSED`, `STAGE3_PREPARING`, `STAGE3_CORRECTION`, `READY_FOR_DELIVERY`, `ON_HOLD_RIGHTS`, `SUPERSEDED`, `WITHDRAWN` | `transition_pipeline()`; stage orchestrator는 직접 UPDATE 금지 |
| `dsp_eligibility_status` / per release_revision × DSP × territory_use_scope | `PENDING`, `ELIGIBLE`, `CORRECTION_REQUIRED`, `REVIEW_REQUIRED`, `INELIGIBLE`, `INELIGIBLE_NO_CONTRACT`, `STALE` | `transition_eligibility()`; Stage2 Policy/Decision만 판정 |
| `delivery_job_status` / package × DSP × operation | `QUEUED`, `LEASED`, `SENT_UNKNOWN`, `ACCEPTED`, `REJECTED`, `CANCELLED_RIGHTS`, `CANCELLED_USER`, `DEAD_LETTER` | `transition_delivery()`; Execution E-0…E-5만 쓴다 |
| `dsp_live_status` / DSP binding | `NOT_SUBMITTED`, `IN_REVIEW`, `LIVE`, `TAKEN_DOWN`, `UNKNOWN` | `transition_live()`; 확인된 DSP 상태 수신만 쓴다 |

**직교성:** release의 `READY_FOR_DELIVERY`만으로 delivery를 `ACCEPTED`, live를 `LIVE`로 쓰거나 Finance에 수익을 생성하지 않는다. DSP A의 `ELIGIBLE`과 DSP B의 `INELIGIBLE`은 동시에 존재할 수 있으며, **Stage3 패키지는 `ELIGIBLE`이고 사용자/계약상 범위 변경 확인까지 완료된 DSP 집합에만 생성한다.** `INELIGIBLE`을 전체 발매의 거절로 합치지 않는다. `check_status`는 `PASS`, `CORRECTION_REQUIRED`, `REVIEW_REQUIRED`, `BLOCKED`, `TECHNICAL_RETRY`, `NOT_APPLICABLE`, `STALE`, `UNKNOWN`으로 별도 유지한다. `review_reason_class=TECHNICAL|RIGHTS|IDENTITY|CONTENT|DSP_POLICY|FINANCE`는 REVIEW 업무의 유형이지 자동 승인 신호가 아니다.

### 2.4.1 허용 전이표 (거부가 기본값)

| 현재 → 다음 | 이벤트 / 쓰기 주체 | 전이 선행 조건 |
|:---|:---|:---|
| DRAFT → SUBMITTED | 0-D 제출 / API | consent package·asset admit·account ACTIVE |
| SUBMITTED → STAGE1_RUNNING | S1 시작 / orchestrator | pinned revision 유효 |
| STAGE1_RUNNING → STAGE1_CORRECTION | 보완 생성 / S1 | 필수 형식·파일 오류 |
| STAGE1_CORRECTION → SUBMITTED | 수정 재제출 / API | 새 revision·필수 동의 버전 확인 |
| STAGE1_RUNNING → STAGE1_PASSED | 결과 확정 / S1 | 필수 check PASS; 상태+S2 job 단일 Tx |
| STAGE1_PASSED → STAGE2_RUNNING | S2 시작 / orchestrator | validation package hash 확인 |
| STAGE2_RUNNING → STAGE2_REVIEW | 수동 심사 필요 / S2 | review_task + reason_class |
| STAGE2_RUNNING → STAGE2_CORRECTION | 보완 요구 / S2 | 결함 및 관련 증빙 기록 |
| STAGE2_REVIEW → STAGE2_RUNNING | 심사 결과 / review manager | 근거·승인권한 확인; 원 검사 덮어쓰기 금지 |
| STAGE2_CORRECTION → SUBMITTED | 사용자 재제출 / API | 새 revision; 영향검사만 재시작 |
| STAGE2_RUNNING → STAGE2_PASSED | 검증 패키지 핀 / S2 | 필수 검사·권리 epoch·유효한 DSP 승인 범위 확인 |
| STAGE2_PASSED → STAGE3_PREPARING | preparation 시작 / S3 | approval hash 동일; 승인 DSP 집합 존재 |
| STAGE3_PREPARING → STAGE3_CORRECTION | 작성 오류 / S3 | 필요 부분 return_to=S3_PREP 또는 S2 기록 |
| STAGE3_CORRECTION → STAGE3_PREPARING | 부분 재준비 / S3 | 영향 파일/필드만 변경 및 재검증 |
| STAGE3_PREPARING → READY_FOR_DELIVERY | 최종 준비 / S3 | preflight PASS; package+outbox+QUEUED 단일 Tx |
| READY_FOR_DELIVERY → ON_HOLD_RIGHTS | 철회·권리 hold / Rights | 해당 미전송 job 취소·outbox 같은 Tx |
| STAGE2_RUNNING → ON_HOLD_RIGHTS | 계약 철회 / Rights | 동일 트랜잭션 hold 및 관련 job 취소 |
| ON_HOLD_RIGHTS → STAGE2_RUNNING | 해제 / 권한 있는 리뷰 | 새 권리 검증·epoch 확인; 과거 PASS 재사용 금지 |
| READY_FOR_DELIVERY → SUPERSEDED | 승인된 새 snapshot / domain | 옛 패키지 불변; 미전송 job 무효화 |
| SUBMITTED → WITHDRAWN | 사용자 철회 / API | 신청 권한과 외부 작업 여부 확인 |
| QUEUED → LEASED | E-0 job claim | 적법한 lease token·일정·hold 없음 |
| LEASED → CANCELLED_RIGHTS | hold 발생 / Rights/E-1 | 외부 전송 전 취소; 이미 송출된 것은 compensating 처리 |
| LEASED → SENT_UNKNOWN | 송출 후 ACK 불명 / E-3 | 자동 재송출 금지; inquiry 작업 생성 |
| LEASED → ACCEPTED | DSP 접수 명시 응답 / E-3 | 실제 외부 접수 ID 검증 |
| SENT_UNKNOWN → ACCEPTED | 외부 상태조회 / E-3 | 같은 시도 ID로 접수 확인 |
| SENT_UNKNOWN → REJECTED | 외부 상태조회 / E-3 | 해당 외부 시도가 거절되었음 확인 |
| NOT_SUBMITTED → IN_REVIEW | DSP 수신 확인 / E-4 | binding/외부 참조 연결 |
| IN_REVIEW → LIVE | DSP 라이브 확인 / E-4 | 실제 DSP 상태/링크 확인 |
| LIVE → TAKEN_DOWN | DSP 삭제 확인 / PR-T/E-4 | 관련 finance_hold/cutover 영향 처리 |

위 표에 없는 전이·`BLOCKED→READY`·`SENT_UNKNOWN→QUEUED` 직접 전이는 실패한다. 모든 전이는 주체·사유·타깃 버전·감사 이벤트를 확인하며 `UPDATE ... WHERE status=:old AND row_version=:expected` CAS 또는 행 잠금으로 동시성을 제어한다. 사업상 '이미 DSP 송출된' 사례는 release 상태만 바꿔 역전시키지 않는다.

## 2.5 코딩 전 채택표 L01–L20 (FIX-034)

아래 번호는 통합 지적서의 요약 주제를 **AUDENIQ 구현 기본값 20항목으로 재구성한 내부 표**다. 선행 `internal-strict` 원문의 L 번호별 일대일 인용표는 현재 자료에 없으므로 원문 항목이 동일 순서라고 주장하지 않는다. 충돌 시 §2.4와 본문 안전 조건을 우선하며 기능 미확정은 `DEFER+GATE`로 명시한다.

| ID | 주제 | 채택값 / 예외·게이트 |
|:---|:---|:---|
| L01 | 부분 DSP 통과 | `ELIGIBLE`인 DSP만 Prep; 제외·변경은 사용자 또는 계약상 승인 확인 |
| L02 | 진행 중 수정 | pinned revision 불일치 시 `STALE`, 영향 작업 취소 |
| L03 | 독점 권리 충돌 | 충돌 확인 시 관련 범위 `BLOCKED`, 불확실하면 `REVIEW_REQUIRED` |
| L04 | 위임 체인 길이 | 자동 검사 최대 2홉. 더 길면 REVIEW; 실제 법률 유효성을 단정하지 않음 |
| L05 | 권리 기간 | UTC `[start,end)` 반개구간; 종료 미정은 별도 무기한 코드·철회 모니터링 |
| L06 | 핑거프린트 | `REVIEW_ONLY`, 유사도만으로 자동 제재/침해 판정 금지 |
| L07 | 다른 조직의 동일 SHA 주장 | `DUPLICATE_CLAIM` REVIEW; 자동 도용 판정 금지 |
| L08 | 신규 ISRC/UPC | 발급 권한/번호대 검증 전 feature OFF, Stage2 발급 금지 |
| L09 | 배급 경로 | `contract_id NOT NULL`; 미계약 DSP·Merlin·LIMBO 후보 금지 |
| L10 | 관리자 예외 | 원 검사 불변, 별도 override, 권리·금전 PASS 2인 승인 |
| L11 | 삭제 | 회계/서명/snapshot은 불변; 개인정보 관련 원본은 별도 법적 보존·삭제 정책 적용 |
| L12 | 조직 소유권 | `org_id` + 현재 ACTIVE membership + `resource_acl`; user_id 단독 테넌시 금지 |
| L13 | 상태와 후속 작업 | jobs/outbox와 도메인 전이 동일 PostgreSQL 트랜잭션 |
| L14 | 알 수 없는 외부 결과 | `SENT_UNKNOWN`/`SUBMITTED_UNKNOWN` 자동 재전송/재송금 금지 |
| L15 | Stage2 개인정보 | 증빙 ID와 범위만 handoff; 원문·L3 접근키 전달 금지 |
| L16 | 입력·검사 버전 | revision/hash/rights_epoch/policy_version 핀, 권리 epoch는 캐시 재사용 전 조회 |
| L17 | Preflight 시점 | S3과 E-1에서 같은 FreshnessGuard 호출(두 시점 유지), 새 권리 승인 금지 |
| L18 | 미성년 동의 | consent_package_hash + 서명자·문서버전·당사자 검증 후 제출; 불명확하면 REVIEW |
| L19 | READY 의미 | `READY_FOR_DELIVERY`는 DSP 접수·LIVE·정산 확정과 무관 |
| L20 | 외부 기능 | 미계약 경로·자동지급·L2/L3 외부 OCR·실제 발급은 DEFER+해당 GATE 통과 후 활성 |

## 2.6 권리 범위·분배 구간·리소스 ACL 데이터 계약

**grant_atom 검사:** 각 원자 권한은 `track_or_release_id, grantor_party_id, grantee_party_id, right_type, territory_set, use_set, starts_at, ends_at_exclusive, exclusive, sublicensable, parent_grant_id, status, revoked_at, revision`을 갖는다. 명시적 국가 집합으로 계산하고 `WORLDWIDE_EXCEPT`는 제외국을 먼저 제거한 집합으로 정규화한다. 발매 요청 범위 ⊆ 요청 경로의 유효 grant 교집합인 경우에만 진행하며, 빈 교집합은 해당 DSP·국가에 INELIGIBLE이다. 같은 원자 범위의 상충된 독점 권한은 자동 통과하지 않고 BLOCKED/REVIEW로 분기한다. 모든 홉의 ACTIVE·기간·지역·이용방식·재허락을 확인한다. 20개 이상 픽스처에 지역 제외, 비독점·독점 겹침, 체인 단절, 만료 경계, 철회 레이스를 포함한다.

**상업적 분배는 배급 권한과 별개다:** `commercial_split_plans(release_id,scope_key,effective_range,contract_revision_id,approved_at,...)`에는 `[start,end)`의 구간과 적용 범위를 고정하고, `commercial_split_lines(plan_id,payee_party_id,share_bps,...)`의 합계를 해당 scope마다 10,000bp로 검사한다. 같은 발매·scope의 ACTIVE 구간은 겹치면 안 되며, 수정은 과거 행 UPDATE가 아닌 새 구간 또는 조정 기록으로 반영한다. 로열티 매칭은 보고서의 **이용 발생시점**에 해당하는 고정 plan·line만 참조한다. 계약 최신값을 직접 조인해 과거 수익을 재배분하는 코드는 금지한다. 지역별·이용방식별 split이 필요한 경우 `scope_key`를 해당 정책의 정규화 키로 만들고 서로 겹치는 지리 범위가 임의로 다른 plan을 통과하지 않도록 승인 범위 검사도 수행한다.

**개발 DDL 골격 — 참조 테이블·실제 enum 생성은 P0a 마이그레이션에서 정리해야 하며 아래만 단독 실행하는 완성 SQL은 아님:**

```sql
CREATE EXTENSION IF NOT EXISTS btree_gist;
CREATE TABLE rights.commercial_split_plans (
  id uuid PRIMARY KEY,
  release_id uuid NOT NULL REFERENCES catalog.releases(id) ON DELETE RESTRICT,
  scope_key text NOT NULL,
  effective_range tstzrange NOT NULL,
  contract_revision_id uuid NOT NULL REFERENCES rights.contract_revisions(id) ON DELETE RESTRICT,
  status text NOT NULL CHECK (status IN ('DRAFT','ACTIVE','SUPERSEDED')),
  CHECK (NOT isempty(effective_range)),
  CHECK (lower_inc(effective_range) AND NOT upper_inc(effective_range)),
  EXCLUDE USING gist (release_id WITH =, scope_key WITH =, effective_range WITH &&)
    WHERE (status = 'ACTIVE')
);
CREATE TABLE rights.commercial_split_lines (
  plan_id uuid NOT NULL REFERENCES rights.commercial_split_plans(id) ON DELETE RESTRICT,
  payee_party_id uuid NOT NULL REFERENCES identity.parties(id) ON DELETE RESTRICT,
  share_bps integer NOT NULL CHECK (share_bps BETWEEN 0 AND 10000),
  PRIMARY KEY (plan_id, payee_party_id)
);
-- 승인 직전 transaction 내 SUM(share_bps)=10000 강제; 불변 ACTIVE의 직접 UPDATE 금지.
-- plan/lines 버전과 해시를 finance.commercial_split_snapshots에 핀한다.
```

**resource_acl:** `org_id, resource_type, resource_id, principal_party_id, action, effective_range, revoked_at`와 해당 리소스의 org 소유 일치를 서버에서 함께 확인한다. 자료별 `org_id`와 ACL이 달라도 접근을 허용하지 않는다. `membership=REVOKED`는 역할 캐시를 재조회하며 계정 hold는 0-A에서 제출 차단된다. 원장·snapshot·감사 참조는 실제 법적 삭제 정책을 따르되 임의 CASCADE로 삭제하지 않는다.

**증빙/미성년:** `consent_packages`에는 동의 문서의 해시·동의권자 확인 결과·필요한 법정대리인 동의 완료·전자서명 검증·정책 버전·연결 신청 버전을 저장한다. `account_trust_tiers`는 하나의 보조 라우팅 테이블만 사용한다. 법정대리인 권한의 불명확성·이해상반 문제는 전문가 확인 전 자동 PASS 금지.

## 2.7 불변조건 테스트 식별자

| Test ID | 반드시 자동 테스트로 입증할 불변식 |
|:---|:---|
| INV-01 | 타 조직 org/ACL 경계: 목록·파일·계약 타 org 0건 |
| INV-02 | 타인의 R2 expected_key·asset_id를 완료 콜백에 넣어도 수락 0건 |
| INV-03 | 제출 revision·snapshot·package 직접 UPDATE는 실패 |
| INV-04 | 검사 결과와 후속 job/outbox는 crash 이전·이후 원자적 상태 |
| INV-05 | 철회 epoch 증가 + hold + 선점 job 취소 후 송출 0건 |
| INV-06 | 권리 영역 교집합·독점·위임 체인·기간 경계 20픽스처 이상 통과 |
| INV-07 | 동시 식별자 pool 예약 중복 ASSIGNED 0건(기능 ON인 경우) |
| INV-08 | 다른 권리자의 불변 split plan 겹침 금지·수취 지분 10,000bp·과거 정산 최신 join 0건 |
| INV-09 | 모호한 royalty match 자동 원장 전기 0건, 원장 차대 합계 일치 |
| INV-10 | READY만으로 DSP ACK·LIVE·금융 지급 승인 상태로 변경 0건 |
| INV-11 | `SENT_UNKNOWN` 재전송·`SUBMITTED_UNKNOWN` 재송금 0건 |
| INV-12 | 미성년 동의 hash 불일치/권한 불명확시 제출 0건 |
| INV-13 | 계약 없는 route·사용자 미승인 부분 DSP 준비 패키지 0건 |
| INV-14 | 변경 영향 없는 제목 수정에서 오디오 재분석 0회 |
| INV-15 | `return_to`와 단계별 owner가 일치; 메타 오류로 전체 Stage1 QC 강제 재실행 0건 |

# 3. 신청 전 Pre-submit 0-A~0-D

미성년자 법정대리인 동의·계약 서명·업로드 기본 접수를 Stage 1/2 안에 혼합하지 않는다. 사용자가 실제 제출 버튼을 누르기 전까지 진행하는 단계이며, 제출 API 자체가 완료 조건을 서버에서 다시 확인한다.

| 모듈 | 입력 → 자동 처리 | 통과/예외 및 시간 절약 |
|:---|:---|:---|
| 0-A Account Gate | 회원 세션, ACTIVE membership, 신청 권한, 현재 계정 hold, 필요한 신원 확인 상태 조회 | 제출 불가 계정의 고비용 QC를 시작하지 않음. 사용자 ID를 권한 근거로 신뢰하지 않음. |
| 0-B Party & Minority Path | 신청 주체·계약 당사자·권리자·미성년 여부를 각 party 역할로 구분. 법정대리인 경로(공동/단독 친권·후견·위임)를 선택 | 미성년 아티스트라는 사실만으로 레이블의 모든 발매에 똑같은 보호자 계약 요구 금지. 권한 불분명 → 별도 확인. |
| 0-C Contract/Consent Capture | AUDENIQ 자체 전자문서로 필요한 계약·위임·법정대리인 동의를 생성; 법률 검토로 확정된 동의 대상자의 신원·전자서명·문서 hash 확인 | 유효한 기존 동의는 범위와 문서 버전이 일치할 때 재사용; 필요한 법정대리인 전원 동의 미완료 시 최종 제출 불가. |
| 0-D Upload Admit | 사용자 기기→R2 직접 업로드; 업로드 완료 통지 후 DB expected_key, 실제 객체 존재·크기·기본 MIME·업로드 완료 및 체크섬 검증 | 업로드 중단/누락은 신청 이전에 수정. 고비용 디코딩은 Stage 1로 이관. |

**Consent Package:** `party_ids`, `minority_path`, `consent_doc_ids`, `signature_ids`, `scope_hash`, `valid_until`, `package_hash`. 최종 제출 API는 실제 신청 revision과 동의 대상 범위의 hash가 같은지 확인해야 한다. 법정대리인 수·서명 방식·인정 서류는 변호사 검토 후 정책 버전으로 확정한다. 만 14세 미만 개인정보 처리 동의와 계약에 관한 법정대리인 동의는 필요한 경우 별도로 기록한다.

## 3.1 R2 직접 업로드의 실제 보안 계약

- Rust API는 업로드 세션을 생성하고 `asset_id`, `org_id`, 예상 객체 키, `max_bytes`, 허용 콘텐츠 유형, 만료 시각, nonce와 multipart 상태를 PostgreSQL에 저장한다. **R2는 엔드유저 소유권 필드를 제공하는 것으로 가정하지 않는다.** 객체 소유 관계는 AUDENIQ DB의 키·조직 바인딩으로 검증한다.
- 서명 URL은 해당 단일 객체 키에만 허용한다. 초기 TTL은 15–60분 범위를 설계 기본값으로 두되 실제 파일 크기·multipart 구성을 시험해 조정한다. 만료·과대 크기·미승인 형식·타 조직 키 재바인딩을 거부한다.
- 클라이언트의 “완료” 통지 자체는 신뢰하지 않는다. Rust가 R2 HeadObject 등으로 실제 키·크기·업로드 완료를 확인하고, 전체 SHA-256은 안전한 스트리밍 검증으로 확보한다. ETag를 전체 파일 SHA-256으로 취급하지 않는다.
- 검증 전 파일은 격리 영역에 두고, 승인된 파일은 새 불변 키로 복사/승격한다. **R2에는 일반 파일시스템의 원자적 rename을 가정하지 않는다**: 복사 성공·해시 확인→DB 참조 전환→구 객체 정리의 실패 복구가 필요하다.
- CORS 허용 출처, multipart 미완료 작업의 주기적 abort, presign 로그의 민감정보 마스킹, 파일 접근 권한을 적용한다. 사용자가 원본 객체 키를 임의 입력해 다른 계정 파일을 신청서에 붙일 수 없어야 한다.

## 3.2 공개 콘텐츠 D1 게시 최소 경로

FAQ·공지·이벤트는 PostgreSQL `catalog.publication_revisions(id,entity_type,entity_id,revision,approved_by,approved_at,body_hash,status)`가 원본이다. 게시 승인과 `PublishToD1` outbox 이벤트를 **동일 트랜잭션**에 기록하고, Worker가 event ID로 D1의 공개 조회용 복제본에 멱등 upsert한다. 성공 시 `operations.publication_outbox_receipts(event_id,d1_revision,published_at)`를 남긴다. 재시도·실패는 공개 조회의 직전 승인 버전을 유지하며 관리자에게 게시 지연을 보여준다. D1에서 권리·정산 업무 원장을 작성하거나 초기 CMS의 과도한 편집 UI를 개발하지 않는다.

# 4. Stage 1 — 신청서·파일 기본 검증

**목표:** 권리 진위 판단 없이, 2차 검증에 필요한 신청서와 파일을 완전하고 일관된 형태로 넘기는 것. Stage 1은 동의 수집·심층 OCR·Chromaprint 침해 판단·ISRC 발급·DDEX 생성 업무를 담당하지 않는다.

| 모듈 | 입력·실제 검사 방법 | 자동 처리/예외 | 시간이 절약되는 지점 |
|:---|:---|:---|:---|
| 1-A 접수 | Pre-submit Consent Package 및 admitted asset hash 검증; 제출자 권한 재확인; `pinned_revision_id` 확정 | 동일 요청 키 재제출을 1건으로 수렴; 유효하지 않은 동의는 PRE_SUBMIT 환송 | 중복 신청 병합과 서명 반복 발송 제거. |
| 1-B 정보 | 앨범·트랙·크레딧·발매일·콘텐츠 유형별 필수 필드; 디스크/트랙 순서·ISRC/UPC 입력 형식 | 값 누락/모순은 필드 경로별 오류코드·수정 링크 생성. 계정 자격은 다시 수집하지 않음 | 사용자 입력 시 즉시 검증; 형식 오류 시 오디오 심층 분석·외부 API 시작 금지. |
| 1-C 기본 QC | R2 승인 대상 파일의 실제 바이트·SHA-256, magic byte, FFprobe/제한 디코딩, 이미지 손상·픽셀·문서 열람 가능 여부 | 기술 오류→TECHNICAL_RETRY. 파일 자체 오류→CORRECTION_REQUIRED. 문서는 구조 확인만; OCR 내용 분석 금지 | 원본을 브라우저→R2 직접 전송; 동일 바이트·알고리즘 버전 QC 재사용. |
| 1-D 결과 | 현재 revision의 모든 필수 검사를 대조; 중복 오류를 원인별로 병합 | PASS인 경우 Stage 2 job+상태 동일 DB 트랜잭션; 기타는 묶음 보완 메일/검토 | 메일 폭주·재제출 때 전체 QC 재실행을 방지. |

**Stage 1 Validation Package (MUST):** `revision_id`, `package_hash`, `asset_refs + basic_qc`, `field_completeness (special_flags, claimant_party_ids)`, `consent_package_hash`, `check_results_1[]`. **MUST NOT:** 권리 진정성 판정, 개인정보 원문 키, 신규 번호 발급, 최종 DSP 적합성, 배급 메시지.

**재검증:** 제목만 수정했으면 텍스트·관련 크레딧 검사만 재실행. 파일을 교체했다면 새 asset_id로 기본 QC 및 Stage 2의 중복·콘텐츠 검사를 새로 한다. 구 revision의 늦은 검사 결과는 STALE이며 다음 단계로 승격하지 않는다.

# 5. Stage 2 — 맞춤형 권리·기존 발매·콘텐츠 검증

**목표:** 단순한 본인 권리자의 일반 발매와 유효한 직계약 레이블의 발매에는 확인된 자료를 재사용하고, 실제 충돌·제3자 권리·위임·고위험 문서에만 추가 검사를 실행한다. 2차 PASS는 AUDENIQ 내부 심사 통과이지, 외부의 모든 권리 사실을 법적으로 보증한다는 뜻이 아니다.

## 5.1 2-0 Applicant Router · 심사 경로 결정

| 경로 | 조회·비교 방법 | 자동 진행 조건 / 분기 |
|:---|:---|:---|
| 본인 권리자 | 인증된 party, 신청서의 권리 진술·원반권리자 표기, 기존 내부 권리·분쟁 기록 비교 | 일반 발매이고 미해결 충돌·추가 권리 필요 사실이 없으면 간소화 경로. **신규/고위험·유사 매칭은 검토 정책 확인**. |
| 직계약 레이블 | `contract_revision` ACTIVE, 아티스트·카탈로그 적용 범위, 신청 직원 membership/role 비교 | 이전 검증 결과를 `contract_revision_id`로 재사용. 외부 마스터·새 지역·다른 이용권은 해당 권한만 추가 검사. |
| 대리·공동 권리자 | 위임자·위임받은 자의 party 관계, 대상·행위·기간·지역, 철회 상태 비교 | 전자서명과 필요한 권한이 확인된 위임만 재사용; 부족한 범위는 자동 문서 요청 또는 REVIEW. |
| 미성년자 본인 | Pre-submit 유효 Consent Package와 대상 계약·당사자 hash 비교 | 정상 완료되면 성인 본인 권리자와 동일한 심사. 친권·후견/위임·이해상반 불명확하면 해당 동의 경로 심층 검토. |

`routing_profile`은 버전 관리된 결정표로 유지한다. 법률 검토가 미완료된 자동 승인 경로는 `auto_pass_allowed=false`로 두고 관리자 검토로 처리한다. `trust_tier`는 신원/이력/정산 연결 등의 객관적 운영 정보로 산정하는 보조 심사 프로필이지 법적 소유권 증명이 아니다.

## 5.2 2-A/2-B Rights Engine · 권리 범위와 문서

**기본 권리 요구 단위:** `(track_or_release, right_type, territory_set, use_set, [start,end), required_sublicense)`. 계약·위임은 `grant_atom`으로 저장한다: `party_id, right_type, territory_set, use_set, start, end_exclusive, exclusive, sublicensable, parent_grant_id, revoked_at`.

검사 흐름은 (1) party의 권한 주장·기존 계약/레이블 범위를 조회, (2) 허락 체인을 root까지 검사하면서 계약 철회·기간 만료·독점 충돌을 검출, (3) 요청 지역·이용 방식·기간이 모든 필수 허락 범위의 **교집합에 포함**되는지 확인, (4) `allowed_scope`, `blocked_scope`, 요구되는 추가 자료를 분리한다. 범위가 불명확하거나 체인 단계가 내부 정책의 자동 검사 한도를 넘으면 REVIEW로 넘기고 임의로 권리를 확대하지 않는다. `WW except` 지역, 상이한 통화·시간대 및 반개구간 경계는 테스트 픽스처로 검사한다.

**문서 경로:** AUDENIQ 자체 전자문서는 구조화된 원본 데이터·서명 서비스 완료 통지·검증된 당사자·문서 hash를 바로 대조하고 OCR하지 않는다. 외부 PDF·스캔 종이 문서는 초기 자동 PASS allowlist에 포함하지 않으며, L1/L2/L3 문서 등급과 외부 전송 허가를 판정한 뒤 필요한 경우만 승인된 OCR로 추출한다. 외부 서류에서 글자·서명 이미지가 추출됐다는 사실만으로 진정성·대리권을 확정하지 않는다. ZDR 조건은 실제 서비스 계약의 보관·로그·처리 국가·하위처리·학습 여부까지 별도 GATE로 검증한다.

**재사용 불가 목록:** 커버곡·리믹스·샘플링·AI 등 새로운 권리 조건, 신규 지역·독점 경로 추가, 계약 revision 변경, 분쟁 open, 서명자 변경, 유효기간 만료, 신뢰 수준 하락의 경우 해당 권리 검사를 재실행한다. 새 권리 문제가 없는 본인 권리자에게 별도 종이 권리 증명서를 기본 의무로 부과하지 않는다.

## 5.3 2-C Catalog & Audio Matcher · 실제 중복 검사

1. ISRC·UPC·기존 DSP ID를 내부 인덱스로 조회하여 기존 발매와 정상적인 재수록·배급사 이전 가능성을 추린다. 2-C는 식별자 **매칭만** 하며 신규 발급을 수행하지 않는다.
2. Stage 1의 전체 SHA-256을 활성 내부 자산 인덱스와 대조한다. 같은 바이트를 다른 조직이 활성 배급 중이면 `DUPLICATE_CLAIM` REVIEW를 생성하되 침해로 확정하지 않는다.
3. 식별자·길이 등의 후보 축소 후 Chromaprint를 비교한다. fingerprint 정책에는 알고리즘 버전·최소 길이·임계값·후속 행동 `REVIEW_ONLY`를 기록한다. 짧은 샘플 구간의 권리 침해 자동 판정에 사용하지 않는다.
4. 허용된 외부 카탈로그 API/계약상의 조회 수단으로 기존 발매 링크와 메타데이터를 확인한다. 외부 조회 불가/검색 결과 없음은 각각 `UNKNOWN/TECHNICAL_RETRY`와 `NO_MATCH_IN_SEARCHED_SCOPE`로 구분한다.
5. 일치 후보가 있으면 기존 권리·실제 배급 경로·이전 사건을 결합해 정상 재발매/앨범 재수록/이전/확인 필요로 분류한다. 기존 라이브 경로가 있으면 단순 신규 발매가 아니라 Migration Case를 생성한다.

**시간 절약:** 정확한 식별자/해시 검색으로 후보를 좁히고, 이미 계산한 파일 fingerprint를 해시+알고리즘 버전으로 재사용한다. 전체 카탈로그 전수 비교·불필요한 외부 검색을 기본값으로 두지 않는다. 후보 상한과 타임아웃은 실측 후 확정한다.

## 5.4 2-D Metadata Engine · 교차 대조

검색용 정규화 문자열을 별도로 만들고 원래 제목·아티스트 표기는 변경하지 않는다. `pg_trgm`은 *후보 검색*에만 사용하며 동일인 확인은 내부 아티스트 ID·확인된 DSP 프로필 연결을 우선한다. 기존 발매일·녹음 버전·크레딧·원반권리자 및 ℗/©를 현재 신청서와 검증된 계약 자료에 교차 대조한다. 확인 불가능한 참여자 값을 임의로 작성하지 않는다. 업스트림 회사 이름을 실제 ℗/Rights Owner로 자동 표기하지 않는다.

**분기:** 명백한 입력 누락은 신청서 수정, 아티스트 동명이인·권리자 변경·최초 발매일 충돌은 관련 근거 요청/REVIEW. 동일 참여자의 복수 역할은 오류가 아니며 기악곡에 존재하지 않는 보컬·작사가 입력을 강요하지 않는다.

## 5.5 2-E Content Engine · 신청 내용과 콘텐츠 비교

1-C가 파일이 읽히는지·형식·해시를 결정했다면 2-E는 1-C의 측정 결과 hash를 재사용하며 무음·피크·클리핑·반복·트랙 간 파일 중복 등의 **콘텐츠 신호**를 평가한다. FFmpeg 측정은 CPU 큐의 동시 작업 1개부터 시작한다. 긴 무음·반복 구간을 자동 저작권 침해·불량 음원으로 단정하지 않고 해당 장르·신고와 불일치할 때만 추가 확인한다.

커버 이미지의 글자는 **존재할 때만** 필요한 OCR로 추출해 신청 메타데이터와 비교한다. 커버가 텍스트를 반드시 포함해야 한다는 규칙은 두지 않는다. 이미지 유사도는 내부 기존 파일 후보를 찾는 보조 증거로만 사용한다. 아티스트·앨범 표현을 시스템이 임의로 수정하지 않는다.

## 5.6 2-F Special Content Engine · 선택적 검사

| 유형 | 활성화되는 실제 조사 | 자동 진행 제한 |
|:---|:---|:---|
| 커버곡 | 원저작물 식별, 원본 녹음 이용 여부, 가사·편곡 변경 신고, 배급 지역·방식별 권리 처리 기록 | 모든 지역에서 하나의 공통 라이선스로 해결된다고 가정하지 않음. |
| 리믹스·샘플·비트 | 원본 자산 식별, 허락된 이용 방식·수익화·재허락·기간·독점 여부 대조 | 구매 영수증만으로 모든 배급권이 존재한다고 자동 결론내지 않음. |
| AI 생성·보조 | 신고된 도구·사용 조건·음성 모방 및 제3자 이용 신고, AUDENIQ 제출 한도·DSP 조건 | AI 탐지 점수만으로 위반/권리 침해 확정 금지. |
| 재발매·라이브·리마스터 | 기존 녹음·발매 이력과 신규 녹음 여부, 기존 ISRC/UPC 관계, 기존 경로·Migration Case | 식별자를 임의 발급·변경하지 않음. |

특수 유형의 필요한 필드는 Stage 1에서 확보한다. Stage 2는 **내용과 배급 적합성**을 검토하고, 새 서류가 필요하면 Pre-submit/문서 업무로 환송한다.

## 5.7 2-G DSP Eligibility · 계약/권리/콘텐츠 가능 범위

DSP·국가·이용 방식·콘텐츠 유형별로 승인된 권리 범위와 실제 활성 계약의 지원 범위를 대조한다. 결과는 DSP별 `ELIGIBLE / ELIGIBLE_WITH_TRANSCODE(플래그) / CORRECTION_REQUIRED / REVIEW_REQUIRED / INELIGIBLE / INELIGIBLE_NO_CONTRACT`로 관리한다. 파일의 실제 변환 수행·최종 기술 규격은 3-E, 발매 마감 계산은 3-G, 메시지 스키마 생성/검증은 3-F/3-H 소관이다.

DSP 정책은 버전·근거 문서·계약 ID·적용일을 보관한다. 계약이 없는 Direct/Merlin/LIMBO 경로는 후보로 만들지 않는다. 부분 DSP 통과는 실제 승인된 DSP 집합만 Stage 3에 전달하고, 제외/변경된 선택은 사용자 또는 계약상 필요한 절차로 확인한다.

## 5.8 2-H Integrity · 2-I Decision & Review

중복 신청, 확인된 분쟁·위임 철회, 유효한 DSP 제한 및 특수 유형 제출 정책을 release/party/contract 단위로 대조한다. 신청 건수만으로 침해를 확정하거나 전체 계정을 제재하지 않는다. 미실행·불명확·시스템 실패를 PASS로 처리하지 않는다.

Decision Engine은 이번 revision의 필수 check_matrix, 필요한 증거 버전, `rights_epoch`, DSP별 `approved_scope`와 현재 hold를 대조한다. 같은 원인에서 발생한 여러 오류는 하나의 보완 요청으로 병합하고, 사용자가 수정 가능한 필드는 미리 승인된 이메일 템플릿·수정 화면으로 안내한다. 관리자 화면에는 원문 근거·차이·적용 규칙만 요약해서 제공한다.

**관리자 override:** 원래 check_results를 고치지 않는다. 별도 override 행에 원 상태/제안 상태/사유/행위자/제2 승인자/만료를 기록한다. 권리·금전 클래스 강제 PASS에는 senior reviewer와 서로 다른 두 번째 승인자가 필요하며, 서버가 두 승인을 강제한다. 관리자도 확인되지 않은 사실을 “검증됨”으로 조작하는 방식으로 넘길 수 없다.

**상업적 분배 스냅샷:** Stage 2 Verification Package에는 `payee_party_id`, `share_bps`, `contract_revision_id`, `effective_model`을 포함한 `commercial_split_snapshot` 참조가 필수다. 정산은 수익 발생 시점에 유효한 **핀된 분배 구간**을 적용하고 임의로 최신 계약을 재조회해 과거 수익을 재배분하지 않는다.

## 5.9 실제 실행 단위 · 자동 PASS 허용 규칙 · 검사 소유권

논리적인 2-0/2-A…2-I 영역은 유지하되 **엔진별 바이너리·큐·잡을 만들지 않는다.** 초기 Rust 모듈은 ① `applicant_rights`(라우터·grant·전자문서), ② `catalog_match`(ISRC/UPC/SHA/선택적 FP), ③ `metadata_content`(크레딧·콘텐츠·특수 유형), ④ `policy_integrity`(DSP·기존 제한), ⑤ `decision_review`(결과 병합·보완) 5개 수준. 하나의 `stage2.review` durable job에 독립적인 하위 검사 결과를 기록하고 필요한 CPU·외부 조회만 별도 제한된 실행 슬롯에 배분한다. 논리 분리 ≠ 물리 서버 분리.

| 소유 단계 | 검사/측정 원본 | 재사용/자동 거절 범위 |
|:---|:---|:---|
| 0-D | R2 expected_key·HeadObject·크기·등록/격리 상태 | 1-C는 동일한 메타 정보가 유효하면 전체 객체를 다시 다운로드하지 않음 |
| 1-C | FFprobe 디코딩 가능·파일 포맷·SHA-256·기본 audio metrics `metric_hash` | 기술적 손상만 CORRECTION. 2-E에서 동일 FFmpeg 전체 재실행 금지 |
| 2-C | ISRC/UPC/파일 SHA 일치 후보, 필요 시 Chromaprint 결과 | 오디오 유사성은 REVIEW_ONLY; 법적 침해 확정 금지 |
| 2-E | 1-C metric_hash + 사용자 신고/기존 자료를 대조한 content_signals | 무음·음량만으로 저작권 침해·미신고 AI 자동 확정 금지 |
| 3-E | DSP별 실제 파생 파일·형식 변환 | 신규 파생 파일에 한해 기술 QC 재실행 |

**자동 PASS allowlist:** (a) 본인 권리자 일반 발매에서 권리 진술·검사 결과가 충돌하지 않고 2-F 추가 허락 대상이 없으며 필요한 범위가 충족됨, (b) 사전 검증된 직계약 레이블의 유효한 scope 안에 있는 발매, (c) 본인/대리·미성년자 경로 중 *AUDENIQ에서 직접 생성한 전자문서*의 신원·서명·계약·동의 hash 검증을 통과하고 해당 계약 유형의 전문가 검토 정책이 활성화된 경우. **외부 PDF/스캔본, 단순 서명 이미지, 불명확한 공동권리·위임체인, 새 샘플·커버·AI 이용권·새 지역, 분쟁 OPEN, rights_epoch 변경**은 기존 PASS 재사용 제외목록이며 각각 REVIEW 또는 추가 서류 검증으로 보낸다. account_trust_tier는 심사 우선순위 보조 자료일 뿐 독립적인 법적 권리 증명은 아니다.

**신청서 1-B 체크리스트 생성:** `special_flags`가 COVER/REMIX/SAMPLE/AI/MIGRATION인 경우 필요한 원작물·원본·허락 문서 유형·DSP 선택·기존 링크를 dynamic `required_fields`/`required_evidence`에 추가하고 **서류 제출 여부만** 1-B에서 확인한다. 실제 진정성·권리 판단과 OCR은 2-B 소관이다. 동의 대상 계약 수정이 필요한 경우에는 Stage2 관련 job 취소 → `return_to=PRE_SUBMIT` → 새 consent package·서명 → 새 revision 제출을 강제한다.

# 6. Stage 3 — DSP별 준비만 수행하는 엔진

Stage 3의 3-A…3-I는 **논리적 단계**이며 초기 실행은 `prepare_release` 1개 durable job 안의 체크포인트로 구현한다(무거운 변환만 독립 자원 슬롯). 아홉 개 별도 큐/바이너리를 만들지 않는다. Stage 3은 2차 승인 범위를 다시 심판하는 단계가 아니다. Verification Package의 hash·승인 DSP 집합·정책값을 핀한 뒤 DSP별 제출 자료·일정·파일을 완성한다. 새로운 권리 충돌이 발견되면 2차로 환송하며, 실제 외부 송출은 E 단계에서만 수행한다.

| 모듈 | 입력·자동 처리·검증 | 시간 절약 및 실패 처리 |
|:---|:---|:---|
| 3-A Finalizer | 최신 승인 revision, Verification Package hash·approved_scope 검증 → 메타·권리범위·파일 SHA·정책값의 불변 `release_snapshot` 생성 | DSP별 원본 신청서 재조회 제거. 승인 버전 stale → Stage 2 환송. |
| 3-B Identifier | 기존 ISRC/UPC 관계를 우선 재사용. 신규인 경우 **발급 권한 GATE 통과 후** 번호 풀에서 원자적 예약·할당; ASSIGNED 불변 | 동시 워커 이중 배정 금지. 권한·번호대 미확보 시 신규 발급 OFF. |
| 3-C Route Planner | `(DSP, 지역, 이용 방식)`별 `contract_id NOT NULL`, ACTIVE 기술 endpoint를 갖춘 후보만 검색; Direct→Merlin→LIMBO 기본 우선순위 | 코드 대신 정책 테이블로 경로 결정. 수수료 금액 계산 금지; `fee_schedule_id`만 핀. 기존 LIVE 경로는 PR-M 환송. |
| 3-D Canonical Model | 스냅샷으로 발매·트랙·기여자·권리 표기·이용 조건 공통 모델 1회 생성 | DSP별 중복 메타데이터 작성 제거; 확인되지 않은 값을 임의 생성하지 않음. |
| 3-E Assets | 원본 SHA + 변환 프로파일 버전으로 기존 파생 자산 검색; 필요한 경우만 제한된 QC로 변환·재검증 | 동일 규격 파일을 DSP마다 재변환하지 않음. 내용 바꾸는 자동 이미지 편집 금지. |
| 3-F Package | 실제 계약으로 확인된 DSP API/DDEX ERN/SFTP 프로파일에 맞춰 패키지 생성·스키마/참조 검사, package hash 저장 | 같은 snapshot+adapter/profile의 패키지는 재사용. 미확인 DSP 기술 명세는 배포 OFF. |
| 3-G Schedule | `submit_not_before`, `submit_deadline`, `consumer_release_at`, `user_confirmed`를 별도로 기록; 지역/시간대·권리 기간 대조 | DSP별 일정 수작업 감소. 일정 변경 시 사용자 확인 및 해당 범위 재검증. |
| 3-H Preflight | pinned scope/hash·계약 revision 유효성·active hold·객체 참조·DSP 패키지 무결성 확인 | **권리 범위 새 승인 금지**; 문제가 있으면 해당 Stage 2 또는 3 하위 모듈로 환송. |
| 3-I Handoff | READY와 DSP별 QUEUED delivery job 및 outbox를 동일 DB 트랜잭션으로 기록 | 전송 작업 누락·중복 방지. READY 표시가 DSP 접수 또는 LIVE를 의미하지 않음. |

**식별자 관리:** Stage 2는 기존 ISRC/UPC *매칭*, Stage 3만 신규 번호 *예약/할당*. `identifier_pool`은 `FREE|RESERVED|ASSIGNED|RETIRED`와 발급 자격/번호대/권한 기록을 가진다. 외부 발급 가능 여부가 확인되지 않으면 신규 발급 기능은 기본 비활성화한다.

**경로 계약 게이트:** Merlin 경유가 가능한 DSP인지, LIMBO의 API가 어떤 권리자를 요구하는지, Direct 계약이 어떤 규격을 허용하는지는 계약·공식 기술 명세를 확보하기 전 확정하지 않는다. `route_candidates`는 반드시 활성 계약과 실제 기술 endpoint를 참조한다.

**산출물 Prep Package:** `snapshot_id`, `verification_package_hash`, 식별자 할당/유지 관계, route_choice(계약·수수료 정책 ID), DSP별 패키지 digest, 일정, Preflight 결과, QUEUED job. 계약서 원문·개인정보 전체·원장·실제 지급·LIVE 상태를 넣지 않는다.

## 6.1 공통 FreshnessGuard와 return_to 결정표 (P-C018)

`FreshnessGuard(revision_id,verification_hash,snapshot_id,rights_epoch,route_contract_id,package_hash,hold_state,now)`를 3-H와 E-1에서 **같은 구현으로 각각 호출**한다. 두 호출 시점은 유지하지만 검증 로직·규칙은 복제하지 않는다. Guard가 새로운 권리 승인이나 DSP 적격성 판정을 직접 수행하지는 않는다. 신규 사실이 발견되면 담당 단계로 환송한다.

| 원인 | return_to | 즉시 취소/보류 대상 | 최소 재검증·담당 |
|:---|:---|:---|:---|
| 법정대리인 동의 문서/서명과 수정 계약 불일치 | `PRE_SUBMIT` | 해당 revision의 S2·S3 미완료 job | 0-B/0-C 서명 재획득 → 새 revision, 영향 검증 |
| account hold 또는 멤버십 REVOKED | `PRE_SUBMIT` | 신규 제출·미전송 작업 | 0-A 권한 복원·현재 ACL 재검증 |
| 제출 원본 음원 손상/교체 | `S1` | 해당 파일 관련 QC/Prep | 1-C 새 asset QC, 2-C/2-E 영향검사 |
| 권리자·위임·계약 철회·독점 충돌 | `S2` | QUEUED/LEASED 취소·rights hold | 권리 epoch 변경, 2-B·2-G 재검증 |
| 기존 음원과 다른 조직의 동일 SHA/ISRC 주장 | `S2` | 대상 발매 승인/준비 hold | 2-C 후보·2-B 증빙 재검토 |
| DSP의 정책·권리 관련 거절 | `S2` | 해당 DSP job 중지 | 2-G 정책·승인 범위 재평가 |
| DSP 메타 필드/변환 규격 거절 | `S3_PREP` | 해당 DSP package/job supersede | 3-D/3-E/3-F 재생성; 오디오 원본 S1 QC 재실행 금지 |
| 발매 일정/시간대·전송 마감 오류 | `S3_PREP` | 해당 DSP 예약 job | 3-G만 재계산, 변경 발매일 사용자 확인 |
| 외부 전송 후 ACK 불명·중복 가능 | `EXEC` | 자동 재전송 중지 | E-3 같은 attempt 조회, 불가하면 수동 화해 |
| 이미 LIVE된 음원의 수정·테이크다운·이전 | `POST` | 관련 DSP/finance 영향 부분 | PR-U/PR-T/PR-M 독립 워크플로 |
| 수익 보고서·분배 수취인/구간 매칭 모호 | `FINANCE` | 자동 전기·지급 hold | Finance matcher·승인자 검토 |

모든 환송은 `reason_code`, affected asset/field/DSP 목록, `cancel_job_ids`, 새로운 pinned revision/hash를 기록한다. 단순 제목 변경은 바뀐 DSP 패키지만 재생성하고, 전체 1-C 오디오 QC 및 전 DSP 파생파일 재생성을 금지한다. 풀 artifact 그래프는 **초기 보류**하며 `affected_dsp_ids`·`affected_check_codes` 수동 규칙표로 시작한다.

## 6.2 배급 경로·배급 패키지 무결성 계약

`route_plans.contract_id`는 활성 계약 FK NOT NULL, `endpoint_id`는 검증된 실제 기술 연결, `fee_schedule_id`만 사용(3단계 수수료 금액 계산 금지). 신규 route 활성화는 해당 DSP·지역·이용방식의 2-G `ELIGIBLE`을 선행 조건으로 한다. 동일 snapshot·DSP·adapter/profile 버전·body_hash에 대해 패키지는 유일하고 바이트 불변이어야 한다. user requested DSP 변경은 부분 승인 여부를 확인한 뒤만 반영한다.

3-H는 3-F가 만든 **바이트 그대로** schema/profile, R2 asset hash, snapshot/verification hash, 현재 계약·hold를 검사한다. E-2에서는 변경 가능한 신청서나 최신 계약을 조회해 메타데이터·수수료를 다시 만들지 않는다. 실제 DSP의 접수는 E-3, 라이브는 E-4만 기록한다.

# 7. Distribution Execution E-0~E-5 및 발매 이후 운영

| 단계 | 책임 | 실제 처리 및 예외 |
|:---|:---|:---|
| E-0 Claim | 발송 작업 선점 | QUEUED job에 lease·lock_token 부여, 중복 선점 금지. |
| E-1 Pre-send Guard | 최종 안전검사 | 대상 package_hash·ACTIVE 계약·rights_epoch·domain_holds·not_before 확인. 철회·보류가 있으면 송출 전 중단. |
| E-2 Adapt & Transmit | DSP/업스트림 전송 | 3-F에서 생성된 패키지 바이트만 사용; 외부 부작용 전에 전송 시도 ID·멱등성 키 기록. |
| E-3 Ingest ACK | 접수/거절/불명확 결과 | `NOT_SENT→SENT_UNKNOWN/ACCEPTED/REJECTED`; 결과 불명확하면 외부 조회 우선, 조회 수단 없으면 자동 재전송 금지. |
| E-4 Track Lifecycle | 승인/라이브/삭제 별도 추적 | `live_binding`에 DSP·외부 발매/트랙 ID·ISRC·live_at·prep_package_hash 기록; ACK를 LIVE로 오인하지 않음. |
| E-5 Retry/Compensating | 재시도 및 복구 | 안전성이 확인된 단계만 재시도; 필요한 취소·테이크다운 등은 별도 보상 job과 승인 이력으로 수행. |

외부 시스템은 정확히 한 번의 네트워크 전달을 보장하는 대상으로 가정하지 않는다. 반복 전달 가능성을 전제로 멱등키·상태조회·업무상 중복 효과 방지 규칙을 구현한다. 이미 외부에서 처리된 요청은 PostgreSQL 트랜잭션으로 취소할 수 없다.

## 7.1 발매 이후 별도 업무

| 모듈 | 변경 요청과 처리 |
|:---|:---|
| PR-U Update | 변경 집합(change_set) 파악 → 영향을 받는 최소 Stage만 재실행 → 구 package supersede 및 대기 job 취소 → 변경 패키지 배급. |
| PR-T Takedown | 요청자 권한·대상 DSP·사유 확인 → 필요한 승인 → E-2 별도 삭제 job → 실제 DSP 삭제 상태 추적 및 Finance hold 필요성 판단. |
| PR-M Migration | 기존 라이브 경로·권리·식별자 확인 → 신규 경로 전달 → DSP 매칭·실제 라이브 확인 → 계약상 가능한 기존 경로 종료. 자동 즉시 삭제 금지. |

공통 환송 식별자는 `PRE_SUBMIT | S1 | S2 | S3_PREP | EXEC | POST | FINANCE`이며 reason_code와 취소해야 할 job 선택 조건을 함께 기록한다. DSP의 필드 거절은 Stage 3 metadata/package로, 권리 거절은 Stage 2로, 파일 거절은 Stage 3 asset로만 환송하도록 한다.

계약 철회, 분쟁, 테이크다운, 경로 전환은 필요한 경우 `finance_hold`와 `route_cutover` 이벤트를 발생시킨다. Finance는 READY 패키지가 아니라 실제 `live_binding` 및 유효한 commercial split 구간을 기준으로 수익을 매칭한다.

## 7.2 사용자 상태·일정 API의 분리

API는 `preparation_status`(Stage 3의 `READY_FOR_DELIVERY` 등), `delivery_status_by_dsp`(QUEUED/LEASED/SENT_UNKNOWN/ACCEPTED/REJECTED 등), `live_status_by_dsp`(NOT_SUBMITTED/IN_REVIEW/LIVE/TAKEN_DOWN/UNKNOWN)를 별도 필드로 반환한다. 스케줄은 `submit_not_before`(AUDENIQ 전송 허용 시작), `submit_deadline`(확인된 DSP/업스트림 제출 마감), `consumer_release_at`(사용자에게 공개되는 시각·지역 정책) 세 필드를 섞지 않는다. 실제 DSP 데이터가 불명확하면 `UNKNOWN` 또는 nullable 근거 필드로 표시하며 READY를 발매 완료로 번역하지 않는다.

# 8. 로열티·원장·지급 관리

**수익 보고서 수신 → 원본 보관 및 중복 검사 → 표준 행 정규화 → 실제 라이브/ISRC/DSP/기간과 매칭 → 핀된 분배 조건 계산 → 이중기입 원장 → 정산 승인 → 지급 주문 → 실제 은행 결과 대조**의 흐름으로 개발한다.

- DSP 보고 수익과 실제 입금된 현금을 구분한다. 모호한 ISRC·DSP ID·권리 구간 매칭은 `AUTO|MANUAL|UNMATCHED` 중 MANUAL/UNMATCHED로 두고 자동 전기·지급하지 않는다.
- 각 원장 거래에는 균형 제약과 수정불가 원칙을 둔다. 환입·clawback은 별도 조정 및 원 거래 참조로 처리한다. 금액은 `NUMERIC/rust_decimal`로 저장·계산하며 적용 FX rate, 수수료 정책, 계약·세금 규칙 버전을 거래에 고정한다.
- 모호한 report line은 `finance.royalty_match_candidates`에 가능한 release/DSP/binding/split 참조와 일치 근거를 남긴 뒤 MANUAL로 보류한다. 독립 검토 전 원장 전기를 금지하며 `ledger_entries`는 동일 통화·거래의 차변 합계와 대변 합계가 같은지 승인 전 검증한다. 원장을 UPDATE/DELETE하지 않고 상쇄 분개를 기록한다.
- `commercial_split_snapshot`의 수취인·지분은 수익 발생 시점에 유효한 구간과 비교한다. 계약이 바뀌면 이후 유효 구간을 생성하며 과거 구간의 원장을 최신 계약으로 다시 계산하지 않는다.
- 실제 지급은 **기본 수동 승인**이며 `auto_payout_enabled=false`. payout_orders에는 고유 idempotency_key, 승인자·실행자, 은행 거래 ID, 지급 결과/환입 상태를 기록한다. 은행 응답이 불명확하면 `SUBMITTED_UNKNOWN`으로 두고 조회·대조 전 자동 재송금 금지.
- 권리 분쟁·테이크다운 등 영향받는 ISRC에는 `finance_hold`를 연결하고 지급 가능 목록에서 제외한다. 관련 없는 발매 전체에 부당하게 지급 보류를 확대하지 않도록 scope를 기록한다.

정산 세율·원천징수·미성년자 수취·계약별 지급 권한은 코드의 임의 상수가 아니라 법률/세무 검토를 거친 정책 버전으로 관리한다. 은행 API는 계약·규제·기술 검증 전 기능 비활성화하고 초기에는 수동 지급을 유지한다.

# 9. 작업 엔진·캐시·관측·복구

## 9.1 PostgreSQL 작업 큐·우선순위·자원 보호

초기 **큐 이름은 interactive/qc/rights/distribution/finance 5개, Worker 바이너리는 최대 2개**를 기준으로 하며 Stage2 모듈별 별도 DB 큐·바이너리를 만들지 않는다. `stage2.review`와 `prepare_release`는 각각 하나의 durable job 안에서 체크포인트를 관리한다. 위험한 외부 송출·지급은 다른 작업과 구별된 잠금·멱등 계약을 사용한다.

기존의 `jobs(queue, attempts, max_attempts, last_error, dead_lettered_at, locked_by, lock_token, lease_until, pinned_revision_id, idempotency_key)` 및 interactive/qc/rights/distribution/finance 분리는 유지한다. 상태 전이+job+outbox는 단일 트랜잭션으로 기록한다. QC의 `locked_by`, `lock_token`, heartbeat, `lease_until`은 **DB 시간**으로 처리하고 자식 FFmpeg 작업 종료 시 자원을 실제 해제했는지 확인한다.

- **선점 규칙:** `QC_PAUSED`·`BACKUP_HEAVY`·메모리/디스크 보호 신호가 있으면 QC 신규 작업을 가져오지 않는다. 기존 QC는 짧은 체크포인트까지 진행한 뒤 종료하고, 백업이 해당 락/실행 수 0을 확인한 후 시작한다. WAL archiving은 pause 대상이 아니다. DB·대화형 API 자원과 발매 전달/정산의 필수 소량 작업에 우선순위를 부여한다.
- **Lease 폭풍 방지:** heartbeat 미도착 작업이 있으면 호스트의 메모리·CPU·디스크와 현재 자식 프로세스를 먼저 확인하고, **같은 idempotency_key에 대한 동시 재선점 금지**를 DB 제약으로 강제한다. 자원 장애 시 무한 즉시 재시도 대신 backoff 및 관리자 확인을 적용한다. 전송/지급의 외부 결과 불명 상태는 자동 재시도하지 않는다.
- **DLQ:** 분배(`distribution`)·금융(`finance`) 신규 dead-letter는 즉시 온콜 알림, 일반 QC dead-letter는 영업시간 담당자 알림. 모든 큐에 owner, 대체 담당자, 업무 중단 영향, 재처리 권한·idempotency 확인 절차를 기록한다. 재처리 도구는 현재 lock_token·pinned revision·외부 부작용 상태를 보여주어야 한다.

## 9.2 관측·경보·온콜 (상용 시작 전 임시 임계)

검사 캐시 키는 `(asset_sha, application_revision_hash, rule_version, contract_revision_hash, rights_epoch, scope_key)` 중 검사에 필요한 구성요소만 사용하고 **재사용 직전에 rights_epoch/hold를 재조회**한다. 철회 전 PASS가 캐시 HIT로 재유입되면 실패 처리한다.

기존 correlation ID(`application_id`, `revision_id`, `snapshot_id`, `package_id`, `job_id`, `external_attempt_id`)와 감사 로그의 개인정보 마스킹·append-only 원칙은 유지한다. 경보는 정상 트래픽 실측 이후 개정할 수 있으나, **운영 시작 전에 우선 아래 보수적 임시값을 실제 모니터링 설정에 등록**한다.

| 지표 | 임시 임계 · 초기 자동 대응 |
|:---|:---|
| `MemAvailable` | <300MiB 60초 → QC pause, 인스턴스·OOM 알림. |
| swap | 30초 이상 지속적인 swap in/out 또는 `SwapUsed >64MiB` 지속 → QC pause 및 API 지연 확인. |
| 남은 디스크 / inode | 잔여 <15% **또는** <10GB → critical, 새로운 미디어 작업 중지, WAL/로그/임시 분석. |
| QC queue depth | >20건 30분 → 대기 안내, QC 분리·처리량 확인. |
| QC queue age p95 | >4시간 → QC 확장 경보, 예상 대기 공지 재계산. |
| lease expire | >5회/시간 **또는** 동일 idempotency_key 재선점 중복 1건 → 즉시 작업 선점 중지·조사. |
| PostgreSQL connections | `used / max_connections >80%` → 신규 QC 선점 정지, 풀 예산 점검. |
| 사용자 API p95 | >2초 반복은 조사, >5초 연속은 긴급 대응(측정 윈도우는 부하 시험에서 고정). |
| 백업 성공 연령 | 마지막 일간 백업 성공 >26시간 또는 백업 실패 1시간 미해결 → 긴급 대응. |
| WAL archive 지연 | 목표 RPO를 위협하는 적체/실패 즉시 대응; archive 성공 타임스탬프·실패 횟수 측정. |
| cloudflared | 재접속 >3회/10분 또는 Workers→원본 연결 지속 실패 → 경로 점검·상태 안내. |
| `DLQ{finance,distribution}` | 새 항목 1건 이상 → 즉시 책임자 호출; QC DLQ는 업무시간 대응. |
| `clock_skew_seconds` | 절대값 >2초 → 일정 발매·시간 민감한 신규 전송 보류, 시간 동기화 점검. |

**온콜:** 실제 운영 전에 주담당/대체담당·알림 채널·휴일과 야간 응답 가능 시간을 기록한다. 즉시 알림을 받을 사람이 없으면 해당 시간대의 자동 배급·지급·위험한 장애 전환을 보류한다. 단독 운영으로 2인 승인 규칙을 충족하지 못할 때는 계정/권리 override나 Access 대체 경로를 임의로 활성화하지 않는다. 장애 타임라인, 고객 공지, 재발 방지 조치 및 알림 누락도 감사한다.

## 9.3 백업·복원·RPO/RTO 목표

- **정책:** pgBackRest의 주간 full + 일간 incremental(또는 검증된 differential), 지속 WAL archive를 최초 운영안으로 둔다. 외부 S3 호환 저장소와의 **전체 복원/PITR 실증**은 필수이다. 무거운 백업 작업은 `QC_PAUSED`와 실행 중 미디어 0을 확인한 후 시작한다. **WAL archive는 QC 작업과 관계없이 상시 유지**하며, 백업이 QC를 정지시킬 수 없으면 백업을 임의 생략하지 않고 제한된 연기·긴급 알림·인력 대응 경로로 전환한다.
- **목표값(미검증):** PostgreSQL RPO ≤1시간, DB 서비스 복원 목표 RTO ≤4시간. 개발 초기의 6시간 이내 복원 연습은 *예비 시험 결과*로만 인정하며 **실사용 GO는 실제 복원 ≤4시간**과 데이터 무결성 검사를 요구한다. 목표 위반 시 달성된 것으로 광고하지 않고 프로파일/백업 설계를 수정한다.
- **복원 드릴:** 다른 호스트/격리 환경에서 최근 full+incremental+WAL을 사용해 지정 시점까지 복원한다. 데이터·FK·불변 snapshot·정산 원장 균형·임의 10건 이상의 R2 asset 참조를 대조하고, 복원 시각·유실 추정 범위·서비스 복구 시점을 기록한다. 최소 월 1회. 첫 정식 운영 전 1회, 배포/백업 정책 변경 시 추가 실행한다.
- **R2/문서:** DB WAL 백업이 R2 원본 파일을 보호하지 않는다. 버전관리/독립 복제·삭제 방어·오브젝트 존재/해시 대조를 별도 시험한다. R2 원본/계약 파일의 RPO/RTO는 DB와 구분해 실측 후 확정하며, 재생산 불가 파일이 빠진 복원은 서비스 복구 완료로 판정하지 않는다.
- **재실행 금지:** 복원 직후 기존 LEASED, `SENT_UNKNOWN`, `SUBMITTED_UNKNOWN` 외부 작업의 송출·지급 재시도를 막는다. 외부 DSP/은행 상태 대조 후 관리자가 재구성하거나 안전한 보상 작업을 생성한다.

## 9.4 경로 안정성·시간·임시 객체 정리

- **Tunnel:** 실사용 전 cloudflared 독립 프로세스 최소 2개와 재연결·프로세스 종료 시험을 준비한다. 동일 호스트 2개로 *호스트/NIC/회선 장애는 해결되지 않는다*. 해당 수준의 연속성이 필요하면 별도 호스트/회선 또는 코어 장애복구 방안을 준비해야 한다.
- **클럭:** `chrony` 등 시간 동기화 수단을 설치해 NTP 이상을 감시한다. `submit_not_before`·`deadline`·`consumer_release_at`의 업무 기준 시계는 PostgreSQL `now()`(트랜잭션 타임스탬프)로 통일한다. 외부 DSP의 지역별 공개시간은 별도 시간대 데이터로 표현한다. skew >2초면 스케줄 관련 신규 송출을 보류하고 동기화 복구를 확인한다.
- **R2/임시 파일:** multipart 미완료는 정책에서 정한 기간(초기 예: 24시간) 후 `abort` 작업으로 정리하되 사용자에게 재개 불가를 안내한다. 작업 중단·재시작·예외에서 QC 임시 파일이 항상 삭제/회수되는지 테스트하고, stale temp 건수·바이트와 multipart 미완료 건수·스토리지 비용을 경보한다. 미검증 문서나 사용자 원본을 단순 경과시간만으로 삭제하지 않는다.

## 9.5 Workers VPC 장애 시 Access 대체 경로 런북

**평상시:** VPC Service + Tunnel이 주경로. Tunnel public hostname + Access service token 대체 경로는 `OFF`; Rust 공개 API 포트는 개방하지 않는다. Access 대체는 자동 무조건 전환하지 않는다.

1. Workers와 내부 Rust 서버의 health를 각각 확인하여 VPC 경로 장애인지 Rust/DB 자체 장애인지 분리한다. 사용자에게 5xx와 상태 안내를 표시하고 circuit breaker로 재시도 폭증을 막는다.
2. 승인 권한자 2인(운영 담당·보안/대체 담당)의 상황·대체 경로 활성화·보안 위험을 기록한다. 대체 담당 부재 시 Access 경로를 임의 개방하지 않는다.
3. 미리 준비한 Access 앱 정책, Service Token 유효기간/권한/로테이션 상태, fallback 호스트명·DNS/라우팅 설정, 원본 Rust 서비스 인증, `workers.dev`/프리뷰 우회 차단 상태를 점검한다.
4. Access 정책이 지정한 **AUDENIQ Workers 서비스 토큰만** 통과시키는지 시험하고, 대체 호스트명을 제한적으로 `ON`으로 변경한다. 실제 응답과 정산/관리자 경로의 접근 통제·사용자 세션 분리를 확인한다.
5. Workers의 사전 정의된 설정을 수동 전환하고 연결·오류율·감사 이벤트를 관찰한다. 원본 서버 일반 인바운드 공개는 금지한다.
6. 주경로 복구를 확인한 후 Workers를 VPC로 되돌리고 fallback을 즉시 `OFF`로 설정한다. 토큰·로그·노출 범위를 점검하고 필요 시 토큰을 폐기/교체한다.
7. 전환 이유, 승인자, 변경 시각, 보안 검증, 재차단 완료를 사건 보고서에 남긴다. **분기 1회** 사전 환경에서 전환·원복 연습한다.

대체 경로 설정이 실제 Cloudflare 계정/요금제/기능 상태에서 구현되지 않는다면 검증 전 운영 fallback으로 표시하지 않고 VPC 장애 시 서비스 일시 중지로 동작한다.

# 10. 단계 간 단일 전달 규격 (유일한 MUST·MUST NOT SoT)

**이 절만이 handoff 필드 정의의 원본이다.** 다른 장의 표는 업무 설명이며, 과거 부록 B의 Consent/Validation/Prep 요약과 다르다면 본 절을 따른다. 표준 필드 이름은 `consent_package_hash`로 통일하고 `consent_hash`는 신규 구현에서 사용하지 않는다. `evidence_ids`는 권한 검증 가능한 참조이며 계약·법정대리인 문서 원문과 R2 비밀 접근키를 전달하지 않는다.

| 유형 (Schema 이름) | MUST 포함 | MUST NOT |
|:---|:---|:---|
| `ConsentPackageV1` | party/acting_org, document_revision, consent_policy_version, lawful_signer_ids, signer_verification_refs, `consent_package_hash`, bound_application_revision | 미확인 법정대리인 동의=PASS, 민감 가족관계 원문 복제 |
| `ValidationPackageV1` (1→2) | revision_id, revision_hash, validated_asset_ids+sha256+metric_hash, special_flags, claimant_party_ids, `consent_package_hash`, stage1_check_refs, rule_version | 권리·침해 확정, OCR 전문·L3 접근키, 신규 ISRC/UPC 발급, DDEX |
| `VerificationPackageV1` (2→3) | validation_hash, rights_epoch, approved_scope/DSP IDs, blocked_scope, evidence_ids, split_plan_snapshot_id, catalog_match_refs, check_matrix_refs, policy_versions, verification_package_hash | 원문 증빙/보호자 PII, 번호 할당, DSP 제출 payload, 금액 수수료, 실제 전송 작업 |
| `PreparationPackageV1` (3→E) | snapshot_id+hash, verification_package_hash, identifier_refs, route_id+contract_id+fee_schedule_id, DSP별 immutable package_digest+asset_manifest, `submit_not_before`/`submit_deadline`/`consumer_release_at`, preflight_ref, queued_job_refs | mutable 신청서 조인, 계약 원문, 원장·지급 주문, DSP LIVE 선언 |

각 유형의 `schema_version=1`은 필수이고 소비자는 자신이 지원하는 스키마·필수값만 수락한다. 다른 버전은 명시적 변환기가 준비되지 않았다면 거부한다. JSON 표현은 다음의 **기계적 스키마 4종**을 기초 구현 대상으로 삼되, 실제 Rust DTO/JSON Schema 파일은 P0a/P1에서 문서와 함께 버전 관리하고 CI에서 필수 필드·추가 PII·해시 무결성 fixture를 검증한다. `additionalProperties=false`로 오타·민감 필드 유입을 막고 ID는 실제 타입·UUID 규격에 맞춰 확정한다. 

```json
{
  "$defs": {
    "id": {
      "type": "string",
      "minLength": 1
    },
    "ids": {
      "type": "array",
      "items": {
        "type": "string",
        "minLength": 1
      }
    }
  },
  "title": "ConsentPackageV1",
  "type": "object",
  "additionalProperties": false,
  "required": [
    "schema_version",
    "party_id",
    "acting_org_id",
    "document_revision",
    "consent_policy_version",
    "lawful_signer_ids",
    "signer_verification_refs",
    "consent_package_hash",
    "bound_application_revision"
  ],
  "properties": {
    "schema_version": {
      "const": 1
    },
    "party_id": {
      "$ref": "#/$defs/id"
    },
    "acting_org_id": {
      "$ref": "#/$defs/id"
    },
    "document_revision": {
      "$ref": "#/$defs/id"
    },
    "consent_policy_version": {
      "$ref": "#/$defs/id"
    },
    "lawful_signer_ids": {
      "$ref": "#/$defs/ids"
    },
    "signer_verification_refs": {
      "$ref": "#/$defs/ids"
    },
    "consent_package_hash": {
      "$ref": "#/$defs/id"
    },
    "bound_application_revision": {
      "$ref": "#/$defs/id"
    }
  }
}
```

```json
{
  "title": "ValidationPackageV1",
  "type": "object",
  "additionalProperties": false,
  "required": [
    "schema_version",
    "revision_id",
    "revision_hash",
    "validated_assets",
    "special_flags",
    "claimant_party_ids",
    "consent_package_hash",
    "stage1_check_refs",
    "rule_version"
  ],
  "properties": {
    "schema_version": {
      "const": 1
    },
    "revision_id": {
      "type": "string"
    },
    "revision_hash": {
      "type": "string"
    },
    "validated_assets": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": [
          "asset_id",
          "sha256",
          "metric_hash"
        ],
        "properties": {
          "asset_id": {
            "type": "string"
          },
          "sha256": {
            "type": "string"
          },
          "metric_hash": {
            "type": "string"
          }
        }
      }
    },
    "special_flags": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "claimant_party_ids": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "consent_package_hash": {
      "type": "string"
    },
    "stage1_check_refs": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "rule_version": {
      "type": "string"
    }
  }
}
```

```json
{
  "title": "VerificationPackageV1",
  "type": "object",
  "additionalProperties": false,
  "required": [
    "schema_version",
    "validation_hash",
    "rights_epoch",
    "approved_scope",
    "approved_dsp_ids",
    "blocked_scope",
    "evidence_ids",
    "split_plan_snapshot_id",
    "catalog_match_refs",
    "check_matrix_refs",
    "policy_versions",
    "verification_package_hash"
  ],
  "properties": {
    "schema_version": {
      "const": 1
    },
    "validation_hash": {
      "type": "string"
    },
    "rights_epoch": {
      "type": "integer",
      "minimum": 0
    },
    "approved_scope": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "approved_dsp_ids": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "blocked_scope": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "evidence_ids": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "split_plan_snapshot_id": {
      "type": "string"
    },
    "catalog_match_refs": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "check_matrix_refs": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "policy_versions": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "verification_package_hash": {
      "type": "string"
    }
  }
}
```

```json
{
  "title": "PreparationPackageV1",
  "type": "object",
  "additionalProperties": false,
  "required": [
    "schema_version",
    "snapshot_id",
    "snapshot_hash",
    "verification_package_hash",
    "identifier_refs",
    "route_id",
    "contract_id",
    "fee_schedule_id",
    "dsp_packages",
    "submit_not_before",
    "submit_deadline",
    "consumer_release_at",
    "preflight_ref",
    "queued_job_refs"
  ],
  "properties": {
    "schema_version": {
      "const": 1
    },
    "snapshot_id": {
      "type": "string"
    },
    "snapshot_hash": {
      "type": "string"
    },
    "verification_package_hash": {
      "type": "string"
    },
    "identifier_refs": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "route_id": {
      "type": "string"
    },
    "contract_id": {
      "type": "string"
    },
    "fee_schedule_id": {
      "type": "string"
    },
    "dsp_packages": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": [
          "dsp_id",
          "package_digest",
          "asset_manifest_ref"
        ],
        "properties": {
          "dsp_id": {
            "type": "string"
          },
          "package_digest": {
            "type": "string"
          },
          "asset_manifest_ref": {
            "type": "string"
          }
        }
      }
    },
    "submit_not_before": {
      "type": "string",
      "format": "date-time"
    },
    "submit_deadline": {
      "type": "string",
      "format": "date-time"
    },
    "consumer_release_at": {
      "type": "string",
      "format": "date-time"
    },
    "preflight_ref": {
      "type": "string"
    },
    "queued_job_refs": {
      "type": "array",
      "items": {
        "type": "string"
      }
    }
  }
}
```

**주의:** 위 스키마는 P0a 코딩 계약용 시작 정의다. `consent_package_hash`와 같은 일부 필드는 개인 유형에 따라 NOT_APPLICABLE 표현/유효한 사전 동의 참조를 제품 정책과 일치시켜 확정해야 하며, 모든 schema 테스트·원문 PII 금지·hash/서명 검증은 런타임 규칙으로도 검사한다. 단순 JSON Schema 형식 검증만으로 권리 인증이 완료된 것은 아니다.

# 11. 자동화의 처리시간·비용 절감 설계

| 업무 | 현재 설계의 최적화 방법 | 실제 측정할 지표 |
|:---|:---|:---|
| 제출 | 입력 중 필드 검증 + Pre-submit 동의·업로드 admit | 제출 후 보완 요청률·이중 제출 건수·작성/서명 대기시간 |
| QC | 기본 규칙 먼저, SHA/검사 버전 캐시, 음원/이미지 사용자→R2 직전송 | 곡당 QC CPU 초·R2 전송량·중복 분석 회피 건수 |
| 권리 | 레이블 계약·전자위임 재사용, 본인 권리자 간소화, `grant_atom` 값 대조 | 발매당 추가 문서 요청수·자동 통과율·관리자 검토시간 |
| 중복 | ISRC/UPC/해시 선검색→후보가 있을 때 fingerprint·허용된 외부 조회 | 검사별 p50/p95·후보 수·오탐·미탐 |
| 문서 | 자체 전자문서의 구조화값 재사용, 외부 PDF만 선택적 OCR | OCR 페이지 수·문서 당 API 비용·서명/권한 재확인 비율 |
| DSP 준비 | canonical release 1회 생성, 동일 변환 프로파일 파생파일 재사용 | DSP당 패키지 생성시간·중복 트랜스코딩 절감 |
| 운영 | 동일 원인 보완 병합, 영향 부분만 재검증, 안전한 상태조회 후 재시도 | 보완메일/재검사 횟수·SENT_UNKNOWN 해결시간·긴급 재배급 건수 |
| 정산 | 고정된 payee/split 구간·자동 형식 정규화·명확한 건만 자동 매칭 | unmatched 비율·월말 조정 건수·중복 지급 0건 |

측정값은 아직 존재하지 않는다. 목표값이나 개선 비율을 실제 테스트 이전에 보장하지 않는다. **관리자 검토를 줄이는 것과 필수 법적 확인·DSP 계약 검증을 생략하는 것은 다르다.**

## 11.1 발매 처리량과 QC 분리 기준 (실측 이전 한도)

운영 검토서의 예상값은 **측정 결과가 아닌 가정**이다. LAB(2/4)에서 보고서가 제시한 싱글 위주 8–20 발매/일, 앨범 위주 2–6 앨범/일은 제품 SLA나 판매 가능한 상용 처리량으로 사용하지 않는다. 특히 Stage 2 인간 검토·외부 카탈로그 조회·DSP 수락 지연은 이 숫자에 포함되지 않는다.

| 프로파일/상황 | 서비스 정책 | 검증 방식 |
|:---|:---|:---|
| LAB 2/4 | 외부 실발매 접수 금지; 내부 부하 측정만 | 다양한 크기·포맷·곡수의 1-C, 2-C/2-E, 3-E, API, 백업을 동시에 계측. |
| 클로즈드 베타 | 임시 접수 상한 **최대 10 releases/day** 제안. 앨범 WAV 등 무거운 제출은 실제 QC 용량 확인 전 별도 제한. | p50/p95 QC 소요, CPU-seconds/track, R2 다운로드, queue age, 오류율, 수동 검토 지연을 1주 이상 기록하여 수정. |
| 정식 일반 운영 | 일/주 접수 상한을 실측으로 계산한 뒤 공개 | 안전 처리량의 70%를 2주 연속 초과하거나 QC p95>4h / queue>20/30m 등 조건이면 확장·접수 속도 조정. |

출시 전 파일 크기·앨범 트랙 수·파일 동시 검사수·총 초당 요청수에 대한 서버측 제한을 구현한다. 사용자 화면의 처리 시간은 외부 DSP 응답/사람 심사 대기와 내부 QC를 분리해서 표시한다. QC와 API의 실제 처리량은 표준 테스트 입력을 고정하여 재현성 있게 측정한다.

# 12. 구현 순서와 릴리스 GATE

| 단계 | 구현 범위 | 완료 조건 |
|:---|:---|:---|
| 문서 v1.4 | v1.3 + 통합 지적서 선별 수용. 단일 SoT·누락 스키마·L01–L20·권리구간·환송표·패키지 스키마·경량 모듈 반영 | 문서 기준 정의 완료. 코드/마이그레이션·법률·운영 실측은 별도 미완료로 추적. |
| P0a 기반 | 단일 enum/전이·스키마/ACL·불변 계층·작업 원자성·설정 파일 및 테스트 픽스처 | 타 org 접근 거부·R2 바인딩·enum 전이·스냅샷 변경 금지의 코드/DDL 시험. 운영 피크 실측은 출시 GATE에서 수행. |
| P0b 엣지 | Workers BFF, Tunnel·VPC, 기본 OFF fallback, Access JWT, 세션/CSRF | Rust 인바운드 미공개, cloudflared 재연결/2프로세스 시험, VPC 장애·Access 대체경로 수동 전환/재차단 리허설. |
| P1 제출·Stage 1 | Pre-submit 0-A..D, Consent Package, revision pin, FFprobe 기본 QC | 동의 없이는 제출 불가, 새 revision에 구 QC PASS 유입 불가, 제목 수정은 오디오 재분석 없음. |
| P2 Stage 2 권리 | router, grant_atom, 전자문서 allowlist, override, split snapshot | 미확정 법률 경로는 자동 승인 금지, 권리 강제 PASS는 2인 승인. |
| P3 Stage 2 콘텐츠 | 식별자/해시/Chromaprint/메타/DSP eligibility, 검증 캐시 | 정당한 재발매·다른 조직의 중복 주장 구분, 부분 DSP 승인 테스트. |
| P4 Stage 3 | snapshot, 식별자 feature gate, 계약 FK route, 패키지/일정/Preflight/Handoff | READY≠LIVE, 계약 없는 경로 0건, 동일 번호·패키지 이중 생성 방지. |
| P5 실행·정산 | E-0..5, 첫 실제 계약 DSP, live_binding, 원장/수동지급, PR-T hold | DSP 결과 불명확 시 재송출 금지, 테이크다운 후 해당 금액 지급 차단, 이중 지급 테스트. |
| P6 운영 확장 | VPC 상태 재검토, QC 서버 분리·Tunnel HA·부하·복원·온콜/관측 개선 | P6의 항목 중 *실트래픽 필수*인 경보·복원·용량·대체 경로 드릴은 P0a/P0b/출시 GATE로 앞당김. 2/4는 LAB 전용. |

## 12.0 실제 아티스트 트래픽 GO/NO-GO 체크포인트

**기본은 NO-GO:** v1.3은 문서 개정안이므로 실사용 준비 완료를 의미하지 않는다. 최초 소규모 클로즈드 베타 개시는 아래 항목 **전부**를 증빙해 승인받은 경우에만 허용한다.

| 영역 | 베타 GO를 위한 필수 완료·증빙 |
|:---|:---|
| 사양 | PROD-A(4/8/160) 또는 PROD-B(코어+QC 별도) 구축; 하드웨어 비용·실제 사용 가능한 메모리/디스크 기록. LAB 2/4 단독 사용 금지. |
| 피크 | QC+API 부하·백업 예약 충돌을 실제 재현하고 자원 보호·작업 취소·API p95/DB 동작 검증. |
| 자원 | 실제 Compose cgroup/CPU limit, 스왑, 연결 풀, 임시 쿼터, 로그 rotation 및 R2 미완료 cleanup 설정 배포. |
| 운영 | §9.2 경보를 실제 알림으로 시험, 주·대체 온콜 지정, finance/distribution DLQ 재처리 도구 확인. |
| 네트워크 | cloudflared 최소 2프로세스, 강제 종료·VPC 단절 시험. fallback은 실제 동작·승인·재차단 1회 연습 전 운영 가능 표시 금지. |
| 복구 | 최신 백업과 WAL로 격리 환경 복원 ≤4시간 및 데이터/R2 무결성 대조. RPO≤1시간 관측 가능. |
| 제품 | 초기 실측 처리량에 맞춘 접수/동시 QC 제한 및 CS 대기 안내. 임시 ≤10 releases/day는 *시작 상한 제안*, 실측에서 더 작으면 하향. |
| 기존 품질 | v1.2의 권리·중복·불변 snapshot·Outbox·실전송 불명·정산 지급 테스트 및 외부 계약·법률 GATE 유지. |

위 조건이 충족되기 전에는 개발·내부 사용 및 격리 스테이징 테스트만 허용한다. **같은 호스트의 cloudflared 복제는 호스트 다운 상황을 해소하지 못한다**는 잔여 위험을 베타 운영 정책에 명시하며, 무중단 가용성을 약속하지 않는다. 초기 예상 처리량과 백업 복구 목표는 측정·훈련으로 입증하기 전 고객 SLA로 제시하지 않는다.

## 12.1 외부 계약·법률 기반 기능 GATE

- **DSP / Merlin / LIMBO:** 실제 계약·접수 규격·상태 조회 및 가능한 샌드박스 검증이 없으면 경로·어댑터 OFF. 한 업체에서 제공하는 기술을 다른 업체도 지원한다고 추정하지 않는다.
- **ISRC/UPC:** 발급 기관·권한 있는 번호대가 확보될 때만 신규 발급 ON. 2차 단계는 신규 발급 없음.
- **미성년자·전자서명:** 인정 동의자, 공동·단독 친권/후견/위임, 계약상 이해상반, 개인정보 동의를 변호사 검토 후 policy_version으로 확정. 법률 미확정 경로는 REVIEW.
- **외부 OCR/AI:** L2/L3 문서의 위탁·국외 이전·데이터 학습/보관·ZDR 실제 적용 여부를 계약으로 검증하기 전 외부 전송 OFF. 초기 자동 PASS 문서 허용 목록은 확인된 AUDENIQ 전자서명 문서만.
- **은행/API 지급:** 계약·은행 처리 상태 조회 기능 검증 전 auto payout OFF. 수동 승인·지급·대조 유지.
- **Workers VPC:** 보고서 기준 베타이므로 운영 전 공식 제공 상태·SLA와 실제 테스트를 재평가한다. 보고서의 ‘GA 또는 90일 안정 운용’은 **내부 검토 게이트 제안**이지 서비스 안정성 보장 사실이 아니다. 대체 경로/장애 런북을 준비한다.

## 12.2 필수 테스트 시나리오

| 사건 | 반드시 관찰할 결과 |
|:---|:---|
| 타 조직이 R2 asset_id·key를 바꿔 완료 콜백 조작 | 타 조직 키 바인딩·파일 참조 거절. |
| 미성년자 제출이 Consent Package 만료/다른 신청서 버전 | 제출 API가 거절, 해당 동의 경로로 환송. |
| 구 revision QC가 새 제출 후 PASS | 구 결과 STALE, 현재 신청서 Stage 2 시작 금지. |
| Stage 2 PASS 후 계약 철회·lease 중 배급 워커 | hold 커밋 후 송출 전 검사에서 취소; 이미 전송된 건은 외부 상태조회/보상으로 처리. |
| Stage 3 준비 중 제목·DSP 범위 변경 | 기존 snapshot/package는 불변, 새 준비 버전·영향 부분 재검증. |
| 관리자 1인이 권리 BLOCKED를 강제 PASS | DB/API에서 거부, 원래 검사 이력 유지. |
| 두 워커의 동일 ISRC/UPC 번호 예약 경쟁 | 한 워커만 원자 예약/할당 성공, 발급 자격 OFF면 둘 다 차단. |
| DSP 파일 전송 후 응답 타임아웃 | SENT_UNKNOWN, 외부 inquiry/수동 확인 전 자동 재전송 없음. |
| 동일 보고서 중복 수입·ISRC 모호 매칭 | 원본 중복 처리 방지, 모호한 행은 자동 전기/지급하지 않음. |
| 테이크다운 뒤 기존 ISRC 정산 지급 시도 | 영향을 받는 범위 지급 hold, 감사 로그 및 해제권한 요구. |
| PG 복원 후 이전 LEASED job 재기동 | 외부 처리 여부 대조 없이 재송출·재송금 금지. |

## 12.3 운영·용량 필수 시험 (OPS K01~K12)

| 시험 | 합격 조건 |
|:---|:---|
| K01/K02 메모리·스왑 | API+QC 부하, 백업 시작, 제한/일시 중지 시나리오에서 DB/API OOM 없음; 스왑 임계 시 QC가 pause하고 응답 지연이 회복됨. |
| K03/K10 디스크·잔여 객체 | QC tmp 쿼터/로그 rotation/WAL 적체·multipart abort 테스트. 디스크 경보가 사전 발생하고 데이터 원본 손실 없이 임시 작업 정지. |
| K04 처리량 | 트랙·앨범 크기별 큐 p50/p95, CPU 초/트랙, 발매/일 최대 안전량의 **실측 결과**와 사용자 대기 안내 일치. |
| K05/K06 lease/연결 | Worker kill·DB 연결 고갈·heartbeat 지연 시 중복 QC/외부 송출 0, 안전한 재선점 및 DB 예약 연결 확보. |
| K07/K08 Tunnel/VPC | cloudflared 1개 강제 종료, VPC 경로 단절, 5xx·회로차단·fallback 수동 전환/재차단 확인. |
| K09 복원 | 실제 외부 스토리지 full+incremental+WAL 복원 ≤4시간, RPO≤1시간 기록 및 필수 불변성 검사 합격. |
| K11 DLQ | 독성 job 1건 생성 시 큐 소유자 알림·승인된 replay·감사 로그 확인. |
| K12 시간 | skew >2초 모의 시 스케줄 송출 보류, DB 기준 시계 복구 후 재계산. |

# 13. 수정 사항 추적 및 원문 보고서 연결

본 문서의 본문은 기존 v1.1의 기술·업무 원칙을 유지하면서 업로드된 「AUDENIQ 백엔드 개발계획안 — 통합 수정 보고서」에 명시된 개선 사항을 반영했다. 다음 부록은 보고서의 FIX-001~043을 **과거 검토 당시의 문구를 역사적 이력으로만** 보존해 검토 출처를 대조할 수 있도록 한 것이다. **v1.4 본문이 유일한 구현 기준이며, 충돌하는 원문·이전 버전 표·명칭은 SUPERSEDED이다.** 보고서의 심각도·완료 판정은 원 보고서의 분류이며 이 문서가 실제 테스트를 완료했다는 뜻은 아니다.


## 13.1 통합 지적서 처리 매트릭스 (문서 수용 ≠ 테스트 완료)

| 분류 | 처리 | 위치 / 게이트 |
|:---|:---|:---|
| P-C013~018, FIX-019/034, C01~C08, E01~E03 | 수용; 기존 미정의/이중 SoT 해소 | §2.1·§2.4–2.7·§6.1·§10; DDL·통합테스트는 P0a/P2/P4 |
| P-C001~012 기존 FIX 설계 | 안전 원칙 유지; 구현 완료 판정 유보 | §2.2–2.7·§3·§8–9·§12.2–12.3 |
| P-H001~014 | 필요한 간소화·정합성 수용, 법률/연동/운영 실측은 GATE 유지 | §1.1, §3.2, §5.9, §6.2, §8–12 |
| P-M001~008 및 F06/F09/F10/F12/F23/F24/F26/F27/F28 | 단계 소유·환송·상태필드·캐시·리뷰유형 명시 | §2.4, §3, §5.9, §6.1, §7.2, §9, §10 |
| E04~E22 경량화 | 논리 단계 유지, 배포/큐/job 단순화; 보안·금전 불변식은 절감 대상 아님 | §5.9·§6·§9.1; P6 이후 확장 검토 |
| K01~K12 | v1.3 운영 시험·온콜 GATE 유지(실측 미완) | §1.3, §9.2–9.5, §12.0–12.3 |
| D01~D14 | 외부 계약·법률·부하 등의 명시적 GATE까지 구현 보류 | §12.1 및 본 개정 서두 |
| 선행 `internal-strict` 원문 L01–L20 | 현재 파일에 번호별 전문 부재. 요약 주제를 v1.4 자체 L01–L20 기본값표로 재구성하되, 원문 일대일 충족 판정 보류 | §2.5; 원문 확보 시 대조 후 충돌만 수정 |

**문서 단계의 수용 판정:** 수용된 항목의 정책·엔티티·데이터 계약·전이·테스트 조건을 본문에 기록했다는 의미이다. SQL 전체 마이그레이션 작성, JSON Schema 파일 및 Rust DTO 생성, 운영 알람 설정, 외부 API 계약 확보, 통합/복원 시험 완료는 이 문서의 수행 결과가 아니다.

\newpage

---

# 14. 자체 배급 코어 개발 목표와 실제 연동 범위

## 14.1 최종 제품 범위 — 실행 가능한 자체 배급사

**프로젝트의 완료 정의:** 아티스트/레이블이 신청한 음원을 AUDENIQ에서 권리·파일·메타데이터 검증하고, 승인한 DSP에 실제로 전송하며, DSP의 접수·거절·라이브·업데이트·테이크다운 응답을 수신·저장하고, 실제 수익 보고서를 반영해 권리자 정산서를 생성할 수 있어야 한다. **정상 작동하는 내부 발매 신청/심사 UI만으로는 자체 배급 시스템 개발 완료가 아니다.** 반대로 DSP 접수 전의 `READY_FOR_DELIVERY`나 전송 성공 HTTP 응답만으로 라이브 완료를 표시하지 않는다.

구성 요소는 (a) 카탈로그·권리·계약 원장, (b) Pre-submit와 Stage 1/2/3, (c) **자체 Distribution Execution**, (d) Post-release, (e) DSP/업스트림별 로열티 수신과 정산, (f) 관리자·아티스트 상태 조회로 정한다. Direct와 업스트림은 `route_kind`만 다를 뿐 동일한 카탈로그 ID·검증·파일·권리·정산 원장을 사용한다. 업스트림의 자체 기능이 필요해도 AUDENIQ의 내부 단계/권리·정산의 권한을 그 업체로 이양하지 않는다.

**이 문서의 상태:** 직접 배급 기술 개발은 최종 개발 **범위에 포함**한다. 그러나 특정 DSP와 이미 계약했거나 API 접속권한/샌드박스를 확보했다는 사실을 뜻하지 않는다. 계약·DDEX 구현 라이선스·전송 명세·테스트 계정·수익 보고 규격이 실제로 확보되면 해당 `dsp_connector`의 운영 기능을 켜는 **GATE 방식**을 채택한다. 첫 연동 DSP는 명세와 테스트 환경을 확보한 업체 중에서 결정하며, 업체별 비공개 사양을 추정해 공통 코드에 하드코딩하지 않는다.

## 14.2 배급사로서 반드시 자체 보유할 자산

| 자산/기능 | AUDENIQ이 직접 보유하는 기준 | 경로 변경 후 보존 |
|:---|:---|:---|
| 원본 녹음/커버 | R2 원본·SHA-256·제출 버전·검증 이력 | 원본 불변; DSP별 파생물은 재생성 가능 |
| 카탈로그·권리 | 아티스트/레이블/권리자, ISRC·UPC, 사용지역/기간/권리범위, 계약·동의 참조 | Direct ↔ 업스트림 변경과 독립 |
| 배급 메시지 | 승인 snapshot, ERN/API 페이로드 원본 바이트·해시·수신자·프로파일·시도 이력 | 이전/새 경로 기록 모두 유지 |
| DSP 라이브 바인딩 | `(route,dsp,release,track,external_ids,first_live_at,verified_at)` | 기존 연결을 제거하지 않고 이관 기록 생성 |
| 로열티·정산 | 원본 보고서, 표준화 행, 대상 매칭, 분배 구간, 이중기입 원장, 지급 이력 | 과거 수익을 새 계약으로 소급 재해석 금지 |
| 감사·운영 | 누가 언제 어떤 버전과 권한으로 전송·삭제·지급했는지 | 변경 불가·최소 권한 |

## 14.3 시스템 분리: 단계는 논리적으로, 실행은 가볍게

`audeniq-api`와 `audeniq-worker` 두 Rust 실행 단위를 기본으로 유지한다. Stage 2의 권리·카탈로그·콘텐츠·정책 검사는 하나의 durable review 작업과 체크포인트, Stage 3의 준비는 하나의 durable preparation 작업과 DSP별 하위 결과로 구현한다. **Execution은 별도의 업무 경계**이며 전송 시도, 원격 부작용, 수신 확인과 조회가 필요한 장기 작업이므로 `distribution` 큐에서 관리한다. 여러 DSP 어댑터를 별도 마이크로서비스로 나누지 않는다. CPU가 큰 오디오 분석을 QC 호스트로 분리하면 해당 작업만 이동한다.

# 15. 실제 DSP 연동을 위한 상대방별 온보딩 계약

## 15.1 파트너 기술·상업 온보딩 체크리스트

각 DSP·업스트림 연결은 **기술 프로파일 및 계약 프로파일 1세트**를 만들어 승인한 뒤 시작한다. 계약이 존재하더라도 기술 접속 정보가 없으면 `route_status=INTEGRATION_PENDING`, 실제 배급을 허용하지 않는다. 기술 접속만 성공해도 계약이 없으면 `INELIGIBLE_NO_CONTRACT`로 유지한다.

| 협의 항목 | DSP/파트너에게 확인할 자료·결정 | 내부 저장 위치 |
|:---|:---|:---|
| 계약 대상·권한 | 계약 법인명, 허용 DSP/지역/이용 방식, 상호 독점/우회·재허락, 배급사 전환·종료, fee schedule | contracts, grant_atoms, route_contracts |
| 송출 형식 | ERN 메시지 버전과 실제 릴리스 프로파일, 자체 JSON/XML API, 필수/금지 필드, 코드리스트 | adapter_profiles, field_mappings |
| 파일 전달 | 파일 규격, 컨테이너, 샘플레이트/비트, 커버, 최대 용량, 업로드 방법, 원본 재전달 정책 | asset_profiles, endpoint_profiles |
| 인증·전송 | SFTP·API·승인된 클라우드 버킷 등, 자격증명·키 회전·IP·mTLS·webhook 검증 | secret refs, dsp_endpoints |
| ACK·에러 | '접수'와 '기술 검증 통과'의 차이, 작업/배치 ID, 거절 코드, 재전송·중복 판단 API | acknowledgement_profiles, retry_policies |
| 라이브·수정·삭제 | 실제 라이브 조회/이벤트, 메타데이터 변경, 발매일 수정, 테이크다운, 긴급 차단, 반영시간 | operation_capabilities |
| 기존 카탈로그·이관 | ISRC/UPC 유지, DSP 식별자 매칭, 중복 라이브·이관 방식, 기존 경로 종료 기준 | migration_policy |
| 정산 보고 | 빈도, 보고서 파일 형식/DSR 프로파일 또는 자체 CSV, 통화·세금·환율·역정산, 입금 방식 | royalty_report_profiles |
| 샌드박스·지원 | 테스트 자격증명, 테스트 발매/거절/수정/삭제, 장애 연락처, 운영상 SLA, 승인 기준 | integration_tests, partner_contacts |

**파트너별 명세 미제공 → 해당 기능을 미지원으로 표시**한다. 예를 들어 ACK만 제공하고 실제 라이브 확인 API가 없다면 `LIVE` 자동 확정 기능은 켜지 않는다. 테스트용 파일 전송이 된다고 실제 상업 송출을 활성화하지 않는다.

## 15.2 Direct / Merlin / LIMBO 온보딩 분리

| 경로 | 우리 시스템에서 구현할 어댑터 | 기능 활성 조건 |
|:---|:---|:---|
| Direct | DSP가 실제 지원하는 ERN/DDEX·API·SFTP 어댑터 | AUDENIQ-DSP 실제 계약, 전달 규격·인증 수단, 테스트 완료, 운영 승인 |
| Merlin | Merlin과의 실제 계약 및 해당 유통 경로의 기술 접속을 근거로 정의한 어댑터 | 가입 및 라이선스·전달/수익 명세 확인; 가입만으로 Direct 연결 성립 아님 |
| LIMBO | 업스트림 전용 접수·수정·삭제·보고서 어댑터 | 체결한 계약·실제 API/업로드/정산 명세; DSP별 중복 경로 방지 조건 |

**DSP별 `route_plans`는 `contract_id NOT NULL`과 검증된 기술 endpoint를 필수로 한다.** 최초 연동 범위에 없는 다른 DSP의 메타데이터를 미리 생성하는 코드는 가능하지만, 미계약 실제 전송·수익 지급은 OFF로 둔다. 기술 문서가 미공개인 DSP는 `UNKNOWN`/`INTEGRATION_PENDING` 상태로 유지한다.

# 16. DDEX 표준 기반 자체 메시지 엔진

## 16.1 실제 버전·표준 채택 정책

2026-09-23 기준 DDEX의 공개 규격 목록에는 ERN **4.3.2**와 ERN 4.3.1+용 Release Profiles **2.3.1**, Cloud-based Storage Choreography **1.8.1**, Web Services Exchange Choreography **1.8**이 안내되어 있다. AUDENIQ은 표준 코어를 **ERN 4 계열 중심으로 설계**하지만, **상대방이 수신 가능한 버전/프로파일/전송 시퀀스는 개별 계약과 인증 시험으로 확정**한다. ERN 3 또는 독자 규격만 받는 상대에게 ERN 4.3.2 XML을 임의로 전송하지 않는다.

**DDEX 구현 라이선스와 개별 DSP 배급 계약은 별개의 확인 항목**이다. DDEX는 ERN/DSR 구현 전에 Implementation Licence를 갖추도록 안내한다. 내부 프로토타입/실제 구현의 허용 범위도 라이선스 약관을 확인하며, 상업 운영 GO는 라이선스·계약·상대 기술승인을 모두 충족해야 한다.

공식 근거: [DDEX ERN 지식베이스](https://kb.ddex.net/implementing-each-standard/electronic-release-notification-message-suite-%28ern%29/), [표준 버전 목록](https://kb.ddex.net/reference-material/standards-specifications/), [웹서비스 choreography](https://kb.ddex.net/implementing-each-standard/electronic-release-notification-message-suite-%28ern%29/ern-choreography-using-web-services/).

## 16.2 공통 카탈로그에서 외부 메시지로 변환하는 파이프라인

`approved release_snapshot → internal canonical release → partner mapping → ERN NewReleaseMessage or vendor payload → schema/profile check → binary package manifest → preflight → immutable distribution_package`.

Canonical 모델은 릴리스와 음원 트랙, 오디오 리소스, 참여자·역할, 식별자, 권리 표기, 릴리스와 개별 녹음의 관계, 사용 가능 국가·기간·유형, 딜, 원본·파생 파일 참조를 구분한다. 내부 `party_id`나 계약 원문, 주민등록번호·보호자 증빙 등은 전송 페이로드에 넣지 않는다. 상대가 요구한 공개 가능한 아티스트/권리 표기만 내보낸다.

| DDEX 관련 데이터(논리적 개념) | AUDENIQ 원본 | 구현 검증 |
|:---|:---|:---|
| 메시지 송수신자·메시지 식별 | 유효한 partner party mapping·message_id | 중복 전송과 수신 ACK 상관관계 |
| Release / Resource | release_snapshot, track, resource relations | 참조 식별자 상호 일치·누락 없음 |
| 식별자 | 승인된 ISRC/UPC 및 필요한 partner ID | 발급권한·기존 매칭·충돌 확인 |
| Credits / Rights | 승인이 끝난 공개 메타데이터, grant scope | 개인 계약 원문 미전송·표기 임의 변경 금지 |
| Deal / Territory / Usage | 2-G에서 승인된 권리 범위 ∩ Route 허용 범위 | 배급되지 않을 국가·기간·방식 미포함 |
| File references | `asset_manifest`의 변형·크기·체크섬 | 메시지의 참조가 실제 전송 파일과 동일 |
| Message profile | partner `adapter_profile_version` | ERN XSD뿐 아니라 해당 DSP 비공개 추가 규칙 통과 |

XML은 표준 스키마로 정적 검증하고, **메시지 안의 리소스 참조·딜 적용·파트너별 필요 필드**는 별도 semantic 검증을 통과시킨다. DDEX 스키마 검사만 성공한 메시지를 DSP 접수/승인으로 간주하지 않는다. 테스트용 synthetic artist와 dummy file을 사용하며 실제 계약서·미성년자 개인정보는 공개 validator에 넣지 않는다.

## 16.3 공통 어댑터 인터페이스와 확장 전략

어댑터는 내부 불변 `DistributionPackage`를 입력받아 다음 인터페이스를 수행한다. `capabilities`는 실제 문서에 근거한 값만 켠다: `validate_package`, `prepare_transfer`, `send_or_publish`, `inquire_submission`, `parse_ack`, `get_release_status`, `update_release`, `takedown`, `receive_royalty_report`.

**첫 구현은 로컬 `MockDSP` + 실제 첫 파트너 어댑터 1개**다. 목 DSP는 ACCEPT/REJECT/TIMEOUT/UNKNOWN/WEBHOOK DUPLICATE/DELAYED LIVE를 재현하며 실제 계약을 가정하지 않는다. 첫 파트너 연결 후 동일 contract-test suite를 적용해 다음 DSP를 추가한다. 공통 어댑터가 모든 업체의 추가 필드를 담으려다 거대한 만능 XML로 변하지 않도록 `canonical core + partner mapping + per-partner capability flags`로 분리한다.

## 16.4 실제 파일 전송/메시지의 원자성 한계

외부 DSP 전송은 PostgreSQL 트랜잭션에 포함될 수 없다. 따라서 로컬 상태/작업/Outbox는 하나의 트랜잭션으로 관리하고, 외부 송출은 **시도 ID와 idempotency key로 상관관계를 연결**한다. DSP가 멱등 키를 지원한다는 가정은 하지 않는다. API 성공이라도 DSP가 비동기 접수할 수 있고, 통신이 끊긴 직후에는 전송 여부가 알 수 없는 상태가 된다. **`SENT_UNKNOWN`에서 자동 재전송하지 않는다.** 외부 배치/메시지 ID 조회 또는 담당자 확인 후 동일 시도를 화해시키거나, 새 송출이 안전함이 확인된 경우에만 별도 승인으로 재시도한다.

# 17. 자체 Distribution Execution 세부 구현

## 17.1 E-0~E-5 동작과 인수인계

| 단계 | 실제 담당 작업 | DB·업무 완료 증거 |
|:---|:---|:---|
| E-0 Claim | `READY_FOR_DELIVERY` package의 일정 도래 및 eligible DSP 작업 선점 | delivery_job LEASED, attempt_id·fencing_token 기록 |
| E-1 FreshnessGuard | 승인 revision/snapshot/hash·권리 epoch·계약·hold·해당 DSP 범위 재확인 | 검증 시각·result; 실패 시 적절한 `return_to` |
| E-2 Materialize | 고정 package bytes와 파일 manifest 검증·실제 전송 자료 준비 | materialized artifact checksum, 원본 불변 |
| E-3 Send & ACK | 실제 API/SFTP/클라우드 choreography에 따라 송출, 외부 응답·거절 수신·조회 | attempt ID, partner message/receipt ID, `SENT_UNKNOWN/ACCEPTED/REJECTED` |
| E-4 Ingest / Live | 별도 DSP ingest·review·live·takedown 상태를 poll/webhook/수동 증빙으로 갱신 | delivery 접수와 별도 `live_bindings` 갱신 |
| E-5 Notify / Reconcile | 아티스트와 관리자에 상태 및 원인 안내, 누락 ACK·기한 초과 화해 | audit event, notification job, reconciliation case |

`E-3`의 ACK가 단지 파일 또는 메시지 수신을 뜻하면 `ACCEPTED`의 의미를 **partner transport accepted**로 제한한다. 실제 미디어 ingest·콘텐츠 심사·라이브는 별도의 단계다. 상대방 응답의 구체적인 의미는 `partner_status_mapping`에 계약/기술 문서 근거와 함께 저장한다. DSP가 별도의 ACK를 제공하지 않는다면 가상의 ACK를 만들어서는 안 된다.

## 17.2 DSP 전송 시도 데이터 구조

| 테이블 | 핵심 칼럼·제약 |
|:---|:---|
| `dsp_connectors` | dsp_id, route_kind, contract_id NOT NULL, adapter_version, profile_version, enabled, capabilities, support_contact |
| `dsp_endpoints` | connector_id, protocol, secret_ref, test_or_prod, callback_auth, allowed_network, active_from/to |
| `distribution_packages` | snapshot_id, target_dsp_id, route_id, operation, adapter/profile_version, byte_hash, manifest_hash, immutable bytes ref |
| `delivery_jobs` | package_id, operation, due_at, status, lease_token, rights_epoch_seen, cancellation_requested, idempotency_key |
| `delivery_attempts` | job_id, attempt_no, correlation_id, partner_message_id, transfer_ref, started/finished_at, response_hash, transport_status |
| `dsp_ack_events` | connector_id, partner_event_id, attempt_id, received_at, signature_valid, raw_payload_ref, normalized_code |
| `live_bindings` | release/track ID, dsp_id, route_id, partner IDs/URL, state, verified_at, first_live_at, ended_at |
| `reconciliation_cases` | ambiguity type, blocked job(s), evidence refs, owner, resolution, audit trail |

`delivery_attempts`와 `dsp_ack_events`는 append-only이다. 동일 callback이 여러 번 오면 `partner_event_id` 또는 문서화된 대체 dedupe key로 한 번만 상태 전이한다. 승인되지 않은 콜백, 외부 ID 불일치, 다른 조직의 릴리스 링크는 격리/알림한다. Webhook이 없는 파트너는 제한된 poll을 수행하며 rate limit·backoff를 준수한다.

## 17.3 재시도·보상·삭제 안전 규칙

일시적인 연결 오류라도 **원격 작업 시작 전 실패가 명확한 경우에만** 같은 package에 대한 정책화된 재시도를 수행한다. 송출 시작 이후 결과가 모호하면 `SENT_UNKNOWN` 유지 + inquiry; E-1 실패는 해당 범위 hold와 `return_to`를 기록한다. 파일 전송과 메시지 전송의 순서를 파트너 choreography에 맞춰 구현하되, 중간 단계 실패 후 **어떤 파일/메시지가 이미 도착했는지** 기록하지 않고 처음부터 다시 보내지 않는다. 원격 부작용을 지울 필요가 있으면 기존 package를 UPDATE하는 대신 cancel/replace/takedown 보상 작업을 생성한다.

## 17.4 아티스트에게 표시하는 배급 상태

`준비 중 / 준비 완료 / 전송 예약 / 전송 중 / 파트너 접수 확인 / 파트너 검토 중 / 발매 확인 / 수정 필요 / 배급 보류 / 삭제 확인` 등으로 보여주되, **파트너 접수 확인과 실제 발매 확인을 구분**한다. 파트너 라이브 데이터가 없으면 '확인 대기'로 표시한다. 각 DSP별 상태와 마지막 검증 시각, 해결 방법을 개별 제공하며 한 DSP의 거절을 전체 음원 거절로 표시하지 않는다.

# 18. 발매 이후 직접 배급 운영·마이그레이션

## 18.1 PR-U 업데이트

이미 LIVE된 릴리스의 metadata-only 수정은 새 작업·새 배포 버전으로 관리한다. 변경된 제목/크레딧/권리 표시가 법적 권리 범위·식별자에 영향을 주는지 판단해 해당 분야 Stage 2로 환송하거나 해당 DSP용 3단계 패키지만 재생성한다. DSP가 어떤 필드의 수정을 지원하는지 `operation_capabilities`를 대조하고, 지원되지 않는 변경은 새 릴리스/관리자 경로로 분기한다. 이미 보낸 payload bytes를 변경하지 않는다. 결과는 각 DSP에서 확인할 때까지 '반영 대기'로 유지한다.

## 18.2 PR-T 테이크다운

권리 철회·분쟁·사용자 삭제요청·정책상 조치의 사유와 승인권자를 구분한다. 긴급 hold는 로컬 신규 송출을 즉시 차단하고, 원격 라이브된 항목은 각 DSP가 제공하는 테이크다운 방식으로 별도 요청한다. 요청 발송·접수·실제 삭제 확인은 별도 상태다. 권리 분쟁·테이크다운과 관련된 해당 정산 범위는 `finance_hold`와 연결하되 **과거 정당한 수익 전액을 일괄 몰수·삭제**하지 않는다. 이미 전송된 파일의 원격 삭제·정산 조정은 실제 파트너 규칙에 따른다.

## 18.3 PR-M 배급사/경로 이전

`migration_cases`에 기존 DSP·route·ISRC/UPC·외부 발매/트랙 ID·권한·기존 계약 종료일·목표 route·사용자 승인·매칭 증빙을 저장한다. 권리와 DSP 정책이 허용하는 범위에서 기존 식별자 및 원래 발매 이력을 유지하며, **신규 경로 전송 → 매칭 확인 → 실제 라이브 및 중복 상태 확인 → 기존 경로 테이크다운/드레인** 순서로 처리한다. DSP가 두 경로 병행을 허용하지 않거나 이관 절차를 별도로 요구하면 해당 규칙을 우선한다. 좋아요·스트리밍 수·플레이리스트 유지 여부는 DSP 매칭 정책에 달려 있으므로 보장 문구를 작성하지 않는다.

동일 DSP·릴리스에 둘 이상의 ACTIVE route가 생길 수 있는 migration window는 별도 승인된 `cutover_policy`와 짧은 기간, 확인 로그를 갖춰야 한다. 평상시의 동일 release × DSP 동시 ACTIVE는 불허한다. 이관 시점 이후 매출 보고서는 `live_binding + route_cutover + usage_period`로 원래 경로에 정확히 귀속시킨다.

# 19. 실제 로열티 보고서 → 권리자 지급 파이프라인

## 19.1 DSP/업스트림 보고서 인입

첫 DSP의 실제 CSV·TSV·XLSX·DDEX DSR 등 **계약상 받는 형식만** 수신 파서를 구현한다. DDEX DSR은 용도별 여러 프로파일이므로 특정 DSP가 DSR을 사용한다고 임의로 가정하지 않는다. 원본 보고서·해시·발신 파트너·보고 기간·통화·입금 참조를 append-only로 저장하고, 중복 파일·기간·재전송/수정본을 분류한 뒤 표준 usage line으로 변환한다.

DDEX의 현재 공개 안내는 DSR flat-file과 프로파일별 형식을 설명한다. 실제 DSP 정산 계약에서 어떤 프로파일 또는 자체 포맷을 사용하는지 확인하고 지원 범위를 확정한다. 공식 근거: [DDEX DSR 설명](https://kb.ddex.net/implementing-each-standard/digital-sales-reporting-message-suite-%28dsr%29/), [DSR 프로파일](https://kb.ddex.net/implementing-each-standard/digital-sales-reporting-message-suite-%28dsr%29/dsr-profiles/).

## 19.2 표준화·매칭·원장

`RawReport → Parser(versioned) → NormalizedUsageLine → MatchCandidate → FinalMatch → CommercialSplitSnapshot → LedgerTransaction → Statement → PayoutOrder`.

표준 라인은 partner, route, reporting_period, **usage_period**, track/release IDs, ISRC/UPC, external DSP IDs, territory, use_type, quantity, currency, reported_gross/net, partner adjustments, report_line_id를 갖는다. 매칭은 **동일 파트너·DSP의 live_binding과 당시 유효한 route**, ISRC/UPC 및 필요시 시간·지역으로 제한한다. 하나 이상의 권리자 또는 릴리스가 후보로 남으면 `MATCH_REVIEW`; 자동 원장 전기 금지. 서로 다른 통화를 같은 장부 항목으로 단순 합산하지 않는다.

수수료는 실제 정산 계약과 보고서에서 확인된 금액, 적용된 `fee_schedule_id`, 발생기간에 유효한 **고정 commercial_split_snapshot**으로 계산하고 정산 항목별 반올림·차액 조정 규칙을 정한다. AUDENIQ 자기 수익, 수취인 미지급 부채, 보류 금액, 환입/재정산을 분리하여 **차변 합계=대변 합계**를 강제한다. 보고 수익/입금 수익/지급 가능 잔액/실제 지급액을 섞지 않는다.

## 19.3 지급 실행

초기 실제 송금은 **관리자 승인 후 수동 지급**으로 유지하되, 법적/금융 계약·API 접속이 확인되었을 때 같은 payout order 모델로 은행 어댑터를 추가할 수 있도록 설계한다. 지급 요청 고유 ID와 수취인 확인, 지급 보류, 일괄 승인·2인 승인 대상, 은행 송금 영수증, 입금/출금 계좌 대사를 남긴다. 은행 응답이 모호하면 `SUBMITTED_UNKNOWN`에서 재송금 금지. 수익금은 AUDENIQ 수수료와 권리자 지급 채무를 분리 관리하며, 세금/원천징수 및 외국환 신고는 변호사·세무사·은행 검토에 따라 적용한다.

# 20. 외부 기술 연동을 위한 실전 테스트 시나리오

## 20.1 MockDSP 필수 시나리오

| ID | 테스트 | 성공 조건 |
|:---|:---|:---|
| DSP-01 | 첫 신규 싱글 발매 | 승인 snapshot → 사전 검증 → 1회 전송 → ACK → 별도 LIVE |
| DSP-02 | 앨범 여러 트랙·다수 지역 | 릴리스·트랙·파일 참조·Deal 지역/기간 모두 일치 |
| DSP-03 | 일부 DSP만 적격 | INELIGIBLE DSP 패키지·송출 0건; 다른 DSP 정상 진행 |
| DSP-04 | 새 계약 철회 직후 LEASED 상태 | 송출 전 권리 재확인·취소; 이미 송출된 건은 보상 상태로 이동 |
| DSP-05 | 송출 성공 후 응답 유실 | SENT_UNKNOWN 보존·중복 재전송 0건·inquiry로 화해 |
| DSP-06 | ACK 중복/역순/서명 오류 | 상태 역행 0건, 중복 1회만 처리, 위조 이벤트 격리 |
| DSP-07 | DSP 메타 필드 거절 | 해당 DSP의 S3 준비로 환송, 음원 전체 QC 재실행 0회 |
| DSP-08 | 별도 LIVE 확인 실패 | '접수 확인' 상태만 유지, LIVE 자동 확정 0건 |
| DSP-09 | 라이브 수정·테이크다운 | 신규 불변 패키지·원격 결과 확인·finance hold 연결 |
| DSP-10 | 배급 경로 전환 | 이관 승인·신규 라이브 매칭 후 기존 경로 종료; 중복 ACTIVE 제어 |
| DSP-11 | DSP별 포맷·URN/코드 오류 | XML schema + profile + partner rule 중 하나라도 실패하면 송출 0건 |
| DSP-12 | 전송 제한·일시 장애 | 정해진 백오프·rate limit·작업 큐 한도 준수 |

## 20.2 실제 파트너 샌드박스 인증 시험

MockDSP에서 성공한 테스트만으로 실제 송출을 허용하지 않는다. 파트너와 합의한 테스트 카탈로그·테스트 자격증명을 이용해 **신규 발매, 다중 트랙/지역, 필드 오류와 수신 거절, ACK 조회, 수정, 테이크다운, 기존 카탈로그 이전(지원 시), 라이브 확인(지원 시), 보고서 수신/조정**을 수행하고 양측 담당자가 접수 ID와 결과를 서명/기록한다. 계약이 없는 기능의 샌드박스 테스트를 강행하지 않는다.

수신 확인 의미·실제 라이브 상태·테이크다운 반영시간을 문서화하고 알 수 없는 항목은 기능 OFF 또는 관리자 확인 경로로 둔다. 각 파트너 운영 프로파일은 `profile_version`, 통신 경로, credential refs, 응답/에러 맵, 전송 제한, 이용 가능 범위, 마지막 상호 인증 시각을 핀한다.

## 20.3 실제 로열티·장애·보안 시험

로열티 시험은 **정상 보고, 동일 보고 재전송, 수정 보고, 음수 조정/환불, 복수 통화, 수취인 변경 전후, 이관 전후 사용기간, 미매칭 ISRC, 동일 ID의 다른 DSP, 중복 지급 요청, 통지 없는 보고 지연**을 포함한다. 보안 시험은 타 org 파일/카탈로그/계약 접근 0건, 관리자 Access 우회 0건, 오디오 업로드 중 메모리 과점유 방지, signed URL 재사용·만료·타키 거부, callback 위조 거부를 포함한다. 운영 시험은 QC+DB 피크, Tunnel/VPC 장애, 백업·복원, RPO/RTO, DLQ 담당자 경보와 사용 중단 모드를 실제로 실행해 기록한다.

# 21. 실제 개발에 사용할 작업 분해 및 선행 관계

| 단계 | 개발 산출물 | 다음 단계로 넘어가는 증빙 |
|:---|:---|:---|
| F0 설계 동결 | v1.4의 단일 상태 SoT·DDL/ERD·패키지 4종 Schema·권한/권리/수익 분배·실행 인터페이스 | 위험한 상태/미계약 경로가 구조적으로 불가능한 테스트 |
| F1 Foundation | Rust API·세션/RBAC/ACL·PostgreSQL·R2 직접 업로드·일관된 job/outbox·감사 | org·upload·crash 안전성 자동 시험 |
| F2 Pre-submit + Stage1 | 전자동의 게이트·신청서·파일 QC·보완·변경점 캐시 | 청소년 예외/서류 검토 GATE, 변경 없는 오디오 재분석 0회 |
| F3 Stage2 | 본인/레이블/대리인 권리 검증, 계약·분배·중복·DSP 적격성·관리자 심사 | 20+ grant 픽스처, 원문 외부 OCR 자동 PASS 0건 |
| F4 Stage3 | Canonical Release, 식별자 관리(발급 OFF), route plan, DDEX/독자 포맷 생성, 패키지 고정 | partner-neutral fixture, XML/메타/파일/권리 preflight 성공 |
| F5 MockDSP + Execution | 실제 전송 인터페이스, durable attempt, ACK, LIVE, UPDATE, TAKEDOWN, migration | DSP-01~12 통과, 중복 송출 0건 |
| F6 실제 첫 파트너 | 계약·프로파일·명세·라이선스·샌드박스·실제 production credentials와 runbook | 파트너 승인·실제 테스트 릴리스·접수·라이브 확인 |
| F7 Finance | 첫 파트너 실제 보고서 파서, 수익 매칭, split snapshot, 이중기입 원장, 수동 지급 | 수정/음수/복수통화/이관/중복지급 테스트, 실보고 샘플 대사 |
| F8 운영 베타 | 자원/온콜·복원·fallback·보안·실제 발매 일일 한도 | GO/NO-GO 표 전부 증거 링크 확보 |
| F9 DSP 확장 | 둘째 DSP/업스트림 어댑터를 기존 계약 테스트로 추가 | 기존 카탈로그/정산·접근권한 회귀 없음 |

**선행 관계:** Stage3의 공통 ERN 생성 코드는 상대의 비공개 명세 없이도 synthetic fixture로 개발할 수 있다. 그러나 첫 연동 파트너는 늦어도 F4 초기에 기술 담당·계약·테스트 환경을 확보해야 하며, F6 일정은 파트너와의 상호 인증 속도에 달려 있다. F7의 원장·수익 배분 모델은 **F0부터** 데이터 계약을 정하되 실제 파서는 첫 파트너 보고서가 확보된 뒤 구현한다. 기능 개발 완료와 실제 파트너 상업 승인, 법률 확인, 성능 GO는 서로 별도 상태로 추적한다.

# 22. 첫 DSP 연동의 기술 담당자 전달 패키지

실제 파트너에게 제출할 문서를 다음과 같이 생성한다. `Integration Profile v1`(회사·기술 담당자·범위·지원 지역·콘텐츠), `Transport & Security Questionnaire`(API/SFTP 등, 인증·키 교환·IP/방화벽·mTLS·권한), `Metadata Mapping Matrix`(ERN 또는 독자 필드·필수/허용 값·크레딧·℗/©), `Audio/Artwork Specification`(포맷·파일 전달), `Message Choreography & Error Map`(ACK/상태/중복/수정/삭제), `Migration Procedure`, `Royalty Reporting & Reconciliation Spec`, `UAT Checklist & Sign-off`, `Incident Contacts & Escalation`.

**AUDENIQ 준비 문서에 넣을 질문:** 실제 ERN 버전/릴리스 프로파일·메시지 인증, 수신자 Party ID/발신자 Party ID, 전송 순서, 업로드 용량/체크섬, 동일 ISRC 업로드 정책, 아티스트 프로필 매칭, 라이브 상태 확인 방법, 권리자 표기와 업스트림 표시 제한, 거절 사유 반환 시각과 코드를 반드시 요청한다. 공개 API가 없거나 접근권한이 발급되지 않았다면 연결 방식을 추정한 임시 프로그램으로 상업 송출하지 않는다. 기술 담당자 이메일 회신이 '계약 완료'를 뜻하지 않으며, 별도의 법적·운영 승인 기록을 요구한다.

# 23. 최종 GO/NO-GO 및 남은 의사결정

## 23.1 개발 착수와 상업 운영을 분리

**개발 착수 GO:** v1.4 기준 단일 state/DDL·ACL·권리/금전 기간 모델·4종 패키지 스키마를 승인하고, MockDSP와 테스트 데이터를 사용할 수 있을 때. **실제 첫 DSP 송출 GO:** 개별 계약·DDEX 구현 라이선스(해당 시)·파트너 명세·프로덕션 인증·상호 샌드박스/수신 테스트·취소/중복/실패 처리와 담당자 승인이 있을 때. **실제 아티스트 운영 GO:** v1.4 §12.0의 서버 자원·복원·VPC/fallback 또는 명시적 일시 중지·온콜·실제 처리량·고객 고지 기준을 통과했을 때.

| 미확정 항목 | 기본 처리 | 종료 시 확보할 자료 |
|:---|:---|:---|
| 국내 DSP별 계약/기술 연동 | Direct 운영 OFF | 계약서, 공식 기술 명세, partner UAT 결과 |
| Merlin/LIMBO | 미계약 경로 운영 OFF | 유효한 업스트림 계약, 실제 연동·정산·이관 명세 |
| ISRC/UPC 자체 발급 | 발급 기능 OFF | 권한/번호대/발급규칙·비용·중복 방지 검증 |
| 미성년자 동의·전자서명 | 상담되지 않은 예외 자동 통과 OFF | 변호사 검토한 동의자·서명/위임/친권 정책 버전 |
| 외부 OCR/AI | 민감 증빙의 외부 전송 OFF | 처리위탁·보관/학습·국외이전·ZDR 실제 계약 |
| 은행 자동 송금 | 자동 지급 OFF | 은행 API 계약·송금 상태 조회·중복금지 테스트 |
| Workers VPC beta | VPC 주경로; 중지/Access 대체 경로 | 현재 서비스 한도 점검·승인된 장애 훈련 결과 |

## 23.2 문서 간 유일한 구현 기준

**이 최종 보고서의 기존 1~13장은 v1.4 본문의 구현 계약을 유지하고, 14~23장은 실제 자체 배급·외부 연동·상업 출시를 위한 추가 설계다.** 14~23장이 앞의 안전 조건을 무효화하는 것으로 읽혀서는 안 된다. 이름 충돌은 §2.4의 단일 상태값, §10의 단일 Package MUST/MUST NOT, §6.1의 FreshnessGuard/return_to, §12.0의 운영 GO 기준을 우선한다. v1.4의 과거 수정 이력 부록은 참조용으로만 남기고 이 보고서에는 구현 규격으로 중복 수록하지 않는다.

## 23.3 기준 문서 및 외부 확인 자료

- AUDENIQ 백엔드 개발계획안 v1.4 본문(사용자가 검토한 기존 내부 설계·43개 수정항목 반영).
- AUDENIQ 통합 전항목 지적서(업로드 자료; 코딩·상용 운영 위험과 보류 조건).
- DDEX ERN 표준·프로파일·Choreography: https://kb.ddex.net/reference-material/standards-specifications/
- DDEX ERN 구현 안내·Implementation Licence: https://kb.ddex.net/implementing-each-standard/electronic-release-notification-message-suite-%28ern%29/
- DDEX DSR 및 프로파일 안내: https://kb.ddex.net/implementing-each-standard/digital-sales-reporting-message-suite-%28dsr%29/
- Workers VPC Service/베타: https://developers.cloudflare.com/workers-vpc/configuration/vpc-services/
- R2 Presigned URL·직접 업로드 보안: https://developers.cloudflare.com/r2/api/s3/presigned-urls/
- IFPI ISRC FAQ(재발매·리마스터): https://isrc.ifpi.org/faqs

**보고서 종결 상태:** 설계·구현 범위·실제 연동에 필요한 확인 항목을 정의한 **개발 기준서**이며, DSP 계약 체결·API 권한 확보·실제 상업 발매·실제 정산 및 운영 실측까지 완료했다는 뜻이 아니다. 각 항목은 증빙 링크가 있는 실행 티켓으로 관리한다.
