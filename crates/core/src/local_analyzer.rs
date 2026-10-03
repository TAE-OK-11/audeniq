//! Bounded local analyzers; no shell, network calls or paid services.
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) fn run_file(
    command: &mut Command,
    path: &std::path::Path,
    accepted: &[i32],
) -> Result<Vec<u8>, &'static str> {
    crate::parser_sandbox::restrict(command, &[path], &[])
        .map_err(|_| "local analyzer sandbox unavailable")?;
    run(command, accepted)
}

pub(crate) fn run(command: &mut Command, accepted: &[i32]) -> Result<Vec<u8>, &'static str> {
    run_timeout(command, accepted, Duration::from_secs(15))
}

pub(crate) fn run_timeout(
    command: &mut Command,
    accepted: &[i32],
    timeout: Duration,
) -> Result<Vec<u8>, &'static str> {
    const LIMIT: u64 = 512 * 1024;
    let deadline = Instant::now() + timeout;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "local analyzer unavailable")?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("local analyzer output unavailable");
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let bytes = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(bytes)) if bytes.len() as u64 <= LIMIT => bytes,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("local analyzer failed or exceeded limits");
        }
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.code().is_some_and(|code| accepted.contains(&code)) => {
                return Ok(bytes);
            }
            Ok(Some(_)) => return Err("local analyzer rejected the input"),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("local analyzer timed out");
            }
        }
    }
}
