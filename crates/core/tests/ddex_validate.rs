//! `ddex_validate` integration tests: run the business-rule layer over
//! real ERN 3.8.2 documents produced by `ddex_ern::generate_ddex_ern_382`.
//! No DB, no network — the fixtures are the same JSON releases the
//! `ddex_ern` tests use.
use audeniq_core::{
    ddex_ern::{DdexErnConfig, MessageSubType, generate_ddex_ern_382},
    ddex_validate::{ErnProfile, ErnVersion, extract_ern_metadata, validate_ern_message},
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
        message_sub_type: MessageSubType::Initial,
        created_at: "2026-09-26T09:00:00Z".into(),
        sender_name: "AUDENIQ".into(),
        sender_party_id: Some("PADPIDA2026092601A".into()),
        sent_on_behalf_of: None,
        recipient_name: "MockDSP".into(),
        recipient_party_id: Some("PADPIDA2026092601M".into()),
        deal_start_date: "2026-10-01".into(),
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
