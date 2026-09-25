//! `ddex_validate` integration tests: run the business-rule layer over
//! real ERN 3.8.2 documents produced by `ddex_ern::generate_ddex_ern_382`.
//! No DB, no network — the fixtures are the same JSON releases the
//! `ddex_ern` tests use.
use audeniq_core::{
    ddex_ern::{DdexErnConfig, MessageSubType, generate_ddex_ern_382},
    ddex_validate::{
        ErnProfile, ErnVersion, extract_ern_metadata, gate_findings, preflight_release,
        validate_ern_message,
    },
    preparation_model::PreparedRelease,
};

fn fixture(index: usize) -> PreparedRelease {
    let raw = [
        include_str!("fixtures/ern/single.json"),
        include_str!("fixtures/ern/ep.json"),
        include_str!("fixtures/ern/album.json"),
    ][index];
    serde_json::from_str(raw).unwrap()
}

fn config() -> DdexErnConfig {
    DdexErnConfig {
        message_id: "MSG-2026-09-26-001".into(),
        message_thread_id: None,
        message_sub_type: MessageSubType::Initial,
        created_at: "2026-09-26T09:00:00Z".into(),
        sender_name: "AUDENIQ".into(),
        sender_party_id: Some("PADPIDA2026092601A".into()),
        sent_on_behalf_of: None,
        recipient_name: "MockDSP".into(),
        recipient_party_id: Some("PADPIDA2026092601M".into()),
        // Must not predate the fixture's release_date (2027-03-01);
        // preflight rejects a deal that starts before the release date.
        deal_start_date: "2027-03-01".into(),
        takedown_date: None,
    }
}

fn expected_profile(prepared: &PreparedRelease) -> ErnProfile {
    if prepared.tracks.len() > 1 {
        ErnProfile::AudioAlbum
    } else {
        ErnProfile::AudioSingle
    }
}

#[test]
fn generated_single_passes_business_rules() {
    let prepared = fixture(0);
    let xml = generate_ddex_ern_382(&prepared, &config()).unwrap();
    let report = validate_ern_message(&xml, Some(expected_profile(&prepared)));
    assert!(report.is_valid(), "findings: {:?}", report.findings);
    assert_eq!(report.version, Some(ErnVersion::V382));
    assert_eq!(report.profile, Some(ErnProfile::AudioSingle));
}

#[test]
fn generated_ep_passes_business_rules() {
    let prepared = fixture(1);
    let xml = generate_ddex_ern_382(&prepared, &config()).unwrap();
    let report = validate_ern_message(&xml, Some(expected_profile(&prepared)));
    assert!(report.is_valid(), "findings: {:?}", report.findings);
    assert_eq!(report.profile, Some(ErnProfile::AudioAlbum));
}

#[test]
fn generated_album_passes_business_rules() {
    let prepared = fixture(2);
    let xml = generate_ddex_ern_382(&prepared, &config()).unwrap();
    let report = validate_ern_message(&xml, Some(expected_profile(&prepared)));
    assert!(report.is_valid(), "findings: {:?}", report.findings);
}

#[test]
fn generated_message_metadata_extracts() {
    let prepared = fixture(2);
    let xml = generate_ddex_ern_382(&prepared, &config()).unwrap();
    let meta = extract_ern_metadata(&xml);
    assert_eq!(meta.version, Some(ErnVersion::V382));
    assert_eq!(meta.profile, Some(ErnProfile::AudioAlbum));
    assert_eq!(meta.message_id.as_deref(), Some("MSG-2026-09-26-001"));
    assert_eq!(meta.release_count, 1);
}

#[test]
fn corrupted_message_fails_business_rules() {
    let prepared = fixture(0);
    let xml = generate_ddex_ern_382(&prepared, &config()).unwrap();
    // Simulate a truncated send: drop the closing ResourceList/ReleaseList.
    let cut = xml.find("</ResourceList>").unwrap();
    let truncated = format!("{}</ern:NewReleaseMessage>", &xml[..cut]);
    let report = validate_ern_message(&truncated, Some(ErnProfile::AudioSingle));
    assert!(!report.is_valid());
}

// ---------------------------------------------------------------------------
// UpdateIndicator / MessageThreadId (generation polish)
// ---------------------------------------------------------------------------

#[test]
fn initial_message_emits_original_update_indicator() {
    let prepared = fixture(0);
    let xml = generate_ddex_ern_382(&prepared, &config()).unwrap();
    assert!(xml.contains("<UpdateIndicator>OriginalMessage</UpdateIndicator>"));
    // Thread id defaults to the message id for a new thread.
    assert!(xml.contains("<MessageThreadId>MSG-2026-09-26-001</MessageThreadId>"));
}

#[test]
fn update_message_emits_update_indicator_and_custom_thread() {
    let prepared = fixture(0);
    let mut cfg = config();
    cfg.message_sub_type = MessageSubType::Update;
    cfg.message_thread_id = Some("THREAD-ORIGINAL".into());
    let xml = generate_ddex_ern_382(&prepared, &cfg).unwrap();
    assert!(xml.contains("<UpdateIndicator>UpdateMessage</UpdateIndicator>"));
    assert!(xml.contains("<MessageThreadId>THREAD-ORIGINAL</MessageThreadId>"));
    // Our own business-rule layer must not flag the indicator we emit.
    let report = validate_ern_message(&xml, Some(ErnProfile::AudioSingle));
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.rule_id != "ERN382-UpdateIndicator"),
        "findings: {:?}",
        report.findings
    );
}

// ---------------------------------------------------------------------------
// Pre-generation preflight
// ---------------------------------------------------------------------------

fn has_rule(findings: &[audeniq_core::ddex_validate::ErnFinding], rule: &str) -> bool {
    findings.iter().any(|f| f.rule_id == rule)
}

#[test]
fn preflight_passes_for_good_release_and_config() {
    let prepared = fixture(0);
    let findings = preflight_release(&prepared, &config());
    assert!(findings.is_empty(), "findings: {findings:?}");
    assert!(gate_findings(&findings, "DDEX_PREFLIGHT", "ERN preflight").is_ok());
}

#[test]
fn preflight_rejects_deal_starting_before_release_date() {
    let prepared = fixture(0);
    let mut cfg = config();
    cfg.deal_start_date = "2027-01-01".into(); // release_date is 2027-03-01
    let findings = preflight_release(&prepared, &cfg);
    assert!(has_rule(&findings, "DDEX-PREFLIGHT-DATE-CHRONOLOGY"));
    let err = gate_findings(&findings, "DDEX_PREFLIGHT", "ERN preflight").unwrap_err();
    assert!(matches!(
        err,
        audeniq_core::error::Error::PolicyGate("DDEX_PREFLIGHT")
    ));
}

#[test]
fn preflight_rejects_bad_date_formats_and_takedown_chronology() {
    let prepared = fixture(0);
    let mut cfg = config();
    cfg.deal_start_date = "03/01/2027".into();
    let findings = preflight_release(&prepared, &cfg);
    assert!(has_rule(&findings, "DDEX-PREFLIGHT-DATE-FORMAT"));

    let mut cfg = config();
    cfg.takedown_date = Some("2027-03-01".into()); // not after deal start
    let findings = preflight_release(&prepared, &cfg);
    assert!(has_rule(&findings, "DDEX-PREFLIGHT-TAKEDOWN-CHRONOLOGY"));
}

#[test]
fn preflight_rejects_missing_duration_before_build() {
    let mut prepared = fixture(0);
    prepared.tracks[0].audio.duration_secs = None;
    let findings = preflight_release(&prepared, &config());
    assert!(has_rule(&findings, "DDEX-PREFLIGHT-DURATION"));
    assert!(gate_findings(&findings, "DDEX_PREFLIGHT", "ERN preflight").is_err());
}

#[test]
fn preflight_rejects_bad_message_identity() {
    let prepared = fixture(0);
    let mut cfg = config();
    cfg.message_id = "   ".into();
    cfg.created_at = "not-a-timestamp".into();
    cfg.sender_name.clear();
    let findings = preflight_release(&prepared, &cfg);
    assert!(has_rule(&findings, "DDEX-PREFLIGHT-MESSAGE-ID"));
    assert!(has_rule(&findings, "DDEX-PREFLIGHT-CREATED"));
    assert!(has_rule(&findings, "DDEX-PREFLIGHT-PARTY"));
}

#[test]
fn preflight_warns_on_single_with_many_tracks_but_passes_gate() {
    let mut prepared = fixture(1); // EP fixture, >1 track
    prepared.release_type = "SINGLE".into();
    let findings = preflight_release(&prepared, &config());
    assert!(has_rule(&findings, "DDEX-PREFLIGHT-RELEASE-TYPE"));
    // Warnings are logged, not fatal.
    assert!(gate_findings(&findings, "DDEX_PREFLIGHT", "ERN preflight").is_ok());
}
