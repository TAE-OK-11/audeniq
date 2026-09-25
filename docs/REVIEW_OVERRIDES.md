# Stage 2 review overrides and separation of duties

Sandbox round 2.5 (2026-09-25) found that the override endpoint was dead in
production (500: no grant on schema `rights` for `audeniq_api`) and, once
granted, that the two-person rule could be bypassed: an OWNER could enrol any
account as a member without its consent and pass that account's id as
`second_approver_user_id` to force `S2_RIGHTS_SCOPE` to PASS. A sole owner
could also self-PASS ordinary review codes, and overrides never re-ran Stage 2,
so releases stayed in `STAGE2_REVIEW` anyway.

## Rules

Overrides never change `operations.check_results`; they are append-only rows
in `rights.review_overrides` that replace a check's effective status the next
time Stage 2 decides. The latest override for a check wins.

| Override | Who | Second person |
|---|---|---|
| Stricter (`REVIEW_REQUIRED`, `BLOCKED`, `CORRECTION_REQUIRED`) | OWNER/EDITOR with write access to the release | no |
| `PASS` on a low-risk code (`S2_RELEASE_DATE_FAR_FUTURE`, `S2_RELEASE_DATE_FAR_PAST`, `S2_META_CREDITS`) | OWNER (a sole owner may self-approve) | no. Audited as `override.self_approved` |
| `PASS` on any other code (for example catalog identifiers, duplicates, content signals, fingerprints) | OWNER/EDITOR with write access files a request | **yes** |
| `PASS` on a rights/money class (`S2_RIGHTS_SCOPE`, `S2_DOCS_ORIGIN`, `S2_SPECIAL_FLAGS`) | OWNER files a request | **yes**, always a genuine second person |

A sole owner can't clear rights-scope or other non-low-risk review on their own.
This is deliberate and follows BLUEPRINT's rule that the action is withheld
when two approvers don't exist.

### Second approver

- The approval comes from the approver's own authenticated session
  (`POST /reviews/overrides/{request}/approve`). The requester never names the
  approver. A request that carries `second_approver_user_id` is refused with
  422 `SECOND_APPROVER_MUST_APPROVE_IN_OWN_SESSION`.
- The approver must be a different person from the requester
  (`SECOND_APPROVER_MUST_DIFFER`). `review_overrides` also has a CHECK
  constraint that enforces this.
- The approver must have an ACTIVE membership and an ACTIVE account.
  Non-members get 403.
- The approver's role must be OWNER or EDITOR. VIEWER is refused with
  `APPROVER_ROLE_NOT_ELIGIBLE`.
- The approver must have accepted their membership at least
  `MIN_APPROVER_TENURE_HOURS` (72 h) earlier (`APPROVER_TENURE_TOO_SHORT`).
  Memberships created before migration 0036 have `accepted_at = NULL`. They
  count as accepted legacy memberships because they predate the invitation
  flow.
- The approver must have read access (an ACL entry) to the release.
- The requester must still be a member, and still an OWNER for
  rights/money-class codes.

Requests expire after 7 days. Repeating an open request returns the same
request. The requester may withdraw a request, and another OWNER/EDITOR may
decline it. Decided requests are immutable, which a trigger enforces.

### Membership consent

`PUT /memberships` for someone who is not already an ACTIVE member creates an
`INVITED` row, which grants no access at all. Only the invitee's own session
can accept it (`POST /memberships/accept`). A revoked member must be invited
again. Role changes of ACTIVE members and revocations still apply directly.

### Re-evaluation

When an override is applied and the release is parked in `STAGE2_REVIEW` on
the overridden revision, a Stage 2 job is queued in the same transaction
(`stage2:{revision}:override:{override}`, reusing the original validation
package). The worker moves `STAGE2_REVIEW -> STAGE2_RUNNING` and decides
again, so the release either passes to Stage 3 or returns to review for the
checks that are still open. Only one re-run can be queued per revision at a
time, and repeating an identical override is a no-op (`ALREADY_APPLIED`): it
queues no job and does not bump the rights epoch. Reasons are capped at 2000
characters and must not contain control characters.

## Grants

`deploy/grants.sql` gives `audeniq_api` these grants:

- `USAGE` on schema `rights`
- `SELECT, INSERT` on `rights.review_overrides`
- `SELECT, INSERT, UPDATE` on `rights.override_requests`

The rights-epoch bump that runs on override insert is SECURITY DEFINER.

## Not covered yet

- There is no UI for the request/approve flow yet; it is API only.
- There are no notifications to potential approvers.
- There is no platform-level (staff) reviewer role. Approvers are members of
  the release's own organization.
