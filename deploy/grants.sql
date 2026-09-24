-- Run by schema owner after migration. Runtime logins are never superusers or table owners.
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA identity,catalog,operations TO audeniq_api;
GRANT SELECT ON ALL TABLES IN SCHEMA identity,catalog TO audeniq_api;
GRANT INSERT,UPDATE ON identity.orgs,identity.parties,identity.users,identity.memberships,identity.sessions,identity.auth_limits,identity.resources,identity.resource_acl TO audeniq_api;
GRANT INSERT,UPDATE ON catalog.artists,catalog.labels,catalog.releases,catalog.tracks,catalog.credits,catalog.assets,catalog.upload_sessions TO audeniq_api;
GRANT DELETE ON catalog.credits TO audeniq_api;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_api;
GRANT INSERT ON operations.audit_events TO audeniq_api;
GRANT SELECT ON operations.allowed_transitions TO audeniq_api;
-- Submitted revisions, signed evidence and stage packages cannot be inserted by the Foundation API.
GRANT USAGE ON SCHEMA operations TO audeniq_worker;
GRANT SELECT,INSERT,UPDATE ON operations.jobs,operations.outbox TO audeniq_worker;
GRANT SELECT,INSERT ON operations.event_receipts TO audeniq_worker;
GRANT INSERT ON operations.audit_events TO audeniq_worker;
REVOKE UPDATE,DELETE,TRUNCATE ON operations.audit_events FROM audeniq_api,audeniq_worker;
