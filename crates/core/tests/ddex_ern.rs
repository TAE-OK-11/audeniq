//! DDEX ERN 3.8.2 builder tests. Structural reference: stardust-distro (MIT)
//! `ERN382Builder`. No DSP service, no credentials, no network.
use audeniq_core::{
    ddex_ern::{DdexErnConfig, MessageSubType, generate_ddex_ern_382},
    error::Error,
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

fn config(sub: MessageSubType) -> DdexErnConfig {
    DdexErnConfig {
        message_id: "MSG-2026-09-25-001".into(),
        message_sub_type: sub,
        created_at: "2026-09-25T11:00:00Z".into(),
        sender_name: "AUDENIQ".into(),
        sender_party_id: Some("PADPIDA2026092501A".into()),
        sent_on_behalf_of: Some(("LABEL001".into(), "Synthetic Label".into())),
        recipient_name: "MockDSP".into(),
        recipient_party_id: Some("PADPIDA2026092501M".into()),
        deal_start_date: "2026-10-01".into(),
        takedown_date: None,
    }
}

fn count(xml: &str, needle: &str) -> usize {
    xml.matches(needle).count()
}

#[test]
fn ddex_ern_album_structure() {
    let c = fixture(2);
    let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
    // Envelope: real DDEX 3.8.2 namespace, never the synthetic fixture one.
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ern:NewReleaseMessage"));
    assert!(xml.contains("xmlns:ern=\"http://ddex.net/xml/ern/382\""));
    assert!(xml.contains("MessageSchemaVersionId=\"ern/382\""));
    assert!(xml.contains("ReleaseProfileVersionId=\"CommonReleaseAudioAlbum/13\""));
    assert!(!xml.contains("urn:audeniq:ern:synthetic"));
    // Header
    assert!(xml.contains("<MessageThreadId>MSG-2026-09-25-001</MessageThreadId>"));
    assert!(xml.contains("<MessageControlType>LiveMessage</MessageControlType>"));
    assert!(xml.contains("<FullName>AUDENIQ</FullName>"));
    assert!(xml.contains("<FullName>MockDSP</FullName>"));
    assert!(xml.contains("<SentOnBehalfOf>"));
    // ResourceList: 4 sound recordings + 1 image.
    assert_eq!(count(&xml, "<SoundRecording>"), 4);
    assert_eq!(count(&xml, "<Image>"), 1);
    assert!(xml.contains("<ResourceReference>A001</ResourceReference>"));
    assert!(xml.contains("<ResourceReference>A004</ResourceReference>"));
    assert!(xml.contains("<ISRC>"));
    // DDEX file naming convention from the reference: UPC_DD_TTT.wav.
    assert!(xml.contains("<FileName>012345678905_01_001.wav</FileName>"));
    assert!(xml.contains("<FileName>012345678905_02_002.wav</FileName>"));
    // SHA-256 pinned hashes, PCM details for WAV.
    assert!(xml.contains("<HashSumAlgorithmType>SHA256</HashSumAlgorithmType>"));
    assert!(xml.contains(&c.tracks[0].audio.sha256));
    assert!(xml.contains("<AudioCodecType>PCM</AudioCodecType>"));
    assert!(xml.contains("<BitRate>1411</BitRate>"));
    // Credits surface as ResourceContributor.
    assert!(xml.contains("<ResourceContributor sequenceNumber=\"1\">"));
    assert!(xml.contains("<Role>COMPOSER</Role>"));
    // Image resource.
    assert!(xml.contains("<ImageType>FrontCoverImage</ImageType>"));
    assert!(xml.contains("<ResourceReference>I001</ResourceReference>"));
    assert!(xml.contains("<FileName>012345678905.jpg</FileName>"));
    assert!(xml.contains("<ImageCodecType>JPEG</ImageCodecType>"));
    // ReleaseList.
    assert!(xml.contains("<Release IsMainRelease=\"true\">"));
    assert!(xml.contains("<ReleaseReference>R001</ReleaseReference>"));
    assert!(xml.contains("<ReleaseType>Album</ReleaseType>"));
    assert!(xml.contains("<ICPN IsEan=\"true\">012345678905</ICPN>"));
    assert!(xml.contains("<GlobalOriginalReleaseDate>2027-03-01</GlobalOriginalReleaseDate>"));
    assert_eq!(count(&xml, "<ReleaseResourceReference>"), 5);
    // DealList.
    assert!(xml.contains("<CommercialModelType>SubscriptionModel</CommercialModelType>"));
    assert!(xml.contains("<UseType>OnDemandStream</UseType>"));
    assert!(xml.contains("<StartDate>2026-10-01</StartDate>"));
    assert!(!xml.contains("<EndDate>"));
    assert!(xml.contains("<DealId>R001_DEAL_1</DealId>"));
    assert!(xml.ends_with("</ern:NewReleaseMessage>\n"));
}

#[test]
fn ddex_ern_single_profile() {
    let c = fixture(0);
    let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
    assert!(xml.contains("ReleaseProfileVersionId=\"CommonReleaseAudioSingle/13\""));
    assert_eq!(count(&xml, "<SoundRecording>"), 1);
    assert!(xml.contains("<ReleaseType>Single</ReleaseType>"));
}

#[test]
fn ddex_ern_ep_profile() {
    let c = fixture(1);
    let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
    assert!(xml.contains("ReleaseProfileVersionId=\"CommonReleaseAudioAlbum/13\""));
    assert!(xml.contains("<ReleaseType>EP</ReleaseType>"));
}

#[test]
fn ddex_ern_escapes_xml_specials() {
    // The album fixture title is `바다 & 빛 <Live> "셋" '넷'`.
    let c = fixture(2);
    let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
    assert!(!xml.contains("바다 & 빛"));
    assert!(!xml.contains("<Live>"));
    assert!(xml.contains("바다 &amp; 빛 &lt;Live&gt; &quot;셋&quot; &apos;넷&apos;"));
}

#[test]
fn ddex_ern_update_and_takedown() {
    let c = fixture(0);
    let update = generate_ddex_ern_382(&c, &config(MessageSubType::Update)).unwrap();
    assert!(update.contains("<MessageControlType>UpdateMessage</MessageControlType>"));
    assert!(!update.contains("<EndDate>"));

    let mut cfg = config(MessageSubType::Takedown);
    cfg.takedown_date = Some("2026-12-31".into());
    let takedown = generate_ddex_ern_382(&c, &cfg).unwrap();
    assert!(takedown.contains("<MessageControlType>UpdateMessage</MessageControlType>"));
    assert!(takedown.contains("<EndDate>2026-12-31</EndDate>"));

    // Takedown without a date is rejected.
    let err = generate_ddex_ern_382(&c, &config(MessageSubType::Takedown)).unwrap_err();
    assert!(matches!(err, Error::Invalid));
}

#[test]
fn ddex_ern_deterministic_and_rejects_bad_config() {
    let c = fixture(0);
    let cfg = config(MessageSubType::Initial);
    let a = generate_ddex_ern_382(&c, &cfg).unwrap();
    let b = generate_ddex_ern_382(&c, &cfg).unwrap();
    assert_eq!(a, b);

    let mut bad = config(MessageSubType::Initial);
    bad.message_id = String::new();
    assert!(matches!(
        generate_ddex_ern_382(&c, &bad).unwrap_err(),
        Error::Invalid
    ));
    let mut bad = config(MessageSubType::Initial);
    bad.sender_name = "  ".into();
    assert!(matches!(
        generate_ddex_ern_382(&c, &bad).unwrap_err(),
        Error::Invalid
    ));
}
