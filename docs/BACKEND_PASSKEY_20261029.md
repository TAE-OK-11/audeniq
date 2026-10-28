# Backend authentication handoff (2026-10-29)

Signup follows the 07:22 revision: ID (email)/password first, then name,
phone and personal/label type, then optional passkey enrollment. The backend
accepts the fields together through POST /api/auth/register; it cannot enforce
visual wizard ordering. Existing signup payloads still work. Terms acceptance,
Argon2 work slots and signup/login rate limits remain in place. A successful
registration does not issue a session: sign in with the emergency password,
then optionally enroll a passkey. Skipping enrollment creates a complete account.

Optional signup fields:
- name: trimmed, nonempty, at most 200 characters, text-policy checked; stored
  as the party display name and workspace name. Labels also use protected-name
  enforcement, matching ordinary organization creation.
- account_type: personal (default) or label; maps to PERSONAL/PERSON or
  LABEL/LEGAL_ENTITY for the organization/party.
- phone: normalized using sms::normalize_phone; only the keyed hash and last
  four digits are stored. verified_at stays null until /api/me/phone/start and
  /api/me/phone/verify succeed. Signup cannot overwrite an existing phone.
  Password recovery explicitly excludes unverified numbers.

Passkey-primary flow: discoverable login still uses POST
/api/auth/passkey/options and /api/auth/passkey/login, with an empty
allowCredentials list. A UV-verified passkey session can list/revoke sessions,
manage passkeys, access account settings and logout without a password.

New endpoints:
- POST /api/auth/passkey/reauth/options: authenticated, CSRF-protected assertion
  challenge bound to the current user and session (five minutes).
- POST /api/auth/passkey/reauth: accepts the existing passkey login assertion
  shape; checks the account, session, origin, RP ID, UP/UV, signature and counter;
  returns reauth_token and expires_in. Does not issue another session.
- PATCH /api/auth/passkeys/{id}: {name}, owner-only rename with text validation
  and audit event. Existing DELETE on the same path remains.

Changed endpoints:
- POST /api/auth/register accepts the optional profile fields above.
- POST /api/auth/passkeys/register accepts either current_password or
  reauth_token. Existing password clients continue working. Obtain creation
  options from the existing /api/auth/passkeys/options endpoint.
- POST /api/auth/oauth/{provider}/start with mode=link now requires reauth_token;
  callbacks must find the initiating session still active. Login/signup modes
  are unchanged. Existing-email accounts are never automatically linked.
- DELETE /api/auth/passkeys/{id} refuses PASSKEY_LAST_SIGN_IN_METHOD when no
  other passkey, emergency Argon2 password or linked OAuth identity remains.
  This intentional security restriction is the only deletion behavior change.
- POST /api/auth/login retains its response and PASSWORD audit reason and also
  records auth.emergency_password_login, distinguishing emergency access.

Proofs are random opaque tokens stored hashed, session-bound, five minutes old
at most, consumed once in the protected operation's transaction, and rejected
for expired or revoked sessions. Expired proofs are cleaned during successful
reauthentication. Challenge one-use behavior and the synced-counter-zero
exception remain. Account-row locking serializes passkey registration/removal
with OAuth unlink, preserving the last sign-in method under concurrency.

Emergency and lost-device recovery: use the existing ID/password login and
register a replacement passkey. Keep the ordinary login rate limits; emergency
access is audited separately. The existing verified-phone SMS reset flow can
recover a forgotten emergency password. OAuth-created !oauth accounts remain
distinct from ID/password signup; they can sign in through their linked provider,
verify a phone and use the existing password reset flow before first enrollment.
If every available credential and verified-phone recovery is lost, an operator
must independently establish identity under the existing administrative identity
verification process before any manually approved credential recovery. This
change adds no public recovery-code or administrator credential-reset endpoint,
and does not permit support to bypass authentication through these endpoints.
The administrative recovery procedure is: open a recovery case; independently
verify identity through the existing identity desk (never accept signup name or
an unverified phone as proof); obtain a second operator's approval and retain
case/evidence references; authorize a controlled credential replacement and
revoke old sessions/connected-agent grants; have the owner enroll and verify a
replacement passkey and review remaining sign-in methods. A dedicated approved
operator recovery tool/runbook is required for the credential replacement step;
this patch deliberately ships no ad hoc database-reset or bypass mechanism.

High-value operations: the authentication layer exposes consume_proof for use
inside the protected operation's database transaction. Financial callers must
require a new passkey assertion and consume its proof alongside their existing
secondary-approval rules. This change does not modify payout/settlement logic
or claim those operations now enforce passkey step-up.

Follow-up frontend work: collect and send the optional signup fields in the
specified order; offer enrollment as the final optional step; default normal
login to passkeys and label password access as emergency; obtain a fresh proof
for another passkey or explicit OAuth linking; expose rename/metadata and
last-method errors. No frontend files are changed here.

Validation:
- All migrations applied successfully to an isolated PostgreSQL 16 database,
  including a run with audeniq_api present; proof-table privileges were checked.
- cargo test -p audeniq-core --test account --test passkeys --test password_reset
  --test social_login: all six tests passed after correcting the stale-snapshot
  race exposed by the new concurrent-removal regression. The first run's new
  concurrency test failed because both deletions could succeed. Lock acquisition
  and the fallback query now use separate statements, so the fallback query
  sees a fresh READ COMMITTED snapshot after any lock wait.
- cargo fmt --all --check and git diff --check passed.
- Standard cargo clippy for the library and all four authentication targets
  passed, with the three existing image-module warnings listed below.
- Strict Clippy (-D warnings) initially found a new collapsible-if warning in
  passkey.rs, now fixed, plus three pre-existing warnings outside this task:
  image_revisions.rs:46 (too_many_arguments), image_variants.rs:26 and :80
  (collapsible_if). Those unrelated image files were not edited.
- Authentication integration tests use software authenticators and local mocked
  Google/Apple providers. Real hardware/browser ceremonies, live providers,
  production latency, and the complete repository test suite were not verified.
