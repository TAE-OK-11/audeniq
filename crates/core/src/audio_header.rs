//! Constant-memory fast probe for ordinary PCM WAV and native FLAC masters.
//! Unusual/ambiguous headers fall back to the isolated general probe. Header
//! metrics never replace the subsequent full decode/integrity checks.
use crate::qc::AudioMetrics;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn duration(frames: u64, rate: u32) -> Option<f64> {
    if frames == 0 || rate == 0 {
        return None;
    }
    // FFprobe reports six decimal places; retain the existing metric hashes.
    format!("{:.6}", frames as f64 / f64::from(rate))
        .parse()
        .ok()
}

pub(crate) fn probe(path: &Path) -> Option<AudioMetrics> {
    let mut file = File::open(path).ok()?;
    let mut head = [0; 12];
    file.read_exact(&mut head).ok()?;
    if &head[..4] == b"fLaC" {
        file.seek(SeekFrom::Start(4)).ok()?;
        let mut metadata = [0; 38];
        file.read_exact(&mut metadata).ok()?;
        if metadata[0] & 0x7f != 0 || metadata[1..4] != [0, 0, 34] {
            return None;
        }
        let info = &metadata[4..];
        let min_block = u16::from_be_bytes([info[0], info[1]]);
        let max_block = u16::from_be_bytes([info[2], info[3]]);
        if min_block < 16 || max_block < min_block {
            return None;
        }
        let packed = u64::from_be_bytes(info[10..18].try_into().ok()?);
        let sample_rate = (packed >> 44) as u32;
        let channels = ((packed >> 41) & 7) as u32 + 1;
        let bits = ((packed >> 36) & 31) as u32 + 1;
        if bits < 4 {
            return None;
        }
        let frames = packed & ((1u64 << 36) - 1);
        return Some(AudioMetrics {
            format_name: "flac".into(),
            codec_name: "flac".into(),
            duration_secs: duration(frames, sample_rate)?,
            sample_rate,
            channels,
            bits_per_sample: Some(bits),
        });
    }
    if &head[..4] != b"RIFF" || &head[8..] != b"WAVE" {
        return None;
    }
    let len = file.metadata().ok()?.len();
    let mut position = 12;
    let mut format = None;
    for _ in 0..4096 {
        if position + 8 > len {
            return None;
        }
        file.seek(SeekFrom::Start(position)).ok()?;
        let mut chunk = [0; 8];
        file.read_exact(&mut chunk).ok()?;
        let size = u64::from(u32::from_le_bytes(chunk[4..].try_into().ok()?));
        let body = position + 8;
        if &chunk[..4] == b"fmt " {
            if !matches!(size, 16 | 18) || body + size > len {
                return None;
            }
            let mut fmt = [0; 16];
            file.read_exact(&mut fmt).ok()?;
            if u16::from_le_bytes([fmt[0], fmt[1]]) != 1 {
                return None;
            }
            let channels = u32::from(u16::from_le_bytes([fmt[2], fmt[3]]));
            let rate = u32::from_le_bytes(fmt[4..8].try_into().ok()?);
            let byte_rate = u32::from_le_bytes(fmt[8..12].try_into().ok()?);
            let alignment = u32::from(u16::from_le_bytes([fmt[12], fmt[13]]));
            let bits = u32::from(u16::from_le_bytes([fmt[14], fmt[15]]));
            if channels == 0
                || !matches!(bits, 8 | 16 | 24)
                || alignment != channels.checked_mul(bits / 8)?
                || byte_rate != rate.checked_mul(alignment)?
            {
                return None;
            }
            format = Some((channels, rate, bits, alignment));
        } else if &chunk[..4] == b"data" {
            if size == 0 || size == u64::from(u32::MAX) {
                return None;
            }
            let (channels, sample_rate, bits, alignment) = format?;
            let available = size.min(len - body);
            // Multiple data chunks / trailing extensions need the general
            // parser. A truncated final data chunk is still measurable.
            if body + size + (size & 1) < len {
                return None;
            }
            if !available.is_multiple_of(u64::from(alignment)) {
                return None;
            }
            return Some(AudioMetrics {
                format_name: "wav".into(),
                codec_name: match bits {
                    8 => "pcm_u8",
                    16 => "pcm_s16le",
                    _ => "pcm_s24le",
                }
                .into(),
                duration_secs: duration(available / u64::from(alignment), sample_rate)?,
                sample_rate,
                channels,
                bits_per_sample: Some(bits),
            });
        }
        position = body.checked_add(size)?.checked_add(size & 1)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostile_streaminfo_does_not_allocate_or_panic() {
        let path = std::env::temp_dir().join(format!("audeniq-header-{}.flac", std::process::id()));
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut bytes = [0u8; 42];
        bytes[..8].copy_from_slice(b"fLaC\x80\x00\x00\x22");
        for _ in 0..1000 {
            for byte in &mut bytes[8..] {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                *byte = seed as u8;
            }
            std::fs::write(&path, bytes).unwrap();
            let _ = probe(&path);
        }
        std::fs::remove_file(path).unwrap();
    }
}
