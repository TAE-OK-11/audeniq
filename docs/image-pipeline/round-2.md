# Round 2 image pipeline — partial implementation, local only

> Historical checkpoint. See [Phase 2](phase-2.md) for the restored offline cache,
> implemented codecs, measurements and current validation status.

The automatic durable WebP path is implemented. **The requested high-quality
lossy WebP and AVIF codecs are not implemented.** This is not a completed round 2.
No migration, grants, deployment, AWS operation or production request was run.

## Upload → queue → worker → registration

Migration `0162_image_jobs.sql` attaches a trigger to registration of JPEG/PNG
IMAGE assets. It stores the sanitized source revision in the same transaction
that registers the upload; rollback cannot leave a job for a failed upload.
No API task, codec or storage request is added to completion. There is a small
SQL scheduling cost in the existing transaction; zero additional latency is not
a literal guarantee. No bulk scan/backfill is introduced.

The worker's existing process, resource probe and global capacity semaphore are
reused. Every 10 seconds, optional work may start if sampled CPU is below 50%,
memory below 70%, and a worker permit is available. Unknown counters defer work.
The database check also defers images while other operations jobs are running or
ready. A transaction advisory lock limits image work to **one across replicas**,
including behind transaction-mode PgBouncer. A five-second heartbeat maintains
and checks that transaction; failure cancels the producer. New user-facing work
can arrive after an image starts: this is a start gate, not CPU reservation.

The queue is a fixed 256 rows, including terminal slots. Uploads reuse empty or
terminal rows first, otherwise the oldest QUEUED row. RUNNING rows are never
evicted; locked slots are skipped instead of blocking uploads. If every eligible
slot is locked, the optional job is dropped, with a bounded database log label.
Queue errors are caught inside the trigger so they do not fail uploads. Evicted
or dropped jobs receive no derivative, and require a future, separately scoped
backfill if desired. Queue policy bounds storage and scheduling work; it is not
a guarantee to encode every upload under overload. Outcomes are visible in
`catalog.image_jobs`; terminal history is bounded and reused, not an audit log.

An attempt is committed before work, up to three attempts total, with five-minute
retry delay. Holding the global lock proves any RUNNING slots from a previous
process are stale: recover them without resetting attempts. On shutdown the
existing worker drain includes the image permit; cancellation kills the codec
child. After a crash, recovery runs when load and ordinary work permit it.

The sandbox child gets nice 19, one single-threaded WebP encoder, the existing
1 GiB address-space sandbox limit, and a 120-second wall timeout. Priority does
not impose a hard CPU quota; resource limits still come from the worker container.
Original images remain serviceable on skip, failure, eviction or queue outage.
Permanent failures and skipped formats retain existing pending `private,
no-cache` semantics for format-capable clients.

The offline CLI and worker share `produce_webp_with`, source admission, sandbox,
codec validation and write-once publisher. The worker has no AppState, API
configuration or service-secret requirement. Shared source admission checks
safety policy, size and exact digest. Existing caps remain: edge 8192, 16,777,216
pixels, input/output 20 MiB, and rejection of 16-bit input. Automatic work never
resizes. Pixel roundtrip equality, storage read-back, and source revision/safety
recheck still precede publication. Sources are never modified or deleted.

Automatic publication requires **both ≥10% and ≥1024 bytes saved**. These are
storage-overhead policy choices, not perceptual quality measurements. Existing
CLI behavior may still explicitly produce larger derivatives; validations are
shared, and automatic-only usefulness filtering is deliberate.

Negotiation is AVIF → WebP → source. The checkout previously returned the source
for AVIF-only Accept if only WebP existed. The user explicitly requested WebP in
that situation; the new fallback allows it unless WebP is explicitly excluded
(e.g. q=0). Wildcards alone still do not opt in. ETags, Vary, revision binding and
pending cache behavior remain in place.

## Encoder and quality limitations

Current encoder: already locked `image 0.25.10`, pure Rust lossless WebP; no new
dependency or external codec executable. Exact decoded RGBA equality is the
quality gate (MSE 0, PSNR infinity, SSIM 1); there is no invented lossy quality
constant. This preserves quality but often misses the requested size savings
on photographic JPEGs. This fallback does **not** meet the requested lossy default.

Considered upstream APIs:

- [webp/libwebp](https://docs.rs/webp/latest/webp/struct.Encoder.html): lossy support;
  wrapper is MIT OR Apache-2.0, underlying libwebp is BSD-3-Clause and native C, not pure Rust.
- [webp-rust](https://docs.rs/crate/webp-rust/0.3.2): a pure Rust lossy candidate;
  its codec quality/performance and license must be audited before adoption.
- [ravif/rav1e](https://docs.rs/ravif/0.13.0/ravif/struct.Encoder.html): pure Rust
  AVIF encoding, explicit `with_num_threads(Some(1))` needed; ravif BSD-3-Clause,
  rav1e BSD-2-Clause. Decode/roundtrip validation is also necessary, not just
  AVIF magic bytes. Optional assembly features must be disabled if pure Rust
  is required.

`cargo info` could not reach the crates.io index through the restricted network;
these dependencies are absent from the local cache. No unavailable codec was
added to the manifest or lockfile. No lossy coefficient, AVIF preset or AVIF
size threshold can honestly be justified without running representative samples.
Actual photographic/cover samples and visual review are still required. The
repository graphics below do not substitute for those measurements.

## Local measurement

Reproduce with:

```sh
cargo build --offline -p audeniq-core --bin audeniq-image-encode
python3 scripts/measure-image-derivatives.py
```

The script runs only the local sandbox codec entry, with local PNGs; it never
initializes DB/storage or reads credentials. Pillow independently decodes both
files and compares every RGBA pixel. PSNR/SSIM values follow mathematically from
exact equality; no approximate metric or human visual assessment is claimed.
Timings include child startup and are development-build, local-machine results.
The first two files may be duplicate artwork and are not independent samples.

| Checkout graphic | Original bytes | WebP bytes | Saving | Encode ms | PSNR | SSIM | Publish? |
|---|---:|---:|---:|---:|---:|---:|---|
| landing social-preview.png | 37620 | 33210 | 11.72% | 529.2 | ∞ | 1 | yes |
| survey social-preview.png | 37620 | 33210 | 11.72% | 241.3 | ∞ | 1 | yes |
| brands/canva.png | 14774 | 16430 | -11.21% | 22.7 | ∞ | 1 | skip |

These are three file measurements of two distinct graphic assets, not real
photographs or album-cover samples. AVIF sizes/quality were not measured. No
manual visual comparison was performed; exact pixel identity was checked by
the codec and independently by Pillow. Values were measured with the built
`target/debug/deps/audeniq_image_encode-6f860649551fd12b` executable using the
script's `--encoder` option, while Cargo was still linking other binaries.

## Validation

Results are listed in the final local report below. The SQL integration suite
includes atomic trigger scheduling, fixed capacity/oldest eviction, preservation
of RUNNING work, idempotent updates, crash recovery, bounded retries/backoff,
source fallback and verified automatic publication with changed ETag, Vary and
pending cache rules. The existing offline publication/storage-fault tests remain.
PostgreSQL execution is required before considering this ready for deployment.

## Remaining work

- Implement and measure high-quality lossy WebP and AVIF, using the same sandbox
  and publisher; add decode-based per-image quality gates and alpha/ICC checks.
- Measure real photos and cover art, including text, transparency and color
  profiles; tune candidate quality/speed settings from metrics and visual review.
- Add measured AVIF pixel/edge eligibility and idle-only budget. The global
  concurrency infrastructure already caps all image work at one, but there is
  no AVIF encoder yet.
- Execute migration, permission and end-to-end queue/serving tests against a
  disposable local PostgreSQL instance. This environment has no PostgreSQL,
  Docker or Podman executable; no existing database URL was used.

## Final local results (2026-10-28)

- `cargo test --offline -p audeniq-core --lib image_encoder::tests`: 5 passed.
- `cargo test --offline -p audeniq-core --lib image_ -- --skip qc::tests`:
  12 passed, including negotiation, revisions, ETags and pending cache behavior.
- `cargo test --offline -p audeniq-core --bin audeniq-worker image_load_tests`:
  1 passed.
- `cargo test --offline -p audeniq-core --test image_serving --no-run`:
  succeeded; executable lists all 6 integration tests. They were NOT executed.
- `cargo build --offline -p audeniq-core --bin audeniq-image-encode`: succeeded.
- Local graphic measurement: 3 file runs passed exact RGBA comparison; results
  above. Two social-preview paths contain identical artwork.
- Initial broad `--lib image_` invocation: 10 passed, 3 failed in existing
  `qc::tests::image_*` tests with `IMAGE_PROBE_FAILED` / image analyzer unavailable.
  Those require the separate `audeniq-music photo` engine, absent here. They were
  not fixed or counted as successful tests.
- `git diff --check`: passed.

Workspace observation: at the beginning, round-1 files were uncommitted. During
this session HEAD changed externally to `e626624` (round-1 follow-ups), on the
same branch. This implementation executed no commit/push/merge/revert/reset or
deploy command. All round-2 source changes remain uncommitted; no attempt was
made to undo the external HEAD change. `deploy/aws/*` was not edited.

## Phase 2 continuation

The historical status above is superseded by [Phase 2](phase-2.md), using the
restored checkout-local offline registry cache. It records implemented codecs,
measurements, verification limitations, policy and current validation.
