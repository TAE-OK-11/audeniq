//! Inbound ERN message validation: version/profile auto-detection, metadata
//! extraction, and business-rule checks for `NewReleaseMessage` documents.
//!
//! Structural reference: daddykev/ddex-workbench (MIT license) —
//! `packages/sdk/src/validator.ts` (version/profile auto-detection and
//! metadata extraction) and `functions/validators/ernValidator.js`
//! (structural business rules per ERN version and release profile).
//! The implementation below is written from scratch in Rust; no code is
//! copied. Only the *categories* of checks are mirrored.
//!
//! Deliberate deviations from the reference, all documented here:
//! - The workbench's 648 Schematron rules are produced by proprietary
//!   tooling and are not in the public repo, so they are not mirrored.
//!   This module covers the structural/business-rule layer only; XSD
//!   validation stays in `ddex_xsd` (xmllint, fail-closed).
//! - Findings are plain Rust values (`ErnFinding`) with stable rule ids
//!   (e.g. `ERN382-MessageHeader`) instead of SVRL documents: AUDENIQ
//!   persists validation outcomes as `operations::check_results` rows,
//!   not as SVRL.
//! - Parsing is streaming via `quick-xml` and fail-closed: a document that
//!   does not parse is invalid, never "unchecked".
//!
//! Primary consumer: `distribution::prepare_release` runs this on every
//! generated ERN document right after the XSD gate, so a structurally
//! broken message (dangling resource reference, wrong profile, missing
//! section) never becomes a persisted `distribution.ddex_messages` row.

use quick_xml::{Reader, events::Event};
use std::borrow::Cow;

/// ERN major versions the validator understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErnVersion {
    /// ERN 3.8.2 (`http://ddex.net/xml/ern/382`).
    V382,
    /// ERN 4.2 (`http://ddex.net/xml/ern/42`).
    V42,
    /// ERN 4.3 (`http://ddex.net/xml/ern/43`).
    V43,
}

impl ErnVersion {
    /// Short tag used in rule ids (`382`, `42`, `43`).
    pub fn tag(self) -> &'static str {
        match self {
            ErnVersion::V382 => "382",
            ErnVersion::V42 => "42",
            ErnVersion::V43 => "43",
        }
    }

    /// Human-readable version string.
    pub fn as_str(self) -> &'static str {
        match self {
            ErnVersion::V382 => "3.8.2",
            ErnVersion::V42 => "4.2",
            ErnVersion::V43 => "4.3",
        }
    }

    /// Namespace URI of the version's `NewReleaseMessage`.
    pub fn namespace(self) -> &'static str {
        match self {
            ErnVersion::V382 => "http://ddex.net/xml/ern/382",
            ErnVersion::V42 => "http://ddex.net/xml/ern/42",
            ErnVersion::V43 => "http://ddex.net/xml/ern/43",
        }
    }
}

/// DDEX release profiles relevant to ERN validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErnProfile {
    AudioAlbum,
    AudioSingle,
    Video,
    Mixed,
    Classical,
    Ringtone,
    DJ,
    /// ERN 3.x only.
    ReleaseByRelease,
}

impl ErnProfile {
    /// Canonical profile name as it appears in `ReleaseProfileVersionId`.
    pub fn as_str(self) -> &'static str {
        match self {
            ErnProfile::AudioAlbum => "AudioAlbum",
            ErnProfile::AudioSingle => "AudioSingle",
            ErnProfile::Video => "Video",
            ErnProfile::Mixed => "Mixed",
            ErnProfile::Classical => "Classical",
            ErnProfile::Ringtone => "Ringtone",
            ErnProfile::DJ => "DJ",
            ErnProfile::ReleaseByRelease => "ReleaseByRelease",
        }
    }
}

/// Auto-detect the ERN version of a document.
///
/// Sniffs the `ern` namespace URI and the `MessageSchemaVersionId`
/// attribute, newest first. Returns `None` when the document carries no
/// recognizable ERN marker.
pub fn detect_ern_version(xml: &str) -> Option<ErnVersion> {
    if xml.contains("http://ddex.net/xml/ern/43")
        || xml.contains("MessageSchemaVersionId=\"ern/43\"")
    {
        return Some(ErnVersion::V43);
    }
    if xml.contains("http://ddex.net/xml/ern/42")
        || xml.contains("MessageSchemaVersionId=\"ern/42\"")
    {
        return Some(ErnVersion::V42);
    }
    if xml.contains("http://ddex.net/xml/ern/382")
        || xml.contains("MessageSchemaVersionId=\"ern/382\"")
    {
        return Some(ErnVersion::V382);
    }
    // Legacy 3.8 namespace without the patch component.
    if xml.contains("http://ddex.net/xml/ern/38") {
        return Some(ErnVersion::V382);
    }
    None
}

/// Auto-detect the release profile of a document from its
/// `ReleaseProfileVersionId` attribute. Returns `None` when the document
/// carries no recognizable profile marker.
pub fn detect_ern_profile(xml: &str) -> Option<ErnProfile> {
    // Extract the attribute value first so a stray "Video" in a title
    // cannot fake a profile.
    let value = attribute_value(xml, "ReleaseProfileVersionId")?;
    // Order matters: check the longer, more specific names first.
    [
        ErnProfile::AudioAlbum,
        ErnProfile::AudioSingle,
        ErnProfile::ReleaseByRelease,
        ErnProfile::Classical,
        ErnProfile::Ringtone,
        ErnProfile::Video,
        ErnProfile::Mixed,
        ErnProfile::DJ,
    ]
    .into_iter()
    .find(|profile| value.contains(profile.as_str()))
}

/// Header metadata pulled out of a `NewReleaseMessage` without full
/// validation. Every field is optional: extraction never fails, it just
/// yields less.
#[derive(Debug, Clone, Default)]
pub struct ErnMetadata {
    pub version: Option<ErnVersion>,
    pub profile: Option<ErnProfile>,
    pub message_id: Option<String>,
    pub created: Option<String>,
    pub release_count: usize,
}

/// Extract header metadata from a document. Never fails; fields that
/// cannot be found stay `None`/`0`.
pub fn extract_ern_metadata(xml: &str) -> ErnMetadata {
    let mut meta = ErnMetadata {
        version: detect_ern_version(xml),
        profile: detect_ern_profile(xml),
        ..Default::default()
    };
    if let Ok(scan) = scan_document(xml) {
        meta.message_id = scan.message_id;
        meta.created = scan.created;
        meta.release_count = scan.release_count;
    }
    meta
}

/// Severity of a single validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The message must not be sent/stored.
    Error,
    /// Suspicious but shippable; logged, not blocking.
    Warning,
}

/// One business-rule finding with a stable rule id.
#[derive(Debug, Clone)]
pub struct ErnFinding {
    /// Stable rule id, e.g. `ERN382-MessageHeader`, `AudioAlbum-ReleaseType`.
    /// `Cow` because the version-prefixed section rules (`ERN{tag}-…`)
    /// are built dynamically; every other rule id is a static string.
    pub rule_id: Cow<'static, str>,
    pub severity: Severity,
    pub message: String,
    /// Optional remediation hint for logs / operator UI.
    pub suggestion: Option<&'static str>,
}

impl ErnFinding {
    fn error(rule_id: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Self {
        ErnFinding {
            rule_id: rule_id.into(),
            severity: Severity::Error,
            message: message.into(),
            suggestion: None,
        }
    }

    fn warning(
        rule_id: impl Into<Cow<'static, str>>,
        message: impl Into<String>,
        suggestion: Option<&'static str>,
    ) -> Self {
        ErnFinding {
            rule_id: rule_id.into(),
            severity: Severity::Warning,
            message: message.into(),
            suggestion,
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Outcome of [`validate_ern_message`].
#[derive(Debug, Clone)]
pub struct ErnValidationReport {
    pub version: Option<ErnVersion>,
    pub profile: Option<ErnProfile>,
    pub findings: Vec<ErnFinding>,
}

impl ErnValidationReport {
    /// True when no `Error`-severity finding exists. Warnings do not
    /// affect validity.
    pub fn is_valid(&self) -> bool {
        !self.findings.iter().any(ErnFinding::is_error)
    }

    pub fn errors(&self) -> Vec<&ErnFinding> {
        self.findings.iter().filter(|f| f.is_error()).collect()
    }

    pub fn warnings(&self) -> Vec<&ErnFinding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .collect()
    }
}

/// Run the business-rule layer over a `NewReleaseMessage` document.
///
/// `expected_profile` is the profile the caller intended to send (e.g.
/// derived from the release being distributed); a mismatch with the
/// document's own `ReleaseProfileVersionId` is a warning, not an error.
///
/// This is the business-rule half of validation. It does not replace XSD
/// validation (`ddex_xsd`): schema first, business rules second.
pub fn validate_ern_message(
    xml: &str,
    expected_profile: Option<ErnProfile>,
) -> ErnValidationReport {
    let version = detect_ern_version(xml);
    let profile = detect_ern_profile(xml);
    let mut findings = Vec::new();

    let scan = match scan_document(xml) {
        Ok(scan) => scan,
        Err(detail) => {
            findings.push(ErnFinding::error(
                "ERN-XML-WELLFORMED",
                format!("document is not well-formed XML: {detail}"),
            ));
            return ErnValidationReport {
                version,
                profile,
                findings,
            };
        }
    };

    // --- Root element -------------------------------------------------
    match scan.root.as_deref() {
        Some("NewReleaseMessage") => {}
        other => {
            findings.push(ErnFinding::error(
                "ERN-ROOT",
                format!(
                    "invalid root element: expected 'NewReleaseMessage', found '{}'",
                    other.unwrap_or("(none)")
                ),
            ));
            // Without the right root, section checks below are meaningless.
            return ErnValidationReport {
                version,
                profile,
                findings,
            };
        }
    }

    // --- Required sections per version --------------------------------
    let tag = version.map(ErnVersion::tag).unwrap_or("UNK");
    let mut require = |present: bool, section: &str, rule: String| {
        if !present {
            findings.push(ErnFinding::error(
                rule,
                format!(
                    "{section} is required in ERN {}",
                    version.map(ErnVersion::as_str).unwrap_or("unknown")
                ),
            ));
        }
    };
    require(
        scan.has_message_header,
        "MessageHeader",
        format!("ERN{tag}-MessageHeader"),
    );
    require(
        scan.has_release_list,
        "ReleaseList",
        format!("ERN{tag}-ReleaseList"),
    );
    require(
        scan.has_resource_list,
        "ResourceList",
        format!("ERN{tag}-ResourceList"),
    );
    // DealList became mandatory with the ERN 4.x message family.
    if matches!(version, Some(ErnVersion::V42) | Some(ErnVersion::V43)) {
        require(scan.has_deal_list, "DealList", format!("ERN{tag}-DealList"));
    }

    // --- 3.8.2 specifics ----------------------------------------------
    if version == Some(ErnVersion::V382) && !scan.has_update_indicator {
        findings.push(ErnFinding::warning(
            "ERN382-UpdateIndicator",
            "UpdateIndicator is recommended but not required in ERN 3.8.2",
            Some("emit UpdateIndicator to make message intent explicit"),
        ));
    }

    // --- Profile / version gating --------------------------------------
    if profile == Some(ErnProfile::ReleaseByRelease)
        && !matches!(version, Some(ErnVersion::V382) | None)
    {
        findings.push(ErnFinding::error(
            "ERN-Profile-Version-Mismatch",
            format!(
                "profile 'ReleaseByRelease' is only available in ERN 3.x, found ERN {}",
                version.map(ErnVersion::as_str).unwrap_or("unknown")
            ),
        ));
    }

    // --- Requested profile vs document profile --------------------------
    if let (Some(expected), Some(actual)) = (expected_profile, profile)
        && expected != actual
    {
        findings.push(ErnFinding::warning(
            "ERN-ProfileMismatch",
            format!(
                "message profile \"{}\" does not match requested profile \"{}\"",
                actual.as_str(),
                expected.as_str()
            ),
            Some("regenerate the message with the intended profile"),
        ));
    }

    // --- Release list sanity -------------------------------------------
    if scan.has_release_list && scan.release_count == 0 {
        findings.push(ErnFinding::error(
            "ERN-EmptyReleaseList",
            "ReleaseList contains no Release elements",
        ));
    }

    // --- Profile-specific rules -----------------------------------------
    match profile {
        Some(ErnProfile::AudioAlbum) => {
            if let Some(first) = scan.releases.first() {
                match first.release_type.as_deref() {
                    Some("Album") | Some("EP") => {}
                    other => findings.push(ErnFinding::warning(
                        "AudioAlbum-ReleaseType",
                        format!(
                            "AudioAlbum profile expects ReleaseType 'Album' or 'EP', found '{}'",
                            other.unwrap_or("(missing)")
                        ),
                        Some("check the release-type mapping before sending"),
                    )),
                }
            }
        }
        Some(ErnProfile::AudioSingle) => {
            if scan.release_count > 1 {
                findings.push(ErnFinding::warning(
                    "AudioSingle-SingleRelease",
                    format!(
                        "AudioSingle profile should contain only one main Release, found {}",
                        scan.release_count
                    ),
                    None,
                ));
            }
        }
        Some(ErnProfile::Video) if !scan.has_video_resource => {
            findings.push(ErnFinding::error(
                "Video-VideoResource",
                "Video profile requires at least one Video resource",
            ));
        }
        _ => {}
    }

    // --- Cross-reference integrity ---------------------------------------
    // Every resource a release points at must be defined in ResourceList;
    // every release a deal points at must be defined in ReleaseList.
    // A dangling reference is a certain DSP rejection, so these are errors.
    for r in &scan.referenced_resources {
        if !scan.defined_resources.iter().any(|d| d == r) {
            findings.push(ErnFinding::error(
                "ERN-Xref-Resource",
                format!(
                    "ReleaseResourceReference '{r}' has no matching ResourceReference in ResourceList"
                ),
            ));
        }
    }
    for r in &scan.deal_release_refs {
        if !scan
            .releases
            .iter()
            .any(|rel| rel.reference.as_deref() == Some(r))
        {
            findings.push(ErnFinding::error(
                "ERN-Xref-Release",
                format!("DealReleaseReference '{r}' has no matching Release in ReleaseList"),
            ));
        }
    }

    ErnValidationReport {
        version,
        profile,
        findings,
    }
}

// ---------------------------------------------------------------------------
// Streaming scan
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct ReleaseInfo {
    reference: Option<String>,
    release_type: Option<String>,
}

#[derive(Debug, Default)]
struct Scan {
    root: Option<String>,
    has_message_header: bool,
    has_release_list: bool,
    has_resource_list: bool,
    has_deal_list: bool,
    has_update_indicator: bool,
    message_id: Option<String>,
    created: Option<String>,
    release_count: usize,
    releases: Vec<ReleaseInfo>,
    defined_resources: Vec<String>,
    referenced_resources: Vec<String>,
    deal_release_refs: Vec<String>,
    has_video_resource: bool,
}

/// Pull a `name="value"` attribute out of the raw document text. Used only
/// for root-level detection where a full parse would be overkill; the
/// business-rule pass itself always uses the streaming parser.
fn attribute_value(xml: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = xml.find(&needle)? + needle.len();
    let end = xml[start..].find('"')?;
    Some(xml[start..start + end].to_string())
}

fn local_name(raw: &[u8]) -> &str {
    std::str::from_utf8(raw).unwrap_or("")
}

/// Streaming single-pass scan of the document. Returns the parse error
/// text on malformed XML (fail-closed: the caller turns this into an
/// `ERN-XML-WELLFORMED` finding).
fn scan_document(xml: &str) -> Result<Scan, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut scan = Scan::default();
    // Stack of local element names; stack[0] is the root.
    let mut stack: Vec<String> = Vec::new();
    // Text of the element currently being read, with its local name.
    let mut pending_text: Option<(String, String)> = None;

    loop {
        match reader.read_event() {
            Err(e) => return Err(e.to_string()),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = local_name(e.local_name().as_ref()).to_string();
                if stack.is_empty() {
                    scan.root = Some(name.clone());
                } else if stack.len() == 1 {
                    match name.as_str() {
                        "MessageHeader" => scan.has_message_header = true,
                        "ReleaseList" => scan.has_release_list = true,
                        "ResourceList" => scan.has_resource_list = true,
                        "DealList" => scan.has_deal_list = true,
                        "UpdateIndicator" => scan.has_update_indicator = true,
                        _ => {}
                    }
                }
                let parent_is_release_list = stack.len() == 2
                    && stack[0] == "NewReleaseMessage"
                    && stack[1] == "ReleaseList";
                let parent_is_resource_list = stack.len() == 2
                    && stack[0] == "NewReleaseMessage"
                    && stack[1] == "ResourceList";
                if name == "Release" && parent_is_release_list {
                    scan.release_count += 1;
                    scan.releases.push(ReleaseInfo::default());
                }
                if name == "Video" && parent_is_resource_list {
                    scan.has_video_resource = true;
                }
                stack.push(name);
            }
            Ok(Event::Empty(e)) => {
                // Self-closing: same bookkeeping as Start, without the push.
                let name = local_name(e.local_name().as_ref()).to_string();
                if stack.is_empty() {
                    scan.root = Some(name.clone());
                } else if stack.len() == 1 {
                    match name.as_str() {
                        "MessageHeader" => scan.has_message_header = true,
                        "ReleaseList" => scan.has_release_list = true,
                        "ResourceList" => scan.has_resource_list = true,
                        "DealList" => scan.has_deal_list = true,
                        "UpdateIndicator" => scan.has_update_indicator = true,
                        _ => {}
                    }
                }
                let parent_is_release_list = stack.len() == 2
                    && stack[0] == "NewReleaseMessage"
                    && stack[1] == "ReleaseList";
                let parent_is_resource_list = stack.len() == 2
                    && stack[0] == "NewReleaseMessage"
                    && stack[1] == "ResourceList";
                if name == "Release" && parent_is_release_list {
                    scan.release_count += 1;
                    scan.releases.push(ReleaseInfo::default());
                }
                if name == "Video" && parent_is_resource_list {
                    scan.has_video_resource = true;
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(top) = stack.last().cloned() {
                    let text = e.decode().map(|c| c.into_owned()).unwrap_or_default();
                    if !text.is_empty() {
                        pending_text = Some((top, text));
                    }
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(top) = stack.last().cloned() {
                    let text = String::from_utf8_lossy(&e.into_inner()).into_owned();
                    if !text.trim().is_empty() {
                        pending_text = Some((top, text));
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = local_name(e.local_name().as_ref()).to_string();
                if let Some((elem, text)) = pending_text.take() {
                    // Attribute the text to the element that just closed.
                    debug_assert_eq!(elem, name);
                    let section = stack.get(1).map(String::as_str).unwrap_or("");
                    let in_release_list = stack.first().map(String::as_str)
                        == Some("NewReleaseMessage")
                        && section == "ReleaseList";
                    let in_resource_list = stack.first().map(String::as_str)
                        == Some("NewReleaseMessage")
                        && section == "ResourceList";
                    let in_deal_list = stack.first().map(String::as_str)
                        == Some("NewReleaseMessage")
                        && section == "DealList";
                    let in_header = stack.first().map(String::as_str) == Some("NewReleaseMessage")
                        && section == "MessageHeader";
                    match elem.as_str() {
                        "MessageId" if in_header => scan.message_id = Some(text),
                        "MessageCreatedDateTime" if in_header => scan.created = Some(text),
                        "ReleaseType" if in_release_list => {
                            if let Some(rel) = scan.releases.last_mut() {
                                rel.release_type = Some(text);
                            }
                        }
                        "ReleaseReference" if in_release_list => {
                            if let Some(rel) = scan.releases.last_mut() {
                                rel.reference = Some(text);
                            }
                        }
                        "ReleaseResourceReference" if in_release_list => {
                            scan.referenced_resources.push(text)
                        }
                        "ResourceReference" if in_resource_list => {
                            scan.defined_resources.push(text)
                        }
                        "DealReleaseReference" if in_deal_list => scan.deal_release_refs.push(text),
                        _ => {}
                    }
                }
                stack.pop();
            }
            _ => {}
        }
    }
    // quick-xml does not flag unclosed elements at EOF, so check the stack
    // ourselves: a truncated document is malformed, full stop.
    if let Some(open) = stack.last() {
        return Err(format!("unclosed element <{open}> at end of document"));
    }
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI_382: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ern:NewReleaseMessage xmlns:ern="http://ddex.net/xml/ern/382" MessageSchemaVersionId="ern/382" ReleaseProfileVersionId="CommonReleaseAudioAlbum/13">
  <MessageHeader>
    <MessageId>MSG-1</MessageId>
    <MessageCreatedDateTime>2026-09-26T00:00:00Z</MessageCreatedDateTime>
  </MessageHeader>
  <UpdateIndicator>OriginalMessage</UpdateIndicator>
  <ResourceList>
    <SoundRecording><ResourceReference>A001</ResourceReference></SoundRecording>
  </ResourceList>
  <ReleaseList>
    <Release><ReleaseReference>R001</ReleaseReference><ReleaseResourceReferenceList><ReleaseResourceReference>A001</ReleaseResourceReference></ReleaseResourceReferenceList><ReleaseType>Album</ReleaseType></Release>
  </ReleaseList>
  <DealList>
    <ReleaseDeal><DealReleaseReference>R001</DealReleaseReference></ReleaseDeal>
  </DealList>
</ern:NewReleaseMessage>"#;

    #[test]
    fn detects_version_and_profile() {
        assert_eq!(detect_ern_version(MINI_382), Some(ErnVersion::V382));
        assert_eq!(detect_ern_profile(MINI_382), Some(ErnProfile::AudioAlbum));
        let v43 = MINI_382
            .replace("http://ddex.net/xml/ern/382", "http://ddex.net/xml/ern/43")
            .replace(
                "MessageSchemaVersionId=\"ern/382\"",
                "MessageSchemaVersionId=\"ern/43\"",
            );
        assert_eq!(detect_ern_version(&v43), Some(ErnVersion::V43));
        assert_eq!(detect_ern_version("<foo/>"), None);
        assert_eq!(detect_ern_profile("<foo/>"), None);
    }

    #[test]
    fn clean_message_is_valid() {
        let report = validate_ern_message(MINI_382, Some(ErnProfile::AudioAlbum));
        assert!(report.is_valid(), "findings: {:?}", report.findings);
        assert_eq!(report.version, Some(ErnVersion::V382));
        assert!(report.errors().is_empty());
    }

    #[test]
    fn malformed_xml_fails_closed() {
        let report = validate_ern_message("<ern:NewReleaseMessage>", None);
        assert!(!report.is_valid());
        assert_eq!(&*report.errors()[0].rule_id, "ERN-XML-WELLFORMED");
    }

    #[test]
    fn wrong_root_is_rejected() {
        let report = validate_ern_message("<CatalogueList/>", None);
        assert!(!report.is_valid());
        assert_eq!(&*report.errors()[0].rule_id, "ERN-ROOT");
    }

    #[test]
    fn missing_sections_are_errors() {
        let xml = r#"<ern:NewReleaseMessage xmlns:ern="http://ddex.net/xml/ern/382" MessageSchemaVersionId="ern/382"><MessageHeader/></ern:NewReleaseMessage>"#;
        let report = validate_ern_message(xml, None);
        let ids: Vec<&str> = report.errors().iter().map(|f| &*f.rule_id).collect();
        assert!(ids.contains(&"ERN382-ReleaseList"), "{ids:?}");
        assert!(ids.contains(&"ERN382-ResourceList"), "{ids:?}");
    }

    #[test]
    fn dangling_resource_reference_is_error() {
        let xml = MINI_382.replace(
            "<ReleaseResourceReference>A001</ReleaseResourceReference>",
            "<ReleaseResourceReference>A099</ReleaseResourceReference>",
        );
        let report = validate_ern_message(&xml, None);
        assert!(!report.is_valid());
        assert!(
            report
                .errors()
                .iter()
                .any(|f| &*f.rule_id == "ERN-Xref-Resource")
        );
    }

    #[test]
    fn dangling_deal_release_reference_is_error() {
        let xml = MINI_382.replace(
            "<DealReleaseReference>R001</DealReleaseReference>",
            "<DealReleaseReference>R002</DealReleaseReference>",
        );
        let report = validate_ern_message(&xml, None);
        assert!(!report.is_valid());
        assert!(
            report
                .errors()
                .iter()
                .any(|f| &*f.rule_id == "ERN-Xref-Release")
        );
    }

    #[test]
    fn profile_mismatch_is_warning_only() {
        let report = validate_ern_message(MINI_382, Some(ErnProfile::AudioSingle));
        assert!(report.is_valid());
        assert!(
            report
                .warnings()
                .iter()
                .any(|f| &*f.rule_id == "ERN-ProfileMismatch")
        );
    }

    #[test]
    fn video_profile_requires_video_resource() {
        let xml = MINI_382.replace("CommonReleaseAudioAlbum/13", "CommonReleaseVideo/13");
        let report = validate_ern_message(&xml, Some(ErnProfile::Video));
        assert!(!report.is_valid());
        assert!(
            report
                .errors()
                .iter()
                .any(|f| &*f.rule_id == "Video-VideoResource")
        );
    }

    #[test]
    fn release_by_release_rejected_on_ern43() {
        let xml = MINI_382
            .replace("http://ddex.net/xml/ern/382", "http://ddex.net/xml/ern/43")
            .replace(
                "MessageSchemaVersionId=\"ern/382\"",
                "MessageSchemaVersionId=\"ern/43\"",
            )
            .replace("CommonReleaseAudioAlbum/13", "ReleaseByRelease/13");
        let report = validate_ern_message(&xml, None);
        assert!(!report.is_valid());
        assert!(
            report
                .errors()
                .iter()
                .any(|f| &*f.rule_id == "ERN-Profile-Version-Mismatch")
        );
    }

    #[test]
    fn metadata_extraction_never_fails() {
        let meta = extract_ern_metadata(MINI_382);
        assert_eq!(meta.version, Some(ErnVersion::V382));
        assert_eq!(meta.profile, Some(ErnProfile::AudioAlbum));
        assert_eq!(meta.message_id.as_deref(), Some("MSG-1"));
        assert_eq!(meta.release_count, 1);
        let empty = extract_ern_metadata("not xml at all");
        assert_eq!(empty.version, None);
        assert_eq!(empty.release_count, 0);
    }
}
