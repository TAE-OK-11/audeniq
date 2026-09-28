//! Partner onboarding model tests (F6 groundwork, contract-free).
//!
//! Proves the readiness gate: `delivery_enabled` cannot flip to live until
//! every onboarding requirement is evidenced, and the gate reports exactly
//! what is missing. Runs as the table owner (platform operator); the API
//! and worker roles hold no grants on this table.

use audeniq_core::{database, dsp_registry::Dsp, partner_onboarding, routing};
use sqlx::PgPool;

async fn migrated(pool: &PgPool) {
    database::MIGRATOR.run(pool).await.unwrap();
}

#[sqlx::test]
async fn mockdsp_seed_reports_only_contract_missing(pool: PgPool) {
    migrated(&pool).await;
    let st = partner_onboarding::status(&pool, "mockdsp").await.unwrap();
    assert_eq!(st.stage, "TECHNICAL");
    assert!(st.dpid_registered);
    assert_eq!(st.credential_status, "STORED");
    // Honest: the seed no longer invents test interop evidence. The test
    // suite records real timestamps via the operator functions when it
    // actually generates/parses; until then the gaps are reported.
    // No contract exists for the test partner.
    assert_eq!(
        st.gaps,
        vec![
            "test_ern_validated".to_string(),
            "test_ack_parsed".to_string(),
            "contract_signed".to_string()
        ]
    );
}

#[sqlx::test]
async fn mockdsp_is_selectable_without_unlocking_real_dsps(pool: PgPool) {
    migrated(&pool).await;
    let mock = Dsp::D36.uuid();
    let profile_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT dsp_id FROM execution.adapter_profiles WHERE partner_id='mockdsp'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(profile_id, mock);
    let routes = routing::public_routes(&pool, uuid::Uuid::new_v4(), &[mock, Dsp::D5.uuid()])
        .await
        .unwrap();
    assert!(routes[0].routable);
    assert_eq!(routes[0].partner_id.as_deref(), Some("mockdsp"));
    assert!(!routes[1].routable);

    sqlx::query(
        "UPDATE execution.adapter_profiles SET delivery_enabled=false WHERE partner_id='mockdsp'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        !routing::public_routes(&pool, uuid::Uuid::new_v4(), &[mock])
            .await
            .unwrap()[0]
            .routable
    );
}

#[sqlx::test]
async fn delivery_flip_blocked_until_onboarding_complete(pool: PgPool) {
    migrated(&pool).await;
    // CONTRACTED partner: the commercial onboarding gate applies.
    // (MOCK partners bypass the gate by design; see 0022 activation_kind.)
    sqlx::query(
        "INSERT INTO execution.adapter_profiles(partner_id, display_name, profile_version, delivery_enabled, transport, activation_kind)
         VALUES ('onb-test', 'Onboarding Test', '1', false, 'mock', 'CONTRACTED')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // No onboarding row at all: the trigger blocks the flip.
    let err = sqlx::query(
        "UPDATE execution.adapter_profiles SET delivery_enabled=true WHERE partner_id='onb-test'",
    )
    .execute(&pool)
    .await
    .unwrap_err();
    let db_err = err.into_database_error().unwrap();
    assert_eq!(db_err.code().as_deref(), Some("23514"));
    assert!(db_err.message().contains("not ready for live delivery"));

    // Complete every requirement via the operator API...
    partner_onboarding::register_dpid(&pool, "onb-test")
        .await
        .unwrap();
    partner_onboarding::register_endpoint(&pool, "onb-test", "https://onb-test.example.com/ddex")
        .await
        .unwrap();
    partner_onboarding::record_credential_stored(&pool, "onb-test", "api_key")
        .await
        .unwrap();
    partner_onboarding::record_test_ern_validated(&pool, "onb-test")
        .await
        .unwrap();
    partner_onboarding::record_test_ack_parsed(&pool, "onb-test")
        .await
        .unwrap();
    // ...but the gate still reports the missing contract: it cannot be
    // invented, only recorded from a real counterparty.
    let st = partner_onboarding::status(&pool, "onb-test").await.unwrap();
    assert_eq!(st.gaps, vec!["contract_signed".to_string()]);
    let err = sqlx::query(
        "UPDATE execution.adapter_profiles SET delivery_enabled=true WHERE partner_id='onb-test'",
    )
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(
        err.into_database_error()
            .unwrap()
            .message()
            .contains("not ready")
    );

    // Record the contract: now the flip succeeds.
    partner_onboarding::record_contract(&pool, "onb-test", "MSA-TEST-001")
        .await
        .unwrap();
    let st = partner_onboarding::status(&pool, "onb-test").await.unwrap();
    assert!(st.gaps.is_empty(), "gaps: {:?}", st.gaps);
    sqlx::query(
        "UPDATE execution.adapter_profiles SET delivery_enabled=true WHERE partner_id='onb-test'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let enabled: bool = sqlx::query_scalar(
        "SELECT delivery_enabled FROM execution.adapter_profiles WHERE partner_id='onb-test'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(enabled);

    sqlx::query("DELETE FROM execution.adapter_profiles WHERE partner_id='onb-test'")
        .execute(&pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn set_stage_live_requires_empty_gaps(pool: PgPool) {
    migrated(&pool).await;
    sqlx::query(
        "INSERT INTO execution.adapter_profiles(partner_id, display_name, profile_version, delivery_enabled, transport)
         VALUES ('onb-stage', 'Stage Test', '1', false, 'mock')",
    )
    .execute(&pool)
    .await
    .unwrap();
    partner_onboarding::ensure(&pool, "onb-stage")
        .await
        .unwrap();
    let err = partner_onboarding::set_stage(&pool, "onb-stage", "LIVE")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        audeniq_core::error::Error::PolicyGate("PARTNER_NOT_READY_FOR_LIVE")
    ));
    sqlx::query("DELETE FROM execution.partner_onboarding WHERE partner_id='onb-stage'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM execution.adapter_profiles WHERE partner_id='onb-stage'")
        .execute(&pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn endpoint_rejects_non_https(pool: PgPool) {
    migrated(&pool).await;
    let err = partner_onboarding::register_endpoint(&pool, "onb-ep", "http://insecure.example.com")
        .await
        .unwrap_err();
    assert!(matches!(err, audeniq_core::error::Error::Invalid));
    sqlx::query("DELETE FROM execution.partner_onboarding WHERE partner_id='onb-ep'")
        .execute(&pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn unknown_partner_status_errors(pool: PgPool) {
    migrated(&pool).await;
    let err = partner_onboarding::status(&pool, "no-such-partner")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        audeniq_core::error::Error::PolicyGate("PARTNER_ONBOARDING_UNKNOWN")
    ));
}

#[sqlx::test]
async fn mock_partner_bypasses_commercial_gate(pool: PgPool) {
    migrated(&pool).await;
    // MOCK partners (test/synthetic) are eligible via delivery_enabled
    // alone; the onboarding gate is the commercial LIVE gate, not a
    // test-partner gate (0022 activation_kind, 0028).
    sqlx::query(
        "INSERT INTO execution.adapter_profiles(partner_id, display_name, profile_version, delivery_enabled, transport, activation_kind)
         VALUES ('onb-mock', 'Mock Test', '1', false, 'mock', 'MOCK')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // No onboarding row at all: the flip still succeeds for MOCK.
    sqlx::query(
        "UPDATE execution.adapter_profiles SET delivery_enabled=true WHERE partner_id='onb-mock'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let enabled: bool = sqlx::query_scalar(
        "SELECT delivery_enabled FROM execution.adapter_profiles WHERE partner_id='onb-mock'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(enabled);
}

#[sqlx::test]
async fn missing_onboarding_row_reports_all_gaps(pool: PgPool) {
    migrated(&pool).await;
    // Regression test for the NOT(NULL) bug (0028): a partner with no
    // onboarding row must report every gap, not zero.
    sqlx::query(
        "INSERT INTO execution.adapter_profiles(partner_id, display_name, profile_version, delivery_enabled, transport, activation_kind)
         VALUES ('onb-norow', 'No Row', '1', false, 'mock', 'CONTRACTED')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let gaps: Vec<String> = sqlx::query_scalar(
        "SELECT requirement FROM execution.partner_onboarding_gaps('onb-norow') ORDER BY requirement",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        gaps,
        vec![
            "contract_signed".to_string(),
            "credential_status".to_string(),
            "dpid_registered".to_string(),
            "endpoint_url".to_string(),
            "test_ack_parsed".to_string(),
            "test_ern_validated".to_string(),
        ]
    );
}

#[sqlx::test]
async fn operator_links_a_partner_to_its_dsp_id(pool: PgPool) {
    migrated(&pool).await;
    // Mimic an adapter without a DSP id to exercise the operator link action.
    // The normal seed now links MockDSP to the D-36 test destination.
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=NULL WHERE partner_id='mockdsp'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        partner_onboarding::set_dsp(&pool, " ", "mockdsp", None)
            .await
            .is_err()
    );
    assert!(
        partner_onboarding::set_dsp(&pool, "tester", "no-such-partner", None)
            .await
            .is_err()
    );
    let id = partner_onboarding::set_dsp(&pool, "tester", "mockdsp", None)
        .await
        .unwrap();
    // Derived ids are stable per partner.
    assert_eq!(
        id,
        partner_onboarding::set_dsp(&pool, "tester", "mockdsp", None)
            .await
            .unwrap()
    );
    let listed = partner_onboarding::list_profiles(&pool).await.unwrap();
    let mock = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["partner_id"] == "mockdsp")
        .unwrap()
        .clone();
    assert_eq!(mock["dsp_id"], id.to_string());
}

/// Pre-launch lock: a DSP that is fully onboarded, contracted, enabled and
/// staged LIVE is still never routed (so no delivery job and no request)
/// until DSP_LIVE_TRANSMISSION=enabled opens it at the official launch.
#[sqlx::test]
async fn fully_onboarded_dsp_is_not_routed_before_launch(pool: PgPool) {
    migrated(&pool).await;
    if audeniq_core::launch::live_transmission_enabled() {
        return; // an operator shell with the lock opened
    }
    let p = "D-5";
    // Direct contract for this test (Merlin-licensed DSPs default to MERLIN).
    audeniq_core::partner_admin::set_route(&pool, "ops", p, "DIRECT")
        .await
        .unwrap();
    sqlx::query(
        "UPDATE execution.adapter_profiles SET ddex_recipient_dpid='PADPIDA2011021601U',
                capabilities = capabilities || '{\"send_or_publish\":true}' WHERE partner_id=$1",
    )
    .bind(p)
    .execute(&pool)
    .await
    .unwrap();
    partner_onboarding::register_dpid(&pool, p).await.unwrap();
    partner_onboarding::register_endpoint(&pool, p, "sftp://sftp.partner.example:22/in")
        .await
        .unwrap();
    partner_onboarding::record_credential_stored(&pool, p, "sftp_key")
        .await
        .unwrap();
    partner_onboarding::record_test_ern_validated(&pool, p)
        .await
        .unwrap();
    partner_onboarding::record_test_ack_parsed(&pool, p)
        .await
        .unwrap();
    partner_onboarding::record_contract(&pool, p, "DSA-TEST-1")
        .await
        .unwrap();
    partner_onboarding::set_stage(&pool, p, "LIVE")
        .await
        .unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET delivery_enabled=true WHERE partner_id=$1")
        .bind(p)
        .execute(&pool)
        .await
        .unwrap();
    let contract_live: bool = sqlx::query_scalar("SELECT execution.platform_contract_live($1)")
        .bind(p)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        contract_live,
        "everything but the launch switch is in place"
    );
    let d5 = audeniq_core::dsp_registry::Dsp::from_code(p)
        .unwrap()
        .uuid();
    let org = uuid::Uuid::new_v4();
    for d in [
        audeniq_core::routing::decide_routes(&pool, org, &[d5])
            .await
            .unwrap(),
        audeniq_core::routing::public_routes(&pool, org, &[d5])
            .await
            .unwrap(),
    ] {
        assert!(!d[0].routable, "{:?}", d[0]);
        assert_eq!(d[0].reason, "PRE_LAUNCH_LOCKED");
    }
}

async fn gaps(pool: &PgPool, p: &str) -> Vec<(String, String)> {
    sqlx::query_as("SELECT requirement, detail FROM execution.partner_onboarding_gaps($1)")
        .bind(p)
        .fetch_all(pool)
        .await
        .unwrap()
}

/// Direct contract or Merlin, per DSP: under MERLIN the Merlin agreement
/// stands in for the DSP contract (technical onboarding still required);
/// MERLIN is refused for DSPs without a Merlin deal; routing stays locked
/// before launch either way.
#[sqlx::test]
async fn merlin_route_uses_the_merlin_agreement_as_contract_evidence(pool: PgPool) {
    use audeniq_core::partner_admin as pa;
    migrated(&pool).await;
    let p = "D-5";
    // Merlin-licensed DSPs start on the MERLIN route (0057).
    let route: String =
        sqlx::query_scalar("SELECT route FROM distribution.dsp_contract_routes WHERE code=$1")
            .bind(p)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(route, "MERLIN");
    for (code, want) in [
        ("D-1", "DIRECT"),
        ("D-11", "DIRECT"),
        ("D-29", "DIRECT"),
        ("D-25", "MERLIN"),
        ("D-26", "MERLIN"),
    ] {
        let r: String =
            sqlx::query_scalar("SELECT route FROM distribution.dsp_contract_routes WHERE code=$1")
                .bind(code)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(r, want, "{code}");
    }
    pa::set_route(&pool, "ops", p, "DIRECT").await.unwrap();
    partner_onboarding::register_dpid(&pool, p).await.unwrap();
    partner_onboarding::register_endpoint(&pool, p, "sftp://sftp.partner.example/in")
        .await
        .unwrap();
    partner_onboarding::record_credential_stored(&pool, p, "sftp_key")
        .await
        .unwrap();
    partner_onboarding::record_test_ern_validated(&pool, p)
        .await
        .unwrap();
    partner_onboarding::record_test_ack_parsed(&pool, p)
        .await
        .unwrap();
    assert_eq!(
        gaps(&pool, p).await,
        vec![(
            "contract_signed".into(),
            "no signed contract on file".into()
        )]
    );
    assert!(matches!(
        pa::set_route(&pool, "ops", "D-1", "MERLIN").await,
        Err(audeniq_core::error::Error::PolicyGate(
            "MERLIN_NOT_AVAILABLE_FOR_DSP"
        ))
    ));
    assert!(pa::set_route(&pool, "", p, "MERLIN").await.is_err());
    let v = pa::set_route(&pool, "ops", p, "merlin").await.unwrap();
    assert_eq!(v["platform"], "Spotify");
    assert_eq!(
        gaps(&pool, p).await,
        vec![(
            "contract_signed".into(),
            "no signed Merlin agreement on file (route MERLIN)".into()
        )]
    );
    partner_onboarding::record_contract(&pool, "merlin", "MERLIN-MEMBER-2026")
        .await
        .unwrap();
    assert!(gaps(&pool, p).await.is_empty());
    partner_onboarding::set_stage(&pool, p, "LIVE")
        .await
        .unwrap();
    let live: bool = sqlx::query_scalar("SELECT execution.platform_contract_live($1)")
        .bind(p)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(live);
    // Merlin eligibility cannot be withdrawn while the DSP uses the route.
    assert!(
        pa::set_merlin_eligible(&pool, "ops", p, false)
            .await
            .is_err()
    );
    // Back to DIRECT: the DSP's own contract is required again.
    pa::set_route(&pool, "ops", p, "DIRECT").await.unwrap();
    let live: bool = sqlx::query_scalar("SELECT execution.platform_contract_live($1)")
        .bind(p)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!live, "a Merlin agreement is not a direct contract");
    if !audeniq_core::launch::live_transmission_enabled() {
        let d5 = audeniq_core::dsp_registry::Dsp::D5.uuid();
        pa::set_route(&pool, "ops", p, "MERLIN").await.unwrap();
        sqlx::query("UPDATE execution.adapter_profiles SET delivery_enabled=true, capabilities=capabilities||'{\"send_or_publish\":true}' WHERE partner_id=$1")
            .bind(p)
            .execute(&pool)
            .await
            .unwrap();
        let d = audeniq_core::routing::decide_routes(&pool, uuid::Uuid::new_v4(), &[d5])
            .await
            .unwrap();
        assert_eq!(d[0].reason, "PRE_LAUNCH_LOCKED");
    }
}
