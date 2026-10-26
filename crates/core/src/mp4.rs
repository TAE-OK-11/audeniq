//! Length and frame size of an uploaded MP4/MOV, read from its `moov` box
//! with a few ranged reads (the media data itself is never fetched), so the
//! artist does not type them in. [`index`] keeps every top-level box header
//! and the index payloads so the media engine can check the whole layout
//! ([`crate::video`]).

use crate::{error::Result, storage::ObjectStore};

/// What the container says about the video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoInfo {
    /// 0 when the file does not say (a fragmented MP4 without `mehd`).
    pub duration_ms: i64,
    pub width: u32,
    pub height: u32,
}

/// Largest `moov` we read into memory. Even hours of video index in a few MB.
const MAX_MOOV: u64 = 32 << 20;
/// Top-level boxes we walk, one ranged read each (ftyp, free, mdat, …; a
/// fragmented file has a `moof`+`mdat` pair per fragment: 10 minutes in
/// 2-second fragments is 600). Past it only the `moov` is used.
const MAX_BOXES: usize = 1024;
/// All `moof` payloads together.
const MAX_FRAGMENT_INDEX: u64 = 32 << 20;
/// Bytes asked for per ranged read when walking headers: a box header, a
/// small `ftyp`/`moof` and the header after it usually come in one request
/// (a fragmented file: one request per fragment instead of three).
const WINDOW: usize = 8 << 10;

/// Ranged reads through one buffered window.
struct Reader<'a> {
    store: &'a dyn ObjectStore,
    key: &'a str,
    at: u64,
    buf: Vec<u8>,
    requests: usize,
}

impl Reader<'_> {
    /// Up to `n` bytes at `at` (shorter at the end of the object).
    async fn read(&mut self, at: u64, n: usize) -> Result<Vec<u8>> {
        let end = at.saturating_add(n as u64);
        let have = self.at + self.buf.len() as u64;
        if at >= self.at && (end <= have || (have < self.at + WINDOW as u64 && at < have)) {
            let from = (at - self.at) as usize;
            let to = (end.min(have) - self.at) as usize;
            return Ok(self.buf[from..to].to_vec());
        }
        self.requests += 1;
        let bytes = self.store.read_range(self.key, at, n.max(WINDOW)).await?;
        let out = bytes[..n.min(bytes.len())].to_vec();
        (self.at, self.buf) = (at, bytes);
        Ok(out)
    }
}

fn u16_at(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u16::from_be_bytes(
        b.get(i..i + 2)?.try_into().ok()?,
    )))
}
fn u32_at(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u32::from_be_bytes(
        b.get(i..i + 4)?.try_into().ok()?,
    )))
}
fn u64_at(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(i..i + 8)?.try_into().ok()?))
}

/// Child boxes of a box payload as (type, payload).
fn children(mut b: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    std::iter::from_fn(move || {
        let size = u32_at(b, 0)?;
        let typ = b.get(4..8)?;
        let (hdr, size) = match size {
            0 => (8, b.len() as u64),
            1 => (16, u64_at(b, 8)?),
            n => (8, n),
        };
        let size = usize::try_from(size).ok()?;
        if size < hdr || size > b.len() {
            return None;
        }
        let payload = &b[hdr..size];
        b = &b[size..];
        Some((typ, payload))
    })
}

fn child<'a>(b: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    children(b).find(|(t, _)| *t == name).map(|(_, p)| p)
}

/// `moov` payload → duration and the first visual track's size. `None` when
/// the movie header is missing or malformed.
pub fn parse_moov(moov: &[u8]) -> Option<VideoInfo> {
    let mvhd = child(moov, b"mvhd")?;
    let (timescale, mut duration) = match *mvhd.first()? {
        0 => (u32_at(mvhd, 12)?, u32_at(mvhd, 16)?),
        1 => (u32_at(mvhd, 20)?, u64_at(mvhd, 24)?),
        _ => return None,
    };
    if timescale == 0 {
        return None;
    }
    // Fragmented files keep the total in mvex/mehd (same timescale).
    if duration == 0
        && let Some(mehd) = child(moov, b"mvex").and_then(|m| child(m, b"mehd"))
    {
        duration = match *mehd.first()? {
            0 => u32_at(mehd, 4)?,
            _ => u64_at(mehd, 4)?,
        };
    }
    // All ones means "unknown" in both versions.
    if duration == u64::from(u32::MAX) || duration == u64::MAX {
        duration = 0;
    }
    let (mut width, mut height) = (0, 0);
    for (t, trak) in children(moov) {
        if t != b"trak" {
            continue;
        }
        let Some(tkhd) = child(trak, b"tkhd") else {
            continue;
        };
        let at = if tkhd.first() == Some(&1) { 88 } else { 76 };
        // 16.16 fixed point; audio tracks are 0×0.
        let (w, h) = (u16_at(tkhd, at), u16_at(tkhd, at + 4));
        if let (Some(w), Some(h)) = (w, h)
            && w > 0
            && h > 0
        {
            (width, height) = (w as u32, h as u32);
            break;
        }
    }
    let ms = u128::from(duration) * 1000 / u128::from(timescale);
    Some(VideoInfo {
        duration_ms: i64::try_from(ms).ok()?,
        width,
        height,
    })
}

/// The stored layout of an MP4/MOV: every top-level box header (up to 16
/// bytes) and the payloads of `ftyp`, `moov` and `moof`, at their offsets in
/// the object. Media data is never read.
#[derive(Debug, Default)]
pub struct Index {
    pub size: u64,
    pub heads: Vec<(u64, Vec<u8>)>,
    pub bodies: Vec<(u64, Vec<u8>)>,
    /// `moov` payload.
    pub moov: Vec<u8>,
    /// Every box was walked within the limits (else only `moov` is usable).
    pub complete: bool,
}

/// Walks the top-level boxes of a stored object with ranged reads. `None`
/// when there is no `moov` to read (not a usable video).
pub async fn index(store: &dyn ObjectStore, key: &str, size: u64) -> Result<Option<Index>> {
    let mut idx = Index {
        size,
        ..Index::default()
    };
    let mut r = Reader {
        store,
        key,
        at: 0,
        buf: Vec::new(),
        requests: 0,
    };
    let mut fragments = 0u64;
    let mut at = 0u64;
    let mut moov = None;
    for _ in 0..MAX_BOXES {
        if at + 8 > size {
            idx.complete = true;
            break;
        }
        let head = r.read(at, 16).await?;
        let (Some(len), Some(typ)) = (u32_at(&head, 0), head.get(4..8)) else {
            break;
        };
        let (hdr, len) = match len {
            0 => (8, size - at),
            1 => match u64_at(&head, 8) {
                Some(n) => (16, n),
                None => break,
            },
            n => (8, n),
        };
        idx.heads.push((at, head.clone()));
        if len < hdr || at + len > size {
            // Truncated or malformed: the engine reports it from the headers.
            idx.complete = moov.is_some();
            break;
        }
        let body_len = len - hdr;
        match typ {
            b"moov" if moov.is_none() => {
                if body_len > MAX_MOOV {
                    return Ok(None);
                }
                let body = r.read(at + hdr, body_len as usize).await?;
                moov = Some(body.clone());
                idx.bodies.push((at + hdr, body));
            }
            b"ftyp" if body_len <= 4096 => {
                let body = r.read(at + hdr, body_len as usize).await?;
                idx.bodies.push((at + hdr, body));
            }
            b"moof" => {
                fragments += body_len;
                if fragments > MAX_FRAGMENT_INDEX {
                    break;
                }
                let body = r.read(at + hdr, body_len as usize).await?;
                idx.bodies.push((at + hdr, body));
            }
            _ => {}
        }
        at += len;
        if at == size {
            idx.complete = true;
            break;
        }
    }
    let Some(moov) = moov else {
        return Ok(None);
    };
    idx.moov = moov;
    tracing::debug!(
        key,
        requests = r.requests,
        boxes = idx.heads.len(),
        "mp4 index read"
    );
    Ok(Some(idx))
}

/// [`index`] then [`parse_moov`].
pub async fn probe(store: &dyn ObjectStore, key: &str, size: u64) -> Result<Option<VideoInfo>> {
    Ok(index(store, key, size)
        .await?
        .and_then(|i| parse_moov(&i.moov)))
}

/// `m:ss` (minutes may run past 59, e.g. `72:05`).
pub fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(typ);
        v.extend_from_slice(payload);
        v
    }
    fn mvhd0(timescale: u32, duration: u32) -> Vec<u8> {
        let mut p = vec![0u8; 100];
        p[12..16].copy_from_slice(&timescale.to_be_bytes());
        p[16..20].copy_from_slice(&duration.to_be_bytes());
        bx(b"mvhd", &p)
    }
    fn tkhd(version: u8, w: u16, h: u16) -> Vec<u8> {
        let at = if version == 1 { 88 } else { 76 };
        let mut p = vec![0u8; at + 8];
        p[0] = version;
        p[at..at + 2].copy_from_slice(&w.to_be_bytes());
        p[at + 4..at + 6].copy_from_slice(&h.to_be_bytes());
        bx(b"tkhd", &p)
    }

    #[test]
    fn reads_duration_and_the_video_track_size() {
        let audio = bx(b"trak", &tkhd(0, 0, 0));
        let video = bx(b"trak", &tkhd(0, 1920, 1080));
        let moov = [mvhd0(600, 600 * 225 + 300), audio, video].concat();
        let i = parse_moov(&moov).unwrap();
        assert_eq!(
            i,
            VideoInfo {
                duration_ms: 225_500,
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(clock(i.duration_ms), "3:45");
    }

    #[test]
    fn version_1_headers_and_fragmented_totals() {
        let mut mvhd = vec![0u8; 120];
        mvhd[0] = 1;
        mvhd[20..24].copy_from_slice(&1000u32.to_be_bytes());
        mvhd[24..32].copy_from_slice(&0u64.to_be_bytes());
        let mut mehd = vec![0u8; 8];
        mehd[4..8].copy_from_slice(&61_000u32.to_be_bytes());
        let moov = [
            bx(b"mvhd", &mvhd),
            bx(b"mvex", &bx(b"mehd", &mehd)),
            bx(b"trak", &tkhd(1, 3840, 2160)),
        ]
        .concat();
        let i = parse_moov(&moov).unwrap();
        assert_eq!(
            i,
            VideoInfo {
                duration_ms: 61_000,
                width: 3840,
                height: 2160
            }
        );
        assert_eq!(clock(i.duration_ms), "1:01");
    }

    /// Object bytes; counts the ranged reads.
    struct Bytes(Vec<u8>, std::sync::atomic::AtomicUsize);
    impl Bytes {
        fn new(b: Vec<u8>) -> Self {
            Self(b, Default::default())
        }
    }
    #[async_trait::async_trait]
    impl ObjectStore for Bytes {
        async fn presign_put(
            &self,
            _: &str,
            _: i64,
            _: &str,
            _: &str,
            _: chrono::DateTime<chrono::Utc>,
        ) -> Result<crate::storage::UploadGrant> {
            unreachable!()
        }
        async fn head(&self, _: &str) -> Result<Option<crate::storage::ObjectMeta>> {
            unreachable!()
        }
        async fn freeze(&self, _: &str, _: &str, _: &str) -> Result<()> {
            unreachable!()
        }
        async fn get(&self, _: &str) -> Result<Vec<u8>> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn a_fragment_costs_one_ranged_read() {
        let moov = bx(
            b"moov",
            &[mvhd0(1000, 0), bx(b"trak", &tkhd(0, 1920, 1080))].concat(),
        );
        let mut file = [bx(b"ftyp", b"iso6\0\0\0\0"), moov].concat();
        for _ in 0..50 {
            file.extend(bx(b"moof", &[1u8; 900]));
            file.extend(bx(b"mdat", &[0u8; 20_000]));
        }
        let size = file.len() as u64;
        let store = Bytes::new(file);
        let i = index(&store, "k", size).await.unwrap().unwrap();
        assert!(i.complete);
        assert_eq!(i.heads.len(), 2 + 100);
        assert_eq!(i.bodies.len(), 2 + 50);
        let reads = store.1.load(std::sync::atomic::Ordering::Relaxed);
        assert!(reads <= 51, "{reads} reads");
        // What was read is what the object holds.
        for (at, b) in i.heads.iter().chain(&i.bodies) {
            assert_eq!(&store.0[*at as usize..*at as usize + b.len()], &b[..]);
        }
    }

    #[tokio::test]
    async fn probes_a_file_with_the_index_after_the_media() {
        let moov = bx(
            b"moov",
            &[mvhd0(1000, 61_000), bx(b"trak", &tkhd(0, 1920, 1080))].concat(),
        );
        let file = [
            bx(b"ftyp", b"qt  \0\0\0\0"),
            bx(b"mdat", &[7u8; 5000]),
            moov,
        ]
        .concat();
        let size = file.len() as u64;
        let i = probe(&Bytes::new(file), "k", size).await.unwrap().unwrap();
        assert_eq!(
            (clock(i.duration_ms), i.width, i.height),
            ("1:01".into(), 1920, 1080)
        );
        // No moov: not a usable video.
        let bare = [bx(b"ftyp", b"isom"), bx(b"mdat", &[0u8; 64])].concat();
        let size = bare.len() as u64;
        assert!(probe(&Bytes::new(bare), "k", size).await.unwrap().is_none());
    }

    #[test]
    fn rejects_a_movie_without_header() {
        assert!(parse_moov(&bx(b"trak", &tkhd(0, 10, 10))).is_none());
        assert!(parse_moov(&mvhd0(0, 10)).is_none());
        assert!(parse_moov(b"\0\0\0\x05xx").is_none());
    }
}
