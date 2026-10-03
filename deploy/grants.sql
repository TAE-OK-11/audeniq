-- Run by schema owner after migration. Runtime logins are never superusers or table owners.
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA identity,catalog,operations TO audeniq_api;
GRANT SELECT ON ALL TABLES IN SCHEMA identity,catalog TO audeniq_api;
GRANT INSERT,UPDATE ON identity.orgs,identity.parties,identity.users,identity.memberships,identity.sessions,identity.auth_limits,identity.resources,identity.resource_acl TO audeniq_api;
GRANT INSERT,UPDATE ON catalog.artists,catalog.labels,catalog.releases,catalog.tracks,catalog.credits,catalog.assets,catalog.upload_sessions TO audeniq_api;
GRANT SELECT ON catalog.asset_fingerprints TO audeniq_api;
GRANT SELECT,INSERT ON catalog.asset_provenance TO audeniq_api;
GRANT SELECT,INSERT ON catalog.asset_safety TO audeniq_api;
REVOKE UPDATE,DELETE,TRUNCATE ON catalog.asset_safety FROM audeniq_api,audeniq_worker;
GRANT DELETE ON catalog.credits TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_api;
GRANT INSERT ON operations.audit_events TO audeniq_api;
GRANT USAGE ON SCHEMA privacy TO audeniq_api;
GRANT INSERT ON privacy.staff_access_logs TO audeniq_api;
REVOKE UPDATE,DELETE,TRUNCATE ON privacy.staff_access_logs FROM audeniq_api;
GRANT SELECT ON operations.allowed_transitions TO audeniq_api;
-- Consent + submit run in the API request: it signs the consent package and
-- freezes the submitted application revision (both append-only; no UPDATE or
-- DELETE is granted, and the tables' own triggers enforce immutability).
GRANT INSERT ON catalog.consent_packages,catalog.application_revisions TO audeniq_api;
-- Pre-submit / submission status read stage results and package summaries.
GRANT SELECT ON operations.check_results TO audeniq_api;
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
GRANT SELECT ON portal.notifications TO audeniq_api;
-- 권리 서류 서명 요청 (0071): 이벤트는 추가만
GRANT SELECT,INSERT,UPDATE ON portal.signing_requests TO audeniq_api;
GRANT SELECT,INSERT ON portal.signing_events TO audeniq_api;
GRANT USAGE ON SEQUENCE portal.signing_events_id_seq TO audeniq_api;
GRANT USAGE ON SCHEMA finance TO audeniq_api;
GRANT SELECT ON finance.ledger_transactions,finance.ledger_entries,finance.payout_orders,finance.royalty_reports,finance.report_lines,finance.finance_holds TO audeniq_api;
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
-- E-0 holds delivery until the release's agreement is signed (portal stays ungranted).
GRANT EXECUTE ON FUNCTION execution.agreement_signed(uuid, uuid) TO audeniq_worker;
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
GRANT INSERT ON finance.ledger_transactions,finance.ledger_entries,finance.commercial_split_snapshots TO audeniq_worker;
GRANT SELECT,INSERT,UPDATE ON finance.payout_orders,finance.finance_holds TO audeniq_worker;
-- Stage packages, canonical releases and delivery evidence cannot be written by the Foundation API.
GRANT USAGE ON SCHEMA operations TO audeniq_worker;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_worker;
GRANT SELECT,INSERT ON operations.event_receipts TO audeniq_worker;
GRANT INSERT ON operations.audit_events TO audeniq_worker;
REVOKE UPDATE,DELETE,TRUNCATE ON operations.audit_events FROM audeniq_api,audeniq_worker;
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
