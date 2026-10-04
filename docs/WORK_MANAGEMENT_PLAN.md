# AUDENIQ 내부 업무 SaaS — 기존 구현 분석과 단계별 구현 계획

작성: 2026-10-04 · 브랜치 `feat/admin-work-saas` (아직 `main`에 병합 전)

이 문서는 코드를 바꾸기 전에 저장소 전체를 조사한 결과다. 원칙은 세 가지다.

1. 이미 있는 기능은 새로 만들지 않고 **adapter(읽기 연결)** 로 업무 화면에 노출한다.
2. 같은 정보를 다른 테이블에 **복사 저장하지 않는다**. 새 테이블에는 "새로 생기는 정보"(담당자·메모·처리 상태·SLA 설정 등)만 둔다.
3. 기존 Release/QC/Rights/Distribution/Settlement 코드는 가능하면 건드리지 않는다. 필요한 경우 권한 정책(RLS 읽기 허용)이나 기존 함수 호출만 추가한다.

> 이 브랜치의 이전 커밋(5f873fa, 7bbf7a8)에서 만든 1차 구현에는 아래 "중복"으로 표시한 문제가 있었다.
> `main`에 병합되거나 배포된 적이 없으므로 마이그레이션 `0076`을 이 계획에 맞게 다시 쓴다.

---

## 1. 이미 구현된 기능 (조사 결과)

### 인증·권한·감사
| 항목 | 위치 | 내용 |
|---|---|---|
| 세션·CSRF·Origin·서비스 시크릿 | `crates/core/src/auth.rs`, `api.rs` `boundary` | 모든 API 공통 |
| 스태프 권한 | `identity.staff_members`, `staff.rs` `StaffRole`/`Duty`/`staff()`/`require()` | ADMIN·REVIEWER·OPERATOR·SUPPORT, 조회는 모든 역할, 쓰기는 Duty별 |
| 스태프 접근 기록 | `privacy.staff_access_logs` | `staff()`가 모든 요청을 기록 (실패 시 차단) |
| 감사 로그 | `operations.audit_events` + `operations::audit()` | actor_user / actor_service, resource_id, action, reason_code, `occurred_at` |
| 조직 ACL | `identity.memberships`, `identity.resources`, `identity.resource_acl` | 아티스트(고객) 측 리소스 권한. 스태프는 조직을 가로질러 조회 |
| RLS | `delivery_staging`(app.staff 허용), `execution.*`(org 범위), `asset_fingerprints`, `identifier_assignments` | 스태프 교차 조회는 `app.staff='on'` 정책이 있어야 함 |

### 업무 흐름 (스태프 포털 `/api/staff/*`, 관리자 화면 `/admin/*`)
| 기능 | 테이블 / 코드 | 업무 SaaS에서의 의미 |
|---|---|---|
| 발매 심사 대기열·결정 | `catalog.releases`, `operations.check_results`, `rights.review_overrides`, `rights.review_notes`(결정 메모) | 검토 대상 |
| **심사 담당(claim)** | `rights.review_claims` | 발매 심사의 "담당자" — 이미 있음 |
| **2차 승인** | `rights.staff_approvals` | "Approval Request" — 이미 있음 |
| 서류 검토 | `portal.documents` (AGREEMENT=배급 계약서, RIGHTS_PROOF=권리 증빙) | 계약 상태·권리 서류 |
| 문의 답변 | `portal.inquiries`, `portal.inquiry_messages` | Support (고객 문의) |
| 배급 현황·보류·재검사 | `distribution.delivery_staging`, `staff::restage` | Distribution |
| DSP 사양·경로 | `distribution.dsp_registry`, `dsp_contract_routes`, `staff::dsps` | DSP 기본 정보 |
| DSP 연동·계약 | `execution.adapter_profiles`(transport, delivery_enabled), `execution.partner_onboarding`(stage, endpoint_health, credential_status, **contract_signed_at/contract_ref**) | DSP Operations의 계약·연동 상태 — 이미 있음 |
| 전송 결과 | `execution.delivery_jobs`, `delivery_attempts`(response), `ack_events` | 실패율·승인 시간 계산 원천 |
| **전송 이상 케이스** | `execution.reconciliation_cases` (MISSING_ACK, SENT_UNKNOWN, OVERDUE_LIVE, PARTNER_REJECTED) | Exception 원천 — 이미 있음 |
| 파이프라인 작업 | `operations.jobs` (DEAD_LETTER), `operations::surface_dead_letter` | Automation Failure 원천 |
| 정산 | `finance.royalty_reports`, `finance.report_lines`(match_status UNMATCHED), `royalty_match_candidates`, `finance_holds` | Settlement Mismatch 원천 |
| 지급 요청 | `portal.payout_requests` | 지급 |
| 거래처 원천 | `identity.orgs`(작업 공간), `catalog.artists`, `catalog.labels`, DSP registry | CRM은 여기에 연결만 |
| 고객 알림 | `portal.notifications` | **고객에게 보임 → 내부 멘션에 쓰면 안 됨** |
| 객체 저장소 | `storage::ObjectStore` (presign_put, head, freeze, get), `upload_safety::scan` | 첨부파일 저장에 재사용 |

### 프론트엔드 (스튜디오 `/admin`)
- 공통 조각: `admin/ui.tsx`(Chip, PageHead, Empty, Skeleton, ErrorBox, Filters, Section, SubTabs, Seg, CountUp, useStaff), `components/{Modal,Confirm,Toast,Glyph,Check}`, `aq-switch`, `styles/admin.css`.
- 화면: 오늘의 업무, 발매 심사·상세, 2차 승인, 서류, 문의, 배급 현황, 플랫폼별 조건, 지급 요청, 공지·이벤트.

## 2. 중복 점검 — 1차 구현에서 고칠 것

| 1차 구현 | 중복된 기존 정보 | 이번 설계 |
|---|---|---|
| `work.tasks.related` jsonb에 `label`(제목) 저장 | 발매 제목·조직명 등 원천 테이블 | `work.links`(종류+ID만 저장), 이름은 읽을 때 원천에서 조회 |
| `work.exceptions` 행 복사 + 트리거 4개 | `reconciliation_cases`, `delivery_jobs`, `delivery_staging`, `portal.documents`, `operations.jobs` | **원천 adapter로 읽기**, `work.exception_triage`에는 담당자·처리 결과·재시도 기록만 |
| `work.contacts.name/company/email` (레이블·아티스트도) | `identity.orgs`, `catalog.artists`, `catalog.labels`, DSP registry | `work.relationships`는 원천 엔티티를 가리키고 담당·관계 상태·다음 연락만. 이름은 원천에 없을 때(외부 파트너)만 저장 |
| `work.contacts.last_contact_at` | 연락 기록에서 계산 가능 | 저장하지 않고 `max(activity.at)` |
| `work.contacts.contract_stage` | `portal.documents` AGREEMENT, `partner_onboarding.contract_*` | 원천에서 계산. 원천이 없는 외부 파트너만 수동 값 |
| `work.dsp_ops.contract_stage/health` | `partner_onboarding` stage·endpoint_health·contract_* | 제거. 담당자·연락처·메모는 CRM 관계(DSP)로, SLA는 SLA 정책의 DSP 범위로 |
| 첨부파일 bytea + 조각 업로드 | `ObjectStore` presign 업로드 | 객체 저장소 presign 업로드 + 바이러스 검사 재사용 |
| 문의·2차 승인 등을 업무함 전용 데이터로 처리 | 기존 테이블 | 업무함은 기존 테이블을 adapter로 읽음 (변경 없음) |

## 3. 재사용 가능한 코드
- 백엔드: `staff::staff()`(인증+역할+접근 기록), `staff::require()`/`Duty`, `operations::audit()`, `text_policy::{check, check_multiline}`, `storage::ObjectStore`, `upload_safety::scan`, `staff::restage()`(배급 재검사 = 전송 실패 재시도), 테스트 지원 `tests/support`.
- 프론트엔드: 위 공통 조각 전부, 1차 구현의 `admin/work/*` 화면(데이터 모양만 새 API에 맞춤), `sla.ts` 계산 함수(테스트 포함).

## 4. 새로 구현할 기능과 DB 설계 (마이그레이션 `0076_work_management.sql`, 스키마 `work`)

### Phase 1 — Task Engine · Internal Notes · Mentions
```
work.tasks          id, seq(WRK-n), parent_id(→subtask), title, description, work_type, status, priority,
                    priority_raised, assignee_id, reviewer_id, due_at, sla_due_at, escalation_level,
                    changes_requested, recurrence jsonb, tags, created_by, created_at, updated_at,
                    assigned_at, started_at, completed_at, archived_at, archived_by, row_version
work.task_events    task_id, actor_id, action, detail, at        -- 업무 화면용 이력(감사는 audit_events)
work.links          id, subject_kind('task'|'relationship'), subject_id, entity_kind, entity_id,
                    created_by, created_at, archived_at           -- 공통 Entity 연결 (이름 저장 안 함)
work.notes          id, target_kind, target_id, author_id, body, mentions uuid[], resolved/by/at,
                    edited_at, archived_at, created_at
work.note_revisions note_id, body, mentions, edited_by, edited_at  -- 수정 이력
work.attachments    id, note_id, name, content_type, size, sha256, object_key, state(ISSUED|READY),
                    antivirus_status, uploaded_by, created_at, archived_at
```
- Entity 종류: `release, org, artist, label, dsp, inquiry, document(계약서·권리 서류), payout, settlement_report, task, relationship`.
  `work::entities` resolver가 존재 확인과 표시 이름 조회를 원천 테이블에서 한다 (특정 업무 유형에 종속 없음).
- 삭제 없음: 업무·메모·연결·첨부 모두 `archived_at`.
- 감사: `work.task_created/updated/archived`, `work.link_added/removed`, `work.note_added/edited/resolved/archived`, `work.attachment_*`.

### Phase 2 — Unified Work Inbox · SLA Engine · Exception Queue
```
work.calendars          id, name, timezone, work_start, work_end, workdays int[]   -- 영업일 확장 구조
work.calendar_holidays  calendar_id, day, name
work.sla_policies       id, work_type, scope_kind/scope_id(예: dsp D-5 별도 SLA), target_minutes,
                        calendar_id(NULL=24시간), warn_ratio, idle_alert_minutes, escalate_to, auto_raise, enabled, archived_at
work.inbox_reads        user_id, item_id, read_at
work.exception_triage   source, source_id(원천 키), assignee_id, status(OPEN|ACKNOWLEDGED|RESOLVED|IGNORED),
                        resolution, resolved_at/by, retry_count, last_retry_at, task_id
work.exception_reports  수동 보고(정산 불일치 등 원천이 없는 경우만)
```
- Work type: 업무 유형(긴급 송출·권리 검토·계약·DSP 문의·정산·법무·고객 문의·일반) + 기존 대기열(`RELEASE_REVIEW`, `SECOND_APPROVAL`, `DOCUMENT_REVIEW`, `SUPPORT_INQUIRY`, `EXCEPTION`)에도 SLA 적용. 기존 대기열의 시작 시각은 원천(`created_at`)에서 계산하므로 저장하지 않는다.
- Exception 원천 adapter (읽기 전용, 원천 상태가 해결되면 자동으로 사라짐):
  | 종류 | 원천 | 재시도 |
  |---|---|---|
  | QC Failure | `check_results` BLOCKED/TECHNICAL_RETRY (현재 revision) | – |
  | Metadata Conflict | `delivery_staging` CONTENT_BLOCKED | 배급 재검사(`staff::restage`) |
  | Missing Rights Document | `portal.documents` RIGHTS_PROOF NEEDS/AWAITING 3일 초과, `releases` ON_HOLD_RIGHTS | – |
  | DSP Rejection | `reconciliation_cases` PARTNER_REJECTED, `ack_events` REJECTED | 배급 재검사 |
  | Distribution Failure | `delivery_jobs` FAILED/DEAD_LETTER, `reconciliation_cases` 기타 | 배급 재검사 |
  | Settlement Mismatch | `finance.report_lines` UNMATCHED(리포트 단위), 수동 보고 | – |
  | Automation Failure | `operations.jobs` DEAD_LETTER | 작업 재대기(감사 기록) |
- 업무함 항목: Assigned Tasks, Review Requests(업무 검토 + 내가 맡은 발매 심사), Approval Requests(2차 승인), Mentions, Failed Automations, SLA Warning, Overdue, Support Escalation(SLA 넘긴 문의), Rights Issue, Distribution Failure. 필터: 담당자·업무 유형·우선순위·상태·기한·Entity 종류.
- 처리시간 측정: 업무는 이벤트로, 기존 대기열은 audit_events로.
- RLS: `reconciliation_cases`에 스태프 읽기 정책 추가 (기존 `delivery_staging` 방식과 동일).
- 구현 메모: 상향(escalation)은 스케줄러 없이 스태프 읽기(업무함·요약·SLA 화면)에서 멱등으로 처리한다. 업무함에 `CONTACT_DUE`(관계 관리 다음 연락)를 Phase 3에서 더했다. API 역할 권한은 `deploy/grants.sql`이 기준이며, 분할 역할 테스트가 모든 Phase 2·3 엔드포인트를 production 권한으로 호출한다.

### Phase 3 — CRM Lite · DSP Operations
```
work.relationships        id, kind(ARTIST|LABEL|DSP|PARTNER|ORG), entity_kind/entity_id(원천), name(원천 없을 때만),
                          owner_id, status(PROSPECT|ACTIVE|AT_RISK|INACTIVE), next_contact_at,
                          manual_contract_status(원천 없을 때만), archived_at
work.relationship_contacts relationship_id, name, role, email, phone, archived_at   -- 사람 연락처(새 정보)
work.relationship_activities relationship_id, kind(CALL|EMAIL|MEETING|NOTE), summary, by_user, at
```
- 마지막 연락일 = 활동 기록 최댓값, 계약 상태 = 원천(AGREEMENT 문서·partner_onboarding), 관련 Release/Contract = `work.links`, 메모 = `work.notes`.
- DSP Operations는 새 테이블 없이 읽기: registry + adapter_profiles + partner_onboarding + dsp_contract_routes + 전송 지표 + DSP 관계(담당·연락처·최근 연락·메모) + DSP 범위 SLA + 최근 장애(Exception adapter).

### Phase 4 — Operations Analytics
- 새 테이블 없음. `audit_events`, `work.task_events`, `check_results`, `delivery_jobs/ack_events`, exception adapter로 계산.
- 지표: 일별 처리량, 사용자별 처리량, 평균 처리시간, 단계별 체류시간, Approval/Rejection rate, Automation rate, Manual review rate, Exception rate, DSP별 실패율, SLA 초과율, 최대 병목.

## 5. 권한 (기존 staff 역할 확장)
| 작업 | 허용 |
|---|---|
| 모든 조회 | 모든 스태프 (기존 규칙과 동일) |
| 업무 생성·수정·메모·연결 | 모든 스태프 (단, 검토 승인은 검토자 또는 ADMIN) |
| 업무 보관(archive) | 만든 사람·담당자 또는 ADMIN |
| 메모 수정 | 작성자만 / 메모 보관: 작성자 또는 ADMIN |
| SLA 정책·업무 캘린더 | ADMIN |
| Exception 처리·재시도 | 원천의 Duty: 배급=DELIVERY, 권리·QC=REVIEW/DOCUMENTS, 문의=INQUIRIES, 정산·자동화=ADMIN |
| CRM 보관 | ADMIN / 그 외 CRM 편집은 모든 스태프 |

`staff.rs`에는 `Duty` 판정만 재사용하고 새 권한 판정은 `work.rs`의 `permit()` 한 곳에 모은다.

## 6. API (`/api/staff/work/*`)
| Phase | Endpoint |
|---|---|
| 1 | `GET /people` · `GET/POST /tasks` · `GET/POST /tasks/{id}` · `POST /tasks/{id}/archive` · `POST /tasks/{id}/links` · `POST /links/{id}/archive` · `GET /entities/search?kind=&q=` · `GET/POST /notes` · `POST /notes/{id}` (수정) · `GET /notes/{id}/revisions` · `POST /notes/{id}/resolve` · `POST /notes/{id}/archive` · `GET /mentions` · `POST /attachments` (presign) · `POST /attachments/{id}/complete` · `GET /attachments/{id}` |
| 2 | `GET /summary` · `GET /inbox` · `POST /inbox/read` · `GET/PUT /sla` · `GET/PUT /calendars` · `GET /exceptions` · `POST /exceptions/{source}/{id}` (assign/ack/resolve/ignore/retry/to_task) · `POST /exceptions/reports` |
| 3 | `GET/POST /relationships` · `GET/POST /relationships/{id}` · contacts·activities · `GET /dsp-ops` |
| 4 | `GET /analytics?days=` |

## 7. 프론트엔드 (스튜디오 `/admin`, 기존 adm-* 요소 재사용)
| Phase | 화면·컴포넌트 |
|---|---|
| 1 | `work/pages/Tasks`(목록·보드·필터), `TaskDetail`(하위 업무·연결·메모·이력), `TaskForm`, `EntityPicker`, `NotesPanel`(멘션·첨부·수정 이력·해결·보관), 발매 심사·서류·문의 화면에 메모/업무 연결, 오늘의 업무에 "나를 언급한 메모" |
| 2 | `Inbox`, `Sla`(정책·캘린더·기한 현황), `Exceptions` |
| 3 | `Crm`, `DspOps`(배급 메뉴 하위 탭) |
| 4 | `Analytics` |

## 8. 예상 변경 파일
- 새로: `migrations/0076_work_management.sql`(Phase마다 같은 브랜치에서 보강, 병합 전), `crates/core/src/work.rs` → Phase 2부터 `work/{mod,tasks,notes,entities,inbox,sla,exceptions,crm,analytics}.rs`로 분리, `crates/core/tests/work*.rs`, `web/studio/app/src/admin/work/**`, `web/studio/app/src/styles/work.css`.
- 최소 변경: `crates/core/src/lib.rs`, `api.rs`(라우트 merge), `docs/API.md`, `admin/AdminApp.tsx`(메뉴·라우트), `admin/ui.tsx`(StaffContext), `components/Glyph.tsx`(아이콘), `admin/pages/{ReviewDetail,Overview,Deliveries,Dsps}.tsx`(메모·연결·탭).
- 변경하지 않음: Release/QC/Rights/Distribution/Settlement 처리 코드. (Phase 2에 `reconciliation_cases` 스태프 읽기 RLS 정책만 추가)

## 9. Phase 종료 시 확인
`cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test`(work + 영향 스위트, 기준선 대비 회귀 비교), 빈 DB에 전체 마이그레이션 적용, 프론트 `tsc`·`vitest`·빌드, 체험 모드 화면 확인.

## 10. 진행 상황
| Phase | 상태 | 확인 |
|---|---|---|
| 0 분석·설계 | 완료 | 이 문서 |
| 1 Task Engine · Internal Notes · Mentions | 완료 | `tests/work.rs` 2개(업무·하위 업무·검토·반복·연결·보관 / 메모·멘션·수정 이력·첨부), clippy·fmt, 빈 DB 마이그레이션, 기준선 대비 회귀 없음, 프론트 tsc·vitest·빌드 |
| 2 Inbox · SLA · Exception Queue | 완료 | `tests/work_phase2.rs`(SLA 정책·휴일·업무 SLA·상향·예외 원천·처리·업무 전환·업무함·읽음·요약), `sla.rs` 영업시간 단위 테스트, 분할 역할(`grants.sql`) 회귀 `sandbox_regressions::staff_endpoints_work_under_split_roles`, 빈 DB 마이그레이션, 프론트 tsc·vitest·빌드·화면 확인 |
| 3 CRM Lite · DSP Operations | 완료 | `tests/work_phase3.rs`(원천 이름·계약 상태, 연락처·이력(추가 전용)·연결·메모·업무, 수동 파트너, 연락 예정 업무함, DSP 운영, 보관 권한), 분할 역할 회귀, 프론트 확인 |
| 4 Operations Analytics | 예정 | |
