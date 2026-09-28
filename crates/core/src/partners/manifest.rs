//! Partner-specific release feed (JSON + CSV manifest).
//!
//! The Korean services (D-1..D-4) publish no distributor spec: their feed
//! format arrives with the contract. This module produces a complete,
//! self-describing feed from the frozen release so onboarding only has to
//! rename columns (`partner_spec.field_map`) or adjust the layout, never
//! collect data again. Values come from the same `PreparedRelease` the
//! DDEX message is built from; nothing is re-read from the live draft.
//!
//! Korean law requires the youth-harmful marking for explicit releases
//! (청소년유해매체물 표시, 청소년보호법 시행령 별표 4): `adult_only` is
//! set on the release and on each explicit track.
use crate::execution::{FileRole, TransferFile};
use crate::preparation_model::PreparedRelease;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Release extras not carried on the prepared release (genre, label).
#[derive(Debug, Clone, Default)]
pub struct Extras {
    pub genre: Option<String>,
    pub label: Option<String>,
}

fn role_bucket(role: &str) -> Option<&'static str> {
    let r = role.trim().to_ascii_lowercase();
    if r.contains("lyric") || r.contains("작사") {
        Some("lyricist")
    } else if r.contains("compos") || r.contains("songwriter") || r.contains("작곡") {
        Some("composer")
    } else if r.contains("arrang") || r.contains("편곡") {
        Some("arranger")
    } else if r.contains("produc") {
        Some("producer")
    } else if r.contains("feat") {
        Some("featuring")
    } else {
        None
    }
}

fn rename(v: Value, map: &BTreeMap<String, String>) -> Value {
    match v {
        Value::Object(o) => {
            let mut out = Map::with_capacity(o.len());
            for (k, val) in o {
                let key = map.get(&k).cloned().unwrap_or(k);
                out.insert(key, rename(val, map));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(|x| rename(x, map)).collect()),
        other => other,
    }
}

/// The JSON manifest. `delivery_id` is the partner message id of this send.
pub fn release_json(
    p: &PreparedRelease,
    files: &[TransferFile],
    extras: &Extras,
    delivery_id: &str,
    action: &str,
    field_map: &BTreeMap<String, String>,
) -> Value {
    let file_for = |sha: &str, role: FileRole| {
        files
            .iter()
            .find(|f| f.role == role && f.sha256 == sha)
            .map(|f| json!({"name": f.delivery_name, "sha256": f.sha256, "size_bytes": f.size_bytes, "content_type": f.content_type}))
            .unwrap_or(Value::Null)
    };
    let canonical_tracks: std::collections::HashMap<_, _> =
        p.canonical.tracks.iter().map(|t| (t.track_id, t)).collect();
    let mut tracks: Vec<&crate::preparation_model::PreparedTrack> = p.tracks.iter().collect();
    tracks.sort_by_key(|t| (t.disc_number, t.track_number));
    let tracks: Vec<Value> = tracks
        .iter()
        .map(|t| {
            let c = canonical_tracks.get(&t.id);
            let credits: Vec<Value> = c
                .map(|c| {
                    c.credits
                        .iter()
                        .map(|cr| json!({"name": cr.party_name, "role": cr.role, "role_group": role_bucket(&cr.role)}))
                        .collect()
                })
                .unwrap_or_default();
            let explicit = c.map(|c| c.parental_advisory).unwrap_or(false);
            json!({
                "disc_number": t.disc_number,
                "track_number": t.track_number,
                "isrc": t.isrc,
                "title": t.title,
                "version": t.version,
                "artist": t.artist,
                "duration_secs": t.audio.duration_secs,
                "sample_rate": t.audio.sample_rate,
                "bits_per_sample": t.audio.bits_per_sample,
                "channels": t.audio.channels,
                "explicit": explicit,
                "adult_only": explicit,
                "credits": credits,
                "file": file_for(&t.audio.sha256, FileRole::Audio),
            })
        })
        .collect();
    let v = json!({
        "schema": "audeniq.partner-feed/1",
        "action": action,
        "delivery_id": delivery_id,
        "upc": p.upc,
        "title": p.title,
        "artist": p.artist,
        "release_type": p.release_type,
        "release_date": p.release_date.format("%Y-%m-%d").to_string(),
        "language": p.language,
        "genre": extras.genre,
        "label": extras.label,
        "p_line": p.p_line,
        "c_line": p.c_line,
        "explicit": p.explicit,
        "adult_only": p.explicit,
        "track_count": p.tracks.len(),
        "cover": file_for(&p.artwork.sha256, FileRole::Artwork),
        "tracks": tracks,
    });
    rename(v, field_map)
}

pub const CSV_COLUMNS: &[&str] = &[
    "upc",
    "album_title",
    "album_artist",
    "release_type",
    "release_date",
    "genre",
    "label",
    "language",
    "p_line",
    "c_line",
    "disc_number",
    "track_number",
    "isrc",
    "title",
    "version",
    "artist",
    "composer",
    "lyricist",
    "arranger",
    "adult_only",
    "duration_secs",
    "file",
    "sha256",
    "cover_file",
];

fn csv_cell(s: &str) -> String {
    // Spreadsheet formula injection: a leading = + - @ is neutralized.
    let s = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// One row per track, UTF-8 with BOM (opens correctly in Korean Excel).
pub fn release_csv(
    p: &PreparedRelease,
    files: &[TransferFile],
    extras: &Extras,
    field_map: &BTreeMap<String, String>,
) -> String {
    let manifest = release_json(p, files, extras, "", "deliver", &BTreeMap::new());
    let cover = manifest
        .pointer("/cover/name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut out = String::from("\u{feff}");
    let header: Vec<String> = CSV_COLUMNS
        .iter()
        .map(|c| csv_cell(field_map.get(*c).map(String::as_str).unwrap_or(c)))
        .collect();
    out.push_str(&header.join(","));
    out.push_str("\r\n");
    let s = |v: &Value| match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    for t in manifest["tracks"].as_array().into_iter().flatten() {
        let credit = |group: &str| {
            t["credits"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| c["role_group"].as_str() == Some(group))
                .filter_map(|c| c["name"].as_str())
                .collect::<Vec<_>>()
                .join("; ")
        };
        let row = [
            s(&manifest["upc"]),
            s(&manifest["title"]),
            s(&manifest["artist"]),
            s(&manifest["release_type"]),
            s(&manifest["release_date"]),
            s(&manifest["genre"]),
            s(&manifest["label"]),
            s(&manifest["language"]),
            s(&manifest["p_line"]),
            s(&manifest["c_line"]),
            s(&t["disc_number"]),
            s(&t["track_number"]),
            s(&t["isrc"]),
            s(&t["title"]),
            s(&t["version"]),
            s(&t["artist"]),
            credit("composer"),
            credit("lyricist"),
            credit("arranger"),
            if t["adult_only"].as_bool() == Some(true) {
                "Y".into()
            } else {
                "N".into()
            },
            s(&t["duration_secs"]),
            t.pointer("/file/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            t.pointer("/file/sha256")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            cover.clone(),
        ];
        let cells: Vec<String> = row.iter().map(|c| csv_cell(c)).collect();
        out.push_str(&cells.join(","));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells_escape_quotes_and_formulas() {
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("=HYPERLINK(1)"), "'=HYPERLINK(1)");
        assert_eq!(csv_cell("-1"), "'-1");
        assert_eq!(csv_cell("노래"), "노래");
    }

    #[test]
    fn role_groups_cover_korean_and_english_roles() {
        assert_eq!(role_bucket("Composer"), Some("composer"));
        assert_eq!(role_bucket("작사"), Some("lyricist"));
        assert_eq!(role_bucket("lyricist"), Some("lyricist"));
        assert_eq!(role_bucket("Arranger"), Some("arranger"));
        assert_eq!(role_bucket("mixer"), None);
    }

    #[test]
    fn rename_applies_at_every_level() {
        let m = BTreeMap::from([("isrc".to_string(), "ISRC코드".to_string())]);
        let v = rename(json!({"tracks":[{"isrc":"KR1"}],"isrc":"x"}), &m);
        assert_eq!(v["tracks"][0]["ISRC코드"], "KR1");
        assert_eq!(v["ISRC코드"], "x");
    }
}
