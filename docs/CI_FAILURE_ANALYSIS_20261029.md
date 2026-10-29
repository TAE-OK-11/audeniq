# rust-postgres CI failure analysis — 2026-10-29

Baseline: `64e1612b`, on `codex-backend-cifix-20261029`.
Evidence: [CI run 40865634385, job 123419628612](https://github.com/TAE-OK-11/audeniq/actions/runs/40865634385/job/123419628612), retrieved with
`gh run view 40865634385 --job 123419628612 --log-failed`.
The job ran `cargo nextest run --locked -p audeniq-core --test-threads 4 --no-fail-fast`.

## Smart-link cover: outdated HTTP assertion

At 11:48:51 UTC the log reports `execution.rs:5295:9`, left 307,
right 200, in `smart_link_public_page_counts_views_and_clicks`.

The test requests `/api/smart-links/{slug}/cover` without `revision`.
`smart_links::router` routes this GET to `h_cover` (`crates/core/src/smart_links.rs`).
That handler checks `visible`, then calls `image_revisions::respond` with kind
`cover` and the smart-link ID. The no-revision branch loads the safety-approved
image, inserts its retained mapping in `portal.image_revisions`, and calls
`redirect`. That function explicitly returns 307, `Cache-Control: no-store`, and
`Location: /api/smart-links/{slug}/cover?revision={sha256}`. The revision branch
checks the digest and scoped mapping, loads/verifies the bytes, negotiates the
format, and returns the image response. The in-process `tower::ServiceExt::oneshot`
request used by `tests/support/mod.rs::send` does not follow redirects.

`2c704f7799b77562e1c461212379452613128451` introduced this deliberate contract
(see its smart-links/image-revisions diff and `docs/STUDIO_IMAGE_CACHE.md`).
It did not update this cover assertion. This is a stale test, not evidence that
the public image endpoint should revert to serving bytes at its mutable alias.

Fix: update the existing integration test to assert 307, no-store, and the exact
SHA revision location, then explicitly GET that location and assert 200 with a
nonempty image body. This preserves the rest of the page/view/click assertions.
No serving code changes are needed.

## Deploy status: fixture violates env-file permissions

At 11:51:13 UTC the log reports `secrets_cli.rs:254:5`,
`assertion failed: plain.status.success()`. This is the first, empty-secrets
case, before assertions about materialized secrets. The original helper captures
and discards subprocess stdout/stderr, so the CI log alone does not expose the
shell error or numeric exit code.

`deploy_status("")` copies `deploy/deploy.sh` into a temporary directory, writes
`production.env` using `std::fs::write`, then invokes `bash deploy.sh --status`
with a Docker stub. File creation uses 0666 filtered by the inherited umask;
with umask 0022 it creates 0644. The script's later `umask 077` does not change
permissions on an existing file. After taking its lock, `deploy.sh:55–57`
rejects symlinks, missing files, or a file whose owner/mode differs from the
current UID and 0600. `die` prints
`production.env must be owned by the deploy user with mode 0600` and exits 1.
This occurs before secrets loading, Docker volume probes, and the `--status`
Compose branch. It is unrelated to image pulling, deployment, or cloud secrets.

`git blame` identifies `08e3fe005` as the introduction of the owner/mode check.
The fixture was not brought into compliance. The CI log does not record its
umask/stat output; the exact CI file mode is inferred from the deterministic
fixture/preflight path and confirmed by local controlled reproduction, rather
than claimed as directly logged.

Fix: use the existing private-file writer with explicit 0600 for `production.env`.
Preserve subprocess stderr in assertion failures. Add a regression test proving
0644 is rejected with the expected diagnostic before Compose. Do not weaken the
production permission check.

## Why neither is caused by passkeys

Both `2c704f77` and `08e3fe00` are ancestors of `496332bf^` (verified with
`git merge-base --is-ancestor`), so their behavior predates the passkey patch.
`git show --stat 496332bf` confines that patch to `auth.rs`, `passkey.rs`,
`tests/passkeys.rs`, passkey documentation, and migration 0164. Neither failing
test, the image handler/helper, nor deploy.sh was changed by it. Its only
`auth.rs` change wraps the token in `issue_session` with `Zeroizing`; public
`h_cover` never calls session issuance. Deploy's subprocess only runs Bash and
its Docker stub and never invokes the Rust API or passkey logic. The image path
uses migration 0161's revision table, not migration 0164's authenticator data.
These code paths and commit ancestry establish that both test defects already
exist in the parent of the passkey commit; a separate pre-passkey CI execution
was not performed.

## Validation

- Controlled standalone Rust harness using the actual deploy fixture and both
  deploy test bodies: 2 passed (0600 secrets ordering/negative cases and 0644
  rejection). Only UUID-based temporary names were substituted with timestamps;
  the unrelated cloud-CLI test was omitted. Docker was stubbed, not executed.
- Full Cargo validation results are recorded below after completion.
- No live deployment, cloud secrets, S3, or CDN validation was attempted.
