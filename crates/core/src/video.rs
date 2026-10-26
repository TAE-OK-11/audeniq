//! Music-video checks with the AUDENIQ media engine (`audeniq-music video
//! probe`), in place of FFprobe: codecs, frame size and rate, sound, a
//! truncated or broken index, and the delivery rules (`mv-delivery-v1`).
//!
//! The upload (up to 8 GB) stays in object storage. Only the top-level box
//! headers and the index boxes are read with ranged reads ([`crate::mp4::index`])
//! and written at their offsets into a sparse file of the object's size; the
//! engine reads nothing else, so it sees the same layout as the real file.
//! Like every parser of uploaded bytes it runs only inside the parser sandbox.
use crate::{error::Result, mp4, storage::ObjectStore};
use serde_json::{Value, json};
use std::{path::Path, process::Command, time::Duration};

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

fn probe_layout(index: &mp4::Index) -> Option<Value> {
    use std::os::unix::fs::FileExt;
    let workspace = crate::upload_safety::Workspace::new().ok()?;
    let path = workspace.0.join("video.mp4");
    let file = std::fs::File::create_new(&path).ok()?;
    file.set_len(index.size).ok()?;
    for (at, bytes) in index.heads.iter().chain(&index.bodies) {
        file.write_all_at(bytes, *at).ok()?;
    }
    drop(file);
    probe_file(&path)
}

/// `audeniq-music video probe PATH` in the sandbox; `None` when the engine is
/// missing, refuses the file or exceeds its limits.
pub fn probe_file(path: &Path) -> Option<Value> {
    let mut command = Command::new(crate::qc_engine::bin());
    command.arg("video").arg("probe").arg(path);
    crate::parser_sandbox::restrict_audio(&mut command, &[path], &[]).ok()?;
    let out =
        crate::local_analyzer::run_timeout(&mut command, &[0], Duration::from_secs(30)).ok()?;
    let report: Value = serde_json::from_slice(&out).ok()?;
    report["issues"].is_array().then_some(report)
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
        parts.push(p.join(" · "));
    }
    parts.join(" / ")
}

/// What the order and the upload answer carry: `mv_ready`, a one-line
/// summary and the engine's issues (code, severity, Korean detail).
pub fn checks(report: &Value) -> Value {
    json!({
        "rule": report["rule"],
        "mv_ready": report["mv_ready"],
        "summary": summary(report),
        "issues": report["issues"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
