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
//! - `HashSum` carries the hex SHA-256 pinned on `catalog.assets`, labelled
//!   `UserDefined`: ERN 3.8.2's `HashSumAlgorithmType` enum has no SHA-256
//!   entry, and mislabelling it SHA1 would be false. Sender convention:
//!   `UserDefined` = lowercase hex SHA-256.
//! - Audio technical details come from the asset `content_type`
//!   (`audio/wav` -> PCM/WAV, `audio/flac` -> FLAC, `audio/mpeg` -> MP3).
//!   The PCM spec fields (BitRate/NumberOfChannels/SamplingRate/
//!   BitsPerSample) carry the ffprobe-measured values persisted on
//!   `catalog.assets` by Stage 1 QC, for WAV only; they are omitted when
//!   unknown, never fabricated.
//! - Duration is required by the schema: emitted as `PT{secs}S` from
//!   `AssetRef::duration_secs` (Stage 1 persists it from ffprobe); missing
//!   duration fails closed with `DDEX_DURATION_UNKNOWN`.
//! - Image dimensions are omitted: not stored on the asset.
//! - Credits are emitted as `ResourceContributor` with roles mapped onto
//!   the DDEX `ContributorRole` allowed-value set (`contributor_role`);
//!   unmapped roles fall back to the generic `Contributor` value.
//! - `MessageControlType` is `LiveMessage` for Initial, `UpdateMessage` for
//!   Update/Takedown. A Takedown closes the deal via
//!   `ValidityPeriod/EndDate`.
//! - `UpdateIndicator` is emitted (`OriginalMessage` for Initial,
//!   `UpdateMessage` otherwise): deprecated in ERN 3.8.2 but still
//!   schema-valid, and receivers (ddex-workbench's validator,
//!   stardust-dsp's ingestion parser) key message intent off it.
//! - `MessageThreadId` defaults to the message id; updates/takedowns must
//!   pass the original thread id via `DdexErnConfig::message_thread_id`.
use crate::{
    ern::{element, ordered_tracks, push_escaped, validate_metadata},
    error::{Error, Result},
    preparation_model::{AssetRef, PreparedRelease, PreparedTrack},
};
use std::borrow::Cow;
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
    /// Thread this message belongs to. `None` falls back to `message_id`
    /// (a new thread). Updates and takedowns MUST pass the original
    /// message's thread id so the recipient can correlate them.
    pub message_thread_id: Option<String>,
    pub message_sub_type: MessageSubType,
    /// ISO-8601 creation timestamp, e.g. `2026-09-25T11:00:00Z`.
    pub created_at: String,
    pub sender_name: String,
    /// The sender's own DPID. Required: without it message generation
    /// fails closed (`DDEX_SENDER_DPID_MISSING`) rather than emitting a
    /// fabricated `DPID:` namespace. The pipeline only calls this when
    /// the org has a sender DPID.
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

/// NFC-normalize the human-text fields of a release.
///
/// Returns a borrow when every field is already NFC (the common case) and
/// an owned normalized copy otherwise, so the hot path never allocates.
fn normalize_nfc(prepared: &PreparedRelease) -> Cow<'_, PreparedRelease> {
    use unicode_normalization::UnicodeNormalization;

    let dirty = |s: &str| !unicode_normalization::is_nfc(s);
    let release_dirty = [
        &prepared.title,
        &prepared.artist,
        &prepared.p_line,
        &prepared.c_line,
    ]
    .iter()
    .any(|s| dirty(s));
    let tracks_dirty = prepared
        .tracks
        .iter()
        .any(|t| dirty(&t.title) || dirty(&t.artist) || dirty(&t.version));
    if !release_dirty && !tracks_dirty {
        return Cow::Borrowed(prepared);
    }
    let nfc = |s: &str| -> String {
        if dirty(s) {
            s.nfc().collect()
        } else {
            s.to_string()
        }
    };
    let mut out = prepared.clone();
    out.title = nfc(&prepared.title);
    out.artist = nfc(&prepared.artist);
    out.p_line = nfc(&prepared.p_line);
    out.c_line = nfc(&prepared.c_line);
    for (dst, src) in out.tracks.iter_mut().zip(prepared.tracks.iter()) {
        dst.title = nfc(&src.title);
        dst.artist = nfc(&src.artist);
        dst.version = nfc(&src.version);
    }
    Cow::Owned(out)
}

fn attr(name: &str, value: &str) -> String {
    let mut out = String::with_capacity(name.len() + value.len() + 4);
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    push_escaped(&mut out, value);
    out.push('"');
    out
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
    for opt in [&c.recipient_party_id] {
        if opt.as_deref().is_some_and(|v| !required(v)) {
            return Err(Error::Invalid);
        }
    }
    // The image ProprietaryId namespace is `DPID:{sender_party_id}`; a
    // missing sender DPID must fail here, never become a fabricated
    // `DPID:AUDENIQ` fallback.
    if !c.sender_party_id.as_deref().is_some_and(required) {
        return Err(Error::PolicyGate("DDEX_SENDER_DPID_MISSING"));
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

/// (codec, file extension). Technical spec fields (bit rate, channels,
/// sample rate, bit depth) are NOT derived here: they come from the
/// ffprobe-measured `AssetRef` fields, and are omitted when unknown rather
/// than fabricated.
fn audio_codec(content_type: &str) -> (&'static str, &'static str) {
    match content_type {
        "audio/wav" | "audio/x-wav" => ("PCM", "wav"),
        "audio/flac" => ("FLAC", "flac"),
        "audio/mpeg" => ("MP3", "mp3"),
        _ => ("Unknown", "bin"),
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
    // XSD order: MessageThreadId?, MessageId, MessageFileName?,
    // MessageSender, SentOnBehalfOf?, MessageRecipient,
    // MessageCreatedDateTime, MessageAuditTrail?, Comment?,
    // MessageControlType?
    out.push_str("<MessageHeader>");
    element(
        out,
        "MessageThreadId",
        c.message_thread_id.as_deref().unwrap_or(&c.message_id),
    );
    element(out, "MessageId", &c.message_id);
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
    element(out, "MessageCreatedDateTime", &c.created_at);
    element(out, "MessageControlType", c.message_sub_type.control_type());
    out.push_str("</MessageHeader>");
}

fn file_block(out: &mut String, file_name: &str, sha256: &str) {
    out.push_str("<File>");
    element(out, "FileName", file_name);
    element(out, "FilePath", file_name);
    // XSD order inside HashSum: HashSum, HashSumAlgorithmType,
    // HashSumDataType?
    // The value is the hex SHA-256 pinned on catalog.assets. ERN 3.8.2's
    // HashSumAlgorithmType enum has no SHA-256 entry (only MD4/MD5/SHA/
    // SHA1/UserDefined), so it is labelled UserDefined: mislabelling it
    // SHA1 would be a lie. Documented sender convention: UserDefined =
    // lowercase hex SHA-256.
    out.push_str("<HashSum>");
    element(out, "HashSum", sha256);
    element(out, "HashSumAlgorithmType", "UserDefined");
    out.push_str("</HashSum></File>");
}

/// Map a free-text credit role onto the DDEX ERN 3.8.2 `ContributorRole`
/// allowed-value set. The table is a curated, documented subset: common
/// studio roles map to their AVS counterpart, and anything unmapped falls
/// back to `Contributor`, the AVS generic value, so emitted XML only ever
/// carries allowed values. The stored role text is unchanged in the
/// database; review this table against the partner's profile AVS before
/// F6 production use.
/// Map a free-text studio credit role onto the ERN 3.8.2
/// `ResourceContributorRole` AVS (`avs_20161006.xsd`). That enum has no
/// Composer/Lyricist/Arranger/Mixer/Engineer values, so studio roles map
/// onto the closest valid value and everything else falls back to the
/// generic `Contributor`. The credit's display name is always preserved in
/// `PartyName`; only the role code is generalized — never an invented
/// enum value, or the message fails XSD validation.
fn contributor_role(role: &str) -> &'static str {
    match role.trim().to_ascii_lowercase().as_str() {
        "producer" | "music producer" | "executive producer" | "co-producer" => "Producer",
        "featured artist" | "featuring" | "feat." | "feat" => "FeaturedArtist",
        "conductor" => "Conductor",
        "narrator" => "Narrator",
        "artist" | "main artist" => "Artist",
        "musician" | "instrumentalist" | "performer" | "associated performer" => {
            "AssociatedPerformer"
        }
        "engineer" | "recording engineer" | "sound engineer" | "audio engineer"
        | "mastering engineer" | "mixer" | "mix engineer" | "mixing engineer" => "StudioPersonnel",
        // Composer, songwriter, lyricist, arranger, remixer, publisher have
        // no 3.8.2 enum value: generic Contributor, name preserved.
        _ => "Contributor",
    }
}

/// Credits keyed by canonical track id, for `ResourceContributor` output.
/// Built from the already-ordered prepared tracks via a canonical-track
/// index: no second sort, no per-track clone of the credit list.
fn credit_map<'a>(
    tracks: &[&'a PreparedTrack],
    canonical_by_id: &std::collections::HashMap<
        uuid::Uuid,
        &'a crate::distribution::CanonicalTrack,
    >,
) -> BTreeMap<uuid::Uuid, Vec<(&'a str, &'a str)>> {
    let mut map: BTreeMap<uuid::Uuid, Vec<(&str, &str)>> = BTreeMap::new();
    for t in tracks {
        if let Some(pinned) = canonical_by_id.get(&t.id) {
            let mut credits: Vec<(&str, &str)> = pinned
                .credits
                .iter()
                .map(|c| (c.party_name.as_str(), c.role.as_str()))
                .collect();
            credits.sort();
            map.insert(t.id, credits);
        }
    }
    map
}

fn resource_list(
    out: &mut String,
    prepared: &PreparedRelease,
    tracks: &[&PreparedTrack],
    sender_dpid: &str,
) -> Result<()> {
    out.push_str("<ResourceList>");
    // The resource refs (A001…), the credit index and the image ref below
    // all follow the one ordered list the caller computed, so the
    // A-numbers agree with the release list.
    let canonical_by_id: std::collections::HashMap<
        uuid::Uuid,
        &crate::distribution::CanonicalTrack,
    > = prepared
        .canonical
        .tracks
        .iter()
        .map(|t| (t.track_id, t))
        .collect();
    let credits = credit_map(tracks, &canonical_by_id);
    for (i, track) in tracks.iter().enumerate() {
        let resource_ref = format!("A{:03}", i + 1);
        let tech_ref = format!("T{resource_ref}");
        let (codec, ext) = audio_codec(&track.audio.content_type);
        let file_name = format!(
            "{}_{:02}_{:03}.{ext}",
            prepared.upc, track.disc_number, track.track_number
        );
        // XSD order: SoundRecordingType?, IsArtistRelated?,
        // SoundRecordingId, ResourceReference, ReferenceTitle, ...,
        // LanguageOfPerformance?, Duration,
        // SoundRecordingDetailsByTerritory.
        out.push_str("<SoundRecording>");
        element(out, "SoundRecordingType", "MusicalWorkSoundRecording");
        out.push_str("<SoundRecordingId>");
        element(out, "ISRC", &track.isrc);
        out.push_str("</SoundRecordingId>");
        element(out, "ResourceReference", &resource_ref);
        out.push_str("<ReferenceTitle>");
        element(out, "TitleText", &track.title);
        out.push_str("</ReferenceTitle>");
        element(out, "LanguageOfPerformance", &prepared.language);
        let secs = track
            .audio
            .duration_secs
            .ok_or(Error::PolicyGate("DDEX_DURATION_UNKNOWN"))?;
        // Full f64 precision: the old `{:.1}` formatting silently rounded
        // probed durations (e.g. 200.046s became PT200.0S).
        element(out, "Duration", format!("PT{secs}S"));
        // The details element is SoundRecordingDetailsByTerritory (not
        // DetailsByTerritory). Inside: TerritoryCode, Title?, DisplayArtist?,
        // ResourceContributor*, PLine?, TechnicalSoundRecordingDetails?.
        // ERN 3.8.2 has no VersionTitle element; the DDEX definition of
        // SubTitle explicitly covers "Titles of Versions used to
        // differentiate different versions of the same Title", so the
        // version/designation goes there — never glued onto the title text.
        out.push_str("<SoundRecordingDetailsByTerritory>");
        element(out, "TerritoryCode", "Worldwide");
        out.push_str("<Title>");
        element(out, "TitleText", &track.title);
        if !track.version.is_empty() {
            element(out, "SubTitle", &track.version);
        }
        out.push_str("</Title>");
        out.push_str("<DisplayArtist><PartyName>");
        element(out, "FullName", &track.artist);
        out.push_str("</PartyName>");
        element(out, "ArtistRole", "MainArtist");
        out.push_str("</DisplayArtist>");
        if let Some(list) = credits.get(&track.id) {
            // ResourceContributor has no sequenceNumber attribute in
            // ERN 3.8.2, and the role element is ResourceContributorRole
            // (not Role).
            for &(name, role) in list {
                out.push_str("<ResourceContributor>");
                out.push_str("<PartyName>");
                element(out, "FullName", name);
                out.push_str("</PartyName>");
                element(out, "ResourceContributorRole", contributor_role(role));
                out.push_str("</ResourceContributor>");
            }
        }
        out.push_str("<PLine>");
        element(out, "Year", prepared.release_date.format("%Y").to_string());
        element(out, "PLineText", &prepared.p_line);
        out.push_str("</PLine>");
        out.push_str("<TechnicalSoundRecordingDetails>");
        element(out, "TechnicalResourceDetailsReference", &tech_ref);
        element(out, "AudioCodecType", codec);
        // Real measured specs for WAV, from Stage 1 ffprobe via
        // `AssetRef`. All four elements are optional per the XSD; when the
        // asset predates spec persistence (or probing failed) they are
        // omitted — never fabricated. The old code emitted
        // 1411/44100/16/2 for every WAV, which is false for e.g. 48kHz
        // 24-bit masters. BitRate's default unit is kbps, SamplingRate's
        // is Hz, so no UnitOfMeasure attributes are needed.
        if matches!(
            track.audio.content_type.as_str(),
            "audio/wav" | "audio/x-wav"
        ) && let (Some(sample_rate), Some(channels), Some(bits_per_sample)) = (
            track.audio.sample_rate,
            track.audio.channels,
            track.audio.bits_per_sample,
        ) {
            // XSD order: BitRate, NumberOfChannels, SamplingRate,
            // BitsPerSample.
            let kbps = (i64::from(sample_rate) * i64::from(channels) * i64::from(bits_per_sample)
                + 500)
                / 1000;
            element(out, "BitRate", kbps);
            element(out, "NumberOfChannels", channels);
            element(out, "SamplingRate", sample_rate);
            element(out, "BitsPerSample", bits_per_sample);
        }
        file_block(out, &file_name, &track.audio.sha256);
        out.push_str("</TechnicalSoundRecordingDetails>");
        out.push_str("</SoundRecordingDetailsByTerritory>");
        out.push_str("</SoundRecording>");
    }
    image_resource(
        out,
        prepared,
        &format!("A{:03}", tracks.len() + 1),
        sender_dpid,
    );
    out.push_str("</ResourceList>");
    Ok(())
}

/// The image's ResourceReference must match the XSD pattern
/// `A[\d\-_a-zA-Z]+` (same as sound recordings), so it takes the next
/// A-number after the tracks rather than an `I001`-style id.
fn image_resource(
    out: &mut String,
    prepared: &PreparedRelease,
    image_ref: &str,
    sender_dpid: &str,
) {
    let art: &AssetRef = &prepared.artwork;
    let (codec, ext) = image_codec(&art.content_type);
    // XSD order: ImageType?, IsArtistRelated?, ImageId, ResourceReference,
    // ..., ImageDetailsByTerritory.
    out.push_str("<Image>");
    element(out, "ImageType", "FrontCoverImage");
    out.push_str("<ImageId>");
    // The proprietary id lives in the *sender's* namespace. The old code
    // hardcoded a foreign DPID here (`DPID:PADPIDA2023081501R`, left over
    // from the structural reference) — misattributing our images to someone
    // else's party id. `validate_config` guarantees a sender DPID is
    // present; there is no fallback namespace.
    let namespace = format!("DPID:{sender_dpid}");
    out.push_str("<ProprietaryId");
    out.push_str(&attr("Namespace", &namespace));
    out.push('>');
    push_escaped(out, &format!("{}_IMG_001", prepared.upc));
    out.push_str("</ProprietaryId>");
    out.push_str("</ImageId>");
    element(out, "ResourceReference", image_ref);
    out.push_str("<ImageDetailsByTerritory>");
    element(out, "TerritoryCode", "Worldwide");
    out.push_str("<TechnicalImageDetails>");
    element(out, "TechnicalResourceDetailsReference", "TI001");
    element(out, "ImageCodecType", codec);
    file_block(out, &format!("{}.{ext}", prepared.upc), &art.sha256);
    out.push_str("</TechnicalImageDetails>");
    out.push_str("</ImageDetailsByTerritory>");
    out.push_str("</Image>");
}

fn release_type_ddex(release_type: &str) -> &'static str {
    match release_type {
        "SINGLE" => "Single",
        "EP" => "EP",
        _ => "Album",
    }
}

fn release_list(out: &mut String, prepared: &PreparedRelease, tracks: &[&PreparedTrack]) {
    let year = prepared.release_date.format("%Y").to_string();
    // XSD order: ReleaseId+, ReleaseReference*,
    // ReferenceTitle, ReleaseResourceReferenceList,
    // ReleaseCollectionReferenceList?, ReleaseType?,
    // ReleaseDetailsByTerritory, PLine?, CLine?, ...,
    // GlobalOriginalReleaseDate?
    // (DisplayTitle is only valid in CatalogItem, not in Release.)
    out.push_str("<ReleaseList>");
    out.push_str(&format!("<Release{}>", attr("IsMainRelease", "true")));
    out.push_str("<ReleaseId>");
    out.push_str("<ICPN");
    out.push_str(&attr("IsEan", "true"));
    out.push('>');
    push_escaped(out, &prepared.upc);
    out.push_str("</ICPN>");
    out.push_str("</ReleaseId>");
    element(out, "ReleaseReference", "R001");
    out.push_str("<ReferenceTitle>");
    element(out, "TitleText", &prepared.title);
    out.push_str("</ReferenceTitle>");
    out.push_str("<ReleaseResourceReferenceList>");
    for (i, _) in tracks.iter().enumerate() {
        element(out, "ReleaseResourceReference", format!("A{:03}", i + 1));
    }
    // Same A-numbered reference as the Image resource above.
    element(
        out,
        "ReleaseResourceReference",
        format!("A{:03}", tracks.len() + 1),
    );
    out.push_str("</ReleaseResourceReferenceList>");
    element(
        out,
        "ReleaseType",
        release_type_ddex(&prepared.release_type),
    );
    out.push_str("<ReleaseDetailsByTerritory>");
    element(out, "TerritoryCode", "Worldwide");
    element(out, "DisplayArtistName", &prepared.artist);
    out.push_str("<DisplayArtist><PartyName>");
    element(out, "FullName", &prepared.artist);
    out.push_str("</PartyName>");
    element(out, "ArtistRole", "MainArtist");
    out.push_str("</DisplayArtist>");
    if prepared.explicit {
        element(out, "ParentalWarningType", "Explicit");
    }
    element(
        out,
        "ReleaseDate",
        prepared.release_date.format("%Y-%m-%d").to_string(),
    );
    out.push_str("</ReleaseDetailsByTerritory>");
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
    out.push_str("</Release></ReleaseList>");
}

fn deal_list(out: &mut String, c: &DdexErnConfig) {
    // XSD: ReleaseDeal = DealReleaseReference, Deal, EffectiveDate?.
    // Deal = DealReference?, DealTerms?, ... — there is no DealId element;
    // DealReference is optional and pattern-constrained, so it is omitted.
    // DealTerms minimal valid set: CommercialModelType?, Usage+,
    // TerritoryCode+, ValidityPeriod+.
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
    out.push_str("</DealTerms></Deal></ReleaseDeal></DealList>");
}

/// Build a DDEX ERN 3.8.2 `NewReleaseMessage` for a prepared release.
pub fn generate_ddex_ern_382(prepared: &PreparedRelease, config: &DdexErnConfig) -> Result<String> {
    validate_metadata(prepared)?;
    validate_config(config)?;
    // Canonicalize human-text fields to Unicode NFC before generating.
    // Korean metadata in particular may arrive NFD (decomposed Jamo) or
    // NFC (precomposed); without normalization the same release would
    // produce byte-different XML and different ern_sha256 rows, breaking
    // idempotency. Identifiers (ISRC/UPC) are ASCII by validation and are
    // left untouched. (Category from ddex-suite's determinism guarantees;
    // implemented here with the unicode-normalization crate.)
    let normalized;
    let prepared: &PreparedRelease = {
        normalized = normalize_nfc(prepared);
        normalized.as_ref()
    };
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
    // XSD sequence after MessageHeader: UpdateIndicator?, IsBackfill?, ...
    // The element is deprecated in 3.8.2 (DDEX recommends against using
    // it), but it is still schema-valid and several receivers
    // (ddex-workbench's validator, stardust-dsp's ingestion parser) key
    // message intent off it, so we emit it explicitly rather than leaving
    // intent implicit.
    element(
        &mut out,
        "UpdateIndicator",
        match config.message_sub_type {
            MessageSubType::Initial => "OriginalMessage",
            MessageSubType::Update | MessageSubType::Takedown => "UpdateMessage",
        },
    );
    // One ordered track list for the resource list, the release list and
    // the image ref: the A-numbers must agree across all three.
    let tracks = ordered_tracks(prepared);
    // validate_config already rejected a missing sender DPID; re-check
    // here so the namespace below can never come from a fallback.
    let sender_dpid = config
        .sender_party_id
        .as_deref()
        .filter(|s| required(s))
        .ok_or(Error::PolicyGate("DDEX_SENDER_DPID_MISSING"))?;
    resource_list(&mut out, prepared, &tracks, sender_dpid)?;
    release_list(&mut out, prepared, &tracks);
    deal_list(&mut out, config);
    out.push_str("</ern:NewReleaseMessage>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::contributor_role;

    #[test]
    fn contributor_role_maps_studio_roles_to_avs() {
        // Every value below must be in the ERN 3.8.2 ResourceContributorRole
        // AVS; anything else fails XSD validation.
        assert_eq!(contributor_role("composer"), "Contributor");
        assert_eq!(contributor_role("Songwriter"), "Contributor");
        assert_eq!(contributor_role("lyricist"), "Contributor");
        assert_eq!(contributor_role("ARRANGER"), "Contributor");
        assert_eq!(contributor_role("Mixing Engineer"), "StudioPersonnel");
        assert_eq!(contributor_role("Mastering Engineer"), "StudioPersonnel");
        assert_eq!(contributor_role("feat."), "FeaturedArtist");
        assert_eq!(contributor_role("  producer  "), "Producer");
        assert_eq!(contributor_role("Conductor"), "Conductor");
    }

    #[test]
    fn contributor_role_unknown_falls_back_to_generic() {
        assert_eq!(contributor_role("vibe curator"), "Contributor");
        assert_eq!(contributor_role(""), "Contributor");
    }
}
