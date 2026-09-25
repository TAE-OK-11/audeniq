# Audio QC policy (Stage 1, rule version 2)

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
| `AUDIO_BIT_DEPTH_LOW` | CORRECTION | Minimum bit depth is 16. For FLAC it is read from `bits_per_raw_sample`. |
| `AUDIO_CHANNEL_INVALID` | CORRECTION | The file must be mono or stereo. |
| `AUDIO_TRUNCATED` | CORRECTION | For WAV, the `data` chunk may not declare more bytes than the file holds. For any format, the decoded length may not fall short of the header duration by more than max(1 s, 1 %). A WAV cut at 80 % used to pass. |
| `AUDIO_SILENT` | CORRECTION | The file's peak is below −80 dBFS, i.e. digital silence. The loudness meter's `-inf` output is handled, not a parse failure. |
| `AUDIO_CLIPPING` | CORRECTION or REVIEW | A clip event is 3 or more consecutive samples at or above 0.999 full scale. If clip events cover at least 0.1 % of all samples, the result is CORRECTION: the waveform is audibly flattened. Any smaller amount of clipping, or a true peak above 0 dBTP (inter-sample overs), gives a REVIEW warning. |
| `AUDIO_LOUDNESS_OUT_OF_RANGE` | REVIEW only | Integrated loudness and true peak (EBU R128) are always recorded. Outside −14 ±1 LUFS, or true peak above −1 dBTP, the result is a **warning only**. |
| `AUDIO_FINGERPRINT_FAILED` | TECHNICAL_RETRY | The perceptual fingerprint could not be computed. |
| `AUDIO_SIMILAR_TO_EXISTING` | REVIEW | The track is perceptually similar to other audio in the same account. |

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

## Not covered yet (follow-ups)

- **AIFF uploads.** RF64/BW64 files larger than 4 GiB are also not covered.
- **Leading/trailing silence and gaps inside a track.** Only the whole-file
  silence check exists today.
- **Lossy sources transcoded to WAV/FLAC.** Detecting this needs a
  spectral-cutoff heuristic, which risks false positives.
- **Cross-account fingerprint comparison.** It needs a tenant-safe index
  outside RLS and a policy on what to disclose.
