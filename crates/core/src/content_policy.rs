//! Source-backed pre-delivery rules, scoped to the requested DSP.
//! Declarations and OCR are evidence; neither proves copyright or authorship.
use crate::{dsp_registry::Dsp, error::Result};
use serde_json::{Value, json};
use sqlx::PgConnection;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

pub const RULE_VERSION: &str = "1";
const APPLE: &str = "https://help.apple.com/itc/musicstyleguide/en.lproj/static.html";
const SPOTIFY: &str =
    "https://support.spotify.com/gw-en/artists/article/metadata-formatting-guidelines/";
const SPOTIFY_ART: &str =
    "https://support.spotify.com/gw-en/artists/article/cover-art-requirements/";
const DISTRIBUTOR_ART: &str = "https://support.distrokid.com/hc/en-us/articles/360013534334-What-Are-the-Requirements-for-Album-Artwork";
const CONTENT_ID: &str = "https://support.google.com/youtube/answer/2605065?hl=en";

#[derive(Default, Debug)]
pub struct ArtworkEvidence {
    pub color: Option<Value>,
    pub text: Option<Value>,
    pub qr: Option<Value>,
}

pub async fn artwork_evidence(
    c: &mut PgConnection,
    revision: Uuid,
    refs: &[Uuid],
) -> Result<ArtworkEvidence> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT check_code,status,detail FROM operations.check_results WHERE revision_id=$1 AND id=ANY($2)
         AND check_code IN ('IMAGE_COLOR_PROFILE','IMAGE_TEXT_SCAN','IMAGE_QR_SCAN') ORDER BY id")
        .bind(revision).bind(refs).fetch_all(c).await?;
    let mut evidence = ArtworkEvidence::default();
    for (code, status, detail) in rows {
        if status != "PASS" {
            continue;
        }
        let Some(report) = detail.and_then(|d| serde_json::from_str::<Value>(&d).ok()) else {
            continue;
        };
        if report["rule_version"] != crate::artwork_policy::RULE_VERSION
            || report["report"]["inspection_status"] != "COMPLETED"
        {
            continue;
        }
        match code.as_str() {
            "IMAGE_COLOR_PROFILE" => evidence.color = Some(report["report"].clone()),
            "IMAGE_TEXT_SCAN" => evidence.text = Some(report["report"].clone()),
            "IMAGE_QR_SCAN" => evidence.qr = Some(report["report"].clone()),
            _ => {}
        }
    }
    Ok(evidence)
}

pub struct Finding {
    pub code: &'static str,
    pub correction: bool,
    pub detail: String,
}

fn normalized(text: &str) -> String {
    text.nfkc().flat_map(char::to_lowercase).collect()
}
fn words(text: &str) -> Vec<String> {
    normalized(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}
fn contact(text: &str) -> bool {
    let text = normalized(text);
    if ["https://", "http://", "www."]
        .iter()
        .any(|s| text.contains(s))
    {
        return true;
    }
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| !c.is_alphanumeric() && !"@.-_".contains(c));
        if let Some((local, domain)) = token.split_once('@') {
            return !local.is_empty()
                && domain.split_once('.').is_some_and(|(host, suffix)| {
                    !host.is_empty() && suffix.chars().all(char::is_alphabetic)
                });
        }
        [".com", ".net", ".org", ".co.kr", ".kr", ".io"]
            .iter()
            .any(|suffix| {
                token.strip_suffix(suffix).is_some_and(|host| {
                    host.len() >= 3
                        && host
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
                })
            })
    })
}
fn phrase(text: &str, pattern: &str) -> bool {
    let text = words(text);
    let pattern = words(pattern);
    !pattern.is_empty() && text.windows(pattern.len()).any(|window| window == pattern)
}
fn promotional(text: &str) -> bool {
    [
        "out now",
        "available now",
        "free download",
        "pre save",
        "link in bio",
        "subscribe now",
        "follow us",
        "for promo use",
        "buy now",
    ]
    .iter()
    .any(|p| phrase(text, p))
}
fn section_instruction(line: &str) -> bool {
    let line = normalized(line);
    let enclosed = (line.starts_with('[') && line.ends_with(']'))
        || (line.starts_with('(') && line.ends_with(')'))
        || line.ends_with(':');
    let tokens = words(&line);
    let Some(first) = tokens.first() else {
        return false;
    };
    let section = matches!(
        first.as_str(),
        "verse" | "chorus" | "intro" | "bridge" | "hook" | "outro" | "후렴" | "간주"
    );
    let modifiers = tokens.iter().skip(1).all(|word| {
        word.chars().all(|c| c.is_ascii_digit())
            || matches!(word.as_str(), "x" | "repeat" | "repeats")
            || word
                .strip_prefix('x')
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            || word
                .strip_suffix('x')
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    });
    (section && modifiers && (enclosed || tokens.len() > 1))
        || (enclosed && first == "repeat" && modifiers)
}

/// Finding codes are shared by Stage 2 and the per-DSP staging view.
/// No inferred transcriptions, remote media checks or automatic rights claims.
pub fn evaluate(body: &Value, dsp: Dsp, art: &ArtworkEvidence) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut add = |code, correction, source, locations: Vec<String>| {
        if locations.is_empty() {
            return;
        }
        out.push(Finding { code, correction, detail:json!({"policy_version":RULE_VERSION,"dsp":dsp.code(),
            "source":source,"locations":locations,"action":if correction {"CORRECTION_REQUIRED"} else {"REVIEW_REQUIRED"}}).to_string() });
    };
    let apple = matches!(dsp, Dsp::D6 | Dsp::D30);
    let spotify = dsp == Dsp::D5;
    let draft = &body["release"]["draft"];
    let tracks: Vec<&Value> = body["tracks"].as_array().into_iter().flatten().collect();
    let mut titles = vec![(
        "release.title".to_owned(),
        body["release"]["title"].as_str().unwrap_or(""),
    )];
    let mut artists = Vec::new();
    if let Some(artist) = draft["artist"].as_str() {
        artists.push(("release.artist".to_owned(), artist));
    }
    for t in &tracks {
        let id = t["id"].as_str().unwrap_or("unknown");
        titles.push((
            format!("track={id}.title"),
            t["title"].as_str().unwrap_or(""),
        ));
        if let Some(artist) = t["artist_name"].as_str() {
            artists.push((format!("track={id}.artist"), artist));
        }
    }
    let metadata: Vec<_> = titles.iter().chain(artists.iter()).collect();
    add(
        "S2_DSP_METADATA_CONTACT",
        true,
        DISTRIBUTOR_ART,
        metadata
            .iter()
            .filter(|(_, text)| contact(text))
            .map(|(loc, _)| loc.clone())
            .collect(),
    );
    add(
        "S2_DSP_METADATA_PROMOTION",
        false,
        APPLE,
        titles
            .iter()
            .filter(|(_, text)| promotional(text))
            .map(|(loc, _)| loc.clone())
            .collect(),
    );
    if apple || spotify {
        const GENERIC: &[&str] = &[
            "christmas hits",
            "sleep music",
            "white noise",
            "yoga",
            "music for dogs",
            "lo fi",
            "workout",
            "meditation",
            "top hits",
            "smooth jazz",
            "rock",
            "hip hop",
        ];
        add(
            "S2_DSP_ARTIST_GENERIC",
            false,
            if apple { APPLE } else { SPOTIFY },
            artists
                .iter()
                .filter(|(_, text)| {
                    let name = words(text).join(" ");
                    if spotify {
                        matches!(name.as_str(), "christmas hits" | "sleep music" | "top hits")
                    } else {
                        GENERIC.contains(&name.as_str())
                    }
                })
                .map(|(loc, _)| loc.clone())
                .collect(),
        );
    }
    if spotify {
        // Spotify asks for contributors in their own fields. Apple uses
        // featured names in displayed titles; do not share this restriction.
        add(
            "S2_DSP_TITLE_FEATURED_ARTIST",
            true,
            SPOTIFY,
            titles
                .iter()
                .filter(|(_, text)| {
                    let n = normalized(text);
                    [
                        "(feat.",
                        "(feat ",
                        "[feat.",
                        "[feat ",
                        "(ft.",
                        "[ft.",
                        "(featuring ",
                        "[featuring ",
                    ]
                    .iter()
                    .any(|s| n.contains(s))
                })
                .map(|(loc, _)| loc.clone())
                .collect(),
        );
    }
    let mut lyrics_format = Vec::new();
    let mut lyrics_style = Vec::new();
    let mut explicit = Vec::new();
    let mut clean = Vec::new();
    for t in &tracks {
        let id = t["id"].as_str().unwrap_or("unknown");
        let lyrics = t["lyrics"].as_str().unwrap_or("");
        if !lyrics.trim().is_empty() {
            let tokens = words(lyrics);
            if t["parental_advisory"] != true
                && tokens.iter().any(|word| {
                    matches!(
                        word.as_str(),
                        "fuck"
                            | "fucking"
                            | "motherfucker"
                            | "motherfucking"
                            | "shit"
                            | "bullshit"
                            | "cunt"
                            | "씨발"
                            | "개새끼"
                    )
                })
            {
                explicit.push(format!("track={id}: strong literal explicit-language signal; compare with the audio before changing the tag"));
            }
            if apple {
                let mut empty = 0;
                let mut invalid = false;
                let mut style = false;
                for line in lyrics.lines() {
                    if line.trim().is_empty() {
                        empty += 1;
                        if empty > 1 {
                            invalid = true;
                        }
                        continue;
                    }
                    empty = 0;
                    if line.trim() != line || section_instruction(line.trim()) {
                        invalid = true;
                    }
                    if line.ends_with(['.', ','])
                        || line
                            .trim_start_matches(|c: char| !c.is_alphabetic())
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_lowercase())
                    {
                        style = true;
                    }
                }
                if invalid {
                    lyrics_format.push(format!("track={id}: section/repeat instructions, surrounding spaces or multiple blank lines"));
                }
                if style {
                    lyrics_style.push(format!("track={id}: line capitalization or terminal punctuation requires formatting review"));
                }
            }
        }
        if t["parental_advisory"] == true
            && words(t["version"].as_str().unwrap_or(""))
                .iter()
                .any(|word| word == "clean")
        {
            clean.push(format!("track={id}: Clean version is marked explicit"));
        }
    }
    add("S2_DSP_EXPLICIT_TAG_REVIEW", false, SPOTIFY, explicit);
    add("S2_DSP_CLEAN_TAG_CONFLICT", true, APPLE, clean);
    add("S2_DSP_LYRICS_FORMAT", true, APPLE, lyrics_format);
    add("S2_DSP_LYRICS_STYLE", false, APPLE, lyrics_style);
    if apple {
        add(
            "S2_DSP_EMOJI_METADATA",
            true,
            APPLE,
            metadata
                .iter()
                .filter(|(_, text)| text.chars().any(crate::submission::is_emoji))
                .map(|(loc, _)| loc.clone())
                .collect(),
        );
        const DISALLOWED: &[&str] = &[
            "official audio",
            "official video",
            "full version",
            "digital only",
            "digital download",
            "album version",
            "original mix",
            "dolby atmos",
            "lossless",
            "high resolution audio",
            "24 bit",
            "192 khz",
        ];
        add(
            "S2_DSP_TITLE_TECHNICAL_INFO",
            true,
            APPLE,
            titles
                .iter()
                .filter(|(_, text)| {
                    let n = normalized(text);
                    DISALLOWED.iter().any(|term| {
                        n.contains(&format!("({term})")) || n.contains(&format!("[{term}]"))
                    })
                })
                .map(|(loc, _)| loc.clone())
                .collect(),
        );
        if tracks.len() > 500 {
            add(
                "S2_DSP_TRACK_COUNT_LIMIT",
                true,
                APPLE,
                vec![format!("{} tracks exceed Apple limit 500", tracks.len())],
            );
        }
    }
    if !body["release"]["artwork"].is_null() {
        if art.color.is_none() || art.text.is_none() || art.qr.is_none() {
            add("S2_DSP_ARTWORK_INSPECTION_REQUIRED",false,DISTRIBUTOR_ART,vec!["missing pinned artwork properties, OCR or QR inspection; automatic approval is unavailable".into()]);
        }
        if art
            .qr
            .as_ref()
            .and_then(|v| v["qr_count"].as_u64())
            .is_some_and(|count| count > 0)
        {
            add("S2_DSP_ARTWORK_QR",true,DISTRIBUTOR_ART,vec!["decoded QR code on cover; remove the code (payload is not retained or followed)".into()]);
        }
        if let Some(text) = &art.text {
            let lines: Vec<_> = text["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let hits:Vec<_>=lines.iter().enumerate().filter(|(_,line)|contact(line)||promotional(line)
                || (["$","€","₩"].iter().any(|currency|line.contains(currency)) && line.chars().any(|c|c.is_ascii_digit()))
                || (apple && ["dolby atmos","high resolution audio","24 bit","192 khz"].iter().any(|term|phrase(line,term))))
                .map(|(i,_)|format!("OCR line {}: potential contact/promotion/format claim; confirm against the original image",i+1)).collect();
            add("S2_DSP_ARTWORK_TEXT_REVIEW", false, DISTRIBUTOR_ART, hits);
        }
        if spotify && let Some(color) = &art.color {
            let p = &color["properties"];
            let mut invalid = Vec::new();
            let label = normalized(p["ColorSpace"].as_str().unwrap_or(""));
            let color_type = p["ColorType"].as_i64();
            if label.contains("cmyk")
                || p["ColorComponents"] == 4
                || matches!(color_type, Some(0 | 3 | 4 | 6))
                || p["PhotometricInterpretation"] == 5
            {
                invalid.push(
                    "cover is CMYK, grayscale, indexed or includes alpha; export 24-bit sRGB RGB"
                        .into(),
                );
            }
            if p.get("ProfileDescription").is_some() || p.get("ProfileID").is_some() {
                invalid.push("embedded ICC profile: apply conversion to sRGB pixels and export without the embedded profile".into());
            }
            if p["Orientation"].as_i64().is_some_and(|n| n != 1) {
                invalid.push(
                    "non-default orientation metadata: bake the orientation into pixels".into(),
                );
            }
            add(
                "S2_DSP_SPOTIFY_ARTWORK_ENCODING",
                true,
                SPOTIFY_ART,
                invalid,
            );
        }
    }
    if dsp == Dsp::D27 {
        let options = &draft["options"];
        if options["contentIdExclusiveRightsAck"] != true
            || options["contentIdOriginalRecordingAck"] != true
        {
            add("S2_DSP_CONTENT_ID_DECLARATION",true,CONTENT_ID,vec!["confirm exclusive reference rights in every claimed territory and an original, distinct recording; ordinary distribution consent is insufficient".into()]);
        }
        let mut risks = Vec::new();
        if body["declarations"]["contains_samples"] == true
            || body["declarations"]["is_remix"] == true
            || body["declarations"]["is_cover"] == true
        {
            risks.push(
                "cover/sample/remix: verify exclusive rights and any required reference exclusions"
                    .into(),
            );
        }
        for (location, text) in &titles {
            if [
                "karaoke",
                "remaster",
                "sound alike",
                "public domain",
                "creative commons",
                "royalty free",
                "non exclusive",
            ]
            .iter()
            .any(|term| phrase(text, term))
            {
                risks.push(format!(
                    "{location}: potentially ineligible or non-distinct Content ID reference"
                ));
            }
        }
        for t in &tracks {
            if ["karaoke", "remaster", "sound alike"]
                .iter()
                .any(|term| phrase(t["version"].as_str().unwrap_or(""), term))
            {
                risks.push(format!(
                    "track={}: verify distinct Content ID reference",
                    t["id"]
                ));
            }
        }
        add("S2_DSP_CONTENT_ID_ELIGIBILITY", false, CONTENT_ID, risks);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body() -> Value {
        json!({"release":{"title":"Normal Song","draft":{"artist":"Simon & Garfunkel","platforms":["spotify"]},"artwork":null},"tracks":[{"id":"a","title":"Normal Song","version":"","lyrics":"","parental_advisory":false}],"declarations":{}})
    }
    fn codes(body: &Value, dsp: Dsp) -> Vec<&'static str> {
        evaluate(body, dsp, &ArtworkEvidence::default())
            .into_iter()
            .map(|f| f.code)
            .collect()
    }
    #[test]
    fn ordinary_names_and_lyric_words_are_not_substrings_or_compound_artist_errors() {
        let mut b = body();
        b["tracks"][0]["lyrics"] =
            json!("Scunthorpe is here\nDiscovery comes near\nChorus of birds");
        assert!(codes(&b, Dsp::D5).is_empty());
        assert!(codes(&b, Dsp::D6).is_empty());
    }
    #[test]
    fn apple_and_spotify_title_requirements_are_scoped() {
        let mut b = body();
        b["tracks"][0]["title"] = json!("Song (feat. Guest)");
        assert!(codes(&b, Dsp::D5).contains(&"S2_DSP_TITLE_FEATURED_ARTIST"));
        assert!(!codes(&b, Dsp::D6).contains(&"S2_DSP_TITLE_FEATURED_ARTIST"));
        b["tracks"][0]["title"] = json!("Song 🎵");
        assert!(codes(&b, Dsp::D6).contains(&"S2_DSP_EMOJI_METADATA"));
        assert!(!codes(&b, Dsp::D5).contains(&"S2_DSP_EMOJI_METADATA"));
    }
    #[test]
    fn lyrics_instructions_and_explicit_signals_require_different_actions() {
        let mut b = body();
        b["tracks"][0]["lyrics"] = json!("[Chorus]\nFuck this\n(Repeat x3)");
        let findings = evaluate(&b, Dsp::D6, &ArtworkEvidence::default());
        assert!(
            findings
                .iter()
                .any(|f| f.code == "S2_DSP_LYRICS_FORMAT" && f.correction)
        );
        assert!(
            findings
                .iter()
                .any(|f| f.code == "S2_DSP_EXPLICIT_TAG_REVIEW" && !f.correction)
        );
        assert!(!codes(&b, Dsp::D5).contains(&"S2_DSP_LYRICS_FORMAT"));
    }
    #[test]
    fn content_id_does_not_apply_to_youtube_music() {
        let mut b = body();
        assert!(codes(&b, Dsp::D7).is_empty());
        assert!(codes(&b, Dsp::D27).contains(&"S2_DSP_CONTENT_ID_DECLARATION"));
        b["release"]["draft"]["options"] =
            json!({"contentIdExclusiveRightsAck":true,"contentIdOriginalRecordingAck":true});
        assert!(codes(&b, Dsp::D27).is_empty());
        b["declarations"]["contains_samples"] = json!(true);
        assert!(codes(&b, Dsp::D27).contains(&"S2_DSP_CONTENT_ID_ELIGIBILITY"));
    }
    #[test]
    fn qr_is_correction_ocr_is_review_and_missing_inspection_cannot_clear() {
        let mut b = body();
        b["release"]["artwork"] = json!({"asset_id":"x"});
        assert!(codes(&b, Dsp::D5).contains(&"S2_DSP_ARTWORK_INSPECTION_REQUIRED"));
        let evidence = ArtworkEvidence {
            color: Some(json!({"properties":{"ColorType":2}})),
            text: Some(json!({"lines":["Buy now www.example.com"]})),
            qr: Some(json!({"qr_count":1})),
        };
        let findings = evaluate(&b, Dsp::D5, &evidence);
        assert!(
            findings
                .iter()
                .any(|f| f.code == "S2_DSP_ARTWORK_QR" && f.correction)
        );
        assert!(
            findings
                .iter()
                .any(|f| f.code == "S2_DSP_ARTWORK_TEXT_REVIEW" && !f.correction)
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.code == "S2_DSP_ARTWORK_INSPECTION_REQUIRED")
        );
    }
    #[test]
    fn spotify_encoding_rules_do_not_reject_apple_profiles() {
        let mut b = body();
        b["release"]["artwork"] = json!({"asset_id":"x"});
        let evidence = ArtworkEvidence {
            color: Some(json!({"properties":{"ColorType":6,"ProfileDescription":"sRGB"}})),
            text: Some(json!({"lines":[]})),
            qr: Some(json!({"qr_count":0})),
        };
        assert!(
            evaluate(&b, Dsp::D5, &evidence)
                .iter()
                .any(|f| f.code == "S2_DSP_SPOTIFY_ARTWORK_ENCODING")
        );
        assert!(evaluate(&b, Dsp::D6, &evidence).is_empty());
    }
}
