//! Perceptual audio fingerprinting for similarity detection.
//!
//! SHA-256 proves byte identity; it cannot catch the same recording
//! re-encoded, trimmed, re-mastered, or lightly edited. This module
//! computes a Philips-style robust hash: 32-bit sub-fingerprints derived
//! from spectral-energy differentials across time and frequency, compared
//! by bit error rate (BER) over the best alignment.
//!
//! Design notes:
//! - Decode is via `ffmpeg` (already a hard QC dependency): mono,
//!   11025 Hz, 16-bit PCM. No new system dependencies.
//! - 33 logarithmically spaced bands over 300-3000 Hz give 32 differential
//!   bits per frame (Philips robust hash construction).
//! - Similarity is BER = Hamming distance / compared bits, minimized over
//!   alignments. Random audio scores ~0.5; the same master re-encoded
//!   scores < 0.10.
//! - A match is REVIEW_REQUIRED, never an auto-block: similarity is a
//!   human judgement, not a verifiable violation.

use crate::error::{Error, Result};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Fingerprint algorithm version, stored alongside the hash so a future
/// algorithm change can coexist with (not silently mix with) old rows.
/// v2 = segmented coverage (head/middle/tail windows); v1 covered the
/// first 600 s contiguously.
pub const FINGERPRINT_VERSION: i16 = 2;

const SAMPLE_RATE: u32 = 11025;
/// Sample rate of the PCM the fingerprint operates on.
pub const FINGERPRINT_SAMPLE_RATE: u32 = SAMPLE_RATE;
const FRAME_SIZE: usize = 2048; // ~186 ms at 11025 Hz
const FRAME_HOP: usize = 1024; // ~93 ms
/// 33 bands -> 32 differential bits per sub-fingerprint.
const N_BANDS: usize = 33;
const FREQ_MIN: f64 = 300.0;
const FREQ_MAX: f64 = 3000.0;

/// BER at or below this means near-duplicate (same master, re-encoded).
pub const NEAR_DUPLICATE_BER: f64 = 0.10;
/// BER at or below this means similar (same recording, different master).
pub const SIMILAR_BER: f64 = 0.25;
/// Minimum overlapping frames for a comparison (~3 s). Shorter overlaps
/// are not statistically meaningful.
const MIN_OVERLAP_FRAMES: usize = 32;

/// PolicyGate code used when audio is too short for a meaningful
/// fingerprint. Not transient: retrying the same bytes will not help.
pub const TOO_SHORT_CODE: &str = "AUDIO_TOO_SHORT_FOR_FINGERPRINT";

/// Number of segments a v2 fingerprint covers, and each segment's length.
/// 3 x 30 s spread over the track replaces the contiguous first-600 s
/// coverage: ~4.7x less decode + FFT work, while the discriminative parts
/// (intro, chorus, outro) stay covered.
pub const FINGERPRINT_SEGMENTS: usize = 3;
pub const FINGERPRINT_SEGMENT_SECS: f64 = 30.0;

/// Time windows (start, length) in seconds covered by a v2 fingerprint:
/// head, middle, and tail of the track. Tracks shorter than the total
/// coverage get a single whole-track window; non-positive durations get
/// none.
pub fn segment_windows(duration_secs: f64) -> Vec<(f64, f64)> {
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Vec::new();
    }
    let total = FINGERPRINT_SEGMENTS as f64 * FINGERPRINT_SEGMENT_SECS;
    if duration_secs <= total {
        return vec![(0.0, duration_secs)];
    }
    let seg = FINGERPRINT_SEGMENT_SECS;
    vec![
        (0.0, seg),
        ((duration_secs / 2.0 - seg / 2.0).max(0.0), seg),
        ((duration_secs - seg).max(0.0), seg),
    ]
}

pub struct Fingerprint {
    /// 32-bit sub-fingerprints, one per frame, in time order.
    pub frames: Vec<u32>,
    pub duration_secs: f64,
}

impl Fingerprint {
    pub fn is_empty(&self) -> bool {
        self.frames.len() < MIN_OVERLAP_FRAMES + 1
    }

    /// Serialize to the `catalog.asset_fingerprints.hash` bytea layout:
    /// 4 bytes big-endian per frame, in frame order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.frames.len() * 4);
        for f in &self.frames {
            out.extend_from_slice(&f.to_be_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let (chunks, remainder) = bytes.as_chunks::<4>();
        if !remainder.is_empty() || bytes.is_empty() {
            return Err(Error::Internal);
        }
        let frames = chunks
            .iter()
            .map(|chunk| u32::from_be_bytes(*chunk))
            .collect::<Vec<_>>();
        Ok(Fingerprint {
            frames,
            duration_secs: 0.0,
        })
    }
}

/// Decode one window of an audio file to mono 11025 Hz f32 PCM via
/// ffmpeg (`-ss`/`-t` as input options, so the decoder seeks instead of
/// decoding from the start). Uses the same helper-thread + timeout pattern
/// as `qc::probe_with` so a hung decoder cannot wedge the worker.
fn decode_window(path: &Path, start_secs: f64, len_secs: f64) -> Result<Vec<f32>> {
    use std::io::Read;
    let mut command = Command::new("ffmpeg");
    command.args([
        "-v",
        "error",
        "-filter_threads",
        "1",
        "-threads",
        "1",
        "-ss",
        &start_secs.to_string(),
        "-t",
        &len_secs.to_string(),
        "-i",
        path.to_str().ok_or(Error::Internal)?,
        "-ac",
        "1",
        "-ar",
        &SAMPLE_RATE.to_string(),
        "-f",
        "s16le",
        "-acodec",
        "pcm_s16le",
        "-threads",
        "1",
        "-",
    ]);
    crate::parser_sandbox::restrict_audio(&mut command, &[path], &[])
        .map_err(|_| Error::Internal)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = crate::analyzer_process::AnalyzerProcess::spawn(&mut command, deadline)
        .map_err(|_| Error::Internal)?;
    let stdout = child.stdout.take().ok_or(Error::Internal)?;
    let stdout = child.reader(stdout).map_err(|_| Error::Internal)?;
    let max_bytes = ((len_secs * SAMPLE_RATE as f64).ceil() as usize + 1) * 2;
    let (tx, rx) = std::sync::mpsc::channel();
    child
        .spawn_reader("fingerprint-decode", move || {
            let mut buf = Vec::new();
            let r = stdout
                .take(max_bytes as u64 + 1)
                .read_to_end(&mut buf)
                .map(|_| buf);
            let _ = tx.send(r);
        })
        .map_err(|_| Error::Internal)?;
    let bytes = match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
    {
        Ok(Ok(buf)) => buf,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Internal);
        }
    };
    if bytes.len() > max_bytes {
        // Defensive: `-t` bounds the output, but never block in wait() on a
        // decoder that is still writing into a pipe nobody reads.
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::Internal);
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => return Err(Error::Internal),
        }
    };
    let (sample_chunks, remainder) = bytes.as_chunks::<2>();
    if !status.success() || !remainder.is_empty() {
        return Err(Error::Internal);
    }
    Ok(sample_chunks
        .iter()
        .map(|chunk| i16::from_le_bytes(*chunk) as f32 / 32768.0)
        .collect())
}

/// FFT bin edges for 33 logarithmically spaced bands over [300, 3000] Hz,
/// computed once.
fn band_edges() -> &'static [usize; N_BANDS + 1] {
    static EDGES: std::sync::OnceLock<[usize; N_BANDS + 1]> = std::sync::OnceLock::new();
    EDGES.get_or_init(|| {
        let bin_hz = SAMPLE_RATE as f64 / FRAME_SIZE as f64;
        let mut edges = [0usize; N_BANDS + 1];
        for (m, edge) in edges.iter_mut().enumerate() {
            let freq = FREQ_MIN * (FREQ_MAX / FREQ_MIN).powf(m as f64 / N_BANDS as f64);
            *edge = (freq / bin_hz).round() as usize;
        }
        edges[N_BANDS] = edges[N_BANDS].min(FRAME_SIZE / 2);
        edges
    })
}

/// Hann window, precomputed once.
fn hann_window() -> &'static [f32] {
    static WINDOW: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    WINDOW.get_or_init(|| {
        (0..FRAME_SIZE)
            .map(|n| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / FRAME_SIZE as f32).cos())
            })
            .collect()
    })
}

fn fft_plan() -> &'static std::sync::Arc<dyn Fft<f32>> {
    static FFT: std::sync::OnceLock<std::sync::Arc<dyn Fft<f32>>> = std::sync::OnceLock::new();
    FFT.get_or_init(|| FftPlanner::<f32>::new().plan_fft_forward(FRAME_SIZE))
}

/// Compute the perceptual fingerprint of an audio file.
/// Perceptual fingerprint over the configured [`segment_windows`]
/// (head/middle/tail), decoded with ffmpeg's own resampler to mono
/// 11025 Hz. Each window is decoded independently with input seeking, so a
/// 3:30 track decodes 90 s of audio instead of the whole track.
pub fn compute_fingerprint(path: &Path, duration_secs: f64) -> Result<Fingerprint> {
    let mut segments: Vec<Vec<f32>> = Vec::new();
    for (start, len) in segment_windows(duration_secs) {
        segments.push(decode_window(path, start, len)?);
    }
    let refs: Vec<&[f32]> = segments.iter().map(|v| v.as_slice()).collect();
    fingerprint_from_segments(&refs)
}

/// Compute the fingerprint over multiple non-contiguous segments, avoiding
/// fake boundary frames: each segment is fingerprinted separately and the
/// frame vectors are concatenated in order. The segments are &[f32] slices
/// of mono 11025 Hz PCM.
pub fn fingerprint_from_segments(segments: &[&[f32]]) -> Result<Fingerprint> {
    let capacity = segments
        .iter()
        .map(|seg| seg.len().saturating_sub(FRAME_SIZE) / FRAME_HOP)
        .sum();
    let mut frames = Vec::with_capacity(capacity);
    let mut workspace = FingerprintWorkspace::new();
    let mut total_secs = 0.0;
    for seg in segments {
        if seg.is_empty() {
            continue;
        }
        // Reset the energy history at every segment boundary, while reusing
        // FFT buffers across all windows of this asset.
        workspace.append_frames(seg, &mut frames)?;
        total_secs += seg.len() as f64 / SAMPLE_RATE as f64;
    }
    if frames.len() < MIN_OVERLAP_FRAMES + 1 {
        return Err(Error::PolicyGate(TOO_SHORT_CODE));
    }
    Ok(Fingerprint {
        frames,
        duration_secs: total_secs,
    })
}

/// Compute the perceptual fingerprint from mono 11025 Hz f32 PCM samples.
///
/// This is the FFT/differential core shared by the file-decode entry point
/// and the QC fast path, where the segment samples are tapped out of the
/// single `decode_analysis` pass instead of a second ffmpeg decode.
pub fn fingerprint_from_samples(samples: &[f32]) -> Result<Fingerprint> {
    let mut frames = Vec::with_capacity(samples.len().saturating_sub(FRAME_SIZE) / FRAME_HOP);
    FingerprintWorkspace::new().append_frames(samples, &mut frames)?;
    Ok(Fingerprint {
        frames,
        duration_secs: samples.len() as f64 / SAMPLE_RATE as f64,
    })
}

/// Each differential needs only the current and previous band energies.
/// Keep FFT scratch per asset instead of a duration-sized energy matrix and
/// a fresh thread pool for every segment. The worker already runs assets in
/// parallel; the immutable FFT plan remains shared across those assets.
struct FingerprintWorkspace {
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl FingerprintWorkspace {
    fn new() -> Self {
        Self {
            buf: vec![Complex::new(0.0, 0.0); FRAME_SIZE],
            scratch: vec![Complex::new(0.0, 0.0); fft_plan().get_inplace_scratch_len()],
        }
    }

    fn append_frames(&mut self, samples: &[f32], frames: &mut Vec<u32>) -> Result<()> {
        if samples.len() < FRAME_SIZE + FRAME_HOP * MIN_OVERLAP_FRAMES {
            return Err(Error::PolicyGate(TOO_SHORT_CODE));
        }
        let n_frames = (samples.len() - FRAME_SIZE) / FRAME_HOP + 1;
        let mut previous = [0.0f32; N_BANDS];
        for i in 0..n_frames {
            let pos = i * FRAME_HOP;
            let bands = self.band_energies(&samples[pos..pos + FRAME_SIZE]);
            if i > 0 {
                frames.push(differential(&previous, &bands));
            }
            previous = bands;
        }
        Ok(())
    }

    fn band_energies(&mut self, samples: &[f32]) -> [f32; N_BANDS] {
        let window = hann_window();
        let edges = band_edges();
        for (j, b) in self.buf.iter_mut().enumerate() {
            b.re = samples[j] * window[j];
            b.im = 0.0;
        }
        fft_plan().process_with_scratch(&mut self.buf, &mut self.scratch);
        let mut bands = [0.0f32; N_BANDS];
        for m in 0..N_BANDS {
            let lo = edges[m];
            let hi = edges[m + 1].max(lo + 1);
            let mut e = 0.0f32;
            for value in self.buf.iter().take(hi.min(FRAME_SIZE / 2)).skip(lo) {
                e += value.norm_sqr();
            }
            // Preserve the f32 summation and floor used by stored v2 hashes.
            bands[m] = (e + 1e-10).ln();
        }
        bands
    }
}

fn differential(previous: &[f32; N_BANDS], bands: &[f32; N_BANDS]) -> u32 {
    let mut bits = 0u32;
    for m in 0..N_BANDS - 1 {
        let d = (previous[m] - previous[m + 1]) - (bands[m] - bands[m + 1]);
        if d > 0.0 {
            bits |= 1 << m;
        }
    }
    bits
}

/// Fingerprint one tapped window with only one overlapping PCM frame in
/// memory. A separate builder per window keeps segment boundaries intact.
pub(crate) struct FingerprintBuilder {
    workspace: FingerprintWorkspace,
    pending: Vec<f32>,
    previous: Option<[f32; N_BANDS]>,
    frames: Vec<u32>,
    samples: usize,
}

impl FingerprintBuilder {
    pub(crate) fn new() -> Self {
        Self {
            workspace: FingerprintWorkspace::new(),
            pending: Vec::with_capacity(FRAME_SIZE),
            previous: None,
            frames: Vec::new(),
            samples: 0,
        }
    }

    pub(crate) fn push_s16le(&mut self, mut pcm: &[[u8; 2]]) {
        self.samples += pcm.len();
        while !pcm.is_empty() {
            let take = pcm.len().min(FRAME_SIZE - self.pending.len());
            self.pending.extend(
                pcm[..take]
                    .iter()
                    .map(|s| i16::from_le_bytes(*s) as f32 / 32768.0),
            );
            pcm = &pcm[take..];
            if self.pending.len() == FRAME_SIZE {
                let bands = self.workspace.band_energies(&self.pending);
                if let Some(previous) = &self.previous {
                    self.frames.push(differential(previous, &bands));
                }
                self.previous = Some(bands);
                self.pending.copy_within(FRAME_HOP.., 0);
                self.pending.truncate(FRAME_SIZE - FRAME_HOP);
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.samples == 0
    }

    pub(crate) fn finish(self) -> Result<Fingerprint> {
        if self.samples < FRAME_SIZE + FRAME_HOP * MIN_OVERLAP_FRAMES {
            return Err(Error::PolicyGate(TOO_SHORT_CODE));
        }
        Ok(Fingerprint {
            frames: self.frames,
            duration_secs: self.samples as f64 / SAMPLE_RATE as f64,
        })
    }
}

/// Bit error rate between two sub-fingerprint sequences at a fixed offset.
/// `offset` shifts `b` relative to `a`: negative = b starts earlier.
fn ber_at_offset(a: &[u32], b: &[u32], offset: isize) -> Option<f64> {
    let (a_lo, b_lo) = if offset >= 0 {
        (offset as usize, 0)
    } else {
        (0, (-offset) as usize)
    };
    let overlap = (a.len().saturating_sub(a_lo)).min(b.len().saturating_sub(b_lo));
    if overlap < MIN_OVERLAP_FRAMES {
        return None;
    }
    let mut dist = 0u64;
    for i in 0..overlap {
        dist += (a[a_lo + i] ^ b[b_lo + i]).count_ones() as u64;
    }
    Some(dist as f64 / (overlap as f64 * 32.0))
}

/// Minimum BER over all alignments with sufficient overlap.
/// Returns `None` when either sequence is too short for a meaningful
/// comparison.
pub fn bit_error_rate(a: &[u32], b: &[u32]) -> Option<f64> {
    if a.len() < MIN_OVERLAP_FRAMES || b.len() < MIN_OVERLAP_FRAMES {
        return None;
    }
    let mut best: Option<f64> = None;
    // Offsets where at least MIN_OVERLAP_FRAMES overlap.
    let min_off = -((b.len() as isize) - MIN_OVERLAP_FRAMES as isize);
    let max_off = a.len() as isize - MIN_OVERLAP_FRAMES as isize;
    for off in min_off..=max_off {
        if let Some(ber) = ber_at_offset(a, b, off) {
            best = Some(best.map_or(ber, |v: f64| v.min(ber)));
        }
    }
    best
}

/// [`bit_error_rate`] restricted to results at or below `limit`: the exact
/// minimum BER when it is `<= limit`, else `None`. Identical answers for
/// every match the similarity check reports, but an alignment is abandoned
/// as soon as its running bit errors exceed the best BER found so far (or
/// `limit`): unrelated audio sits near BER 0.5, so most alignments stop
/// about halfway, and once a close match is found the rest stop far sooner.
pub fn bit_error_rate_within(a: &[u32], b: &[u32], limit: f64) -> Option<f64> {
    if a.len() < MIN_OVERLAP_FRAMES
        || b.len() < MIN_OVERLAP_FRAMES
        || !limit.is_finite()
        || !(0.0..=1.0).contains(&limit)
    {
        return None;
    }
    let mut bound = limit;
    let mut best: Option<f64> = None;
    let max_negative = b.len() - MIN_OVERLAP_FRAMES;
    let max_positive = a.len() - MIN_OVERLAP_FRAMES;
    // Most copies align at or near zero. Establish a tight error bound
    // there first, then visit every remaining valid offset symmetrically.
    let offsets =
        std::iter::once(0).chain((1..=max_negative.max(max_positive)).flat_map(|shift| {
            [
                (shift <= max_positive).then_some(shift as isize),
                (shift <= max_negative).then_some(-(shift as isize)),
            ]
            .into_iter()
            .flatten()
        }));
    'offsets: for off in offsets {
        let (a_lo, b_lo) = if off >= 0 {
            (off as usize, 0)
        } else {
            (0, (-off) as usize)
        };
        // Same alignments as `ber_at_offset`, including the ones it skips.
        let overlap = (a.len().saturating_sub(a_lo)).min(b.len().saturating_sub(b_lo));
        if overlap < MIN_OVERLAP_FRAMES {
            continue;
        }
        let bits = overlap as f64 * 32.0;
        // An alignment whose errors pass this count cannot beat `bound`.
        let max_dist = bound * bits;
        let (xs, ys) = (&a[a_lo..a_lo + overlap], &b[b_lo..b_lo + overlap]);
        let mut dist = 0u64;
        for (cx, cy) in xs.chunks(16).zip(ys.chunks(16)) {
            dist += cx
                .iter()
                .zip(cy)
                .map(|(x, y)| u64::from((x ^ y).count_ones()))
                .sum::<u64>();
            if dist as f64 > max_dist {
                continue 'offsets;
            }
        }
        let ber = dist as f64 / bits;
        if ber <= bound {
            bound = ber;
            best = Some(ber);
            if ber == 0.0 {
                return best;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_fingerprint_matches_batch_at_frame_boundaries() {
        let minimum = FRAME_SIZE + FRAME_HOP * MIN_OVERLAP_FRAMES;
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let pcm: Vec<[u8; 2]> = (0..30 * SAMPLE_RATE as usize + 13)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed as i16).to_le_bytes()
            })
            .collect();
        for len in [minimum - 1, minimum, minimum + FRAME_HOP, pcm.len()] {
            let samples: Vec<f32> = pcm[..len]
                .iter()
                .map(|s| i16::from_le_bytes(*s) as f32 / 32768.0)
                .collect();
            for chunk in [1, 2, FRAME_HOP - 1, FRAME_SIZE * 2 + 1] {
                let mut builder = FingerprintBuilder::new();
                for part in pcm[..len].chunks(chunk) {
                    builder.push_s16le(part);
                }
                match (fingerprint_from_samples(&samples), builder.finish()) {
                    (Ok(batch), Ok(stream)) => {
                        assert_eq!(stream.frames, batch.frames, "len={len} chunk={chunk}");
                        assert_eq!(stream.duration_secs, batch.duration_secs);
                    }
                    (Err(Error::PolicyGate(a)), Err(Error::PolicyGate(b))) => assert_eq!(a, b),
                    _ => panic!("stream and batch disagreed at len={len} chunk={chunk}"),
                }
            }
        }
    }

    #[test]
    fn long_tracks_fingerprint_a_bounded_segment() {
        // Sandbox: 11 and 45 minute masters hung the fingerprint step until
        // the 600 s timeout and then died after five retries. Only the
        // configured head/middle/tail windows (90 s) are decoded now, so
        // length no longer matters.
        let p = std::env::temp_dir().join(format!("audeniq-fp-long-{}.wav", std::process::id()));
        let st = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=660:sample_rate=22050",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&p)
            .status()
            .expect("ffmpeg missing");
        assert!(st.success());
        let started = std::time::Instant::now();
        let fp = compute_fingerprint(&p, 660.0);
        let _ = std::fs::remove_file(&p);
        let fp = fp.expect("11 minute track fingerprints");
        assert!(!fp.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(120));
    }

    #[test]
    fn segment_windows_cover_head_middle_tail() {
        // 210 s track: 3 x 30 s at head, middle, tail.
        assert_eq!(
            segment_windows(210.0),
            vec![(0.0, 30.0), (90.0, 30.0), (180.0, 30.0)]
        );
        // Shorter than the total coverage: one whole-track window.
        assert_eq!(segment_windows(90.0), vec![(0.0, 90.0)]);
        assert_eq!(segment_windows(60.0), vec![(0.0, 60.0)]);
        // Degenerate durations: no windows.
        assert!(segment_windows(0.0).is_empty());
        assert!(segment_windows(-5.0).is_empty());
        assert!(segment_windows(f64::NAN).is_empty());
    }

    fn sine_frames(freq: f64, secs: f64, phase: f64) -> Fingerprint {
        // Build a fingerprint directly from synthetic PCM, bypassing ffmpeg.
        let n = (secs * SAMPLE_RATE as f64) as usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                (2.0 * std::f32::consts::PI * freq as f32 * i as f32 / SAMPLE_RATE as f32
                    + phase as f32)
                    .sin()
                    * 0.8
            })
            .collect();
        fingerprint_from_samples(&samples).unwrap()
    }

    #[test]
    fn identical_audio_has_zero_ber() {
        let a = sine_frames(440.0, 8.0, 0.0);
        let b = sine_frames(440.0, 8.0, 0.0);
        let ber = bit_error_rate(&a.frames, &b.frames).unwrap();
        assert!(ber < 0.01, "identical audio BER should be ~0, got {ber}");
    }

    #[test]
    fn different_audio_has_high_ber() {
        let a = sine_frames(440.0, 8.0, 0.0);
        let b = sine_frames(880.0, 8.0, 0.0);
        let ber = bit_error_rate(&a.frames, &b.frames).unwrap();
        assert!(ber > 0.35, "different audio BER should be high, got {ber}");
    }

    #[test]
    fn time_shifted_audio_still_matches() {
        // Same tone, one starting 2 s later: alignment search must find it.
        let a = sine_frames(440.0, 10.0, 0.0);
        let b_full = sine_frames(440.0, 12.0, 0.0);
        let b: Vec<u32> = b_full.frames[21..].to_vec(); // ~2 s shift
        let ber = bit_error_rate(&a.frames, &b).unwrap();
        assert!(
            ber < NEAR_DUPLICATE_BER,
            "shifted identical audio should match, got {ber}"
        );
    }

    #[test]
    fn bounded_ber_matches_exhaustive_search() {
        // Deterministic pseudo-random frames, plus noisy copies of them at
        // several shifts and error rates: the pruned search must return
        // exactly the exhaustive minimum whenever it is within the limit.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u32
        };
        let base: Vec<u32> = (0..400).map(|_| next()).collect();
        for (shift, flip_every) in [(0usize, 0u32), (7, 9), (40, 3), (120, 2), (0, 1)] {
            let other: Vec<u32> = base[shift..]
                .iter()
                .map(|&f| {
                    let r = next();
                    if flip_every > 0 && r % flip_every == 0 {
                        f ^ r
                    } else {
                        f
                    }
                })
                .collect();
            let exact = bit_error_rate(&base, &other).unwrap();
            for limit in [0.0, 0.1, SIMILAR_BER, 0.5, 1.0] {
                let got = bit_error_rate_within(&base, &other, limit);
                if exact <= limit {
                    assert_eq!(got, Some(exact), "shift={shift} limit={limit}");
                } else {
                    assert_eq!(got, None, "shift={shift} limit={limit}");
                }
            }
        }
        let a = sine_frames(440.0, 10.0, 0.0);
        let b = sine_frames(440.0, 12.0, 0.0);
        assert_eq!(
            bit_error_rate_within(&a.frames, &b.frames[21..], SIMILAR_BER),
            bit_error_rate(&a.frames, &b.frames[21..])
        );
        assert!(bit_error_rate_within(&[0; 10], &[0; 10], 1.0).is_none());
    }

    #[test]
    fn short_sequences_are_not_comparable() {
        let a = vec![0u32; 10];
        let b = vec![0u32; 10];
        assert!(bit_error_rate(&a, &b).is_none());
    }

    #[test]
    fn trimmed_copy_matches_at_both_unequal_length_extremes() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let original: Vec<u32> = (0..400)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as u32
            })
            .collect();
        // The matching offset is outside the old, reversed search bounds.
        for copy in [&original[300..], &original[368..]] {
            assert_eq!(bit_error_rate(&original, copy), Some(0.0));
            assert_eq!(bit_error_rate(copy, &original), Some(0.0));
            assert_eq!(bit_error_rate_within(&original, copy, 0.0), Some(0.0));
            assert_eq!(bit_error_rate_within(copy, &original, 0.0), Some(0.0));
        }
    }

    #[test]
    fn bounded_comparison_rejects_invalid_thresholds() {
        for limit in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert_eq!(bit_error_rate_within(&[0; 40], &[0; 40], limit), None);
        }
    }

    #[test]
    fn fingerprint_bytes_round_trip() {
        let a = sine_frames(440.0, 8.0, 0.0);
        let bytes = a.to_bytes();
        assert_eq!(bytes.len(), a.frames.len() * 4);
        let back = Fingerprint::from_bytes(&bytes).unwrap();
        assert_eq!(back.frames, a.frames);
    }
}
