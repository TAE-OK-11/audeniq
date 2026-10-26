//! DDEX ERN 3.8.2 `NewReleaseMessage` for a music video: a `VideoSingle`
//! release of one `Video` resource (its own ISRC), linked to the audio
//! release it comes from (`RelatedRelease` / `IsFromAudio`, by UPC).
//!
//! The release facts (artist, P/C lines, language, territories, explicit)
//! come from the audio release's prepared snapshot; the video facts from
//! the add-on order (title, director, synopsis, 영상물등급위원회 rating) and
//! the media engine's report of the uploaded file (codec, frame size and
//! rate, bit rates, sound). Like the audio builder it is pure: no DB,
//! clock or network. Technical fields the report does not give are omitted,
//! never invented; the file's SHA-256 is not stated (an 8 GB master is not
//! hashed on upload).
use crate::{
    ddex_ern::{
        DDEX_ERN_382_NAMESPACE, DDEX_ERN_382_SCHEMA, DdexErnConfig, DealTiming, MessageSubType,
        artist_party_id, attr, deal_list, message_header, required, validate_config,
    },
    ern::{element, push_escaped},
    error::{Error, Result},
    preparation_model::PreparedRelease,
};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

/// What the media engine measured (`audeniq-music video probe`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoTech {
    /// `MP4` or `QuickTime`.
    pub container: &'static str,
    /// FFprobe codec name (`h264`, `hevc`, `prores`, …) and profile.
    pub codec: String,
    pub profile: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
    pub interlaced: bool,
    pub video_kbps: Option<u64>,
    pub overall_kbps: Option<u64>,
    pub audio_codec: Option<String>,
    pub audio_kbps: Option<u64>,
    pub audio_channels: Option<u32>,
    pub audio_rate: Option<u32>,
    pub audio_bits: Option<u32>,
}

impl VideoTech {
    /// From an engine report; `None` without a video stream.
    pub fn from_report(r: &Value) -> Option<VideoTech> {
        let v = &r["video"][0];
        if !v.is_object() {
            return None;
        }
        let a = &r["audio"][0];
        let kbps = |x: &Value| x.as_f64().map(|b| (b / 1000.0).round() as u64);
        Some(VideoTech {
            container: if r["container"]["format"] == "mov" {
                "QuickTime"
            } else {
                "MP4"
            },
            codec: v["codec_name"].as_str()?.to_owned(),
            profile: v["profile"].as_str().map(str::to_owned),
            width: v["width"].as_u64()? as u32,
            height: v["height"].as_u64()? as u32,
            fps: v["fps"].as_f64(),
            interlaced: v["field_order"] == "interlaced",
            video_kbps: kbps(&v["bit_rate"]),
            overall_kbps: kbps(&r["container"]["bit_rate"]),
            audio_codec: a["codec_name"].as_str().map(str::to_owned),
            audio_kbps: kbps(&a["bit_rate"]),
            audio_channels: a["channels"].as_u64().map(|c| c as u32),
            audio_rate: a["sample_rate"].as_u64().map(|c| c as u32),
            audio_bits: a["bits_per_sample"].as_u64().map(|c| c as u32),
        })
    }
}

pub struct PreparedVideo<'a> {
    /// The audio release this video belongs to.
    pub release: &'a PreparedRelease,
    pub isrc: String,
    pub title: String,
    pub director: Option<String>,
    pub synopsis: Option<String>,
    /// 영상물등급위원회 grade: `ALL`, `12`, `15`, `18`.
    pub rating: Option<String>,
    /// Public release date of the video; the audio release date when none.
    pub release_date: Option<chrono::NaiveDate>,
    pub duration_secs: f64,
    pub tech: VideoTech,
    pub file_name: String,
}

fn kmrb(grade: &str) -> Option<&'static str> {
    Some(match grade {
        "ALL" => "전체관람가",
        "12" => "12세이상관람가",
        "15" => "15세이상관람가",
        "18" => "청소년관람불가",
        _ => return None,
    })
}

/// AVS `VideoCodecType`, or `UserDefined` with the codec named.
fn video_codec(t: &VideoTech) -> (&'static str, Option<String>) {
    match t.codec.as_str() {
        "h264" => ("AVC", None),
        "mpeg4" => ("MPEG-4", None),
        "mpeg2video" => ("MPEG-2", None),
        "hevc" => ("UserDefined", Some("HEVC".into())),
        "prores" => (
            "UserDefined",
            Some(
                format!("ProRes {}", t.profile.as_deref().unwrap_or(""))
                    .trim_end()
                    .to_owned(),
            ),
        ),
        other => ("UserDefined", Some(other.to_uppercase())),
    }
}

fn audio_codec(c: &str) -> Option<&'static str> {
    Some(match c {
        "aac" => "AAC",
        "mp3" => "MP3",
        "flac" => "FLAC",
        c if c.starts_with("pcm_") => "PCM",
        _ => return None,
    })
}

/// `<Name UnitOfMeasure="…">value</Name>`.
fn measured(out: &mut String, name: &str, unit: &str, value: impl std::fmt::Display) {
    out.push('<');
    out.push_str(name);
    out.push_str(&attr("UnitOfMeasure", unit));
    out.push('>');
    push_escaped(out, &value.to_string());
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

fn check_isrc(isrc: &str) -> bool {
    let b = isrc.as_bytes();
    b.len() == 12
        && b[..2].iter().all(u8::is_ascii_uppercase)
        && b[2..5]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && b[5..].iter().all(u8::is_ascii_digit)
}

/// Build the music video's ERN 3.8.2 message.
pub fn generate_ddex_ern_382_video(v: &PreparedVideo, config: &DdexErnConfig) -> Result<String> {
    validate_config(config)?;
    if !check_isrc(&v.isrc) {
        return Err(Error::PolicyGate("DDEX_VIDEO_ISRC_INVALID"));
    }
    if v.duration_secs.is_nan() || v.duration_secs <= 0.0 {
        return Err(Error::PolicyGate("DDEX_DURATION_UNKNOWN"));
    }
    if v.title.trim().is_empty() {
        return Err(Error::PolicyGate("DDEX_VIDEO_TITLE_MISSING"));
    }
    config
        .sender_party_id
        .as_deref()
        .filter(|s| required(s))
        .ok_or(Error::PolicyGate("DDEX_SENDER_DPID_MISSING"))?;
    let r = v.release;
    let (title, artist) = (nfc(v.title.trim()), nfc(&r.artist));
    let (p_line, c_line) = (nfc(&r.p_line), nfc(&r.c_line));
    let date = v.release_date.unwrap_or(r.release_date);
    let year = date.format("%Y").to_string();
    let artist_id = config
        .artist_party_id
        .as_deref()
        .zip(config.recipient_party_id.as_deref());

    let mut out =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ern:NewReleaseMessage");
    out.push_str(&attr("xmlns:ern", DDEX_ERN_382_NAMESPACE));
    out.push_str(&attr(
        "xmlns:xs",
        "http://www.w3.org/2001/XMLSchema-instance",
    ));
    out.push_str(&attr("MessageSchemaVersionId", "ern/382"));
    out.push_str(&attr(
        "ReleaseProfileVersionId",
        "CommonReleaseVideoSingle/13",
    ));
    out.push_str(&attr("LanguageAndScriptCode", "en"));
    out.push_str(&attr("xs:schemaLocation", DDEX_ERN_382_SCHEMA));
    out.push('>');
    message_header(&mut out, config);
    element(
        &mut out,
        "UpdateIndicator",
        match config.message_sub_type {
            MessageSubType::Initial => "OriginalMessage",
            MessageSubType::Update | MessageSubType::Takedown => "UpdateMessage",
        },
    );

    // ResourceList: the one Video.
    out.push_str("<ResourceList><Video>");
    element(&mut out, "VideoType", "ShortFormMusicalWorkVideo");
    out.push_str("<VideoId>");
    element(&mut out, "ISRC", &v.isrc);
    out.push_str("</VideoId>");
    element(&mut out, "ResourceReference", "A001");
    out.push_str("<ReferenceTitle>");
    element(&mut out, "TitleText", &title);
    out.push_str("</ReferenceTitle>");
    if !r.language.is_empty() {
        element(&mut out, "LanguageOfPerformance", &r.language);
    }
    element(&mut out, "Duration", format!("PT{}S", v.duration_secs));
    out.push_str("<VideoDetailsByTerritory>");
    element(&mut out, "TerritoryCode", "Worldwide");
    out.push_str("<Title>");
    element(&mut out, "TitleText", &title);
    out.push_str("</Title>");
    out.push_str("<DisplayArtist><PartyName>");
    element(&mut out, "FullName", &artist);
    out.push_str("</PartyName>");
    artist_party_id(&mut out, artist_id);
    element(&mut out, "ArtistRole", "MainArtist");
    out.push_str("</DisplayArtist>");
    if let Some(d) = v
        .director
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        out.push_str("<ResourceContributor><PartyName>");
        element(&mut out, "FullName", nfc(d));
        out.push_str("</PartyName>");
        element(&mut out, "ResourceContributorRole", "FilmDirector");
        out.push_str("</ResourceContributor>");
    }
    out.push_str("<PLine>");
    element(&mut out, "Year", &year);
    element(&mut out, "PLineText", &p_line);
    out.push_str("</PLine>");
    if r.explicit {
        element(&mut out, "ParentalWarningType", "Explicit");
    }
    if let Some(text) = v.rating.as_deref().and_then(kmrb) {
        out.push_str("<AvRating>");
        element(&mut out, "RatingText", text);
        element(&mut out, "RatingAgency", "KMRB");
        out.push_str("</AvRating>");
    }
    if let Some(s) = v
        .synopsis
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        element(&mut out, "Synopsis", nfc(s));
    }
    out.push_str("<CLine>");
    element(&mut out, "Year", &year);
    element(&mut out, "CLineText", &c_line);
    out.push_str("</CLine>");
    // TechnicalVideoDetails, XSD order.
    let t = &v.tech;
    out.push_str("<TechnicalVideoDetails>");
    element(&mut out, "TechnicalResourceDetailsReference", "TA001");
    if let Some(k) = t.overall_kbps {
        element(&mut out, "OverallBitRate", k);
    }
    element(&mut out, "ContainerFormat", t.container);
    let (codec, named) = video_codec(t);
    out.push_str("<VideoCodecType");
    if let Some(n) = &named {
        out.push_str(&attr("UserDefinedValue", n));
    }
    out.push('>');
    out.push_str(codec);
    out.push_str("</VideoCodecType>");
    if let Some(k) = t.video_kbps {
        element(&mut out, "VideoBitRate", k);
    }
    if let Some(f) = t.fps {
        measured(
            &mut out,
            "FrameRate",
            if t.interlaced {
                "Hz(interlaced)"
            } else {
                "Hz(non-interlaced)"
            },
            f,
        );
    }
    measured(&mut out, "ImageHeight", "Pixel", t.height);
    measured(&mut out, "ImageWidth", "Pixel", t.width);
    element(
        &mut out,
        "VideoDefinitionType",
        if t.width.min(t.height) >= 720 {
            "HighDefinition"
        } else {
            "StandardDefinition"
        },
    );
    if let Some(c) = t.audio_codec.as_deref().and_then(audio_codec) {
        element(&mut out, "AudioCodecType", c);
        if let Some(k) = t.audio_kbps {
            element(&mut out, "AudioBitRate", k);
        }
        if let Some(c) = t.audio_channels {
            element(&mut out, "NumberOfAudioChannels", c);
        }
        if let Some(r) = t.audio_rate {
            element(&mut out, "AudioSamplingRate", r);
        }
        if let Some(b) = t.audio_bits {
            element(&mut out, "AudioBitsPerSample", b);
        }
    }
    element(&mut out, "Duration", format!("PT{}S", v.duration_secs));
    out.push_str("<File>");
    element(&mut out, "FileName", &v.file_name);
    out.push_str("</File>");
    out.push_str("</TechnicalVideoDetails>");
    out.push_str("</VideoDetailsByTerritory></Video></ResourceList>");

    // ReleaseList: VideoSingle identified by the video's ISRC.
    out.push_str("<ReleaseList>");
    out.push_str(&format!("<Release{}>", attr("IsMainRelease", "true")));
    out.push_str("<ReleaseId>");
    element(&mut out, "ISRC", &v.isrc);
    out.push_str("</ReleaseId>");
    element(&mut out, "ReleaseReference", "R001");
    out.push_str("<ReferenceTitle>");
    element(&mut out, "TitleText", &title);
    out.push_str("</ReferenceTitle>");
    out.push_str("<ReleaseResourceReferenceList>");
    element(&mut out, "ReleaseResourceReference", "A001");
    out.push_str("</ReleaseResourceReferenceList>");
    element(&mut out, "ReleaseType", "VideoSingle");
    out.push_str("<ReleaseDetailsByTerritory>");
    element(&mut out, "TerritoryCode", "Worldwide");
    element(&mut out, "DisplayArtistName", &artist);
    out.push_str("<DisplayArtist><PartyName>");
    element(&mut out, "FullName", &artist);
    out.push_str("</PartyName>");
    artist_party_id(&mut out, artist_id);
    element(&mut out, "ArtistRole", "MainArtist");
    out.push_str("</DisplayArtist>");
    if !r.upc.is_empty() {
        out.push_str("<RelatedRelease><ReleaseId><ICPN");
        out.push_str(&attr("IsEan", "true"));
        out.push('>');
        push_escaped(&mut out, &r.upc);
        out.push_str("</ICPN></ReleaseId>");
        element(&mut out, "ReleaseRelationshipType", "IsFromAudio");
        out.push_str("</RelatedRelease>");
    }
    if r.explicit {
        element(&mut out, "ParentalWarningType", "Explicit");
    }
    if let Some(text) = v.rating.as_deref().and_then(kmrb) {
        out.push_str("<AvRating>");
        element(&mut out, "RatingText", text);
        element(&mut out, "RatingAgency", "KMRB");
        out.push_str("</AvRating>");
    }
    element(&mut out, "ReleaseDate", date.format("%Y-%m-%d").to_string());
    out.push_str("</ReleaseDetailsByTerritory>");
    out.push_str("<PLine>");
    element(&mut out, "Year", &year);
    element(&mut out, "PLineText", &p_line);
    out.push_str("</PLine><CLine>");
    element(&mut out, "Year", &year);
    element(&mut out, "CLineText", &c_line);
    out.push_str("</CLine>");
    element(
        &mut out,
        "GlobalOriginalReleaseDate",
        date.format("%Y-%m-%d").to_string(),
    );
    out.push_str("</Release></ReleaseList>");

    deal_list(
        &mut out,
        config,
        &r.territories,
        DealTiming {
            release_time: None,
            preorder_start: None,
            rights_end: r.rights_end,
        },
    );
    out.push_str("</ern:NewReleaseMessage>\n");
    Ok(out)
}
