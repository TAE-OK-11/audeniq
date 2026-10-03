# Operations retention and capacity contract

This responds to `TECH_REVIEW_F0F1.md`. These are engineering rules and release gates, not a claimed legal retention schedule or an enabled purge job.

| Data | Foundation behavior | Required before operational retention is enabled |
| --- | --- | --- |
| auth_limits | Limits reset their 15-minute window | Owner-only `audeniq-admin privacy purge-transient` removes windows older than 24 hours, up to 10,000 per transaction. Schedule it and monitor failures. Current windows stay. |
| sessions | No bearer tokens stored | The same owner task purges only sessions expired/revoked over 24 hours ago, in bounded batches. Live sessions stay. 24 hours is an operational recovery grace, not a statutory period. |
| jobs | QUEUED/RUNNING/DEAD_LETTER and terminal rows retained | Archive only terminal jobs whose outbox/business dependencies are complete. Never remove DLQ/unknown external outcomes without operator resolution. |
| outbox / event_receipts | Events and consumer receipts retained | Receipt deduplication must outlive all replay/recovery paths. Archiving payloads must preserve unique idempotency keys and receipts. Never purge pending events. |
| staff_access_logs | Runtime INSERT only; UPDATE/TRUNCATE rejected, DELETE refused until retain_until | Default 2 years; owner task retires expired rows. Keep independently protected copies and access-log review evidence. |
| audit_events | Append-only; runtime roles cannot mutate/delete | Legal/security review determines retention. Export to access-controlled immutable archive, verify row ranges, digest and restore/query procedure. Only a separately authorized maintenance role can retire approved partitions. |
| submitted revisions / rights / packages | Immutable; retained | Preserve referenced lineage and dispute/legal holds. No automatic age-based deletion. |
| quarantine / unbound registered R2 objects | No deletion worker yet | Reconcile DB and object inventory. Quarantine is eligible only after URL expiry plus recovery grace; registered objects require proof of no active session, asset/revision/package reference or hold. A HEAD/copy race may leave an orphan; never infer eligibility from prefix alone. |

Measure relation/index sizes, dead tuples, autovacuum lag, queue age, expired leases, DLQ counts and storage orphan inventory before enabling customers. The 2 vCPU/4GB lab is not a throughput baseline. No unmeasured threshold constitutes operating approval.

Start with existing primary/foreign keys and indexes. Do not partition blindly: PostgreSQL range partitioning can change uniqueness/FK enforcement unless the partition key participates in keys. A future partition migration must demonstrate preservation of immutable lineage, idempotency uniqueness and restore behavior on a disposable copy first. Candidate append-heavy audit data may be partitioned only after those requirements and archive retention are agreed. Operational job/receipt tables must not lose global deduplication.

## Cost worksheet (no invented prices or paid resources)

Record a date, region, currency and applicable signed rate card for each input. Estimate separately:

- server instance + attached database disk + snapshots + network;
- R2 retained audio/artwork bytes, quarantine and frozen copies, inventory/HEAD/PUT/Copy/delete operations;
- Workers requests/CPU and applicable VPC/Tunnel entitlement;
- PostgreSQL full/incremental backups + WAL growth + restore-test transfer and temporary disk;
- retained logs and archive storage.

Monthly estimate = sum(quantity × verified unit rate) + fixed plan charges + applicable tax. Head/copy validation adds storage operations even when the final request fails; zero-priced download transfer does not imply zero storage or operation cost. Provider plans, contract terms, real usage and currency must be confirmed before filling monetary figures. This implementation neither subscribes to plans nor asserts a monthly price.
