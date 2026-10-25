# Staff levels and admin console operations

The admin console (`web/admin`, Studio `/admin`) now runs the operator CLI's
day-to-day commands, gated by staff level. Code: `crates/core/src/staff_ops.rs`,
`staff.rs` (`StaffRole::level`), migration `0146_staff_levels.sql`; screens
`web/admin/src/pages/Ops.tsx`, `web/studio/app/src/admin/pages/Ops.tsx`.

## Levels

| Level | Roles | Adds |
|---|---|---|
| 1 | SUPPORT | Inquiries, identity desk |
| 2 | REVIEWER, OPERATOR | Review / delivery work as before; AI daily limits; read protected names and partners. OPERATOR also has the Finance duty |
| 3 | ADMIN | Protected names, AI agents, developer app review, partner setup, content |
| 4 | OWNER (new) | Staff levels, identifier ranges, partner go-live |

A higher level can do everything below it (`StaffRole::is_admin` is level 3+;
in work management an OWNER works as an ADMIN). Payouts and royalty reports
need the Finance duty (OPERATOR, ADMIN, OWNER), as the CLI required.

OWNER is granted only with the CLI: `audeniq-admin staff grant EMAIL OWNER`.
Only an OWNER adds settlement balance by hand (`balance.credit`, console
정산·지급 처리 › 정산금 추가): up to 10,000,000 KRW in whole won per
action, a title the artist sees in the statement and an internal reason.
`finance.staff_credit_balance` (0159) re-checks the OWNER in the database,
posts `MANUAL_CREDIT` debit / `ROYALTY_PAYABLE` credit (never an edit),
notifies the artist and the change is audited as `balance.credit`.
In the console an OWNER grants ADMIN, REVIEWER, OPERATOR or SUPPORT, never
changes their own row and never another OWNER. `identity.staff_set` and
`identity.staff_reset_mfa` (SECURITY DEFINER) enforce this in the database too.

## Operations

`GET /api/staff/ops` lists what the signed-in staff member may run.
Reads: `GET /api/staff/ops/{area}/{op}?…`; changes: `POST /api/staff/ops/{area}/{op}`
with a JSON body (plus `code`, a fresh authenticator code, where step-up applies).

| Area | Operations (level) | Step-up |
|---|---|---|
| staff | list (3); grant, revoke, reset-mfa (4) | changes |
| protected | list (2); add, activate, deactivate, alias, remove-alias, grant-exception, revoke-exception (3) | — |
| developer-apps | list, approve, return (3) | — |
| ai-clients | list, add, disable (3) | add |
| ai-limits | list, set, clear (2) | — |
| partners | list, status (2); config-check, set-dsp, probe, dpid, test-ern, test-ack, capabilities, contract, route, merlin-eligible, suspend (3); go-live (4) | go-live |
| identifiers | list (3); register (4) | register |
| payouts | list, order, approve, submit, result, cancel, manual, reissue, return (Finance) | approve, submit, result, manual, reissue, return |
| royalty | show, import, match, receipt, post (Finance) | receipt, post |
| balance | show, credit (4) | credit |

Errors: `STAFF_LEVEL_REQUIRED` (403, `details.level` or `details.duty`),
`STAFF_STEP_UP_REQUIRED` / `STAFF_MFA_CODE_INVALID`, `OPS_ARGUMENT_INVALID`
(`details.field`), `OPS_REJECTED` (422, the CLI routine's own explanation),
`STAFF_OPS_UNAVAILABLE` (503), and the staff rules `STAFF_SELF_CHANGE`,
`STAFF_OWNER_PROTECTED`, `STAFF_ROLE_INVALID`, `STAFF_NOT_FOUND`, `ACCOUNT_NOT_FOUND`.
Every change is audited with `staff:<user id>` as the operator, plus the
usual staff access log. Approver and executor of a payout must differ.

Deployment and infrastructure commands stay in the CLI: upload sandbox
check, antivirus update, storage probe, privacy purge, fingerprint backfill,
seals, external recordings, `dsp list`.

## Turning it on

The operations run on their own login, not the API role:

```sql
CREATE ROLE audeniq_staff_ops LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '…';
```

then re-run `deploy/grants.sql` (its staff-ops block applies only when the
role exists) and set `AUDENIQ_STAFF_OPS_DATABASE_URL` for the API. Without it
the console shows the operations as unavailable and the CLI stays the only way.
The role writes only the tables those operations change, never
`identity.staff_members` or `identity.staff_mfa` directly; partner onboarding
gets an RLS policy for this role only (0146). `tests/staff_ops.rs` runs the
operations as this role.
