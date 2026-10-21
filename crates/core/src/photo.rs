//! AUDENIQ image toolkit (`audeniq-music photo`): image/audio metadata, QR
//! count, OCR and upload sanitization, in the same engine binary as the audio
//! QC ([`crate::qc_engine::bin`]). Like the audio commands it only ever runs
//! inside the parser sandbox; it replaces ExifTool, ZBar, Tesseract and the
//! Python/Pillow/Poppler upload sanitizer.
use std::process::Command;

/// `audeniq-music photo <subcommand>`; callers add arguments and the sandbox.
pub(crate) fn command(subcommand: &str) -> Command {
    let mut command = Command::new(crate::qc_engine::bin());
    command.arg("photo").arg(subcommand);
    command
}
