//! Music-video checks with the AUDENIQ media engine (`audeniq-music video
//! probe`), in place of FFprobe: codecs, frame size and rate, sound, a
//! truncated or broken index, and the delivery rules (`mv-delivery-v1`).
//!
//! The upload (up to 8 GB) stays in object storage. Only the top-level box
//! headers and the index boxes are read with ranged reads ([`crate::mp4::index`])
//! and written at their offsets into a sparse file of the object's size; the
//! engine reads nothing else, so it sees the same layout as the real file.
//! Like every parser of uploaded bytes it runs only inside the parser sandbox.
//!
//! The sound check (`addon_video.sound` job, [`measure_sound`]) runs later on
//! the worker: it asks the engine where the sound track's data is
//! (`video sound-ranges`), fetches just those ranges into the same sparse
//! copy and has the engine decode and measure them (`video probe --sound`).
use crate::{
    error::{Error, Result},
    mp4,
    storage::ObjectStore,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{path::Path, process::Command, sync::Arc, time::Duration};
use uuid::Uuid;

/// Job kind of the sound check (queue `qc`).
pub const SOUND_JOB: &str = "addon_video.sound";
/// Gap between sound ranges read through rather than split into another
/// request (about one request's latency of transfer).
const READ_THROUGH: u64 = 4 << 20;
/// Largest single ranged read, and all reads of one video together.
const MAX_READ: u64 = 32 << 20;
const MAX_FETCH: u64 = 2 << 30;
/// Ranged reads in flight.
const FETCHES: usize = 8;

/// What the upload is: the container's length and size, and the engine's
/// report when the engine could read it.
pub struct Inspection {
    pub info: mp4::VideoInfo,
    /// Full engine report (`streams`, `issues`, …).
    pub report: Option<Value>,
}

/// `None`: not a usable MP4/MOV (no movie index).
pub async fn inspect(store: &dyn ObjectStore, key: &str, size: u64) -> Result<Option<Inspection>> {
    let Some(index) = mp4::index(store, key, size).await? else {
        return Ok(None);
    };
    let Some(info) = mp4::parse_moov(&index.moov) else {
        return Ok(None);
    };
    let report = if index.complete {
        tokio::task::spawn_blocking(move || probe_layout(&index))
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    if report.is_none() {
        tracing::warn!(
            key,
            "video engine report unavailable; container length and size only"
        );
    }
    Ok(Some(Inspection { info, report }))
}

/// The index written at its offsets into a sparse file of the object's size.
fn write_sparse(index: &mp4::Index, path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    let file = std::fs::File::create_new(path)?;
    file.set_len(index.size)?;
    for (at, bytes) in index.heads.iter().chain(&index.bodies) {
        file.write_all_at(bytes, *at)?;
    }
    Ok(())
}

fn probe_layout(index: &mp4::Index) -> Option<Value> {
    let workspace = crate::upload_safety::Workspace::new().ok()?;
    let path = workspace.0.join("video.mp4");
    write_sparse(index, &path).ok()?;
    probe_file(&path)
}

/// One `audeniq-music video …` command in the sandbox; `None` when the
/// engine is missing, refuses the file or exceeds its limits.
fn engine(args: &[&str], path: &Path, timeout: Duration) -> Option<Value> {
    let mut command = Command::new(crate::qc_engine::bin());
    command.arg("video").args(args).arg(path);
    crate::parser_sandbox::restrict_audio(&mut command, &[path], &[]).ok()?;
    let out = crate::local_analyzer::run_timeout(&mut command, &[0], timeout).ok()?;
    serde_json::from_slice(&out).ok()
}

/// `audeniq-music video probe PATH` (index only).
pub fn probe_file(path: &Path) -> Option<Value> {
    engine(&["probe"], path, Duration::from_secs(30)).filter(|r| r["issues"].is_array())
}

/// `audeniq-music video probe --sound PATH`.
pub fn probe_sound_file(path: &Path) -> Option<Value> {
    engine(&["probe", "--sound"], path, Duration::from_secs(300)).filter(|r| r["issues"].is_array())
}

/// `audeniq-music video sound-ranges PATH` → (offset, length) pairs.
pub fn sound_ranges_file(path: &Path) -> Option<Vec<(u64, u64)>> {
    let v = engine(&["sound-ranges"], path, Duration::from_secs(30))?;
    v["ranges"]
        .as_array()?
        .iter()
        .map(|r| Some((r[0].as_u64()?, r[1].as_u64()?)))
        .collect()
}

/// Reads covering `ranges`: neighbours closer than [`READ_THROUGH`] share a
/// read, and no read is longer than [`MAX_READ`].
fn plan_reads(ranges: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut reads: Vec<(u64, u64)> = Vec::new();
    for &(o, l) in ranges {
        if l == 0 {
            continue;
        }
        match reads.last_mut() {
            Some((ro, rl)) if o >= *ro + *rl && o - (*ro + *rl) <= READ_THROUGH => {
                *rl = o + l - *ro;
            }
            _ => reads.push((o, l)),
        }
    }
    reads
        .into_iter()
        .flat_map(|(o, l)| {
            (0..l.div_ceil(MAX_READ))
                .map(move |i| (o + i * MAX_READ, (l - i * MAX_READ).min(MAX_READ)))
        })
        .collect()
}

/// The parts of `ranges` inside the read `(at, bytes)`: (file offset, slice).
fn pieces<'a>(
    ranges: &'a [(u64, u64)],
    at: u64,
    bytes: &'a [u8],
) -> impl Iterator<Item = (u64, &'a [u8])> {
    let end = at + bytes.len() as u64;
    ranges.iter().filter_map(move |&(o, l)| {
        let (lo, hi) = (o.max(at), (o + l).min(end));
        (lo < hi).then(|| (lo, &bytes[(lo - at) as usize..(hi - at) as usize]))
    })
}

/// The `addon_video.sound` job: measure a ready upload's sound and update
/// its report and checks. `Ok(true)`: try again later (engine or storage
/// unavailable).
pub async fn measure_sound(
    pool: &PgPool,
    store: &Arc<dyn ObjectStore>,
    file: Uuid,
) -> Result<bool> {
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT object_key, size_bytes FROM portal.addon_files WHERE id=$1 AND status='READY'",
    )
    .bind(file)
    .fetch_optional(pool)
    .await?;
    let Some((key, size)) = row else {
        return Ok(false);
    };
    let size = size as u64;
    let Some(index) = mp4::index(store.as_ref(), &key, size).await? else {
        return Ok(false);
    };
    if !index.complete {
        return Ok(false);
    }
    let workspace = crate::upload_safety::Workspace::new()?;
    let path = workspace.0.join("video.mp4");
    {
        let path = path.clone();
        tokio::task::spawn_blocking(move || write_sparse(&index, &path))
            .await
            .map_err(|_| Error::Internal)?
            .map_err(|_| Error::Storage)?;
    }
    let ranges = {
        let path = path.clone();
        tokio::task::spawn_blocking(move || sound_ranges_file(&path))
            .await
            .map_err(|_| Error::Internal)?
    };
    let Some(ranges) = ranges else {
        tracing::warn!(%file, "video sound-ranges unavailable");
        return Ok(true);
    };
    let reads = plan_reads(&ranges);
    let total: u64 = reads.iter().map(|r| r.1).sum();
    if total > MAX_FETCH {
        tracing::warn!(%file, total, "sound data spread too thin to fetch; not measured");
        return Ok(false);
    }
    let ranges = Arc::new(ranges);
    let out = Arc::new(
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .map_err(|_| Error::Storage)?,
    );
    let mut pending = reads.into_iter();
    let mut set = tokio::task::JoinSet::new();
    loop {
        while set.len() < FETCHES {
            let Some((at, len)) = pending.next() else {
                break;
            };
            let (store, key, ranges, out) =
                (store.clone(), key.clone(), ranges.clone(), out.clone());
            set.spawn(async move {
                let bytes = store.read_range(&key, at, len as usize).await?;
                tokio::task::spawn_blocking(move || {
                    use std::os::unix::fs::FileExt;
                    for (o, piece) in pieces(&ranges, at, &bytes) {
                        out.write_all_at(piece, o).map_err(|_| Error::Storage)?;
                    }
                    Ok::<_, Error>(())
                })
                .await
                .map_err(|_| Error::Internal)?
            });
        }
        let Some(done) = set.join_next().await else {
            break;
        };
        done.map_err(|_| Error::Internal)??;
    }
    drop(out);
    let report = tokio::task::spawn_blocking(move || probe_sound_file(&path))
        .await
        .map_err(|_| Error::Internal)?;
    let Some(report) = report else {
        tracing::warn!(%file, "video probe --sound unavailable");
        return Ok(true);
    };
    let checks = checks(&report);
    sqlx::query("UPDATE portal.addon_files SET video_report=$2, video_checks=$3 WHERE id=$1 AND status='READY'")
        .bind(file)
        .bind(&report)
        .bind(&checks)
        .execute(pool)
        .await?;
    drop(workspace);
    Ok(false)
}

/// Queue the sound check of a completed upload.
pub async fn enqueue_sound(pool: &PgPool, file: Uuid) -> Result<()> {
    let mut c = pool.acquire().await?;
    crate::operations::enqueue(
        &mut c,
        "qc",
        SOUND_JOB,
        &json!({"file_id": file}),
        &format!("addon-video-sound:{file}"),
        None,
    )
    .await
    .map(|_| ())
}

fn codec_label(name: &str) -> String {
    match name {
        "h264" => "H.264".into(),
        "hevc" => "HEVC".into(),
        "prores" => "ProRes".into(),
        "aac" => "AAC".into(),
        "alac" => "ALAC".into(),
        n if n.starts_with("pcm_") => "PCM".into(),
        n => n.to_uppercase(),
    }
}

/// One line for staff and the artist, e.g.
/// `H.264 High · 1920×1080 · 29.97fps · 10.2Mbps / AAC LC · 48kHz · 2ch`.
fn summary(report: &Value) -> String {
    let mut parts = Vec::new();
    let v = &report["video"][0];
    if v.is_object() {
        let mut p = vec![codec_label(v["codec_name"].as_str().unwrap_or(""))];
        if let Some(profile) = v["profile"].as_str() {
            p[0] = format!("{} {profile}", p[0]);
        }
        p.push(format!("{}×{}", v["width"], v["height"]));
        if let Some(fps) = v["fps"].as_f64() {
            p.push(format!("{fps}fps"));
        }
        if let Some(b) = v["bit_rate"].as_f64() {
            p.push(format!("{:.1}Mbps", b / 1e6));
        }
        parts.push(p.join(" · "));
    }
    let a = &report["audio"][0];
    if a.is_object() {
        let mut p = vec![codec_label(a["codec_name"].as_str().unwrap_or(""))];
        if let Some(profile) = a["profile"].as_str() {
            p[0] = format!("{} {profile}", p[0]);
        }
        if let Some(rate) = a["sample_rate"].as_f64() {
            p.push(format!("{}kHz", rate / 1000.0));
        }
        p.push(format!("{}ch", a["channels"]));
        let sound = &report["sound"];
        if let Some(l) = sound["integrated_lufs"].as_f64() {
            p.push(format!("{l:.1} LUFS"));
        }
        if let Some(t) = sound["true_peak_dbtp"].as_f64() {
            p.push(format!("TP {t:+.1}dBTP"));
        }
        parts.push(p.join(" · "));
    }
    parts.join(" / ")
}

/// What the order and the upload answer carry: `mv_ready`, a one-line
/// summary, the engine's issues (code, severity, Korean detail) and whether
/// the sound was measured yet (`sound`: `pending`, `measured`, `skipped`).
pub fn checks(report: &Value) -> Value {
    let sound = if report["sound"].is_object() {
        "measured"
    } else if report["sound_note"].is_string() {
        "skipped"
    } else {
        "pending"
    };
    json!({
        "rule": report["rule"],
        "mv_ready": report["mv_ready"],
        "summary": summary(report),
        "issues": report["issues"],
        "sound": sound,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_merge_close_ranges_and_stay_bounded() {
        // Audio every 66 kB (one video frame apart): one read.
        let close: Vec<(u64, u64)> = (0..100).map(|i| (1000 + i * 66_000, 1000)).collect();
        assert_eq!(plan_reads(&close), [(1000, 99 * 66_000 + 1000)]);
        // Far apart (ProRes): one read each.
        let far = [(0, 100), (50 << 20, 100)];
        assert_eq!(plan_reads(&far), far);
        // Long stretches are split.
        let long = [(10, 80 << 20)];
        let reads = plan_reads(&long);
        assert_eq!(reads.len(), 3);
        assert!(reads.iter().all(|r| r.1 <= MAX_READ));
        assert_eq!(reads.iter().map(|r| r.1).sum::<u64>(), 80 << 20);
        // Only the sound bytes of a read are kept.
        let bytes: Vec<u8> = (0..100).collect();
        let got: Vec<_> = pieces(&[(5, 10), (95, 20)], 0, &bytes).collect();
        assert_eq!(got, [(5, &bytes[5..15]), (95, &bytes[95..100])]);
    }

    #[test]
    fn summary_line() {
        let report = json!({
            "rule": "mv-delivery-v1", "mv_ready": true, "issues": [],
            "video": [{"codec_name": "h264", "profile": "High", "width": 1920, "height": 1080,
                       "fps": 29.97, "bit_rate": 10_240_000}],
            "audio": [{"codec_name": "aac", "profile": "LC", "sample_rate": 48000, "channels": 2}],
        });
        assert_eq!(
            checks(&report)["summary"],
            "H.264 High · 1920×1080 · 29.97fps · 10.2Mbps / AAC LC · 48kHz · 2ch"
        );
        assert_eq!(checks(&report)["sound"], "pending");
        let mut measured = report.clone();
        measured["sound"] = json!({"integrated_lufs": -9.84, "true_peak_dbtp": 0.31});
        assert_eq!(
            checks(&measured)["summary"],
            "H.264 High · 1920×1080 · 29.97fps · 10.2Mbps / AAC LC · 48kHz · 2ch · -9.8 LUFS · TP +0.3dBTP"
        );
        assert_eq!(checks(&measured)["sound"], "measured");
        let pcm = json!({"video": [], "audio": [{"codec_name": "pcm_s24le", "sample_rate": 44100, "channels": 2}]});
        assert_eq!(summary(&pcm), "PCM · 44.1kHz · 2ch");
    }

    struct Bytes(Vec<u8>);
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
            Ok(self.0.clone())
        }
    }

    fn ffmpeg(out: &Path, args: &[&str]) -> bool {
        Command::new("ffmpeg")
            .args(["-v", "error", "-y"])
            .args(args)
            .arg(out)
            .status()
            .is_ok_and(|s| s.success())
    }

    /// The sparse copy (headers and index only) reads exactly like the real
    /// file, for an index before and after the media and for a fragmented
    /// file. Needs FFmpeg and the engine (CI has both).
    #[tokio::test]
    async fn sparse_index_reads_like_the_whole_file() {
        let dir = crate::upload_safety::Workspace::new().unwrap();
        let src = [
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1920x1080:rate=30000/1001",
            "-f",
            "lavfi",
            "-i",
            "sine=sample_rate=48000",
            "-t",
            "2",
            "-ac",
            "2",
            "-c:v",
            "libx264",
            "-g",
            "15",
            "-c:a",
            "aac",
        ];
        for (name, extra) in [
            ("end.mp4", &[][..]),
            ("start.mov", &["-movflags", "+faststart"][..]),
            (
                "frag.mp4",
                &["-movflags", "frag_keyframe+empty_moov+default_base_moof"][..],
            ),
        ] {
            let path = dir.0.join(name);
            if !ffmpeg(&path, &[&src[..], extra].concat()) {
                return;
            }
            let Some(whole) = probe_file(&path) else {
                // CI points AUDENIQ_MUSIC_BIN at the pinned engine: it must run.
                assert!(
                    std::env::var_os("AUDENIQ_MUSIC_BIN").is_none(),
                    "audeniq-music video probe failed"
                );
                eprintln!("audeniq-music video probe unavailable; skipped");
                return;
            };
            let bytes = std::fs::read(&path).unwrap();
            let size = bytes.len() as u64;
            let got = inspect(&Bytes(bytes), "k", size).await.unwrap().unwrap();
            assert_eq!(got.report.as_ref(), Some(&whole), "{name}");
            assert_eq!((got.info.width, got.info.height), (1920, 1080));
            let c = checks(&whole);
            assert!(
                c["summary"]
                    .as_str()
                    .unwrap()
                    .starts_with("H.264 High · 1920×1080 · 29.97fps"),
                "{c}"
            );
        }
        // Cut short (an upload that stopped): seen from the headers alone.
        let mut bytes = std::fs::read(dir.0.join("start.mov")).unwrap();
        bytes.truncate(bytes.len() * 2 / 3);
        let size = bytes.len() as u64;
        let r = inspect(&Bytes(bytes), "k", size)
            .await
            .unwrap()
            .unwrap()
            .report
            .unwrap();
        assert_eq!(r["mv_ready"], false);
        assert_eq!(r["issues"][0]["code"], "TRUNCATED", "{r}");
    }
}
