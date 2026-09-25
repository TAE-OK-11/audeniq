//! DDEX ERN 3.8.2 `NewReleaseMessage` builder.
//!
//! Structural reference: stardust-distro (MIT license)
//! `template/src/services/ern/ern-382.js` (`ERN382Builder`). The message
//! skeleton (MessageHeader / ResourceList / ReleaseList / DealList), the
//! `ern/382` namespace, the `CommonReleaseAudio{Single,Album}/13` profile ids
//! and the `UPC_DD_TTT.ext` file naming convention follow that reference.
//! The implementation below is written from scratch in Rust; no code is
//! copied.
//!
//! This is NOT a licensed DDEX profile and is NOT DDEX-certified. It is the
//! distribution system's real release-notification document, replacing the
//! synthetic fixture profile (`ern::SYNTHETIC_PROFILE`) wherever an actual
//! DSP-facing message is required. No DB, no clock, no network: the caller
//! supplies message ids and timestamps so output is deterministic.
//!
//! Deliberate deviations from the reference, all documented here:
//! - `HashSum` uses SHA-256 (the hash pinned on `catalog.assets`), not MD5.
//! - Audio technical details come from the asset `content_type`
//!   (`audio/wav` -> PCM/WAV, `audio/flac` -> FLAC, `audio/mpeg` -> MP3).
//!   PCM-specific fields (1411/44100/16/2) are emitted for WAV only.
//! - Duration is omitted: `PreparedTrack` carries no duration.
//! - Image dimensions are omitted: not stored on the asset.
//! - Credits are emitted as `ResourceContributor` with roles mapped onto
//!   the DDEX `ContributorRole` allowed-value set (`contributor_role`);
//!   unmapped roles fall back to the generic `Contributor` value.
//! - `MessageControlType` is `LiveMessage` for Initial, `UpdateMessage` for
//!   Update/Takedown. A Takedown closes the deal via
//!   `ValidityPeriod/EndDate`.
use crate::{
    ern::{ordered_tracks, validate_metadata},
    error::{Error, Result},
    preparation_model::{AssetRef, PreparedRelease},
};
use std::collections::BTreeMap;

pub const DDEX_ERN_382_NAMESPACE: &str = "http://ddex.net/xml/ern/382";
pub const DDEX_ERN_382_SCHEMA: &str =
    "http://ddex.net/xml/ern/382 http://ddex.net/xml/ern/382/release-notification.xsd";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageSubType {
    Initial,
    Update,
    Takedown,
}

impl MessageSubType {
    fn control_type(self) -> &'static str {
        match self {
            MessageSubType::Initial => "LiveMessage",
            MessageSubType::Update | MessageSubType::Takedown => "UpdateMessage",
        }
    }
}

/// Caller-supplied envelope values. Everything that would otherwise need a
/// clock or an id generator lives here so the builder stays pure.
pub struct DdexErnConfig {
    pub message_id: String,
    pub message_sub_type: MessageSubType,
    /// ISO-8601 creation timestamp, e.g. `2026-09-25T11:00:00Z`.
    pub created_at: String,
    pub sender_name: String,
    pub sender_party_id: Option<String>,
    /// (party_id, name) of the party the sender acts for.
    pub sent_on_behalf_of: Option<(String, String)>,
    pub recipient_name: String,
    pub recipient_party_id: Option<String>,
    /// Deal start, `YYYY-MM-DD`.
    pub deal_start_date: String,
    /// Deal end, `YYYY-MM-DD`. Required for `MessageSubType::Takedown`.
    pub takedown_date: Option<String>,
}

fn escaped(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('\r', "&#13;")
}

fn element(out: &mut String, name: &str, value: impl std::fmt::Display) {
    out.push_str(&format!("<{name}>{}</{name}>", escaped(&value.to_string())));
}

fn attr(name: &str, value: &str) -> String {
    format!(" {name}=\"{}\"", escaped(value))
}

fn required(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().all(crate::ern::xml_char)
}

fn validate_config(c: &DdexErnConfig) -> Result<()> {
    if !required(&c.message_id)
        || !required(&c.created_at)
        || !required(&c.sender_name)
        || !required(&c.recipient_name)
        || !required(&c.deal_start_date)
    {
        return Err(Error::Invalid);
    }
    for opt in [&c.sender_party_id, &c.recipient_party_id] {
        if opt.as_deref().is_some_and(|v| !required(v)) {
            return Err(Error::Invalid);
        }
    }
    if c.sent_on_behalf_of
        .as_ref()
        .is_some_and(|(id, name)| !required(id) || !required(name))
    {
        return Err(Error::Invalid);
    }
    match c.message_sub_type {
        MessageSubType::Takedown => {
            if c.takedown_date.as_deref().map(required).unwrap_or(false) {
                Ok(())
            } else {
                Err(Error::Invalid)
            }
        }
        _ => Ok(()),
    }
}

/// (codec, file extension); PCM-specific technical fields only for WAV.
fn audio_codec(content_type: &str) -> (&'static str, &'static str, bool) {
    match content_type {
        "audio/wav" | "audio/x-wav" => ("PCM", "wav", true),
        "audio/flac" => ("FLAC", "flac", false),
        "audio/mpeg" => ("MP3", "mp3", false),
        _ => ("Unknown", "bin", false),
    }
}

fn image_codec(content_type: &str) -> (&'static str, &'static str) {
    match content_type {
        "image/jpeg" => ("JPEG", "jpg"),
        "image/png" => ("PNG", "png"),
        _ => ("Unknown", "bin"),
    }
}

fn message_header(out: &mut String, c: &DdexErnConfig) {
    out.push_str("<MessageHeader>");
    element(out, "MessageThreadId", &c.message_id);
    element(out, "MessageId", &c.message_id);
    element(out, "MessageCreatedDateTime", &c.created_at);
    element(out, "MessageControlType", c.message_sub_type.control_type());
    out.push_str("<MessageSender>");
    if let Some(id) = &c.sender_party_id {
        element(out, "PartyId", id);
    }
    out.push_str("<PartyName>");
    element(out, "FullName", &c.sender_name);
    out.push_str("</PartyName></MessageSender>");
    if let Some((id, name)) = &c.sent_on_behalf_of {
        out.push_str("<SentOnBehalfOf>");
        element(out, "PartyId", id);
        out.push_str("<PartyName>");
        element(out, "FullName", name);
        out.push_str("</PartyName></SentOnBehalfOf>");
    }
    out.push_str("<MessageRecipient>");
    if let Some(id) = &c.recipient_party_id {
        element(out, "PartyId", id);
    }
    out.push_str("<PartyName>");
    element(out, "FullName", &c.recipient_name);
    out.push_str("</PartyName></MessageRecipient>");
    out.push_str("</MessageHeader>");
}

fn file_block(out: &mut String, file_name: &str, sha256: &str) {
    out.push_str("<File>");
    element(out, "FileName", file_name);
    element(out, "FilePath", file_name);
    out.push_str("<HashSum>");
    element(out, "HashSumAlgorithmType", "SHA256");
    element(out, "HashSum", sha256);
    out.push_str("</HashSum></File>");
}

/// Map a free-text credit role onto the DDEX ERN 3.8.2 `ContributorRole`
/// allowed-value set. The table is a curated, documented subset: common
/// studio roles map to their AVS counterpart, and anything unmapped falls
/// back to `Contributor`, the AVS generic value, so emitted XML only ever
/// carries allowed values. The stored role text is unchanged in the
/// database; review this table against the partner's profile AVS before
/// F6 production use.
fn contributor_role(role: &str) -> &'static str {
    match role.trim().to_ascii_lowercase().as_str() {
        "composer" | "songwriter" | "writer" | "music" => "Composer",
        "lyricist" | "lyrics" | "words" => "Lyricist",
        "arranger" => "Arranger",
        "producer" | "music producer" | "executive producer" | "co-producer" => "Producer",
        "publisher" | "music publisher" => "Publisher",
        "engineer" | "recording engineer" | "sound engineer" | "audio engineer"
        | "mastering engineer" => "Engineer",
        "mixer" | "mix engineer" | "mixing engineer" => "Mixer",
        "remixer" => "Remixer",
        "conductor" => "Conductor",
        "narrator" => "Narrator",
        "author" => "Author",
        "musician" | "instrumentalist" | "performer" => "Musician",
        "featured artist" | "featuring" | "feat." | "feat" => "FeaturedArtist",
        "artist" | "main artist" => "Artist",
        _ => "Contributor",
    }
}

/// Credits keyed by canonical track id, for `ResourceContributor` output.
fn credit_map(prepared: &PreparedRelease) -> BTreeMap<uuid::Uuid, Vec<(String, String)>> {
    let mut map: BTreeMap<uuid::Uuid, Vec<(String, String)>> = BTreeMap::new();
    let mut tracks: Vec<_> = prepared.canonical.tracks.iter().collect();
    tracks.sort_by_key(|t| (t.disc_number, t.track_number));
    for t in tracks {
        let mut credits: Vec<_> = t
            .credits
            .iter()
            .map(|c| (c.party_name.clone(), c.role.clone()))
            .collect();
        credits.sort();
        map.insert(t.track_id, credits);
    }
    map
}

fn resource_list(out: &mut String, prepared: &PreparedRelease) {
    out.push_str("<ResourceList>");
    let credits = credit_map(prepared);
    for (i, track) in ordered_tracks(prepared).iter().enumerate() {
        let resource_ref = format!("A{:03}", i + 1);
        let tech_ref = format!("T{resource_ref}");
        let (codec, ext, is_pcm) = audio_codec(&track.audio.content_type);
        let file_name = format!(
            "{}_{:02}_{:03}.{ext}",
            prepared.upc, track.disc_number, track.track_number
        );
        out.push_str("<SoundRecording>");
        element(out, "SoundRecordingType", "MusicalWorkSoundRecording");
        element(out, "ResourceReference", &resource_ref);
        out.push_str("<SoundRecordingId>");
        element(out, "ISRC", &track.isrc);
        out.push_str("</SoundRecordingId>");
        out.push_str("<ReferenceTitle>");
        element(out, "TitleText", &track.title);
        out.push_str("</ReferenceTitle>");
        out.push_str("<DisplayTitle>");
        element(out, "TitleText", &track.title);
        out.push_str("</DisplayTitle>");
        out.push_str("<DisplayArtist><PartyName>");
        element(out, "FullName", &track.artist);
        out.push_str("</PartyName>");
        element(out, "ArtistRole", "MainArtist");
        out.push_str("</DisplayArtist>");
        out.push_str("<DetailsByTerritory>");
        element(out, "TerritoryCode", "Worldwide");
        if let Some(list) = credits.get(&track.id) {
            for (n, (name, role)) in list.iter().enumerate() {
                out.push_str(&format!(
                    "<ResourceContributor{}>",
                    attr("sequenceNumber", &(n + 1).to_string())
                ));
                out.push_str("<PartyName>");
                element(out, "FullName", name);
                out.push_str("</PartyName>");
                element(out, "Role", contributor_role(role));
                out.push_str("</ResourceContributor>");
            }
        }
        element(out, "LanguageOfPerformance", &prepared.language);
        out.push_str("<PLine>");
        element(out, "Year", prepared.release_date.format("%Y").to_string());
        element(out, "PLineText", &prepared.p_line);
        out.push_str("</PLine>");
        out.push_str("<TechnicalSoundRecordingDetails>");
        element(out, "TechnicalResourceDetailsReference", &tech_ref);
        element(out, "AudioCodecType", codec);
        if is_pcm {
            element(out, "BitRate", "1411");
            element(out, "SamplingRate", "44100");
            element(out, "BitsPerSample", "16");
            element(out, "NumberOfChannels", "2");
        }
        file_block(out, &file_name, &track.audio.sha256);
        out.push_str("</TechnicalSoundRecordingDetails>");
        out.push_str("</DetailsByTerritory>");
        out.push_str("</SoundRecording>");
    }
    image_resource(out, prepared);
    out.push_str("</ResourceList>");
}

fn image_resource(out: &mut String, prepared: &PreparedRelease) {
    let art: &AssetRef = &prepared.artwork;
    let (codec, ext) = image_codec(&art.content_type);
    out.push_str("<Image>");
    element(out, "ImageType", "FrontCoverImage");
    element(out, "ResourceReference", "I001");
    out.push_str("<ImageId>");
    out.push_str(&format!(
        "<ProprietaryId{}>{}</ProprietaryId>",
        attr("Namespace", "DPID:PADPIDA2023081501R"),
        escaped(&format!("{}_IMG_001", prepared.upc))
    ));
    out.push_str("</ImageId>");
    out.push_str("<DetailsByTerritory>");
    element(out, "TerritoryCode", "Worldwide");
    out.push_str("<TechnicalImageDetails>");
    element(out, "TechnicalResourceDetailsReference", "TI001");
    element(out, "ImageCodecType", codec);
    file_block(out, &format!("{}.{ext}", prepared.upc), &art.sha256);
    out.push_str("</TechnicalImageDetails>");
    out.push_str("</DetailsByTerritory>");
    out.push_str("</Image>");
}

fn release_type_ddex(release_type: &str) -> &'static str {
    match release_type {
        "SINGLE" => "Single",
        "EP" => "EP",
        _ => "Album",
    }
}

fn release_list(out: &mut String, prepared: &PreparedRelease) {
    let year = prepared.release_date.format("%Y").to_string();
    out.push_str("<ReleaseList>");
    out.push_str(&format!("<Release{}>", attr("IsMainRelease", "true")));
    element(out, "ReleaseReference", "R001");
    element(
        out,
        "ReleaseType",
        release_type_ddex(&prepared.release_type),
    );
    out.push_str("<ReleaseId>");
    out.push_str(&format!(
        "<ICPN{}>{}</ICPN>",
        attr("IsEan", "true"),
        escaped(&prepared.upc)
    ));
    out.push_str("</ReleaseId>");
    out.push_str("<ReferenceTitle>");
    element(out, "TitleText", &prepared.title);
    out.push_str("</ReferenceTitle>");
    out.push_str("<DisplayTitle>");
    element(out, "TitleText", &prepared.title);
    out.push_str("</DisplayTitle>");
    out.push_str("<DisplayArtist><PartyName>");
    element(out, "FullName", &prepared.artist);
    out.push_str("</PartyName>");
    element(out, "ArtistRole", "MainArtist");
    out.push_str("</DisplayArtist>");
    out.push_str("<PLine>");
    element(out, "Year", &year);
    element(out, "PLineText", &prepared.p_line);
    out.push_str("</PLine>");
    out.push_str("<CLine>");
    element(out, "Year", &year);
    element(out, "CLineText", &prepared.c_line);
    out.push_str("</CLine>");
    element(
        out,
        "GlobalOriginalReleaseDate",
        prepared.release_date.format("%Y-%m-%d").to_string(),
    );
    out.push_str("<ReleaseDetailsByTerritory>");
    element(out, "TerritoryCode", "Worldwide");
    element(out, "DisplayArtistName", &prepared.artist);
    element(
        out,
        "ReleaseDate",
        prepared.release_date.format("%Y-%m-%d").to_string(),
    );
    out.push_str("<ReleaseResourceReferenceList>");
    for (i, _) in ordered_tracks(prepared).iter().enumerate() {
        element(out, "ReleaseResourceReference", format!("A{:03}", i + 1));
    }
    element(out, "ReleaseResourceReference", "I001");
    out.push_str("</ReleaseResourceReferenceList>");
    out.push_str("</ReleaseDetailsByTerritory>");
    out.push_str("</Release>");
    out.push_str("</ReleaseList>");
}

fn deal_list(out: &mut String, c: &DdexErnConfig) {
    out.push_str("<DealList><ReleaseDeal>");
    element(out, "DealReleaseReference", "R001");
    out.push_str("<Deal><DealTerms>");
    element(out, "CommercialModelType", "SubscriptionModel");
    out.push_str("<Usage>");
    element(out, "UseType", "OnDemandStream");
    element(out, "UseType", "NonInteractiveStream");
    out.push_str("</Usage>");
    element(out, "TerritoryCode", "Worldwide");
    out.push_str("<ValidityPeriod>");
    element(out, "StartDate", &c.deal_start_date);
    if c.message_sub_type == MessageSubType::Takedown
        && let Some(end) = &c.takedown_date
    {
        element(out, "EndDate", end);
    }
    out.push_str("</ValidityPeriod>");
    out.push_str("</DealTerms>");
    element(out, "DealId", "R001_DEAL_1");
    out.push_str("</Deal></ReleaseDeal></DealList>");
}

/// Build a DDEX ERN 3.8.2 `NewReleaseMessage` for a prepared release.
pub fn generate_ddex_ern_382(prepared: &PreparedRelease, config: &DdexErnConfig) -> Result<String> {
    validate_metadata(prepared)?;
    validate_config(config)?;
    let profile = if prepared.tracks.len() > 1 {
        "AudioAlbum"
    } else {
        "AudioSingle"
    };
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
        &format!("CommonRelease{profile}/13"),
    ));
    out.push_str(&attr("LanguageAndScriptCode", "en"));
    out.push_str(&attr("xs:schemaLocation", DDEX_ERN_382_SCHEMA));
    out.push('>');
    message_header(&mut out, config);
    resource_list(&mut out, prepared);
    release_list(&mut out, prepared);
    deal_list(&mut out, config);
    out.push_str("</ern:NewReleaseMessage>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::contributor_role;

    #[test]
    fn contributor_role_maps_studio_roles_to_avs() {
        assert_eq!(contributor_role("composer"), "Composer");
        assert_eq!(contributor_role("Songwriter"), "Composer");
        assert_eq!(contributor_role("lyricist"), "Lyricist");
        assert_eq!(contributor_role("ARRANGER"), "Arranger");
        assert_eq!(contributor_role("Mixing Engineer"), "Mixer");
        assert_eq!(contributor_role("Mastering Engineer"), "Engineer");
        assert_eq!(contributor_role("feat."), "FeaturedArtist");
        assert_eq!(contributor_role("  producer  "), "Producer");
    }

    #[test]
    fn contributor_role_unknown_falls_back_to_generic() {
        assert_eq!(contributor_role("vibe curator"), "Contributor");
        assert_eq!(contributor_role(""), "Contributor");
    }
}
