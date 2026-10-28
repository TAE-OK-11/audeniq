# Phase 2: lossy WebP and AVIF (2026-10-28)

This continues the preserved Phase 2 working tree. The round-2 and round-3
reports are historical; the offline cache now supplies the encoders. No AWS,
production, deployment, commit, push, merge, migration or grant execution occurred.

## Chosen parameters and policy

- Automatic WebP: pinned `webp =0.3.1`, bundled libwebp C, quality **95**, default
  method **4**, alpha quality **100**, single codec thread. `webp-rust =0.3.2` was probed, but its
  preserved nominal-Q90 lake trials were slower and had lower PSNR/SSIM; it is not an application
  dependency. The native wrapper is MIT/Apache-2.0 and bundled libwebp BSD-3-Clause.
- Automatic AVIF: pinned `ravif =0.13.0`, defaults disabled (no assembly/threading
  features), quality **90**, alpha quality **100**, speed **8**, explicit one
  thread. The pinned ravif defaults produce 10-bit YCbCr 4:4:4 (BT.601
  conversion) with unassociated alpha; these defaults were used in the grid.
  ravif is BSD-3-Clause; rav1e is BSD-2-Clause.
- Never resize automatically. AVIF requires **at most 2,250,000 source pixels**;
  reject above this before pixel decode. A preserved 9 MP Q80/speed8 encode took
  **35.218 s**; 2.25 MP Q90/speed8 took 8.455 s in the earlier trial and 19.940 s
  during this continuation's concurrent offline build. The 2.16 MP flower took
  25.632 s. This is a conservative optional-work cap, not a guaranteed latency
  bound. Keep the 120-second per-codec parent wall timeout and kill-on-drop.
- ICC-bearing sources use lossless WebP with their ICC profile preserved; AVIF
  skips them. Lossless remains available through the explicit offline CLI.
  JPEG/PNG only, max edge 8192, max 16,777,216 pixels, 20 MiB input/output and
  rejection of 16-bit input remain in force.
- Retain **both at least 10% AND at least 1,024 bytes saved against the source**
  for each automatic derivative. Q95 WebP saves 23–48% on the admitted photos
  below, while the 9 MP lake result is larger than its original and is skipped.
  There is no measured reason to store marginal derivatives below this threshold.
  This is a per-format bandwidth/storage tradeoff, not a total-storage reduction
  promise: retaining the original and two variants can consume up to 2.8 times
  the original size. AVIF is not required to beat WebP by a further 10%, because
  each format serves a different client set. Offline CLI usefulness filtering
  remains deliberately optional.

Quality 95/90 is chosen from the grid below: lower qualities lose more texture
and edge detail. At 2.25 MP the checked lake WebP Q90 has SSIM .977022 versus
.987476 at Q95; AVIF Q80 has .956770 versus .988059 at Q90. Speed6 reduces Q90
AVIF by only 205/399/70/1697 bytes on small lake/large lake/tower/flower and has
negligible perceptual improvement. Speed8 preserves more idle worker capacity.
These are small-corpus settings, not a guarantee of perceptual equality for all
uploads, text, artwork, color spaces or transparent images.

## Measurements

Three distinct real photographs: lake from cached `webp-0.3.1/assets/lake.jpg`,
architecture from `jpeg-decoder-0.3.2/benches/tower.jpg`, and flower from
`jpeg-decoder-0.3.2/benches/large_image.jpg`. All came from the checkout-local
registry cache; no image was downloaded. Lake appears at three sizes, not three
independent scenes. The smaller lake sources were preserved JPEGs. Architecture
was normalized to JPEG Q95; the flower was fit to 1800x1250 (result 1800x1200)
and normalized to JPEG Q95. Reported original sizes refer to those input JPEGs,
not upstream originals. These preparations are benchmark inputs only.

The preserved small-lake raw pixels were from before JPEG serialization. This
continuation detected that mismatch and reran those codec rows from their actual
JPEG decoded pixels; old artifacts/results were preserved. The 4000px lake raw
pixels exactly match Pillow's decoded original. All table metrics compare output
to decoded input JPEG, not to the scene before JPEG compression.

Release helper codec-only timings include process startup, exclude source decode,
metrics, storage and DB. Earlier 9 MP timings were taken without this build;
continuation timings overlap an offline dependency build and are not controlled
performance comparisons. WebP was decoded with Pillow; AVIF was decoded with
existing `libheif.so.1` through Python ctypes, never a codec executable. RGB PSNR
uses maximum 255. SSIM is Rec.709 luma, 11x11 box windows with reflected borders,
population variance, C1=6.5025/C2=58.5225; it is not the Gaussian skimage default.

| Input | Dimensions | Original bytes | WebP Q95 bytes / seconds / PSNR / SSIM | AVIF Q90 speed8 bytes / seconds / PSNR / SSIM |
|---|---|---:|---|---|
| lake-1000 | 1000×562 | 124,635 | 88,902 / 0.342 / 45.892 / 0.991127 | 55,427 / 6.000 / 46.221 / 0.991368 |
| lake-2000 | 2000×1125 | 544,970 | 408,602 / 0.649 / 45.352 / 0.987476 | 251,469 / 19.940 / 45.833 / 0.988059 |
| lake-4000 | 4000×2250 | 1,848,667 | 1,856,874 / 1.776 / 45.310 / 0.986196 (skip: larger) | Ineligible; Q80 trial below |
| tower | 512×512 | 70,768 | 54,234 / 0.132 / 44.759 / 0.992995 | 36,358 / 3.349 / 44.044 / 0.991421 |
| photo | 1800×1200 | 363,975 | 188,318 / 0.582 / 45.409 / 0.991415 | 109,489 / 25.632 / 46.382 / 0.991459 |

Full grid (same input reference and metric definitions):

| Input | Codec / quality / speed | Bytes | Saving | Seconds | PSNR dB | SSIM | Passes size/eligibility policy |
|---|---|---:|---:|---:|---:|---:|---|
| lake-1000 | WebP / 85 / 4 | 37,104 | 70.23% | 0.284 | 41.620 | 0.974575 | yes |
| lake-1000 | WebP / 90 / 4 | 55,704 | 55.31% | 0.154 | 43.543 | 0.984207 | yes |
| lake-1000 | WebP / 95 / 4 | 88,902 | 28.67% | 0.342 | 45.892 | 0.991127 | yes |
| lake-1000 | AVIF / 80 / 8 | 22,770 | 81.73% | 5.929 | 40.922 | 0.970371 | yes |
| lake-1000 | AVIF / 90 / 8 | 55,427 | 55.53% | 6.000 | 46.221 | 0.991368 | yes |
| lake-1000 | AVIF / 90 / 6 | 55,222 | 55.69% | 8.204 | 46.323 | 0.991653 | yes |
| lake-2000 | WebP / 85 / 4 | 159,040 | 70.82% | 1.165 | 40.757 | 0.962233 | yes |
| lake-2000 | WebP / 90 / 4 | 253,150 | 53.55% | 0.907 | 42.824 | 0.977022 | yes |
| lake-2000 | WebP / 95 / 4 | 408,602 | 25.02% | 0.649 | 45.352 | 0.987476 | yes |
| lake-2000 | AVIF / 80 / 8 | 94,162 | 82.72% | 19.909 | 40.207 | 0.956770 | yes |
| lake-2000 | AVIF / 90 / 8 | 251,469 | 53.86% | 19.940 | 45.833 | 0.988059 | yes |
| lake-2000 | AVIF / 90 / 6 | 251,070 | 53.93% | 25.701 | 45.938 | 0.988403 | yes |
| lake-4000 | WebP / 85 / 4 | 782,830 | 57.65% | 1.510 | 40.363 | 0.951860 | yes |
| lake-4000 | WebP / 90 / 4 | 1,213,264 | 34.37% | 1.607 | 42.639 | 0.972404 | yes |
| lake-4000 | WebP / 95 / 4 | 1,856,874 | -0.44% | 1.776 | 45.310 | 0.986196 | no saving |
| lake-4000 | AVIF / 80 / 8 | 505,784 | 72.64% | 35.218 | 40.515 | 0.953010 | ineligible |
| tower | WebP / 85 / 4 | 29,464 | 58.37% | 0.127 | 40.112 | 0.987120 | yes |
| tower | WebP / 90 / 4 | 37,074 | 47.61% | 0.166 | 42.154 | 0.989607 | yes |
| tower | WebP / 95 / 4 | 54,234 | 23.36% | 0.132 | 44.759 | 0.992995 | yes |
| tower | AVIF / 80 / 8 | 22,912 | 67.62% | 2.232 | 39.081 | 0.986465 | yes |
| tower | AVIF / 90 / 8 | 36,358 | 48.62% | 3.349 | 44.044 | 0.991421 | yes |
| tower | AVIF / 90 / 6 | 36,288 | 48.72% | 3.003 | 44.117 | 0.991504 | yes |
| photo | WebP / 85 / 4 | 78,888 | 78.33% | 0.699 | 42.067 | 0.986163 | yes |
| photo | WebP / 90 / 4 | 118,260 | 67.51% | 0.434 | 43.791 | 0.988783 | yes |
| photo | WebP / 95 / 4 | 188,318 | 48.26% | 0.582 | 45.409 | 0.991415 | yes |
| photo | AVIF / 80 / 8 | 51,803 | 85.77% | 18.964 | 43.097 | 0.987225 | yes |
| photo | AVIF / 90 / 8 | 109,489 | 69.92% | 25.632 | 46.382 | 0.991459 | yes |
| photo | AVIF / 90 / 6 | 107,792 | 70.38% | 27.482 | 46.450 | 0.991493 | yes |

Machine-readable results: `phase-2-measurements.json`. The 9 MP AVIF Q80 trial
is measurement evidence only; the automatic pixel cap forbids publishing it.

Visual criterion: compare 320x320 source/output detail crops at native pixel scale
for texture loss, halos, ringing, color shifts, edge damage and gradient banding;
also inspect overall composition. The local contact sheets are under
`target/encoder-dependency-probe/measure/`. An assistant visual review found
Q95 WebP and Q90 AVIF close to their sources in these crops, with subtle
texture/smoothness changes; Q80 AVIF softens detail more. This is not a blinded
human study and does not establish universal near-identity. No actual album cover
corpus was available; graphic/text-artwork tuning remains unvalidated.

Reproduce offline (never use the default network registry):

```sh
export CARGO_HOME="$PWD/target/encoder-dependency-probe/cargo-offline-retry"
cargo fetch --offline --manifest-path target/encoder-dependency-probe/webp/Cargo.toml
cargo fetch --offline --manifest-path target/encoder-dependency-probe/webp-rust/Cargo.toml
cargo fetch --offline --manifest-path target/encoder-dependency-probe/ravif/Cargo.toml
cargo build --offline --release --manifest-path scripts/image-codec-probe/Cargo.toml \
  --target-dir target/photo-codec-probe
python3 scripts/measure-photo-codecs.py \
  --probe target/photo-codec-probe/release/measure-encoders \
  --out target/photo-codec-results \
  lake=target/encoder-dependency-probe/measure/lake-1000.jpg \
  tower=target/encoder-dependency-probe/measure/tower.jpg \
  flower=target/encoder-dependency-probe/measure/photo.jpg
```

## Verification and audit

There is **no available offline Rust AVIF decoder**. Cache inspection found
ravif/avif-serialize encoder/container crates, but no dav1d, rav1d or libaom
Rust decoder dependencies. `image`'s optional `avif-native` feature requires
absent dav1d dependencies. A system libheif benchmark decoder is not a Rust
worker decoder and is not linked or invoked by the worker.

Exactly what production code verifies:

1. Safety-approved stored source, size and expected SHA-256 before encoding;
   the child independently checks the source hash and decodes admitted JPEG/PNG
   with edge/pixel/allocation limits.
2. Lossless WebP: decode using Rust `image`/`image-webp`, exact RGBA equality.
   Lossy WebP: same Rust decoder, exact dimensions and alpha equality. There is
   no per-image PSNR/SSIM or perceptual acceptance gate in production.
3. AVIF: successful ravif encode and output cap, then parent size, SHA-256 and
   `ftyp`/`avif` signature checks. **No AVIF decode, decoded dimensions, decoded
   alpha or pixel roundtrip is verified by the worker.** These signature checks
   are not a full container validation. Alpha quality100 is an encoder setting,
   not an independently proven lossless-alpha guarantee.
4. Unique derivative key, exact byte-for-byte object storage read-back, then
   fresh source safety/revision recheck before write-once DB registration.
   Registered-row reuse verifies storage bytes, recorded size, digest and format
   signature. No original is overwritten or deleted. An uncertain DB commit can
   leave an orphan object; it is never deleted speculatively.

Automatic work registers WebP first, attempts AVIF second, and retries reuse
an already registered WebP. AVIF failure does not remove the fallback. At most
three attempts; retry delays are 5 then 10 minutes (`2^(attempts-1)`). Queue stays
at 256 slots. Global transaction advisory lock limits active image conversion to
one across replicas; the existing worker capacity permit and CPU <50% / memory
<70% start gates remain, with unknown pressure deferring work. The child runs
nice19 in the credential-free Landlock/seccomp parser sandbox with 1 GiB
address-space limit and 120-second parent wall timeout. A final crash-path audit
found that kill-on-drop alone did not cover SIGKILL of the worker. The codec now
also sets Linux `PR_SET_PDEATHSIG=SIGKILL` and verifies its captured parent PID
in the child, closing the fork-to-prctl race. A subprocess test checks that the
signal is set on the forking thread and that nice19 is applied. These load gates are start gates,
not a CPU reservation or mid-encode pressure cancellation.

Both stored-asset and static-sidecar negotiation now use AVIF → WebP → original,
including AVIF-only to WebP fallback unless WebP is explicitly excluded.
Wildcards alone never opt in. Immutable stored-asset fallbacks keep `private,
no-cache` while a requested preferred variant is missing; completed AVIF gets
private immutable caching. Static public-asset cache rules remain their existing
rules, including revalidation for fingerprinted pending variants. Vary and
representation ETags remain intact. Source stays available on all optional skips
and errors. No new HTTP-request conversion was introduced.

## Validation

The three required dependency fetches (`webp`, `webp-rust`, `ravif`) all exited
0 with `--offline` and the specified checkout-local CARGO_HOME. The isolated
benchmark package also fetched offline and built in release mode successfully.
The tracked measurement script reproduced all six tower candidates with exact
byte sizes and PSNR/SSIM matches. Python syntax compilation passed for both
measurement modules. A separate 16x16 AVIF smoke encode with alpha values 0..255 was decoded through
libheif: dimensions were 16x16 and all 256 alpha values matched (max error 0).
This is benchmark-only evidence and does not add worker AVIF decoding.

Initial broad `cargo test --offline -p audeniq-core --lib image_ -- --nocapture`:
**15 passed, 4 failed**. Three pre-existing `qc::tests::image_*` tests reported
`IMAGE_PROBE_FAILED` / `image analyzer unavailable or timed out`, because the
separate `audeniq-music photo` engine is unavailable. The fourth was a mistake in
the new parent-death test: Linux's parent-death setting is thread-specific and
libtest queried it on a new test thread (observed 0 instead of 9). A diagnostic
confirmed this; the test now queries the child forking thread before exec.
An interim QC-excluded run before that correction was **15 passed, 1 failed**.
`cargo test --offline -p audeniq-core --bin audeniq-worker image_load_tests`:
**1 passed, 0 failed**.

Final corrected `cargo test --offline -p audeniq-core --lib image_ -- --skip qc::tests`:
**16 passed, 0 failed**, including all **9 encoder tests**.

The same freshly built unit-test executable, filtered to `parser_sandbox::tests`:
**2 passed, 0 failed**, exercising allocation failure in a child and preservation
of a lower inherited address-space limit. Python emitted a denied-sitecustomize
warning under the enforced sandbox; both memory assertions passed.

`cargo test --offline -p audeniq-core --test image_serving --no-run`: succeeded;
the produced executable lists **6 tests**. They were not executed because the
disposable database could not be initialized as described below.

`cargo build --offline -p audeniq-core --bin audeniq-image-encode`: succeeded.
`cargo fmt --all -- --check`, benchmark helper `rustfmt --check`, Python syntax
checks, and `git diff --check`: passed.

An additional local Rust harness invokes the built encoder through the real
`parser_sandbox::restrict` policy (Landlock/seccomp and 1 GiB address-space limit),
with a 120-second child timeout. All **4 smoke encodes passed**, and independent
Pillow/libheif decoding confirmed the output dimensions. This harness needs no
database/storage or external codec executable.

| Input | Built application mode | Output bytes | Seconds | Decoded dimensions |
|---|---|---:|---:|---|
| tower | sandbox-webp | 215,676 | 0.238 | 512×512 |
| tower | sandbox-lossy-webp | 54,318 | 0.205 | 512×512 |
| tower | sandbox-avif | 36,383 | 1.810 | 512×512 |
| lake-2000 | sandbox-avif | 251,384 | 13.853 | 2000×1125 |

These use the application's Rust JPEG decode, so their encoded sizes differ
slightly from the earlier Pillow-input quality grid. The 2.25 MP AVIF succeeds
under the actual 1 GiB parser limit. This is codec/sandbox verification only;
DB registration and object-store read-back are still unexecuted. The local
harness and results remain under `target/encoder-dependency-probe/`.


PostgreSQL 16 server tools were found under `/usr/lib/postgresql/16/bin`, despite
not being on PATH. An isolated `/tmp` cluster attempt failed before initialization:
`os.chown` returned `[Errno 22] Invalid argument`, and running initdb as the
postgres user returned verbatim `runuser: cannot set groups: Operation not
permitted` (exit 1). The process runs as root, and the sandbox does not permit
switching to a database owner. No database/server was contacted or started, and
the empty temporary directory was removed. Migration/trigger/grants execution
and DB-backed tests remain unverified for this exact reason. Docker/Podman
executables are absent, so the image build/container sandbox check is unverified.

## File inventory

Phase 2 files, including retained partial work (all remain uncommitted):

- `crates/core/src/image_encoder.rs`: finalize measured constants, reject oversized
  AVIF before decode, bind codec lifetime to the worker, and test child settings,
  lossy alpha, AVIF signature, ICC and pixel eligibility.
- `crates/core/src/image_variants.rs`: expose the shared fallback eligibility rule.
- `crates/core/src/studio_static.rs`: apply that rule to sidecars and test AVIF-only
  fallback and explicit WebP exclusion.
- `crates/core/tests/image_serving.rs`: update automatic publication assertions for
  AVIF preference and independently decoded lossy WebP dimensions/alpha.
- `crates/core/src/image_jobs.rs`: format and retain WebP-first registration,
  reuse on AVIF retry, bounded attempts and exponential backoff.
- `crates/core/src/bin/audeniq-image-encode.rs`: retain the inherited sandbox
  dispatch for lossless WebP, lossy WebP and AVIF.
- `crates/core/Cargo.toml`: retain the inherited exact codec pins/default disabling.
- `Cargo.lock`: retain the inherited 386-line codec dependency addition.
- `scripts/image-codec-probe/Cargo.toml`: isolated pinned offline benchmark package.
- `scripts/image-codec-probe/Cargo.lock`: reproducible benchmark dependencies.
- `scripts/image-codec-probe/src/main.rs`: local Rust WebP/AVIF quality-grid helper.
- `scripts/measure-photo-codecs.py`: decoded-source photo measurement runner.
- `scripts/measure_avif.py`: benchmark-only ctypes decoding with existing libheif.
- `docs/image-pipeline/phase-2.md`: choices, evidence, audit and limitations.
- `docs/image-pipeline/phase-2-measurements.json`: 28 rows with source SHA-256 values.
- `docs/image-pipeline/round-2.md`: link the historical report to this continuation.
- `docs/image-pipeline/round-3-dependency-diagnosis.md`: link the historical failed
  dependency diagnosis to the restored-cache outcome.

Audited and retained from previous work: root `Cargo.lock` and core `Cargo.toml`
codec pins, worker load gates, `lib.rs` job module registration, uploads/cache
handling, `migrations/0162_image_jobs.sql`, worker-only grants, original graphic
measurement script, Docker build changes and backend 5xx handling. Pre-existing
`deploy/aws/*` modifications were left untouched; this continuation performed no
AWS/deployment operation.
