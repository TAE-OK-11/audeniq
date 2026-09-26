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
use rustfft::{FftPlanner, num_complex::Complex};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Fingerprint algorithm version, stored alongside the hash so a future
/// algorithm change can coexist with (not silently mix with) old rows.
pub const FINGERPRINT_VERSION: i16 = 1;

const SAMPLE_RATE: u32 = 11025;
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

/// Length of audio the fingerprint covers: the first 10 minutes. Longer
/// tracks are fingerprinted over this bounded segment (ffmpeg stops decoding
/// at the limit), which is ample for similarity matching and keeps time and
/// memory bounded (~13 MB of PCM) for 45-minute masters. Previously a longer
/// track overflowed the decode cap and failed every attempt.
pub const MAX_FINGERPRINT_SECS: u32 = 600;
const MAX_SAMPLES: usize = SAMPLE_RATE as usize * MAX_FINGERPRINT_SECS as usize;

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

/// Decode an audio file to mono 11025 Hz f32 PCM via ffmpeg.
/// Uses the same helper-thread + timeout pattern as `qc::probe_with` so a
/// hung decoder cannot wedge the worker.
fn decode_mono(path: &Path) -> Result<Vec<f32>> {
    use std::io::Read;
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-i",
            path.to_str().ok_or(Error::Internal)?,
            "-t",
            &MAX_FINGERPRINT_SECS.to_string(),
            "-ac",
            "1",
            "-ar",
            &SAMPLE_RATE.to_string(),
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::Internal)?;
    let stdout = child.stdout.take().ok_or(Error::Internal)?;
    let max_bytes = (MAX_SAMPLES + 1) * 2;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let r = stdout
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut buf)
            .map(|_| buf);
        let _ = tx.send(r);
    });
    let bytes = match rx.recv_timeout(Duration::from_secs(120)) {
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
    let status = child.wait().map_err(|_| Error::Internal)?;
    let (sample_chunks, remainder) = bytes.as_chunks::<2>();
    if !status.success() || !remainder.is_empty() {
        return Err(Error::Internal);
    }
    Ok(sample_chunks
        .iter()
        .map(|chunk| i16::from_le_bytes(*chunk) as f32 / 32768.0)
        .collect())
}

/// FFT bin edges for 33 logarithmically spaced bands over [300, 3000] Hz.
fn band_edges() -> [usize; N_BANDS + 1] {
    let bin_hz = SAMPLE_RATE as f64 / FRAME_SIZE as f64;
    let mut edges = [0usize; N_BANDS + 1];
    for (m, edge) in edges.iter_mut().enumerate() {
        let freq = FREQ_MIN * (FREQ_MAX / FREQ_MIN).powf(m as f64 / N_BANDS as f64);
        *edge = (freq / bin_hz).round() as usize;
    }
    edges[N_BANDS] = edges[N_BANDS].min(FRAME_SIZE / 2);
    edges
}

/// Hann window, precomputed.
fn hann_window() -> Vec<f32> {
    (0..FRAME_SIZE)
        .map(|n| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / FRAME_SIZE as f32).cos()))
        .collect()
}

/// Compute the perceptual fingerprint of an audio file.
pub fn compute_fingerprint(path: &Path) -> Result<Fingerprint> {
    let samples = decode_mono(path)?;
    if samples.len() < FRAME_SIZE + FRAME_HOP * MIN_OVERLAP_FRAMES {
        return Err(Error::PolicyGate(TOO_SHORT_CODE));
    }
    let window = hann_window();
    let edges = band_edges();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FRAME_SIZE);
    let mut buf = vec![Complex::new(0.0f32, 0.0); FRAME_SIZE];

    // Log band energies per frame.
    let mut energies: Vec<[f32; N_BANDS]> = Vec::new();
    let mut pos = 0;
    while pos + FRAME_SIZE <= samples.len() {
        for (i, b) in buf.iter_mut().enumerate() {
            b.re = samples[pos + i] * window[i];
            b.im = 0.0;
        }
        fft.process(&mut buf);
        let mut bands = [0f32; N_BANDS];
        for m in 0..N_BANDS {
            let lo = edges[m];
            let hi = edges[m + 1].max(lo + 1);
            let mut e = 0.0f32;
            for value in buf.iter().take(hi.min(FRAME_SIZE / 2)).skip(lo) {
                let mag = value.norm();
                e += mag * mag;
            }
            // Log energy with floor: Philips uses log energies; the floor
            // keeps silence from producing unstable differentials.
            bands[m] = (e + 1e-10).ln();
        }
        energies.push(bands);
        pos += FRAME_HOP;
    }
    if energies.len() < MIN_OVERLAP_FRAMES + 1 {
        return Err(Error::PolicyGate(TOO_SHORT_CODE));
    }

    // Philips differential: bit m of frame n is 1 when
    // (E[n][m] - E[n][m+1]) - (E[n+1][m] - E[n+1][m+1]) > 0.
    let mut frames = Vec::with_capacity(energies.len() - 1);
    for n in 0..energies.len() - 1 {
        let mut bits = 0u32;
        for m in 0..(N_BANDS - 1) {
            let d = (energies[n][m] - energies[n][m + 1])
                - (energies[n + 1][m] - energies[n + 1][m + 1]);
            if d > 0.0 {
                bits |= 1 << m;
            }
        }
        frames.push(bits);
    }
    Ok(Fingerprint {
        frames,
        duration_secs: samples.len() as f64 / SAMPLE_RATE as f64,
    })
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
    let min_off = -((a.len() as isize) - MIN_OVERLAP_FRAMES as isize);
    let max_off = b.len() as isize - MIN_OVERLAP_FRAMES as isize;
    for off in min_off..=max_off {
        if let Some(ber) = ber_at_offset(a, b, off) {
            best = Some(best.map_or(ber, |v: f64| v.min(ber)));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_tracks_fingerprint_a_bounded_segment() {
        // Sandbox: 11 and 45 minute masters hung the fingerprint step until
        // the 600 s timeout and then died after five retries. Only the first
        // MAX_FINGERPRINT_SECS are decoded now, so length no longer matters.
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
        let fp = compute_fingerprint(&p);
        let _ = std::fs::remove_file(&p);
        let fp = fp.expect("11 minute track fingerprints");
        assert!(!fp.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(120));
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

    fn fingerprint_from_samples(samples: &[f32]) -> Result<Fingerprint> {
        // Test-only path mirroring compute_fingerprint without ffmpeg.
        let window = hann_window();
        let edges = band_edges();
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FRAME_SIZE);
        let mut buf = vec![Complex::new(0.0f32, 0.0); FRAME_SIZE];
        let mut energies: Vec<[f32; N_BANDS]> = Vec::new();
        let mut pos = 0;
        while pos + FRAME_SIZE <= samples.len() {
            for (i, b) in buf.iter_mut().enumerate() {
                b.re = samples[pos + i] * window[i];
                b.im = 0.0;
            }
            fft.process(&mut buf);
            let mut bands = [0f32; N_BANDS];
            for m in 0..N_BANDS {
                let lo = edges[m];
                let hi = edges[m + 1].max(lo + 1);
                let mut e = 0.0f32;
                for value in buf.iter().take(hi.min(FRAME_SIZE / 2)).skip(lo) {
                    let mag = value.norm();
                    e += mag * mag;
                }
                bands[m] = (e + 1e-10).ln();
            }
            energies.push(bands);
            pos += FRAME_HOP;
        }
        let mut frames = Vec::with_capacity(energies.len().saturating_sub(1));
        for n in 0..energies.len().saturating_sub(1) {
            let mut bits = 0u32;
            for m in 0..(N_BANDS - 1) {
                let d = (energies[n][m] - energies[n][m + 1])
                    - (energies[n + 1][m] - energies[n + 1][m + 1]);
                if d > 0.0 {
                    bits |= 1 << m;
                }
            }
            frames.push(bits);
        }
        Ok(Fingerprint {
            frames,
            duration_secs: samples.len() as f64 / SAMPLE_RATE as f64,
        })
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
    fn short_sequences_are_not_comparable() {
        let a = vec![0u32; 10];
        let b = vec![0u32; 10];
        assert!(bit_error_rate(&a, &b).is_none());
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
