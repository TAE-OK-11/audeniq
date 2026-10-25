-- Run by schema owner after migration. Runtime logins are never superusers or table owners.
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA identity,catalog,operations TO audeniq_api;
GRANT SELECT ON ALL TABLES IN SCHEMA identity,catalog TO audeniq_api;
GRANT INSERT,UPDATE ON identity.orgs,identity.parties,identity.users,identity.memberships,identity.sessions,identity.auth_limits,identity.resources,identity.resource_acl TO audeniq_api;
GRANT INSERT,UPDATE ON catalog.artists,catalog.labels,catalog.releases,catalog.tracks,catalog.credits,catalog.assets,catalog.upload_sessions TO audeniq_api;
GRANT SELECT ON catalog.asset_fingerprints TO audeniq_api;
GRANT SELECT,INSERT ON catalog.asset_provenance TO audeniq_api;
GRANT SELECT,INSERT ON catalog.asset_safety TO audeniq_api;
GRANT SELECT,INSERT ON catalog.inline_file_safety TO audeniq_api;
REVOKE UPDATE,DELETE,TRUNCATE ON catalog.inline_file_safety FROM audeniq_api,audeniq_worker;
REVOKE UPDATE,DELETE,TRUNCATE ON catalog.asset_safety FROM audeniq_api,audeniq_worker;
GRANT DELETE ON catalog.credits TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_api;
GRANT INSERT ON operations.audit_events TO audeniq_api;
GRANT USAGE ON SCHEMA privacy TO audeniq_api;
GRANT INSERT ON privacy.staff_access_logs TO audeniq_api;
-- The access log insert is `ON CONFLICT(actor_user_id,request_id,route) DO
-- NOTHING`, which needs SELECT on the conflict columns. Only those three: the
-- API still cannot read who looked at what, or when.
GRANT SELECT(actor_user_id,request_id,route) ON privacy.staff_access_logs TO audeniq_api;
REVOKE UPDATE,DELETE,TRUNCATE ON privacy.staff_access_logs FROM audeniq_api;
-- 공지·이벤트·서버 점검(0086): 삭제는 deleted_at 기록뿐이라 DELETE는 주지 않는다.
GRANT USAGE ON SCHEMA content TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON content.notices,content.events,content.maintenance TO audeniq_api;
GRANT SELECT ON operations.allowed_transitions TO audeniq_api;
-- Consent + submit run in the API request: it signs the consent package and
-- freezes the submitted application revision (both append-only; no UPDATE or
-- DELETE is granted, and the tables' own triggers enforce immutability).
GRANT INSERT ON catalog.consent_packages,catalog.application_revisions TO audeniq_api;
-- Pre-submit / submission status read stage results and package summaries.
GRANT SELECT ON operations.check_results TO audeniq_api;
-- 업로드한 곡의 사전 심사 결과를 곡별로 보여 준다 (GET /assets/{id}/checks, 읽기만)
GRANT SELECT ON operations.asset_qc_results TO audeniq_api;
GRANT USAGE ON SCHEMA distribution TO audeniq_api;
GRANT SELECT ON distribution.validation_packages,distribution.verification_packages TO audeniq_api;
-- Review overrides (POST /reviews/overrides and the two-person approval
-- flow) run in the API request. review_overrides is append-only (immutable
-- trigger); override_requests only moves PENDING -> APPROVED/DECLINED (guard
-- trigger). The rights-epoch bump on override insert is SECURITY DEFINER.
GRANT USAGE ON SCHEMA rights TO audeniq_api;
GRANT SELECT,INSERT ON rights.review_overrides TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON rights.override_requests TO audeniq_api;
-- Studio portal (docs/API.md "Portal"): profile, payout account, inquiries,
-- notifications, documents, signed applications, payout requests. Staff
-- replies/approvals come from operations tooling, not this role. Finance is
-- read-only for the API (balances, statements, reports); payout requests are
-- turned into finance.payout_orders by operations.
GRANT USAGE ON SCHEMA portal TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON portal.artist_profiles,portal.payout_accounts,portal.inquiries,portal.documents TO audeniq_api;
GRANT SELECT,INSERT ON portal.inquiry_messages,portal.notification_reads,portal.release_applications,portal.payout_requests TO audeniq_api;
-- Payout-account consents, append-only (0127).
GRANT SELECT,INSERT ON portal.consent_records TO audeniq_api;
GRANT SELECT ON portal.notifications TO audeniq_api;
-- 알림 메일 (0112): 사용자별 수신 설정, 보낼 메일 대기열 (넣기는 트리거가, 보내기·결과 기록은 API가)
GRANT SELECT,INSERT,UPDATE ON portal.notification_settings TO audeniq_api;
GRANT SELECT,UPDATE,DELETE ON portal.notification_emails TO audeniq_api;
-- 권리 서류 서명 요청 (0071): 이벤트는 추가만
GRANT SELECT,INSERT,UPDATE ON portal.signing_requests TO audeniq_api;
GRANT SELECT,INSERT ON portal.signing_events TO audeniq_api;
-- 발매 중단 요청 (0111): 아티스트가 접수·취소하고, 배급 담당 스태프가 승인·반려한다
GRANT SELECT,INSERT,UPDATE ON portal.takedown_requests TO audeniq_api;
-- 권리침해 신고 (0114): 누구나 접수, 스태프 처리, 아티스트 소명
GRANT SELECT,INSERT,UPDATE ON portal.infringement_claims TO audeniq_api;
-- 플레이리스트 피칭 (0116): 아티스트 접수·철회, 스태프 진행 기록
GRANT SELECT,INSERT,UPDATE ON portal.playlist_pitches TO audeniq_api;
-- 스마트링크 (0117): 공개 발매 페이지와 일별 조회·클릭 수
GRANT SELECT,INSERT,UPDATE ON portal.smart_links, portal.smart_link_stats TO audeniq_api;
-- 플랫폼 요청 (0120): 아티스트 프로필·Content ID 요청과 처리 기록, 발매별 Content ID 상태
GRANT SELECT,INSERT,UPDATE ON portal.platform_requests, portal.content_id_status TO audeniq_api;
GRANT SELECT,INSERT ON portal.platform_request_events TO audeniq_api;
-- 계약 종료·권리 만료 (0121): 예약·취소·최종 정산 기록. 배송 작업자는 종료 여부만 함수로 본다
GRANT SELECT,INSERT,UPDATE ON portal.release_terms, portal.org_contract_terms TO audeniq_api;
-- 아티스트 플랫폼 페이지 (0122): 신청·아티스트·스태프가 기록하고 다음 신청·배송이 재사용
GRANT SELECT,INSERT,UPDATE ON catalog.artist_platform_pages TO audeniq_api;
-- 코드 관리 (codes.rs): 발급된 UPC·ISRC 목록 (RLS가 조직으로 제한)
GRANT SELECT ON distribution.identifier_assignments TO audeniq_api;
GRANT SELECT ON catalog.artist_platform_pages TO audeniq_worker;
-- 발매 알림 (0118): 팬 이메일은 봉인해 두고 보내면 바로 지운다. 끝난 줄은 정리
GRANT SELECT,INSERT,UPDATE,DELETE ON portal.release_alerts TO audeniq_api;
-- 아티스트 소개 페이지 (0151): 주소·소개·링크와 날마다 방문·클릭 수
GRANT SELECT,INSERT,UPDATE ON portal.artist_links, portal.artist_link_stats TO audeniq_api;
-- 프리세이브 (0152): 한 번 쓰는 시작 상태, 봉인한 플랫폼 토큰은 저장·실패·만료되면 바로 지운다
GRANT SELECT,INSERT,UPDATE,DELETE ON portal.presave_starts, portal.presaves TO audeniq_api;
-- 아티스트 페이지 소식·팬 응원 (0154): 아티스트가 고치고 지우며, 팬 응원은 숨기기·지우기
GRANT SELECT,INSERT,UPDATE,DELETE ON portal.artist_posts, portal.artist_cheers TO audeniq_api;
-- 아티스트 프로필 사진 (0155): 바꾸면 새 사진으로 덮어쓴다 (보낸 요청은 그때 사진을 가리킨다)
GRANT SELECT,INSERT,UPDATE ON portal.artist_photos TO audeniq_api;
-- 보도자료 (0156): 보도용 소개와 아티스트가 쓰는 보도자료 (지우기 있음)
GRANT SELECT,INSERT,UPDATE ON portal.artist_press TO audeniq_api;
GRANT SELECT,INSERT,UPDATE,DELETE ON portal.artist_press_releases TO audeniq_api;
-- 발매 후 정보 수정 (0115): 크레딧·장르만. 승인되면 수정 메시지 작업을 넣는다
GRANT SELECT,INSERT,UPDATE ON portal.metadata_corrections TO audeniq_api;
GRANT USAGE ON SCHEMA portal TO audeniq_worker;
GRANT SELECT ON portal.metadata_corrections TO audeniq_worker;
GRANT USAGE ON SEQUENCE portal.signing_events_id_seq TO audeniq_api;
-- 계정 정보 (0083): 본인인증 결과와 해외 납세 의무 (사용자 본인만)
GRANT SELECT,INSERT,UPDATE ON portal.account_identities,portal.tax_residencies TO audeniq_api;
-- 직원 실명인증 기록 (0136): 쌓기만 한다 (UPDATE·DELETE 없음)
GRANT SELECT,INSERT ON portal.offline_identity_checks TO audeniq_api;
GRANT EXECUTE ON FUNCTION portal.identity_desk_notice(uuid, text, text) TO audeniq_api;
-- 휴대폰 번호 등록·비밀번호 재설정 문자 인증 (0084)
GRANT SELECT,INSERT,UPDATE ON portal.account_phones,identity.sms_challenges TO audeniq_api;
-- 스태프 TOTP (0103): API가 등록·확인한다. 초기화는 owner CLI(audeniq-admin staff reset-mfa)만.
GRANT SELECT,INSERT,UPDATE ON identity.staff_mfa TO audeniq_api;
-- 패스키 (0104)
GRANT SELECT,INSERT,UPDATE,DELETE ON identity.passkeys,identity.webauthn_challenges TO audeniq_api;
-- Google·Apple 로그인 (0105)
GRANT SELECT,INSERT,UPDATE,DELETE ON identity.oauth_identities,identity.oauth_states TO audeniq_api;
-- AI 어시스턴트 연결 (MCP OAuth, 0131): 만료 코드·토큰은 API가 지운다
GRANT SELECT,INSERT,UPDATE ON identity.mcp_clients,identity.mcp_grants TO audeniq_api;
GRANT SELECT,INSERT,UPDATE,DELETE ON identity.mcp_codes,identity.mcp_tokens,identity.mcp_device_codes,identity.mcp_daily_usage TO audeniq_api;
GRANT SELECT ON identity.mcp_usage_limits TO audeniq_api;
GRANT SELECT,INSERT,UPDATE,DELETE ON identity.mcp_idempotency TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON identity.mcp_user_settings,identity.mcp_pending_submissions,identity.mcp_permission_requests,identity.mcp_uploads,identity.mcp_action_requests TO audeniq_api;
GRANT EXECUTE ON FUNCTION identity.mcp_notice(uuid, uuid, text, text, text, text) TO audeniq_api;
-- 출력물 식별 기록 (0107): 추가·조회만
GRANT SELECT,INSERT ON operations.print_stamps TO audeniq_api;
-- 문서 봉인·약관 동의 (0108): API가 서명·접수 트랜잭션 안에서 추가한다. 원본 보관·고정은 owner CLI만.
GRANT SELECT,INSERT ON operations.document_seals,identity.terms_acceptances TO audeniq_api;
GRANT USAGE ON SEQUENCE operations.document_seals_seq_seq TO audeniq_api;
-- 송금 실행 전 출금 취소 (0085): 상태 전이 하나만 하는 함수
GRANT EXECUTE ON FUNCTION finance.cancel_unsent_payout(uuid, uuid) TO audeniq_api;
-- 공동작업 수익 분배 (0119): 아티스트가 분배 비율·초대를 관리하고, 공동작업자가 수락·거절한다.
-- 보류 금액 이동은 원장 쓰기 권한 없이 이 함수 하나로만 한다. 정산 반영(posting)은 owner CLI.
GRANT SELECT,INSERT,UPDATE ON portal.split_shares TO audeniq_api;
GRANT SELECT ON finance.split_allocations, finance.royalty_postings TO audeniq_api;
GRANT EXECUTE ON FUNCTION finance.settle_split_share(uuid) TO audeniq_api;
GRANT USAGE ON SCHEMA finance TO audeniq_api;
GRANT SELECT ON finance.ledger_transactions,finance.ledger_entries,finance.payout_orders,finance.royalty_reports,finance.report_lines,finance.finance_holds TO audeniq_api;
-- Held (disputed) royalties are not withdrawable (0124).
GRANT EXECUTE ON FUNCTION finance.held_payable(uuid) TO audeniq_api,audeniq_worker;
-- One withdrawable balance for request, order, approval and submission (0128).
GRANT EXECUTE ON FUNCTION finance.withdrawable(uuid, uuid) TO audeniq_api,audeniq_worker;
-- DSP registry + delivery staging (0042/0043): artists read their release's
-- per-DSP status; staff (/api/staff, identity.staff_members) approve or hold
-- staged rows and read the DSP overview. Staging rows are written only by
-- the worker; the API may change the approval columns (RLS: app.org_id, or
-- app.staff after the staff role check).
GRANT SELECT ON distribution.dsp_registry TO audeniq_api;
GRANT SELECT,UPDATE ON distribution.delivery_staging TO audeniq_api;
GRANT SELECT ON distribution.distribution_packages TO audeniq_api;
GRANT USAGE ON SCHEMA execution TO audeniq_api;
GRANT SELECT ON execution.adapter_profiles,execution.delivery_jobs,execution.live_bindings TO audeniq_api;
-- Artist DSP availability and submit gate use the same contracted route verdict.
GRANT SELECT ON execution.route_coverage TO audeniq_api;
GRANT SELECT (org_id,dsp_id,route_kind,enabled,endpoint_id,contract_id,contract_revision_id) ON distribution.route_plans TO audeniq_api;
GRANT SELECT (org_id,dsp_id,id,integration_status) ON distribution.dsp_endpoints TO audeniq_api;
GRANT SELECT (id,org_id,contract_id,policy_version) ON rights.contract_revisions TO audeniq_api;
GRANT SELECT ON distribution.canonical_releases,distribution.identifier_issuers TO audeniq_api;
GRANT EXECUTE ON FUNCTION execution.partner_readiness(text) TO audeniq_api;
-- Signed partner webhooks (0053): the API only files them; the worker applies them.
GRANT INSERT ON execution.partner_inbox TO audeniq_api;
GRANT EXECUTE ON FUNCTION execution.platform_contract_live(text) TO audeniq_api;
-- Staff ADMIN chooses each DSP's contract route (direct / Merlin, 0056).
GRANT SELECT, UPDATE (route, updated_by, updated_at) ON distribution.dsp_contract_routes TO audeniq_api;
-- Approved rights documents become grants (0126).
GRANT SELECT,INSERT ON rights.grant_atoms TO audeniq_api;
-- Staff review (0044): second-person approvals and reviewer notes. The
-- staff role table itself is read-only for the API (granted by the CLI).
GRANT SELECT,INSERT,UPDATE ON rights.staff_approvals TO audeniq_api;
GRANT SELECT,INSERT ON rights.review_notes TO audeniq_api;
GRANT SELECT,INSERT,UPDATE,DELETE ON rights.review_claims TO audeniq_api;
GRANT SELECT ON operations.audit_events TO audeniq_api;
-- 0064: the staff release timeline shows DSP send attempts and ACKs (read-only).
GRANT SELECT ON execution.delivery_attempts,execution.ack_events TO audeniq_api;
-- Distribution pipeline schemas (F2/F4/F5/F7). The worker runs the job
-- queues; the API never writes here (the roles test asserts 42501 for api
-- inserts into distribution). The reconciler enumerates identity.orgs,
-- which carries no RLS (tenant isolation there is by grant, not policy).
GRANT USAGE ON SCHEMA distribution,finance,execution,rights,identity,catalog TO audeniq_worker;
GRANT SELECT ON ALL TABLES IN SCHEMA distribution,finance,execution,rights TO audeniq_worker;
GRANT SELECT ON catalog.application_revisions,catalog.artists,catalog.labels,catalog.tracks,catalog.credits,catalog.assets,catalog.consent_packages,catalog.upload_sessions TO audeniq_worker;
GRANT SELECT ON catalog.asset_provenance TO audeniq_worker;
GRANT SELECT ON catalog.asset_safety TO audeniq_worker;
GRANT SELECT ON catalog.external_recordings,catalog.external_recording_epoch TO audeniq_worker;
GRANT SELECT ON identity.orgs,identity.memberships,identity.parties TO audeniq_worker;
-- Stage 1 re-checks protected artist names (list is operator-managed; no runtime writes).
GRANT SELECT ON catalog.protected_artists,catalog.protected_artist_aliases,catalog.protected_artist_exceptions TO audeniq_worker;
-- Worker writes: stage transitions, frozen artifacts, delivery state,
-- finance ledger, review overrides/epochs.
GRANT SELECT,UPDATE ON catalog.releases,catalog.assets TO audeniq_worker;
GRANT SELECT,INSERT ON catalog.asset_fingerprints TO audeniq_worker;
-- Cross-org similarity (REVIEW only) reads other orgs' fingerprints via one
-- narrow SECURITY DEFINER function; the table itself stays org-scoped.
GRANT EXECUTE ON FUNCTION catalog.fingerprints_outside_org(uuid, smallint) TO audeniq_worker;
GRANT EXECUTE ON FUNCTION catalog.fingerprints_outside_org_page(uuid, smallint, uuid, integer) TO audeniq_worker;
GRANT EXECUTE ON FUNCTION catalog.fingerprints_outside_org_since(uuid, smallint, timestamptz, timestamptz, uuid, integer) TO audeniq_worker;
-- 0109: incremental similarity scan state (other orgs' asset ids among the matches: worker only).
GRANT SELECT,INSERT,UPDATE ON operations.fingerprint_scans TO audeniq_worker;
GRANT EXECUTE ON FUNCTION catalog.fingerprints_still_present(smallint, uuid[]) TO audeniq_worker;
-- E-0 holds delivery until the release's agreement is signed (portal stays ungranted).
GRANT EXECUTE ON FUNCTION execution.agreement_signed(uuid, uuid) TO audeniq_worker;
-- Contract scope of an approved/signed agreement (0125).
GRANT EXECUTE ON FUNCTION execution.agreement_scope(uuid, uuid) TO audeniq_worker,audeniq_api;
GRANT EXECUTE ON FUNCTION portal.release_term_ended(uuid, uuid) TO audeniq_worker;
-- 0063: a delivery job is only leased while staff approval of its route holds.
GRANT EXECUTE ON FUNCTION execution.delivery_gate_open(uuid, text) TO audeniq_worker;
GRANT INSERT ON catalog.application_revisions,catalog.consent_packages TO audeniq_worker;
GRANT INSERT ON distribution.canonical_releases,distribution.distribution_packages,distribution.verification_packages,distribution.validation_packages,distribution.preparation_artifacts,distribution.identifier_assignments,distribution.ddex_messages TO audeniq_worker;
-- Stage 3 issues missing UPC/ISRC codes (migration 0041): the worker reads the
-- active issuer (SELECT via the schema-wide grant) and advances its counter.
GRANT INSERT,UPDATE ON distribution.identifier_counters TO audeniq_worker;
-- 0046: a VIRTUAL code may be retired (trigger-guarded, one way) when the
-- registered range replaces it.
GRANT UPDATE(status,retired_at) ON distribution.identifier_assignments TO audeniq_worker;
GRANT INSERT,UPDATE ON execution.delivery_jobs,execution.delivery_attempts,execution.live_bindings,execution.reconciliation_cases TO audeniq_worker;
GRANT INSERT,UPDATE ON execution.route_decisions TO audeniq_worker;
-- Partner ACK dedupe, webhook inbox and event-org resolution (0053).
GRANT SELECT,INSERT ON execution.ack_events TO audeniq_worker;
GRANT SELECT,UPDATE ON execution.partner_inbox TO audeniq_worker;
GRANT EXECUTE ON FUNCTION execution.partner_event_org(text, text, text) TO audeniq_worker;
-- Distributor-level DSP contract verdict (0054) for routing and Stage 2.
GRANT EXECUTE ON FUNCTION execution.platform_contract_live(text) TO audeniq_worker;
-- Reconciler scope: only orgs with open delivery work (0055).
GRANT EXECUTE ON FUNCTION execution.orgs_with_open_deliveries() TO audeniq_worker;
GRANT INSERT,UPDATE ON distribution.delivery_staging TO audeniq_worker;
GRANT SELECT,INSERT ON operations.check_results TO audeniq_worker;
GRANT SELECT,INSERT ON operations.asset_qc_results TO audeniq_worker;
GRANT SELECT ON operations.allowed_transitions TO audeniq_worker;
GRANT INSERT ON rights.review_overrides,rights.rights_epochs TO audeniq_worker;
-- Row locks need UPDATE; the worker share-locks the epoch only through 0088.
GRANT EXECUTE ON FUNCTION rights.share_lock_epoch(uuid, uuid) TO audeniq_worker;
GRANT INSERT ON finance.ledger_transactions,finance.ledger_entries,finance.commercial_split_snapshots TO audeniq_worker;
GRANT SELECT,INSERT,UPDATE ON finance.payout_orders,finance.finance_holds TO audeniq_worker;
-- Stage packages, canonical releases and delivery evidence cannot be written by the Foundation API.
GRANT USAGE ON SCHEMA operations TO audeniq_worker;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_worker;
GRANT SELECT,INSERT ON operations.event_receipts TO audeniq_worker;
GRANT INSERT ON operations.audit_events TO audeniq_worker;
REVOKE UPDATE,DELETE,TRUNCATE ON operations.audit_events FROM audeniq_api,audeniq_worker;
-- Internal work module (0076, /api/staff/work/*): tasks, events, links, notes,
-- revisions and attachments are archived, never deleted. Same grants as the
-- migration gives when the role already exists, so a role created later is
-- covered too.
GRANT USAGE ON SCHEMA work TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON ALL TABLES IN SCHEMA work TO audeniq_api;
GRANT USAGE,SELECT ON SEQUENCE work.task_seq TO audeniq_api;
-- Phase 2 (0078): holidays are calendar settings, removed by ADMIN (audited).
-- Staff read delivery results across workspaces (RLS app.staff policy) for the
-- exception queue; reconciliation cases are read-only for the API.
GRANT DELETE ON work.calendar_holidays TO audeniq_api;
-- Phase 3 (0079): contact history is append-only (trigger too).
REVOKE UPDATE ON work.relationship_activities FROM audeniq_api;
GRANT SELECT ON execution.reconciliation_cases,execution.delivery_attempts,execution.ack_events TO audeniq_api;
-- Kanban + Docs (0081): per-document grants are replaced on share (DELETE);
-- version snapshots are immutable (trigger too); access is work.doc_level.
GRANT DELETE ON work.doc_permissions TO audeniq_api;
REVOKE UPDATE ON work.doc_versions FROM audeniq_api;
GRANT EXECUTE ON FUNCTION work.doc_level(uuid,uuid,text) TO audeniq_api;
-- Runtime timeouts as role defaults: they apply to every server connection,
-- including the ones PgBouncer opens in transaction pooling mode, where the
-- per-connection SET in database::connect cannot survive.
-- Needs superuser or CREATEROLE + ADMIN on the roles (the compose owner is
-- the image superuser); elsewhere it warns instead of aborting the grants.
-- Roles are cluster-wide, so runs against different databases (parallel test
-- binaries) can race on the same pg_db_role_setting row: ALTER ROLE then fails
-- with "tuple concurrently updated" instead of waiting. Settings already in
-- place are skipped, and a lost race is retried after the other run commits.
DO $$
DECLARE r text; s text; attempt int;
BEGIN
 FOREACH r IN ARRAY ARRAY['audeniq_api','audeniq_worker'] LOOP
  FOREACH s IN ARRAY ARRAY['statement_timeout=15s','lock_timeout=3s','idle_in_transaction_session_timeout=30s'] LOOP
   FOR attempt IN 1..20 LOOP
    BEGIN
     IF NOT EXISTS (SELECT 1 FROM pg_db_role_setting d JOIN pg_roles o ON o.oid = d.setrole
                    WHERE d.setdatabase = 0 AND o.rolname = r AND s = ANY(d.setconfig)) THEN
      EXECUTE format('ALTER ROLE %I SET %I = %L', r, split_part(s, '=', 1), split_part(s, '=', 2));
     END IF;
     EXIT;
    EXCEPTION
     WHEN insufficient_privilege THEN
      RAISE WARNING 'cannot set % on role % (run as superuser): required behind PgBouncer', s, r;
      EXIT;
     WHEN internal_error THEN
      IF SQLERRM <> 'tuple concurrently updated' OR attempt = 20 THEN RAISE; END IF;
      PERFORM pg_sleep(0.05 * attempt);
    END;
   END LOOP;
  END LOOP;
 END LOOP;
END $$;

-- No ambient CONNECT or temporary-table privileges for arbitrary login roles.
-- Owner/migrator remains separate; the two runtime logins get CONNECT only.
DO $$ BEGIN
 EXECUTE format('REVOKE CONNECT,TEMPORARY ON DATABASE %I FROM PUBLIC', current_database());
 EXECUTE format('GRANT CONNECT ON DATABASE %I TO audeniq_api,audeniq_worker', current_database());
END $$;

-- Staff operations login (0146, crate::staff_ops): the admin console runs the
-- operator CLI's day-to-day commands with it. Optional: nothing here applies
-- until the role exists (docs/STAFF_LEVELS.md). It reads what those commands
-- read and writes only the tables they change; staff rows and authenticators
-- change only through identity.staff_set / identity.staff_reset_mfa, which
-- re-check that the actor is an OWNER.
DO $$ BEGIN
IF EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_staff_ops') THEN
  GRANT USAGE ON SCHEMA identity,catalog,operations,execution,distribution,finance,portal TO audeniq_staff_ops;
  GRANT SELECT ON ALL TABLES IN SCHEMA identity,catalog,execution,distribution,finance,portal TO audeniq_staff_ops;
  GRANT INSERT ON operations.audit_events TO audeniq_staff_ops;
  -- protected names, AI agents and limits, developer app review
  GRANT INSERT,UPDATE ON catalog.protected_artists,catalog.protected_artist_exceptions TO audeniq_staff_ops;
  GRANT INSERT,UPDATE,DELETE ON catalog.protected_artist_aliases TO audeniq_staff_ops;
  GRANT INSERT ON catalog.protected_artist_changes TO audeniq_staff_ops;
  GRANT INSERT,UPDATE ON identity.mcp_clients TO audeniq_staff_ops;
  GRANT UPDATE ON identity.mcp_grants TO audeniq_staff_ops;
  GRANT DELETE ON identity.mcp_tokens TO audeniq_staff_ops;
  GRANT INSERT,UPDATE,DELETE ON identity.mcp_usage_limits TO audeniq_staff_ops;
  -- partner onboarding and identifier ranges
  GRANT INSERT,UPDATE ON execution.partner_onboarding TO audeniq_staff_ops;
  GRANT UPDATE ON execution.adapter_profiles,distribution.dsp_contract_routes TO audeniq_staff_ops;
  GRANT INSERT,UPDATE ON distribution.identifier_issuers TO audeniq_staff_ops;
  GRANT INSERT ON distribution.identifier_counters TO audeniq_staff_ops;
  GRANT EXECUTE ON FUNCTION execution.partner_onboarding_gaps(text), execution.platform_contract_live(text) TO audeniq_staff_ops;
  GRANT EXECUTE ON FUNCTION finance.withdrawable(uuid,uuid), finance.settle_split_share(uuid) TO audeniq_staff_ops;
  -- payouts and royalty reports
  GRANT INSERT,UPDATE ON finance.payout_orders,finance.finance_holds,finance.royalty_reports,finance.report_lines TO audeniq_staff_ops;
  GRANT INSERT ON finance.ledger_transactions,finance.ledger_entries,finance.payout_returns,finance.royalty_receipts,
    finance.royalty_match_candidates,finance.royalty_postings,finance.split_allocations TO audeniq_staff_ops;
  GRANT UPDATE ON portal.payout_requests TO audeniq_staff_ops;
  GRANT EXECUTE ON FUNCTION identity.staff_set(uuid,uuid,text,text), identity.staff_reset_mfa(uuid,uuid) TO audeniq_staff_ops;
END IF;
END $$;
