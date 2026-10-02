//! Free measurements only. OCR text is a review signal, never a verdict.
use crate::{
    domain::sha256_json,
    qc::{CheckOutcome, CheckStatus},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

pub const RULE_VERSION: &str = "1";

fn measured(code: &'static str, report: Result<Value, &'static str>) -> CheckOutcome {
    let (status, body) = match report {
        Ok(body) => (CheckStatus::Pass, body),
        Err(reason) => (
            CheckStatus::TechnicalRetry,
            json!({"inspection_status":"FAILED","reason":reason}),
        ),
    };
    let report = json!({"rule_version":RULE_VERSION,"report":body});
    CheckOutcome {
        check_code: code,
        status,
        input_hash: sha256_json(&report),
        detail: report.to_string(),
    }
}

pub fn inspect(path: &Path) -> Vec<CheckOutcome> {
    // Every successful read is only a measurement. The requested DSP's
    // policy evaluates it later against the frozen application, so one
    // DSP's rules cannot contaminate the shared, asset-only QC cache.
    vec![
        measured("IMAGE_COLOR_PROFILE", color(path)),
        measured("IMAGE_TEXT_SCAN", text(path)),
        measured("IMAGE_QR_SCAN", qr(path)),
    ]
}

fn color(path: &Path) -> Result<Value, &'static str> {
    let executable = std::env::var("EXIFTOOL_BIN").unwrap_or_else(|_| "exiftool".into());
    let bytes = crate::local_analyzer::run(
        Command::new(executable)
            .args([
                "-j",
                "-n",
                "-s",
                "-ColorSpace",
                "-ColorType",
                "-PhotometricInterpretation",
                "-SamplesPerPixel",
                "-BitsPerSample",
                "-BitDepth",
                "-ColorComponents",
                "-ProfileDescription",
                "-ProfileID",
                "-Orientation",
                "--",
            ])
            .arg(path),
        &[0],
    )?;
    let Value::Array(rows) =
        serde_json::from_slice::<Value>(&bytes).map_err(|_| "invalid image properties")?
    else {
        return Err("invalid image properties");
    };
    if rows.len() != 1 || rows[0].get("Error").is_some() {
        return Err("invalid image properties");
    }
    let mut properties = rows[0].clone();
    if let Some(object) = properties.as_object_mut() {
        object.remove("SourceFile");
    }
    Ok(json!({"inspection_status":"COMPLETED","properties":properties}))
}

fn text(path: &Path) -> Result<Value, &'static str> {
    let executable = std::env::var("TESSERACT_BIN").unwrap_or_else(|_| "tesseract".into());
    let bytes = crate::local_analyzer::run(
        Command::new(executable)
            .arg(path)
            .args(["stdout", "-l", "eng+kor", "--psm", "11", "tsv"])
            .env("OMP_THREAD_LIMIT", "1"),
        &[0],
    )?;
    parse_tsv(std::str::from_utf8(&bytes).map_err(|_| "invalid OCR report")?)
}

fn parse_tsv(tsv: &str) -> Result<Value, &'static str> {
    if !tsv
        .lines()
        .next()
        .is_some_and(|line| line.starts_with("level\tpage_num\t"))
    {
        return Err("invalid OCR report");
    }
    let mut lines: BTreeMap<(u32, u32, u32, u32), Vec<String>> = BTreeMap::new();
    let mut characters = 0;
    for row in tsv.lines().skip(1) {
        let columns: Vec<_> = row.splitn(12, '\t').collect();
        if columns.len() != 12 {
            return Err("invalid OCR report");
        }
        if columns[0] != "5" {
            continue;
        }
        let confidence: f32 = columns[10].parse().map_err(|_| "invalid OCR confidence")?;
        if !confidence.is_finite() || confidence < 80.0 {
            continue;
        }
        let word = columns[11].trim();
        if word.is_empty() {
            continue;
        }
        characters += word.len();
        if characters > 16 * 1024 {
            return Err("OCR evidence exceeded limits");
        }
        let key = (
            columns[1].parse().map_err(|_| "invalid OCR position")?,
            columns[2].parse().map_err(|_| "invalid OCR position")?,
            columns[3].parse().map_err(|_| "invalid OCR position")?,
            columns[4].parse().map_err(|_| "invalid OCR position")?,
        );
        lines.entry(key).or_default().push(word.to_owned());
    }
    Ok(
        json!({"inspection_status":"COMPLETED","method":"LOCAL_TESSERACT_OCR",
        "minimum_word_confidence":80,"lines":lines.into_values().map(|words| words.join(" ")).collect::<Vec<_>>(),
        "limitation":"OCR may miss or misread text; absence is not proof of compliant artwork"}),
    )
}

fn qr(path: &Path) -> Result<Value, &'static str> {
    let executable = std::env::var("ZBARIMG_BIN").unwrap_or_else(|_| "zbarimg".into());
    let bytes = crate::local_analyzer::run(
        Command::new(executable)
            .args([
                "--quiet",
                "--nodbus",
                "--xml",
                "-Sdisable",
                "-Sqrcode.enable",
                "--",
            ])
            .arg(path),
        &[0, 4],
    )?;
    let mut reader = quick_xml::Reader::from_reader(bytes.as_slice());
    let mut count = 0;
    let mut root = false;
    loop {
        use quick_xml::events::Event;
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                if e.name().as_ref() == "barcodes" {
                    root = true;
                }
                if e.name().as_ref() == "symbol" {
                    for attribute in e.attributes() {
                        let a = attribute.map_err(|_| "invalid QR report")?;
                        if a.key.as_ref() == "type" && a.value.as_ref() == "QR-Code" {
                            count += 1;
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(Event::DocType(_)) | Err(_) => return Err("invalid QR report"),
            _ => {}
        }
    }
    if !root {
        return Err("invalid QR report");
    }
    // Never retain or follow the decoded payload (including private URLs).
    Ok(
        json!({"inspection_status":"COMPLETED","method":"LOCAL_ZBAR_QR","qr_count":count,
        "limitation":"undetected codes do not establish absence"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ocr_rejects_invalid_reports_and_ignores_low_confidence_words() {
        assert!(parse_tsv("").is_err());
        let header = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";
        let report=parse_tsv(&format!("{header}5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t95\tNormal\n5\t1\t1\t1\t1\t2\t0\t0\t10\t10\t50\twww.invalid.test\n")).unwrap();
        assert_eq!(report["lines"], json!(["Normal"]));
    }
}
