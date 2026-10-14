//! Leftover scratch files from a killed process.
//!
//! Every temp file and directory this service creates is named `audeniq-*`
//! and removed by a Drop guard. A SIGKILL (a deploy past its grace period,
//! the OOM killer) runs no Drop, and TMPDIR is a persistent volume in
//! production: masters of up to 512 MiB and delivery staging directories
//! would pile up until the disk is full and every job fails. The API and the
//! worker sweep them at startup and then hourly.

use std::{
    path::Path,
    time::{Duration, SystemTime},
};

/// Older than any live use: the longest job (a DSP transfer or a decode) is
/// bounded at an hour, and a live staging directory is touched as it fills.
pub const MAX_AGE: Duration = Duration::from_secs(6 * 3600);

/// Removes `audeniq-*` entries directly under `dir` last modified more than
/// `max_age` ago. Returns how many were removed. Symlinks are removed, never
/// followed.
pub fn sweep(dir: &Path, max_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("audeniq-") {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let stale = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > max_age);
        if !stale {
            continue;
        }
        let result = if meta.is_dir() {
            std::fs::remove_dir_all(entry.path())
        } else {
            std::fs::remove_file(entry.path())
        };
        match result {
            Ok(()) => removed += 1,
            Err(error) => {
                tracing::warn!(path = %entry.path().display(), %error, "stale temp entry not removed")
            }
        }
    }
    removed
}

/// Sweeps `dirs` now and then every hour, on the blocking pool.
pub fn spawn(dirs: Vec<std::path::PathBuf>) {
    tokio::spawn(async move {
        loop {
            let dirs = dirs.clone();
            let removed = tokio::task::spawn_blocking(move || {
                dirs.iter().map(|d| sweep(d, MAX_AGE)).sum::<usize>()
            })
            .await
            .unwrap_or(0);
            if removed > 0 {
                tracing::warn!(removed, "removed stale temp files left by a killed process");
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_old_audeniq_entries() {
        let root = std::env::temp_dir().join(format!("sweep-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("audeniq-dlv-old/sub")).unwrap();
        std::fs::write(root.join("audeniq-dlv-old/sub/f"), b"x").unwrap();
        std::fs::write(root.join("audeniq-qc-new"), b"x").unwrap();
        std::fs::write(root.join("other-file"), b"x").unwrap();
        // Nothing is older than an hour yet.
        assert_eq!(sweep(&root, Duration::from_secs(3600)), 0);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(sweep(&root, Duration::from_millis(1)), 2);
        assert!(root.join("other-file").exists());
        assert!(!root.join("audeniq-dlv-old").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
