# DB readiness follow-up test triage — 2026-10-28

Command: `cargo test -p audeniq-core --lib` (no DATABASE_URL supplied).
Current: **404 passed, 43 failed, 1 ignored**, 0 measured/filtered; 11.72s.
Raw output: [current.txt](db-ready-triage/current.txt).

The earlier blanket “pre-existing” wording was too broad: socket tests are
sandbox-blocked and their intended behavior has not been verified here.
Independent Python socket creation confirms AF_INET and AF_UNIX both fail with
`PermissionError(1, 'Operation not permitted')`. No sandbox bypass was attempted.
Four SQLx tests stop before database assertions because DATABASE_URL is absent.
The other 22 require the configured media engine, which does not exist at
`/usr/local/bin/audeniq-music`; no AUDENIQ_MUSIC_BIN override was configured.
These are missing prerequisites, not evidence of incorrect test expectations.
No test expectations or Rust implementation were changed in this round.

Baseline: **403 passed, 43 failed, 1 ignored**, 0 measured/filtered; 11.58s.
Raw output: [baseline.txt](db-ready-triage/baseline.txt).
To isolate the DB changes without touching the checkout, copied the working tree
(excluding .git/target) to /tmp/audeniq-db-baseline, then restored only
crates/core/src/{api.rs,database.rs,error.rs} from HEAD
`5f450a2d428d3973530354f607c354618a80c03c` in that temporary copy. All other
sources, including the protected CloudFront follow-up, stayed as in the working
tree. The binary-only audeniq-api.rs change cannot affect --lib tests.
Ran `CARGO_TARGET_DIR=/home/hatch/workspace/cloudfront-commit/target cargo test
--manifest-path /tmp/audeniq-db-baseline/Cargo.toml -p audeniq-core --lib` under the
same sandbox and environment. Both commands exit 101. All 43 failed test names
and their reason categories match; no baseline-only or current-only failures.
The extra current passing test is database::tests::failure_labels_do_not_include_error_details;
the modified service-boundary readiness test also passes. **0 observed DB-change
regressions; no Rust fixes required.** The comparison establishes independence
from the DB diff here, not that blocked tests pass elsewhere. In the table,
“pre-existing prerequisite” means reproduced without the DB diff, not a proven
application bug. “Sandbox-blocked” remains unverified despite matching baseline.

| Failed test | Cause | Observed reason |
|---|---|---|
| `api::tests::json_is_compressed_for_the_accepted_encoding` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `auth::tests::over_limit_buckets_are_not_rewritten` | Pre-existing prerequisite | SQLx fixture aborts: DATABASE_URL unset; DB assertions never run. |
| `av_update::tests::plaintext_never_opens_an_external_connection` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `av_update::tests::the_mirror_tunnel_relays_both_ways_until_idle` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `av_update::tests::tunnels_cannot_target_other_hosts_or_plaintext_ports` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `catalog::list_body_tests::list_body_matches_list` | Pre-existing prerequisite | SQLx fixture aborts: DATABASE_URL unset; DB assertions never run. |
| `cloud::oci::tests::instance_principal_federates_and_renews` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `edge_tls::tests::connections_for_another_name_are_closed_before_the_certificate` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `edge_tls::tests::connections_from_outside_cloudfront_are_closed_before_tls` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `edge_tls::tests::connections_over_the_limit_are_closed` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `edge_tls::tests::offers_h2_to_clients_that_ask_for_it` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `edge_tls::tests::serves_https_with_the_configured_certificate` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `external_http::tests::client_rejects_plaintext_before_opening_a_connection` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `external_http::tests::public_client_never_connects_to_this_network` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `fingerprint::tests::long_tracks_fingerprint_a_bounded_segment` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; fingerprint returns Internal. |
| `lossless::tests::additional_lossless_formats_preserve_pcm` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; conversion returns UPLOAD_CONVERSION_UNAVAILABLE. |
| `lossless::tests::float_aiff_is_rejected_before_conversion` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; conversion returns UPLOAD_CONVERSION_UNAVAILABLE. |
| `lossless::tests::wav_that_cannot_be_delivered_is_refused_before_conversion` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; conversion returns UPLOAD_CONVERSION_UNAVAILABLE. |
| `qc::tests::alac_converts_to_flac_losslessly_and_aac_is_refused` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; conversion returns UPLOAD_CONVERSION_UNAVAILABLE. |
| `qc::tests::audio_corrupt_header_is_correction_not_retry` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected CorrectionRequired. |
| `qc::tests::audio_flags_loudness_out_of_range` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected ReviewRequired. |
| `qc::tests::audio_passes_loudness_in_range` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected Pass. |
| `qc::tests::audio_passes_on_clean_wav` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; analyzer returns TechnicalRetry. |
| `qc::tests::audio_rejects_flac_declared_as_wav_but_accepts_declared_flac` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; analyzer returns TechnicalRetry. |
| `qc::tests::audio_rejects_float_wav` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected CorrectionRequired. |
| `qc::tests::audio_rejects_hard_clipping_and_warns_on_hot_peaks` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected CorrectionRequired. |
| `qc::tests::audio_rejects_sample_rate_above_192k_without_decoding` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected CorrectionRequired. |
| `qc::tests::audio_silence_is_rejected_without_parse_failure` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected CorrectionRequired. |
| `qc::tests::content_suspect_flags_sandbox_round2_patterns` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected ReviewRequired. |
| `qc::tests::engine_tap_windows_follow_segment_windows` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; analysis absent; Option unwrap panics. |
| `qc::tests::image_flags_nonsquare_cover` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected CorrectionRequired. |
| `qc::tests::image_flags_small_cover` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected CorrectionRequired. |
| `qc::tests::image_passes_on_large_cover` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; analyzer returns TechnicalRetry. |
| `qc::tests::native_header_metrics_match_engine_probe_for_pcm_and_flac` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; probe returns Unavailable. |
| `qc::tests::content_suspect_passes_ordinary_programme` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; got TechnicalRetry, expected Pass. |
| `qc::tests::segmented_fingerprint_properties` | Pre-existing prerequisite | Configured media engine /usr/local/bin/audeniq-music absent; fingerprint returns Internal. |
| `review::tests::checkpoints_and_stage1_holds_preserve_actual_results` | Pre-existing prerequisite | SQLx fixture aborts: DATABASE_URL unset; DB assertions never run. |
| `storage::tests::downloads_reject_oversized_headers_and_chunked_bodies` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `storage::tests::quarantine_copy_outlasts_the_metadata_timeout` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `storage::tests::quarantine_copy_retries_once_on_transient_errors_only` | Sandbox-blocked | TCP listener bind denied with EPERM (os error 1); behavior unverified. |
| `submission::title_tests::similarity_rescans_only_new_fingerprints_and_forgets_deleted_matches` | Pre-existing prerequisite | SQLx fixture aborts: DATABASE_URL unset; DB assertions never run. |
| `upload_safety::tests::absent_or_stale_scanner_is_fail_closed` | Sandbox-blocked | Unix listener bind denied with EPERM (os error 1); behavior unverified. |
| `upload_safety::tests::scanner_streams_exact_bytes_and_accepts_only_explicit_clean_verdict` | Sandbox-blocked | Unix listener bind denied with EPERM (os error 1); behavior unverified. |

## PgBouncer verification

**Real verification pending:** `docker` is not installed (`command not found`).
Run `python3 deploy/tests/db-ready-local.py` on a local Docker host. It builds the
actual deploy/pgbouncer image, creates isolated throwaway PostgreSQL 17 and
PgBouncer containers, random test credentials, an internal network, no published
ports, and no host data volumes. Cleanup removes its containers, anonymous
volumes, network and test image, including on assertion failure.

The harness asserts both roles succeed via direct/pooled/healthcheck paths
(exit 0), explicit wrong-password probes return `kind=auth` (20), closed-port
probes return `kind=unreachable_or_timeout` (1), and a real cached backend login
failure returns `kind=backend_login_retry` (1). Only inside the disposable
PgBouncer it lengthens server_login_retry to 60s for deterministic cache testing;
it changes a disposable role password and terminates only that test role's
sessions. It also extracts the exact deploy wait_database function and runs it
against real docker exec for success and fail-fast auth; no deploy script is run.
The real 120s timeout/recovery behavior still needs verification separately.

Here, `python3 deploy/tests/db-ready-probe.py` passed **9 tests** (mocked psql and
mocked deploy execution). Those verify routing, classification, fail-fast and
retry/timeout control flow, but do not verify real PostgreSQL error strings or
PgBouncer behavior. Running the new local harness here exits with the explicit
PENDING message before creating resources; it has not passed a real run.
Shell syntax (`bash -n deploy/pgbouncer/entrypoint.sh deploy/deploy.sh`) and
`git diff --check` pass. No Rust code changed this round.
