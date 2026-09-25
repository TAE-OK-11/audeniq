use audeniq_core::{
    ddex_ern::{DdexErnConfig, MessageSubType, generate_ddex_ern_382},
    preparation_model::PreparedRelease,
};
fn main() {
    let raw = include_str!("../tests/fixtures/ern/album.json");
    let p: PreparedRelease = serde_json::from_str(raw).unwrap();
    let c = DdexErnConfig {
        message_id: "MSG-2026-09-25-001".into(),
        message_thread_id: None,
        message_sub_type: MessageSubType::Initial,
        created_at: "2026-09-25T11:00:00Z".into(),
        sender_name: "AUDENIQ".into(),
        sender_party_id: Some("PADPIDA2026092501A".into()),
        sent_on_behalf_of: None,
        recipient_name: "MockDSP".into(),
        recipient_party_id: Some("PADPIDA2026092501M".into()),
        deal_start_date: "2026-10-01".into(),
        takedown_date: None,
    };
    print!("{}", generate_ddex_ern_382(&p, &c).unwrap());
}
