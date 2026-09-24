//! Partner-neutral ERN preparation fixture profile, NOT a licensed DDEX profile.
//! No DB, clock, random IDs, external schema fetches, or partner endpoints.
//! The private namespace prevents these fixtures being mistaken for production ERN.
use crate::{
    error::{Error, Result},
    identifiers::{validate_isrc, validate_upc},
    preparation_model::{AssetRef, CanonicalRelease, CanonicalTrack},
};
use std::collections::BTreeSet;

pub const SYNTHETIC_PROFILE: &str = "audeniq-ern-synthetic-1";
pub const SYNTHETIC_NAMESPACE: &str = "urn:audeniq:ern:synthetic:1";

pub(crate) fn xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

fn required(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().all(xml_char)
}

pub(crate) fn sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn asset_valid(a: &AssetRef) -> bool {
    !a.id.is_nil() && required(&a.object_key) && required(&a.content_type)
        && sha256(&a.sha256) && a.size_bytes > 0
        && !a.object_key.starts_with('/')
        && !a.object_key.contains(['\\', '?', '#'])
        && a.object_key.split('/').all(|p| !matches!(p, "" | "." | ".."))
}

pub(crate) fn ordered_tracks(c: &CanonicalRelease) -> Vec<&CanonicalTrack> {
    let mut tracks: Vec<_> = c.tracks.iter().collect();
    tracks.sort_by_key(|t| (t.disc_number, t.track_number));
    tracks
}

pub fn validate_metadata(c: &CanonicalRelease) -> Result<()> {
    if [c.org_id, c.release_id, c.revision_id, c.snapshot_id, c.verification_package_id].iter().any(uuid::Uuid::is_nil)
        || !sha256(&c.revision_hash) || !sha256(&c.verification_package_hash)
        || c.rights_epoch < 0
        || [&c.title, &c.artist, &c.language, &c.p_line, &c.c_line].iter().any(|s| !required(s))
        || !matches!(c.release_type.as_str(), "SINGLE" | "EP" | "ALBUM")
        || c.tracks.is_empty() || c.tracks.len() > 1000
        || !asset_valid(&c.artwork) || !c.artwork.content_type.starts_with("image/")
        || !c.language.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
    { return Err(Error::Invalid); }
    validate_upc(&c.upc)?;
    let mut ids = BTreeSet::new();
    let mut positions = BTreeSet::new();
    let mut isrcs = BTreeSet::new();
    let mut assets = BTreeSet::from([c.artwork.id]);
    let mut keys = BTreeSet::from([c.artwork.object_key.as_str()]);
    for t in &c.tracks {
        validate_isrc(&t.isrc)?;
        if t.id.is_nil() || !ids.insert(t.id) || !isrcs.insert(&t.isrc)
            || !positions.insert((t.disc_number,t.track_number))
            || t.disc_number == 0 || t.track_number == 0
            || !required(&t.title) || !required(&t.artist)
            || !asset_valid(&t.audio) || !t.audio.content_type.starts_with("audio/")
            || !assets.insert(t.audio.id) || !keys.insert(t.audio.object_key.as_str())
        { return Err(Error::Invalid); }
    }
    let dsps: BTreeSet<_> = c.approved_scope.iter().map(|s| s.dsp_id).collect();
    if dsps.len() != c.approved_scope.len() || dsps.iter().any(uuid::Uuid::is_nil) {
        return Err(Error::Invalid);
    }
    Ok(())
}

fn escaped(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&apos;").replace('\r', "&#13;")
}

fn element(out: &mut String, name: &str, value: impl std::fmt::Display) {
    out.push_str(&format!("<{name}>{}</{name}>", escaped(&value.to_string())));
}

fn file(out: &mut String, a: &AssetRef) {
    out.push_str("<File>");
    element(out, "AssetId", a.id);
    element(out, "URI", &a.object_key);
    element(out, "SHA256", &a.sha256);
    element(out, "Size", a.size_bytes);
    element(out, "MediaType", &a.content_type);
    out.push_str("</File>");
}

pub fn generate_ern(canonical: &CanonicalRelease) -> Result<String> {
    let c = canonical;
    validate_metadata(c)?;
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<NewReleaseMessage xmlns=\"{SYNTHETIC_NAMESPACE}\" profile=\"{SYNTHETIC_PROFILE}\" deliveryEnabled=\"false\">");
    out.push_str("<MessageHeader>");
    element(&mut out, "MessageId", c.snapshot_id);
    element(&mut out, "VerificationPackageId", c.verification_package_id);
    element(&mut out, "VerificationPackageHash", &c.verification_package_hash);
    element(&mut out, "RevisionId", c.revision_id);
    element(&mut out, "RevisionHash", &c.revision_hash);
    element(&mut out, "RightsEpoch", c.rights_epoch);
    out.push_str("</MessageHeader><ResourceList>");
    for t in ordered_tracks(c) {
        out.push_str("<SoundRecording>");
        element(&mut out, "ResourceReference", t.id);
        element(&mut out, "ISRC", &t.isrc);
        element(&mut out, "Title", &t.title);
        element(&mut out, "DisplayArtist", &t.artist);
        file(&mut out, &t.audio);
        out.push_str("</SoundRecording>");
    }
    out.push_str("<Image>");
    element(&mut out, "ResourceReference", c.artwork.id);
    file(&mut out, &c.artwork);
    out.push_str("</Image></ResourceList><Release>");
    element(&mut out, "ReleaseId", c.release_id);
    element(&mut out, "UPC", &c.upc);
    element(&mut out, "Title", &c.title);
    element(&mut out, "DisplayArtist", &c.artist);
    element(&mut out, "ReleaseType", &c.release_type);
    element(&mut out, "ReleaseDate", c.release_date);
    element(&mut out, "Language", &c.language);
    element(&mut out, "PLine", &c.p_line);
    element(&mut out, "CLine", &c.c_line);
    out.push_str("<TrackList>");
    for t in ordered_tracks(c) {
        out.push_str("<Track>");
        element(&mut out, "ResourceReference", t.id);
        element(&mut out, "DiscNumber", t.disc_number);
        element(&mut out, "TrackNumber", t.track_number);
        out.push_str("</Track>");
    }
    out.push_str("</TrackList>");
    element(&mut out, "ArtworkReference", c.artwork.id);
    out.push_str("</Release><ApprovedDestinations>");
    let dsps: BTreeSet<_> = c.approved_scope.iter().map(|s| s.dsp_id).collect();
    for id in dsps { element(&mut out, "DSP", id); }
    out.push_str("</ApprovedDestinations></NewReleaseMessage>\n");
    Ok(out)
}

/// Strict internal profile validator: only the canonical serializer's exact bytes
/// are accepted. This rejects malformed XML, DTD/entities, unknown elements,
/// missing/duplicate resources and pin tampering, without a network-capable parser.
/// This is not a general DDEX XSD validator. The fixture XSD is independently
/// checked with xmllint in CI; future partner adapters must validate their XSDs.
pub fn validate_xml(c: &CanonicalRelease, xml: &str) -> Result<()> {
    if generate_ern(c)? != xml { return Err(Error::Invalid); }
    Ok(())
}
