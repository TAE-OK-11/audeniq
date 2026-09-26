# Audio QC policy (Stage 1, rule version 4)

This document explains each Stage 1 audio check: what it measures, how strict
it is, and why. The code lives in `crates/core/src/qc.rs` (thresholds are
`pub const` there) and `crates/core/src/submission.rs` (orchestration). It was
written in response to the 2026-09 sandbox distribution test
(`audeniq-sandbox-tests/REPORT.md`).

Check outcomes:

- **BLOCKED**: integrity failure, e.g. the bytes are not the bytes that were uploaded.
- **CORRECTION_REQUIRED**: the release returns to `STAGE1_CORRECTION`. The artist
  replaces the audio and resubmits.
- **REVIEW_REQUIRED**: a warning only. It never stops the release on its own.
- **TECHNICAL_RETRY**: our side failed (analyzer unavailable, storage error). The
  job is retried; see "Giving up" below.

## Upload-time gates (before any QC job)

| Gate | Rule |
|---|---|
| `UPLOAD_TYPE_UNSUPPORTED` | Audio must be declared `audio/wav`, `audio/x-wav` or `audio/flac`. Images must be `image/png` or `image/jpeg`. |
| `UPLOAD_EMPTY` | Size must be at least 1 byte. |
| `UPLOAD_AUDIO_TOO_LARGE` / `UPLOAD_IMAGE_TOO_LARGE` | Audio is limited to 512 MiB (about 50 min of 24-bit/48 kHz stereo). Images are limited to 20 MiB. |
| `UPLOAD_CONTENT_MISMATCH` | At `complete`, the stored object is streamed once. Its SHA-256 is recorded in `catalog.assets.sha256`, and its magic number must match the declared type. An MP3 or FLAC renamed `.wav` is never registered. |

Every API error now carries a stable `code` and a human-readable `message`.

## Stage 1 audio checks

| Code | Outcome | Rule |
|---|---|---|
| `SHA256_MISMATCH` | BLOCKED | The file analyzed must hash to the value recorded at upload. |
| `AUDIO_MAGIC_MISMATCH` | CORRECTION | The container is detected from content, not from the file name. MP3 (lossy) is always refused. WAV must be declared as WAV, and FLAC as FLAC. |
| `AUDIO_PROBE_FAILED` | CORRECTION | ffprobe cannot parse the file (for example a valid `RIFF` signature followed by a corrupt header). This used to loop on TECHNICAL_RETRY. |
| `AUDIO_SAMPLE_FORMAT_UNSUPPORTED` | CORRECTION | WAV must be integer PCM of at most 24 bits (`pcm_s16le`, `pcm_s24le`, `pcm_u8`/`pcm_s8`; 8-bit then fails the bit-depth check). FLAC must be at most 24 bits. See "Float WAV" below. |
| `AUDIO_TOO_SHORT` | CORRECTION | Minimum length is 30 s (the common DSP minimum for a monetizable track). |
| `AUDIO_SAMPLE_RATE_LOW` | CORRECTION | Minimum sample rate is 44.1 kHz. |
| (sample rate > 192 kHz) | CORRECTION | Reported under `AUDIO_SAMPLE_FORMAT_UNSUPPORTED`: 192 kHz is the DSP hi-res ceiling. A file with a rejected sample format or channel layout is **not decoded** at all (a 9-minute 384 kHz 8-channel "bomb" used to be fully decoded and then stuck in `STAGE1_RUNNING`). |
| `AUDIO_BIT_DEPTH_LOW` | CORRECTION | Minimum bit depth is 16. For FLAC it is read from `bits_per_raw_sample`. |
| `AUDIO_CHANNEL_INVALID` | CORRECTION | The file must be mono or stereo. |
| `AUDIO_TRUNCATED` | CORRECTION | For WAV, the `data` chunk may not declare more bytes than the file holds. For any format, the decoded length may not fall short of the header duration by more than max(1 s, 1 %). A WAV cut at 80 % used to pass. |
| `AUDIO_SILENT` | CORRECTION | Digital silence: the file's peak is below −80 dBFS (lossless encoders leave about −91 dBFS on synthetic silence). Or inaudible content (rule 4): nothing above the EBU R128 absolute gate (≤ −70 LUFS or unmeasurable) with ≥ 99 % of 50 ms blocks below −60 dBFS (faint hiss/dither), or less than 1 s in total above −80 dBFS (a lone click on silence, which R128 gating measures at about −45 LUFS). A quiet real programme (e.g. a tone at −63 dBFS) is not silent; it stays a review-only "near-silent" suspicion. The loudness meter's `-inf` output is handled, not a parse failure. |
| `AUDIO_CLIPPING` | CORRECTION or REVIEW | A clip event is 3 or more consecutive samples at or above 0.999 full scale. If clip events cover at least 0.1 % of all samples, the result is CORRECTION: the waveform is audibly flattened. Any smaller amount of clipping, or a true peak above 0 dBTP (inter-sample overs), gives a REVIEW warning. |
| `AUDIO_LOUDNESS_OUT_OF_RANGE` | REVIEW only | Integrated loudness and true peak (EBU R128) are always recorded. Outside −14 ±1 LUFS, or true peak above −1 dBTP, the result is a **warning only**. |
| `AUDIO_CONTENT_SUSPECT` | REVIEW only | Spam/filler heuristics (see below). Routes the release to a human; never blocks on its own. |
| `AUDIO_FINGERPRINT_FAILED` | TECHNICAL_RETRY | The perceptual fingerprint could not be computed. |
| `AUDIO_SIMILAR_TO_EXISTING` | REVIEW | The track is perceptually similar to other audio in the same account **or in another account** (rule 3). |
| `ASSET_REUSED` | REVIEW | The same uploaded master is already on a track of another submitted release in this account under a different or missing ISRC. Reusing a recording (single → album) with its original ISRC passes. |

Stage 1 also runs account-policy checks on the frozen revision (not audio,
listed here because they share the correction flow):
`TEXT_INVALID_CHARACTERS` (control/invisible/bidi characters),
`ARTIST_NAME_PROTECTED` (see `docs/PROTECTED_ARTISTS.md`) and
`IDENTIFIER_IN_USE` (UPC/ISRC already used by another release or track in the
account, or in the identifier ledger). All three are CORRECTION: they used to
pass Stage 1/2 and then fail packaging permanently.

### Content heuristics (`AUDIO_CONTENT_SUSPECT`)

Measured in the same streamed decode pass (50 ms blocks):

| Reason | Rule (constants in `qc.rs`) |
|---|---|
| near-silent programme | integrated loudness below −45 LUFS (`NEAR_SILENT_LUFS`), or below the loudness gate, while not digitally silent |
| long silence | ≥ 50 % of blocks below −60 dBFS, or one silent stretch ≥ 30 s (e.g. 5 s of audio padded with 45 s of silence) |
| one channel silent | stereo file where one channel peaks below −60 dBFS and the other does not |
| steady broadband noise | zero-crossing rate ≥ 0.30 and block-level standard deviation < 1.5 dB (white noise) |
| loop | the block-energy envelope repeats with mean difference < 0.75 dB at some lag between 1 and 15 s, at least 4 times, in a track with real dynamics (std ≥ 3 dB) |

These are deliberately review-only: ambient, noise and minimalist music
exist, and a human decides.

### Why loudness is not enforced

Spotify, Apple Music, YouTube and others normalize playback loudness. None of
them reject a master for being loud or quiet. Most commercial masters measure
between −10 and −6 LUFS, so enforcing −14 ±1 would reject nearly all of them.
The measurement is kept so the review UI can advise the artist.

### Float WAV

32-bit float and 32-bit integer WAV are **rejected with a correction
request**. The reasons:

- The DDEX/DSP audio profiles we package for accept 16- or 24-bit linear PCM.
- Several ingestion pipelines reject IEEE-float WAV outright.
- Float masters can carry sample values above 0 dBFS, which silently clip on
  conversion.

The correction message asks for a 24-bit PCM export. This is deliberately
strict rather than converting on the artist's behalf: silent conversion would
change the bytes the artist approved.

### Fingerprinting long tracks

Only the first 10 minutes (`fingerprint::MAX_FINGERPRINT_SECS`) are
fingerprinted. That is plenty to detect re-uploads and duplicates. Before this
change, fingerprinting decoded the whole file, so 11- and 45-minute masters hit
the 600 s timeout and died after 5 retries. Loudness, clipping and truncation
analysis still cover the whole file, in a single streamed ffmpeg pass. Its
timeout scales with duration (60 s + duration/2, capped at 1 h).

### Memory and disk

The worker streams the object to a temp file under `TMPDIR` (a volume in
compose, not the 16 MiB `/tmp` tmpfs). It then analyzes the file with
ffmpeg/ffprobe child processes, which read from disk. Worker memory does not
grow with file size: a 45-minute 476 MB master analyzes at about 55 MB RSS.
Upload completion hashes the object with a streamed read in constant memory.

## Giving up and recovery

A Stage 1 job that exhausts its retries does not leave the release stuck in
`STAGE1_RUNNING`. This covers both an explicit TECHNICAL_RETRY on the last
attempt and a worker whose lease expired. In either case the job is
dead-lettered and, in the same transaction:

- **Stage 1**: the release moves to `STAGE1_CORRECTION` with a
  `QC_ANALYSIS_FAILED` check. Its human-readable detail asks the artist to
  re-export or re-upload. The artist can edit tracks and credits (replace
  audio), archive the release, or resubmit. Migration 0031 allows draft edits
  in `STAGE1_CORRECTION`.
- **Stage 2**: the release moves to `STAGE2_REVIEW` for a human operator.

### RF64 / BW64

EBU RF64/BW64 WAV (the >4 GiB variant) is accepted: the container sniffer
treats `RF64....WAVE` / `BW64....WAVE` as WAV (it used to be refused with
`detected=UNKNOWN`), ffmpeg reads it as `wav`, and the RIFF truncation check
skips its 0xFFFFFFFF placeholder sizes (decoded-length truncation still applies).

## Recoverability (all stages)

No automatic failure may leave a release in a running state. In addition to
Stage 1/2 above (rule 2):

- **Stage 3 (`prepare_release`)**: a permanent failure (identifier conflict,
  invalid metadata) is surfaced immediately; a transient one after its
  retries. The release moves to `STAGE3_CORRECTION` with a
  `STAGE3_PREPARATION_FAILED` check whose message says what to fix; the
  artist edits and resubmits (migration 0032: `STAGE3_CORRECTION → SUBMITTED`,
  and `catalog.is_editable_status` allows edits in DRAFT and every
  `*_CORRECTION` state).
- The 64 MiB preflight/execution cap is gone: preflight and delivery
  verification hash assets with a streamed read up to the 512 MiB upload limit
  (a 90 MB, 8-minute WAV used to die in `STAGE3_PREPARING`).
- **Delivery**: a send job that finds its delivery job leased by a crashed
  worker waits for that lease without consuming attempts; if the delivery job
  has no attempts left it is marked `DEAD_LETTER` instead of staying `LEASED`.
- **Worker shutdown**: SIGTERM/SIGINT stop claiming and wait up to
  `WORKER_DRAIN_SECONDS` (default 30) for in-flight jobs before exiting.

## Not covered yet (follow-ups)

- **AIFF uploads.**
- **Deep structural validation**: FLAC MD5 / STREAMINFO `total_samples`
  mismatches, WAV+payload polyglots, large junk LIST/ID3 chunks and forged
  header sample rates are not detected yet (ffmpeg decodes them "successfully").
- **Pitch/speed-shifted or reversed copies**: the fingerprint is not invariant
  to these.
- **Leading/trailing silence and gaps inside a track.** Only the whole-file
  silence check exists today.
- **Lossy sources transcoded to WAV/FLAC.** Detecting this needs a
  spectral-cutoff heuristic, which risks false positives.
- **Scaling cross-account similarity**: rule 3 scans every fingerprint of the
  same algorithm version (migration 0034, SECURITY DEFINER read). A large
  catalog needs an indexed (LSH) lookup.
