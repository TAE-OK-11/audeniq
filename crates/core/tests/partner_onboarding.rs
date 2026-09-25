//! Partner onboarding model tests (F6 groundwork, contract-free).
//!
//! Proves the readiness gate: `delivery_enabled` cannot flip to live until
//! every onboarding requirement is evidenced, and the gate reports exactly
//! what is missing. Runs as the table owner (platform operator); the API
//! and worker roles hold no grants on this table.

use audeniq_core::{database, partner_onboarding};
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
