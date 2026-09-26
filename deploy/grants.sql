-- Run by schema owner after migration. Runtime logins are never superusers or table owners.
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA identity,catalog,operations TO audeniq_api;
GRANT SELECT ON ALL TABLES IN SCHEMA identity,catalog TO audeniq_api;
GRANT INSERT,UPDATE ON identity.orgs,identity.parties,identity.users,identity.memberships,identity.sessions,identity.auth_limits,identity.resources,identity.resource_acl TO audeniq_api;
GRANT INSERT,UPDATE ON catalog.artists,catalog.labels,catalog.releases,catalog.tracks,catalog.credits,catalog.assets,catalog.upload_sessions TO audeniq_api;
GRANT SELECT ON catalog.asset_fingerprints TO audeniq_api;
GRANT DELETE ON catalog.credits TO audeniq_api;
-- Housekeeping: expired rate-limit buckets are deleted by the API (0040).
GRANT DELETE ON identity.auth_limits TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_api;
GRANT INSERT ON operations.audit_events TO audeniq_api;
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
GRANT USAGE ON SCHEMA finance TO audeniq_api;
GRANT SELECT ON finance.ledger_transactions,finance.ledger_entries,finance.payout_orders,finance.royalty_reports,finance.report_lines,finance.finance_holds TO audeniq_api;
-- Distribution pipeline schemas (F2/F4/F5/F7). The worker runs the job
-- queues; the API never writes here (the roles test asserts 42501 for api
-- inserts into distribution). The reconciler enumerates identity.orgs,
-- which carries no RLS (tenant isolation there is by grant, not policy).
GRANT USAGE ON SCHEMA distribution,finance,execution,rights,identity,catalog TO audeniq_worker;
GRANT SELECT ON ALL TABLES IN SCHEMA distribution,finance,execution,rights TO audeniq_worker;
GRANT SELECT ON catalog.application_revisions,catalog.artists,catalog.labels,catalog.tracks,catalog.credits,catalog.assets,catalog.consent_packages,catalog.upload_sessions TO audeniq_worker;
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
-- Duration-bounded variant used by Stage 1 (migration 0040).
GRANT EXECUTE ON FUNCTION catalog.fingerprints_outside_org_near(uuid, smallint, double precision, double precision, double precision) TO audeniq_worker;
GRANT INSERT ON catalog.application_revisions,catalog.consent_packages TO audeniq_worker;
GRANT INSERT ON distribution.canonical_releases,distribution.distribution_packages,distribution.verification_packages,distribution.validation_packages,distribution.preparation_artifacts,distribution.identifier_assignments,distribution.ddex_messages TO audeniq_worker;
GRANT INSERT,UPDATE ON execution.delivery_jobs,execution.delivery_attempts,execution.live_bindings,execution.reconciliation_cases TO audeniq_worker;
GRANT INSERT,UPDATE ON execution.route_decisions TO audeniq_worker;
GRANT SELECT,INSERT ON operations.check_results TO audeniq_worker;
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
