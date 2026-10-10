//! AUDENIQ image toolkit (audeniq-photo): image/audio metadata, QR count, OCR
//! and upload sanitization in one Rust binary, built at the commit pinned in
//! deploy/audeniq-photo.rev. Like audeniq-qc it only ever runs inside the
//! parser sandbox; it replaces ExifTool, ZBar, Tesseract and the
//! Python/Pillow/Poppler upload sanitizer.
use std::process::Command;

/// The binary. The sandbox replaces `PATH`, so the default is absolute.
pub fn bin() -> String {
    std::env::var("AUDENIQ_PHOTO_BIN").unwrap_or_else(|_| "/usr/local/bin/audeniq-photo".into())
}

/// `audeniq-photo <subcommand>`; callers add arguments and the sandbox.
pub(crate) fn command(subcommand: &str) -> Command {
    let mut command = Command::new(bin());
    command.arg(subcommand);
    command
}
