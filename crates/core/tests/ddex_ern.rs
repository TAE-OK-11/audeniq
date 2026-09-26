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
        message_thread_id: None,
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
    assert!(xml.contains("<HashSumAlgorithmType>UserDefined</HashSumAlgorithmType>"));
    assert!(xml.contains(&c.tracks[0].audio.sha256));
    assert!(xml.contains("<AudioCodecType>PCM</AudioCodecType>"));
    // The JSON fixtures carry no measured audio specs, so the technical
    // fields are omitted — the old code fabricated 1411/44100/16/2 here.
    assert!(!xml.contains("<BitRate>"));
    assert!(!xml.contains("<SamplingRate>"));
    assert!(!xml.contains("<BitsPerSample>"));
    assert!(!xml.contains("<NumberOfChannels>"));
    // Credits surface as ResourceContributor, role mapped to the AVS value.
    assert!(xml.contains("<ResourceContributor>"));
    // The fixture credit is "composer"; ERN 3.8.2 has no Composer enum value,
    // so it maps to the generic Contributor (name preserved in PartyName).
    assert!(xml.contains("<ResourceContributorRole>Contributor</ResourceContributorRole>"));
    // Image resource.
    assert!(xml.contains("<ImageType>FrontCoverImage</ImageType>"));
    // Image takes the next A-number after the 4 tracks (XSD pattern
    // requires ResourceReference to start with 'A').
    assert!(xml.contains("<ResourceReference>A005</ResourceReference>"));
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
    // ERN 3.8.2 has no DealId element: Deal = DealReference?, DealTerms?,
    // ... — the builder must not emit one.
    assert!(!xml.contains("DealId"));
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

/// Well-formedness guard for the emitted ERN: parses the whole document
/// with quick-xml and asserts the root element is the ERN message.
/// Full schema conformance is asserted by `ddex_ern_fixtures_pass_xsd_validation`
/// against the vendored ERN 3.8.2 XSD.
#[test]
fn ddex_ern_output_is_well_formed_xml() {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    for i in 0..3 {
        let c = fixture(i);
        let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
        let mut r = Reader::from_str(&xml);
        let mut root: Option<String> = None;
        let mut depth = 0usize;
        loop {
            match r.read_event().unwrap() {
                Event::Start(e) => {
                    if depth == 0 {
                        root = Some(String::from_utf8_lossy(e.name().as_ref()).into_owned());
                    }
                    depth += 1;
                }
                Event::Empty(_) => {
                    if depth == 0 {
                        panic!("root element must not be empty");
                    }
                }
                Event::End(_) => {
                    depth -= 1;
                }
                Event::Eof => break,
                _ => {}
            }
        }
        assert_eq!(depth, 0, "unbalanced XML tags");
        assert_eq!(root.as_deref(), Some("ern:NewReleaseMessage"));
    }
}

#[test]
fn ddex_ern_parental_warning_explicit() {
    let mut f = fixture(0);
    f.explicit = true;
    let xml = generate_ddex_ern_382(&f, &config(MessageSubType::Initial)).unwrap();
    assert!(
        xml.contains("<ParentalWarningType>Explicit</ParentalWarningType>"),
        "explicit release must carry ParentalWarningType"
    );
    let mut f2 = fixture(0);
    f2.explicit = false;
    let xml2 = generate_ddex_ern_382(&f2, &config(MessageSubType::Initial)).unwrap();
    assert!(
        !xml2.contains("ParentalWarningType"),
        "non-explicit release must not carry ParentalWarningType"
    );
}

#[test]
fn ddex_ern_version_emitted_as_subtitle_when_set() {
    // ERN 3.8.2 has no VersionTitle element. The DDEX definition of SubTitle
    // explicitly covers "Titles of Versions used to differentiate different
    // versions of the same Title", so the version/designation goes there —
    // never glued onto the title text (Spotify Metadata Style Guide 8.2/8.4).
    let mut f = fixture(0);
    f.tracks[0].version = "Radio Edit".into();
    let xml = generate_ddex_ern_382(&f, &config(MessageSubType::Initial)).unwrap();
    assert!(
        xml.contains("<SubTitle>Radio Edit</SubTitle>"),
        "version must be emitted as SubTitle"
    );
    assert!(
        !xml.contains("VersionTitle"),
        "ERN 3.8.2 has no VersionTitle element"
    );
    assert!(
        xml.contains("<TitleText>Track 1</TitleText>"),
        "title element must stay clean of version info"
    );
    let f2 = fixture(0);
    let xml2 = generate_ddex_ern_382(&f2, &config(MessageSubType::Initial)).unwrap();
    assert!(
        !xml2.contains("SubTitle"),
        "empty version must not emit SubTitle"
    );
}

#[test]
fn ddex_ern_missing_duration_fails_closed() {
    // SoundRecording/Duration is schema-required. Without a measured
    // duration the builder must fail with DDEX_DURATION_UNKNOWN, never
    // emit a schema-invalid message.
    let mut f = fixture(0);
    f.tracks[0].audio.duration_secs = None;
    let err = generate_ddex_ern_382(&f, &config(MessageSubType::Initial)).unwrap_err();
    assert!(
        matches!(err, Error::PolicyGate("DDEX_DURATION_UNKNOWN")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn ddex_ern_duration_format() {
    let c = fixture(0); // single.json: duration_secs = 205.3
    let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
    assert!(
        xml.contains("<Duration>PT205.3S</Duration>"),
        "xs:duration format expected"
    );
}

/// Full XSD validation against the vendored ERN 3.8.2 schema: every
/// fixture (single/EP/album) must produce a schema-valid
/// `NewReleaseMessage`. This is the F6 pre-contract proof that any
/// interchange message we would send is well-formed per the standard.
#[test]
fn ddex_ern_fixtures_pass_xsd_validation() {
    use audeniq_core::ddex_xsd::validate_ern_382_xml;
    for i in 0..3 {
        let c = fixture(i);
        let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
        validate_ern_382_xml(&xml)
            .unwrap_or_else(|e| panic!("fixture {i} failed XSD validation: {e:?}"));
    }
}

/// Measured audio specs (ffprobe -> catalog.assets -> AssetRef) are emitted
/// as the real `TechnicalSoundRecordingDetails`: 48kHz/24-bit stereo is
/// 2304 kbps, not the fabricated 1411 the old code wrote for every WAV.
#[test]
fn ddex_ern_emits_measured_audio_specs() {
    let mut c = fixture(0); // single, one WAV track
    let audio = &mut c.tracks[0].audio;
    audio.sample_rate = Some(48000);
    audio.channels = Some(2);
    audio.bits_per_sample = Some(24);
    let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
    assert!(xml.contains("<BitRate>2304</BitRate>"), "48000*2*24/1000");
    assert!(xml.contains("<NumberOfChannels>2</NumberOfChannels>"));
    assert!(xml.contains("<SamplingRate>48000</SamplingRate>"));
    assert!(xml.contains("<BitsPerSample>24</BitsPerSample>"));
}

/// The image's proprietary id lives in the *sender's* namespace. The old
/// builder hardcoded a foreign DPID left over from the structural
/// reference; that must never appear again.
#[test]
fn ddex_ern_image_namespace_uses_sender_dpid() {
    let c = fixture(0);
    let xml = generate_ddex_ern_382(&c, &config(MessageSubType::Initial)).unwrap();
    assert!(
        !xml.contains("PADPIDA2023081501R"),
        "foreign DPID must not be hardcoded"
    );
    assert!(xml.contains("Namespace=\"DPID:PADPIDA2026092501A\""));
}

/// A missing sender DPID fails closed: the builder must not emit a
/// fabricated `DPID:AUDENIQ` namespace.
#[test]
fn ddex_ern_missing_sender_dpid_fails_closed() {
    let c = fixture(0);
    let mut cfg = config(MessageSubType::Initial);
    cfg.sender_party_id = None;
    let err = generate_ddex_ern_382(&c, &cfg).unwrap_err();
    assert!(matches!(err, Error::PolicyGate("DDEX_SENDER_DPID_MISSING")));
}
