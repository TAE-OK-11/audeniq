//! Partner onboarding state (F6 groundwork, contract-free).
//!
//! Tracks what a future DSP / Merlin / LIMBO partnership needs *around*
//! the contract signature: party identification (DPID), endpoint and
//! credential metadata, and interop proof (XSD-valid test ERNs, parsed
//! ACKs). The contract signature itself is tracked but can only be
//! completed with a real counterparty.
//!
//! Operator-owned: these functions run as the platform operator (table
//! owner). The API and worker roles hold no grants on
//! `execution.partner_onboarding` and must never call these.

use crate::error::{Error, Result};
use serde::Serialize;
use sqlx::{PgPool, Row};

/// One onboarding row plus the live readiness gaps.
#[derive(Debug, Clone, Serialize)]
pub struct OnboardingStatus {
    pub partner_id: String,
    pub stage: String,
    pub dpid_registered: bool,
    pub endpoint_url: Option<String>,
    pub endpoint_health: String,
    pub credential_kind: Option<String>,
    pub credential_status: String,
    pub test_ern_validated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub test_ack_parsed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub contract_signed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub contract_ref: Option<String>,
    /// Requirements still missing before the partner may go live.
    /// Empty = ready (see `execution.partner_onboarding_gaps`).
    pub gaps: Vec<String>,
}

/// Read the onboarding status for a partner, including readiness gaps.
pub async fn status(pool: &PgPool, partner_id: &str) -> Result<OnboardingStatus> {
    let row = sqlx::query(
        "SELECT stage, dpid_registered, endpoint_url, endpoint_health,
                credential_kind, credential_status,
                test_ern_validated_at, test_ack_parsed_at,
                contract_signed_at, contract_ref
           FROM execution.partner_onboarding WHERE partner_id=$1",
    )
    .bind(partner_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::PolicyGate("PARTNER_ONBOARDING_UNKNOWN"))?;
    let gaps: Vec<String> =
        sqlx::query_scalar("SELECT requirement FROM execution.partner_onboarding_gaps($1)")
            .bind(partner_id)
            .fetch_all(pool)
            .await?;
    Ok(OnboardingStatus {
        partner_id: partner_id.to_string(),
        stage: row.get("stage"),
        dpid_registered: row.get("dpid_registered"),
        endpoint_url: row.get("endpoint_url"),
        endpoint_health: row.get("endpoint_health"),
        credential_kind: row.get("credential_kind"),
        credential_status: row.get("credential_status"),
        test_ern_validated_at: row.get("test_ern_validated_at"),
        test_ack_parsed_at: row.get("test_ack_parsed_at"),
        contract_signed_at: row.get("contract_signed_at"),
        contract_ref: row.get("contract_ref"),
        gaps,
    })
}

/// Ensure an onboarding row exists for a partner profile.
pub async fn ensure(pool: &PgPool, partner_id: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO execution.partner_onboarding(partner_id) VALUES($1) ON CONFLICT(partner_id) DO NOTHING",
    )
    .bind(partner_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark the recipient DPID as registered for a partner.
pub async fn register_dpid(pool: &PgPool, partner_id: &str) -> Result<()> {
    ensure(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.partner_onboarding SET dpid_registered=true, updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Register the delivery endpoint URL. Only https URLs are accepted
/// (enforced again by the CHECK constraint).
pub async fn register_endpoint(pool: &PgPool, partner_id: &str, url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        return Err(Error::Invalid);
    }
    ensure(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.partner_onboarding SET endpoint_url=$2, endpoint_health='UNKNOWN', updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .bind(url)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record credential *metadata* only: which kind of credential the Secure
/// Vault holds for this partner. The secret itself never touches this
/// table — see the module docs.
pub async fn record_credential_stored(pool: &PgPool, partner_id: &str, kind: &str) -> Result<()> {
    if !matches!(kind, "oauth2" | "api_key" | "mtls" | "sftp_key") {
        return Err(Error::Invalid);
    }
    ensure(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.partner_onboarding SET credential_kind=$2, credential_status='STORED', updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .bind(kind)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record that an XSD-valid test ERN was generated for this partner.
/// Called by the Stage 3 pipeline after `ddex_xsd::validate_ern_382_xml`
/// passes on a test message — interop proof producible without any contract.
pub async fn record_test_ern_validated(pool: &PgPool, partner_id: &str) -> Result<()> {
    ensure(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.partner_onboarding SET test_ern_validated_at=now(), updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record that a partner-style ACK was successfully parsed.
pub async fn record_test_ack_parsed(pool: &PgPool, partner_id: &str) -> Result<()> {
    ensure(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.partner_onboarding SET test_ack_parsed_at=now(), updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record a signed contract. `contract_ref` is the operator's filing
/// reference (e.g. "MSA-2026-001"); the countersigned document itself
/// lives outside the database.
pub async fn record_contract(pool: &PgPool, partner_id: &str, contract_ref: &str) -> Result<()> {
    if contract_ref.trim().is_empty() {
        return Err(Error::Invalid);
    }
    ensure(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.partner_onboarding SET contract_signed_at=now(), contract_ref=$2, stage='READY', updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .bind(contract_ref.trim())
    .execute(pool)
    .await?;
    Ok(())
}

/// Advance the onboarding stage. LIVE can only be set when the readiness
/// gate is empty; the `guard_delivery_enabled` trigger enforces the same
/// invariant on the actual kill-switch.
pub async fn set_stage(pool: &PgPool, partner_id: &str, stage: &str) -> Result<()> {
    if !matches!(
        stage,
        "INTAKE" | "TECHNICAL" | "COMMERCIAL" | "READY" | "LIVE"
    ) {
        return Err(Error::Invalid);
    }
    if stage == "LIVE" {
        let gaps: Vec<String> =
            sqlx::query_scalar("SELECT requirement FROM execution.partner_onboarding_gaps($1)")
                .bind(partner_id)
                .fetch_all(pool)
                .await?;
        if !gaps.is_empty() {
            return Err(Error::PolicyGate("PARTNER_NOT_READY_FOR_LIVE"));
        }
    }
    ensure(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.partner_onboarding SET stage=$2, updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .bind(stage)
    .execute(pool)
    .await?;
    Ok(())
}
