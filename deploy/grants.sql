-- Run by schema owner after migration. Runtime logins are never superusers or table owners.
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA identity,catalog,operations TO audeniq_api;
GRANT SELECT ON ALL TABLES IN SCHEMA identity,catalog TO audeniq_api;
GRANT INSERT,UPDATE ON identity.orgs,identity.parties,identity.users,identity.memberships,identity.sessions,identity.auth_limits,identity.resources,identity.resource_acl TO audeniq_api;
GRANT INSERT,UPDATE ON catalog.artists,catalog.labels,catalog.releases,catalog.tracks,catalog.credits,catalog.assets,catalog.upload_sessions TO audeniq_api;
GRANT SELECT ON catalog.asset_fingerprints TO audeniq_api;
GRANT DELETE ON catalog.credits TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_api;
GRANT INSERT ON operations.audit_events TO audeniq_api;
GRANT SELECT ON operations.allowed_transitions TO audeniq_api;
-- Distribution pipeline schemas (F2/F4/F5/F7). The worker runs the job
-- queues; the API never writes here (the roles test asserts 42501 for api
-- inserts into distribution). The reconciler enumerates identity.orgs,
-- which carries no RLS (tenant isolation there is by grant, not policy).
GRANT USAGE ON SCHEMA distribution,finance,execution,rights,identity,catalog TO audeniq_worker;
GRANT SELECT ON ALL TABLES IN SCHEMA distribution,finance,execution,rights TO audeniq_worker;
GRANT SELECT ON catalog.application_revisions,catalog.artists,catalog.labels,catalog.tracks,catalog.credits,catalog.assets,catalog.consent_packages,catalog.upload_sessions TO audeniq_worker;
GRANT SELECT ON identity.orgs,identity.memberships,identity.parties TO audeniq_worker;
-- Worker writes: stage transitions, frozen artifacts, delivery state,
-- finance ledger, review overrides/epochs.
GRANT SELECT,UPDATE ON catalog.releases,catalog.assets TO audeniq_worker;
GRANT SELECT,INSERT ON catalog.asset_fingerprints TO audeniq_worker;
GRANT INSERT ON catalog.application_revisions,catalog.consent_packages TO audeniq_worker;
GRANT INSERT ON distribution.canonical_releases,distribution.distribution_packages,distribution.verification_packages,distribution.validation_packages,distribution.preparation_artifacts,distribution.identifier_assignments,distribution.ddex_messages TO audeniq_worker;
GRANT INSERT,UPDATE ON execution.delivery_jobs,execution.delivery_attempts,execution.live_bindings,execution.reconciliation_cases TO audeniq_worker;
GRANT INSERT,UPDATE ON execution.route_decisions TO audeniq_worker;
GRANT INSERT ON operations.check_results TO audeniq_worker;
GRANT INSERT ON rights.review_overrides,rights.rights_epochs TO audeniq_worker;
GRANT INSERT ON finance.ledger_transactions,finance.ledger_entries,finance.commercial_split_snapshots TO audeniq_worker;
GRANT SELECT,INSERT,UPDATE ON finance.payout_orders,finance.finance_holds TO audeniq_worker;
-- Submitted revisions, signed evidence and stage packages cannot be inserted by the Foundation API.
GRANT USAGE ON SCHEMA operations TO audeniq_worker;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_worker;
GRANT SELECT,INSERT ON operations.event_receipts TO audeniq_worker;
GRANT INSERT ON operations.audit_events TO audeniq_worker;
REVOKE UPDATE,DELETE,TRUNCATE ON operations.audit_events FROM audeniq_api,audeniq_worker;
