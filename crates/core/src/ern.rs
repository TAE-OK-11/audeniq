//! Partner-neutral ERN preparation fixture profile, NOT a licensed DDEX profile.
//! No DB, clock, random IDs, external schema fetches, or partner endpoints.
//! The private namespace prevents these fixtures being mistaken for production ERN.
use crate::{
    distribution::CanonicalCredit,
    error::{Error, Result},
    identifiers::{validate_isrc, validate_upc},
    preparation_model::{AssetRef, PreparedRelease, PreparedTrack},
};
use std::collections::{BTreeSet, HashMap};

pub const SYNTHETIC_PROFILE: &str = "audeniq-ern-synthetic-1";
pub const SYNTHETIC_NAMESPACE: &str = "urn:audeniq:ern:synthetic:1";

pub(crate) fn xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

fn required(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().all(xml_char)
}

pub(crate) fn sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn asset_valid(a: &AssetRef) -> bool {
    !a.id.is_nil()
        && required(&a.object_key)
        && required(&a.content_type)
        && sha256(&a.sha256)
        && a.size_bytes > 0
        && !a.object_key.starts_with('/')
        && !a.object_key.contains(['\\', '?', '#'])
        && a.object_key
            .split('/')
            .all(|p| !matches!(p, "" | "." | ".."))
        // Measured audio specs are optional, but when present they must be
        // positive — matching the CHECK constraints on catalog.assets. A
        // zero/negative sample rate or channel count is corrupt probe data,
        // never a valid master.
        && a.sample_rate.is_none_or(|v| v > 0)
        && a.channels.is_none_or(|v| v > 0)
        && a.bits_per_sample.is_none_or(|v| v > 0)
        && a.duration_secs.is_none_or(|v| v.is_finite() && v > 0.0)
}

pub(crate) fn ordered_tracks(c: &PreparedRelease) -> Vec<&PreparedTrack> {
    let mut tracks: Vec<_> = c.tracks.iter().collect();
    tracks.sort_by_key(|t| (t.disc_number, t.track_number));
    tracks
}

pub fn validate_metadata(c: &PreparedRelease) -> Result<()> {
    validate_binding(c)?;
    if [
        c.org_id,
        c.release_id,
        c.revision_id,
        c.snapshot_id,
        c.verification_package_id,
    ]
    .iter()
    .any(uuid::Uuid::is_nil)
        || !sha256(&c.revision_hash)
        || !sha256(&c.verification_package_hash)
        || c.rights_epoch < 0
        || [&c.title, &c.artist, &c.language, &c.p_line, &c.c_line]
            .iter()
            .any(|s| !required(s))
        || !matches!(c.release_type.as_str(), "SINGLE" | "EP" | "ALBUM")
        || c.tracks.is_empty()
        || c.tracks.len() > 1000
        || !asset_valid(&c.artwork)
        || !c.artwork.content_type.starts_with("image/")
        || !c
            .language
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'-')
    {
        return Err(Error::Invalid);
    }
    validate_upc(&c.upc)?;
    let mut ids = BTreeSet::from([c.artwork.id]);
    let mut positions = BTreeSet::new();
    let mut isrcs = BTreeSet::new();
    let mut assets = BTreeSet::from([c.artwork.id]);
    let mut keys = BTreeSet::from([c.artwork.object_key.as_str()]);
    for t in &c.tracks {
        validate_isrc(&t.isrc)?;
        if t.id.is_nil()
            || !ids.insert(t.id)
            || !isrcs.insert(&t.isrc)
            || !positions.insert((t.disc_number, t.track_number))
            || t.disc_number == 0
            || t.track_number == 0
            || !required(&t.title)
            || !required(&t.artist)
            || !asset_valid(&t.audio)
            || !t.audio.content_type.starts_with("audio/")
            || !assets.insert(t.audio.id)
            || !keys.insert(t.audio.object_key.as_str())
        {
            return Err(Error::Invalid);
        }
    }
    let dsps: BTreeSet<_> = c.approved_scope.iter().map(|s| s.dsp_id).collect();
    if dsps.len() != c.approved_scope.len() || dsps.iter().any(uuid::Uuid::is_nil) {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Append `s` to `out`, escaping the five XML special chars plus CR in a
/// single pass. Byte-identical to the old six-`replace` chain, but without
/// the intermediate allocations — this is the hottest path in ERN
/// generation (called once per element per track).
pub(crate) fn push_escaped(out: &mut String, s: &str) {
    // Fast path: the common case needs no escaping at all.
    if !s
        .bytes()
        .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\'' | b'\r'))
    {
        out.push_str(s);
        return;
    }
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}

pub(crate) fn element(out: &mut String, name: &str, value: impl std::fmt::Display) {
    out.push('<');
    out.push_str(name);
    out.push('>');
    push_escaped(out, &value.to_string());
    out.push_str("</");
    out.push_str(name);
    out.push('>');
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

pub fn generate_prepared_ern(canonical: &PreparedRelease) -> Result<String> {
    let c = canonical;
    validate_metadata(c)?;
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<NewReleaseMessage xmlns=\"{SYNTHETIC_NAMESPACE}\" profile=\"{SYNTHETIC_PROFILE}\" deliveryEnabled=\"false\">"
    );
    out.push_str("<MessageHeader>");
    element(&mut out, "MessageId", c.snapshot_id);
    element(&mut out, "VerificationPackageId", c.verification_package_id);
    element(
        &mut out,
        "VerificationPackageHash",
        &c.verification_package_hash,
    );
    element(&mut out, "RevisionId", c.revision_id);
    element(&mut out, "RevisionHash", &c.revision_hash);
    element(&mut out, "CanonicalHash", c.canonical.canonical_hash());
    element(&mut out, "RightsEpoch", c.rights_epoch);
    out.push_str("</MessageHeader><ResourceList>");
    // Credits live on the canonical snapshot, keyed by track id. Index them
    // once: the old per-track linear scan was O(tracks²).
    let credit_by_track: HashMap<uuid::Uuid, &[CanonicalCredit]> = c
        .canonical
        .tracks
        .iter()
        .map(|t| (t.track_id, t.credits.as_slice()))
        .collect();
    // Sort once and reuse for both the resource list and the track list.
    let tracks = ordered_tracks(c);
    for t in &tracks {
        out.push_str("<SoundRecording>");
        element(&mut out, "ResourceReference", t.id);
        element(&mut out, "ISRC", &t.isrc);
        element(&mut out, "Title", &t.title);
        if !t.version.is_empty() {
            element(&mut out, "VersionTitle", &t.version);
        }
        element(&mut out, "DisplayArtist", &t.artist);
        let pinned_credits = credit_by_track.get(&t.id).ok_or(Error::Invalid)?;
        credits(&mut out, pinned_credits);
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
    for t in &tracks {
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
    for id in dsps {
        element(&mut out, "DSP", id);
    }
    out.push_str("</ApprovedDestinations></NewReleaseMessage>\n");
    Ok(out)
}

/// Strict internal profile validator: only the canonical serializer's exact bytes
/// are accepted. This rejects malformed XML, DTD/entities, unknown elements,
/// missing/duplicate resources and pin tampering, without a network-capable parser.
/// This is not a general DDEX XSD validator. The fixture XSD is independently
/// checked with xmllint in CI; future partner adapters must validate their XSDs.
pub fn validate_xml(c: &PreparedRelease, xml: &str) -> Result<()> {
    if generate_prepared_ern(c)? != xml {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Exact public interface to Muse's immutable canonical snapshot. Only available
/// fields are serialized: no fabricated UPC, artwork, dates, URLs or party IDs.
/// This canonical-only fixture is not a complete DSP submission. Use the enriched
/// preparation API and four checks before treating a submission as prepared.
pub fn generate_ern(c: &crate::distribution::CanonicalRelease) -> Result<String> {
    validate_canonical(c)?;
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CanonicalReleaseMessage xmlns=\"{SYNTHETIC_NAMESPACE}\" deliveryEnabled=\"false\">"
    );
    element(&mut out, "CanonicalHash", c.canonical_hash());
    element(&mut out, "ReleaseId", c.release_id);
    element(&mut out, "RevisionId", c.revision_id);
    element(&mut out, "RevisionHash", &c.revision_hash);
    element(&mut out, "VerificationPackageId", c.verification_package_id);
    element(
        &mut out,
        "VerificationPackageHash",
        &c.verification_package_hash,
    );
    element(&mut out, "RightsEpoch", c.rights_epoch);
    element(&mut out, "Title", &c.release_title);
    element(&mut out, "ReleaseType", &c.release_type);
    out.push_str("<Tracks>");
    let mut tracks: Vec<_> = c.tracks.iter().collect();
    tracks.sort_by_key(|t| (t.disc_number, t.track_number));
    for t in tracks {
        out.push_str("<Track>");
        element(&mut out, "ResourceReference", t.track_id);
        element(&mut out, "ISRC", t.isrc.as_deref().ok_or(Error::Invalid)?);
        element(&mut out, "Title", &t.title);
        if !t.version.is_empty() {
            element(&mut out, "VersionTitle", &t.version);
        }
        element(&mut out, "DisplayArtist", &t.artist_name);
        element(&mut out, "DiscNumber", t.disc_number);
        element(&mut out, "TrackNumber", t.track_number);
        element(&mut out, "AssetId", t.asset_id.ok_or(Error::Invalid)?);
        element(
            &mut out,
            "SHA256",
            t.asset_sha256.as_deref().ok_or(Error::Invalid)?,
        );
        credits(&mut out, &t.credits);
        out.push_str("</Track>");
    }
    out.push_str("</Tracks><ApprovedDestinations>");
    for id in c.approved_dsp_ids.iter().copied().collect::<BTreeSet<_>>() {
        element(&mut out, "DSP", id);
    }
    out.push_str("</ApprovedDestinations></CanonicalReleaseMessage>\n");
    Ok(out)
}

fn credits(out: &mut String, values: &[crate::distribution::CanonicalCredit]) {
    out.push_str("<Credits>");
    let mut values: Vec<_> = values.iter().collect();
    values.sort_by(|a, b| {
        (&a.role, &a.party_name, a.party_id).cmp(&(&b.role, &b.party_name, b.party_id))
    });
    for credit in values {
        out.push_str("<Credit>");
        element(out, "PartyId", credit.party_id);
        element(out, "PartyName", &credit.party_name);
        element(out, "Role", &credit.role);
        out.push_str("</Credit>");
    }
    out.push_str("</Credits>");
}

fn validate_canonical(c: &crate::distribution::CanonicalRelease) -> Result<()> {
    if !matches!(c.schema_version, 1 | 2)
        || !required(&c.rule_version)
        || c.rights_epoch < 0
        || [
            c.org_id,
            c.release_id,
            c.revision_id,
            c.verification_package_id,
        ]
        .iter()
        .any(uuid::Uuid::is_nil)
        || !sha256(&c.revision_hash)
        || !sha256(&c.verification_package_hash)
        || !required(&c.release_title)
        || !matches!(c.release_type.as_str(), "SINGLE" | "EP" | "ALBUM")
        || c.tracks.is_empty()
        || c.tracks.len() > 1000
    {
        return Err(Error::Invalid);
    }
    let mut ids = BTreeSet::new();
    let mut positions = BTreeSet::new();
    let mut isrcs = BTreeSet::new();
    for t in &c.tracks {
        let isrc = t
            .isrc
            .as_deref()
            .ok_or(Error::PolicyGate("EXISTING_ISRC_REQUIRED"))?;
        validate_isrc(isrc)?;
        if t.track_id.is_nil()
            || t.artist_id.is_nil()
            || !ids.insert(t.track_id)
            || !positions.insert((t.disc_number, t.track_number))
            || !isrcs.insert(isrc)
            || t.disc_number <= 0
            || t.track_number <= 0
            || !required(&t.title)
            || !required(&t.artist_name)
            || t.asset_id.is_none_or(|id| id.is_nil())
            || !t.asset_sha256.as_deref().is_some_and(sha256)
            || t.credits
                .iter()
                .any(|x| x.party_id.is_nil() || !required(&x.party_name) || !required(&x.role))
        {
            return Err(Error::Invalid);
        }
    }
    let dsps: BTreeSet<_> = c.approved_dsp_ids.iter().collect();
    if dsps.len() != c.approved_dsp_ids.len() || dsps.iter().any(|id| id.is_nil()) {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Supplements may add required submission fields; they cannot replace snapshot
/// metadata or asset pins. Credits are always taken directly from the snapshot.
fn validate_binding(p: &PreparedRelease) -> Result<()> {
    let c = &p.canonical;
    validate_canonical(c)?;
    // Index once: the old per-track linear scan was O(tracks²).
    let pinned_by_id: HashMap<uuid::Uuid, &crate::distribution::CanonicalTrack> =
        c.tracks.iter().map(|t| (t.track_id, t)).collect();
    if p.org_id != c.org_id
        || p.release_id != c.release_id
        || p.revision_id != c.revision_id
        || p.revision_hash != c.revision_hash
        || p.verification_package_id != c.verification_package_id
        || p.verification_package_hash != c.verification_package_hash
        || p.rights_epoch != c.rights_epoch
        || p.title != c.release_title
        || p.release_type != c.release_type
        || p.tracks.len() != c.tracks.len()
        || p.approved_scope
            .iter()
            .map(|s| s.dsp_id)
            .collect::<BTreeSet<_>>()
            != c.approved_dsp_ids.iter().copied().collect::<BTreeSet<_>>()
    {
        return Err(Error::Conflict);
    }
    for t in &p.tracks {
        let pinned = pinned_by_id.get(&t.id).ok_or(Error::Conflict)?;
        if t.title != pinned.title
            || t.artist != pinned.artist_name
            || Some(&t.isrc) != pinned.isrc.as_ref()
            || Some(t.audio.id) != pinned.asset_id
            || Some(&t.audio.sha256) != pinned.asset_sha256.as_ref()
            || i32::try_from(t.disc_number).ok() != Some(pinned.disc_number)
            || i32::try_from(t.track_number).ok() != Some(pinned.track_number)
        {
            return Err(Error::Conflict);
        }
    }
    Ok(())
}
