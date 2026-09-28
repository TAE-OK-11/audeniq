//! Operator workflow that takes a contracted DSP from INTAKE to LIVE
//! (`audeniq-admin partner …`). Runs with the schema owner, like the rest
//! of `partner_onboarding`; every change is audited with the operator name.
//!
//! Order a contract usually follows:
//! 1. `config-check` — the partner's JSON config in PARTNER_CONFIG_DIR parses
//!    and every secret it references resolves.
//! 2. `probe` — connect/authenticate with the configured transport; records
//!    the endpoint (without credentials) and the credential kind.
//! 3. `dpid` — the partner's DDEX party id (recipient) from the contract.
//! 4. `test-ern FILE` / `test-ack FILE` — interop proof: an XSD- and
//!    rule-valid ERN, and one of the partner's real ACK files parsed.
//! 5. `capabilities JSON` — only what the partner's documentation supports.
//! 6. `contract REF` — the signed contract's filing reference.
//! 7. `go-live` — stage LIVE + delivery_enabled; refused while any gap
//!    remains (the 0027 trigger checks the same list).
//!
//! `suspend` reverses step 7 at once (kill switch + stage TECHNICAL).
use crate::error::{Error, Result};
use crate::execution::Capabilities;
use crate::partner_config::{AdapterKind, HttpAuth, PartnerConfig, TransportConfig};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

async fn audit(
    pool: &PgPool,
    operator: &str,
    partner_id: &str,
    action: &str,
    detail: &str,
) -> Result<()> {
    if operator.trim().is_empty() {
        return Err(Error::InvalidCode("OPERATOR_REQUIRED"));
    }
    let mut c = pool.acquire().await?;
    crate::operations::audit(
        &mut c,
        None,
        None,
        None,
        action,
        &format!("OPERATOR:{} {partner_id} {detail}", operator.trim())
            .chars()
            .take(500)
            .collect::<String>(),
        Uuid::new_v4(),
    )
    .await
}

fn need_operator(operator: &str) -> Result<()> {
    if operator.trim().is_empty() {
        Err(Error::InvalidCode("OPERATOR_REQUIRED"))
    } else {
        Ok(())
    }
}

async fn profile_exists(pool: &PgPool, partner_id: &str) -> Result<()> {
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.adapter_profiles WHERE partner_id=$1)",
    )
    .bind(partner_id)
    .fetch_one(pool)
    .await?;
    if ok { Ok(()) } else { Err(Error::NotFound) }
}

fn load(partner_id: &str) -> Result<PartnerConfig> {
    crate::partner_config::load(partner_id)?.ok_or(Error::PolicyGate("PARTNER_CONFIG_MISSING"))
}

/// Every config file: parses, secrets resolve, what it points at.
pub fn check_configs() -> Value {
    let items: Vec<Value> = crate::partner_config::load_all()
        .into_iter()
        .map(
            |(id, r)| match r.and_then(|c| resolve_secrets(&c).map(|_| c)) {
                Ok(c) => json!({
                    "partner_id": id,
                    "ok": true,
                    "adapter": format!("{:?}", c.adapter),
                    "endpoint": c.endpoint_label(),
                    "webhook": c.webhook.is_some(),
                    "status_api": c.status_api.is_some(),
                }),
                Err(e) => json!({"partner_id": id, "ok": false, "error": format!("{e}")}),
            },
        )
        .collect();
    json!({"config_dir": crate::partner_config::config_dir(), "partners": items})
}

fn resolve_secrets(c: &PartnerConfig) -> Result<()> {
    match &c.transport {
        TransportConfig::Sftp {
            private_key,
            known_hosts,
            ..
        } => {
            private_key.resolve_path("id_partner")?;
            known_hosts.resolve_path("known_hosts")?;
        }
        TransportConfig::S3 {
            access_key_id,
            secret_access_key,
            ..
        } => {
            access_key_id.resolve()?;
            secret_access_key.resolve()?;
        }
        _ => {}
    }
    for api in [&c.http_api, &c.status_api].into_iter().flatten() {
        match &api.auth {
            HttpAuth::Bearer { token } => drop(token.resolve()?),
            HttpAuth::ApiKey { key, .. } => drop(key.resolve()?),
            HttpAuth::Oauth2 {
                client_id,
                client_secret,
                ..
            } => {
                client_id.resolve()?;
                client_secret.resolve()?;
            }
            HttpAuth::Hmac { secret, .. } => drop(secret.resolve()?),
        }
    }
    if let Some(w) = &c.webhook {
        w.secret.resolve()?;
    }
    Ok(())
}

fn credential_kind(c: &PartnerConfig) -> &'static str {
    match (&c.transport, &c.http_api) {
        (TransportConfig::Sftp { .. }, _) => "sftp_key",
        (TransportConfig::S3 { .. }, _) => "s3_key",
        (_, Some(api)) => match api.auth {
            HttpAuth::Oauth2 { .. } => "oauth2",
            HttpAuth::Hmac { .. } => "hmac",
            _ => "api_key",
        },
        _ => "api_key",
    }
}

/// Connect with the configured transport (or API) and record the result.
pub async fn probe(pool: &PgPool, operator: &str, partner_id: &str) -> Result<Value> {
    need_operator(operator)?;
    profile_exists(pool, partner_id).await?;
    let c = load(partner_id)?;
    resolve_secrets(&c)?;
    // Pre-launch lock: a probe is a real request to the DSP.
    let host = match &c.transport {
        TransportConfig::Sftp { host, .. } => host.clone(),
        TransportConfig::S3 { endpoint, .. } => url::Url::parse(endpoint)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default(),
        TransportConfig::Local { .. } => "localhost".into(),
        TransportConfig::None => c
            .http_api
            .as_ref()
            .and_then(|a| url::Url::parse(&a.base_url).ok())
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default(),
    };
    if !crate::launch::wire_allowed(&host) {
        return Err(Error::PolicyGate(crate::launch::LOCKED));
    }
    let target = match c.adapter {
        AdapterKind::HttpApi => {
            let api = crate::partners::http::ApiClient::new(
                c.http_api
                    .clone()
                    .ok_or(Error::PolicyGate("PARTNER_HTTP_API_MISSING"))?,
            )?;
            // Any HTTP answer proves reachability + TLS; auth errors are
            // reported as such.
            match api.send(reqwest::Method::GET, "/", None, None).await {
                Ok(_) | Err(crate::partners::http::HttpFailure::Rejected { status: 404, .. }) => {}
                Err(crate::partners::http::HttpFailure::Rejected {
                    status: 401 | 403, ..
                }) => {
                    return Err(Error::PolicyGate("PARTNER_AUTH_REJECTED"));
                }
                Err(crate::partners::http::HttpFailure::Rejected { .. }) => {}
                Err(_) => return Err(Error::PolicyGate("PARTNER_TRANSPORT_UNREACHABLE")),
            }
            api.config.base_url.clone()
        }
        _ => crate::transport::probe_config(&c.transport).await?,
    };
    let endpoint = c.endpoint_label();
    crate::partner_onboarding::ensure(pool, partner_id).await?;
    if ["https://", "sftp://", "s3://"]
        .iter()
        .any(|p| endpoint.starts_with(p))
    {
        crate::partner_onboarding::register_endpoint(pool, partner_id, &endpoint).await?;
    }
    sqlx::query(
        "UPDATE execution.partner_onboarding SET endpoint_health='HEALTHY', updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .execute(pool)
    .await?;
    let kind = credential_kind(&c);
    crate::partner_onboarding::record_credential_stored(pool, partner_id, kind).await?;
    audit(pool, operator, partner_id, "partner.probe", &target).await?;
    Ok(
        json!({"partner_id": partner_id, "reachable": target, "endpoint": endpoint, "credential_kind": kind}),
    )
}

/// Register the partner's DDEX party id (the recipient DPID of our ERNs).
pub async fn set_recipient_dpid(
    pool: &PgPool,
    operator: &str,
    partner_id: &str,
    dpid: &str,
) -> Result<()> {
    need_operator(operator)?;
    let dpid = dpid.trim();
    // DDEX party ids: PADPIDA + 11 alphanumerics (e.g. PADPIDA2011021601U).
    let valid = dpid.len() == 18
        && dpid.starts_with("PADPIDA")
        && dpid[7..].bytes().all(|b| b.is_ascii_alphanumeric());
    if !valid {
        return Err(Error::InvalidCode("DPID_INVALID"));
    }
    profile_exists(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.adapter_profiles SET ddex_recipient_dpid=$2, updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .bind(dpid)
    .execute(pool)
    .await?;
    crate::partner_onboarding::register_dpid(pool, partner_id).await?;
    audit(pool, operator, partner_id, "partner.dpid", dpid).await
}

/// Interop proof: the ERN in `xml` passes the XSD and business rules.
pub async fn test_ern(pool: &PgPool, operator: &str, partner_id: &str, xml: &str) -> Result<Value> {
    need_operator(operator)?;
    profile_exists(pool, partner_id).await?;
    crate::ddex_xsd::validate_ern_382_xml(xml)
        .map_err(|_| Error::PolicyGate("TEST_ERN_XSD_INVALID"))?;
    let report = crate::ddex_validate::validate_ern_message(xml, None);
    let errors: Vec<String> = report
        .errors()
        .iter()
        .map(|f| format!("{}: {}", f.rule_id, f.message))
        .collect();
    if !errors.is_empty() {
        return Ok(json!({"partner_id": partner_id, "valid": false, "errors": errors}));
    }
    crate::partner_onboarding::record_test_ern_validated(pool, partner_id).await?;
    audit(pool, operator, partner_id, "partner.test_ern", "valid").await?;
    Ok(json!({"partner_id": partner_id, "valid": true}))
}

/// Interop proof: one of the partner's real ACK/result documents parses to
/// a definite answer with our parser.
pub async fn test_ack(
    pool: &PgPool,
    operator: &str,
    partner_id: &str,
    bytes: &[u8],
) -> Result<Value> {
    need_operator(operator)?;
    profile_exists(pool, partner_id).await?;
    let doc = crate::partners::ack::parse_document(bytes)
        .ok_or(Error::PolicyGate("TEST_ACK_UNPARSEABLE"))?;
    let status = format!("{:?}", doc.status);
    if doc.status == crate::partners::ack::AckStatus::Pending {
        return Err(Error::PolicyGate("TEST_ACK_NOT_DEFINITE"));
    }
    crate::partner_onboarding::record_test_ack_parsed(pool, partner_id).await?;
    audit(pool, operator, partner_id, "partner.test_ack", &status).await?;
    Ok(json!({"partner_id": partner_id, "status": status, "refs": doc.refs}))
}

/// Capabilities the partner's documentation supports (JSON object of
/// booleans; unknown keys refused). Merged over the current flags.
pub async fn set_capabilities(
    pool: &PgPool,
    operator: &str,
    partner_id: &str,
    flags: &Value,
) -> Result<Value> {
    need_operator(operator)?;
    const KNOWN: &[&str] = &[
        "validate_package",
        "prepare_transfer",
        "send_or_publish",
        "inquire_submission",
        "parse_ack",
        "get_release_status",
        "update_release",
        "takedown",
        "receive_royalty_report",
    ];
    let obj = flags
        .as_object()
        .ok_or(Error::InvalidCode("CAPABILITIES_INVALID"))?;
    for (k, v) in obj {
        if !KNOWN.contains(&k.as_str()) || !v.is_boolean() {
            return Err(Error::InvalidCode("CAPABILITIES_INVALID"));
        }
    }
    profile_exists(pool, partner_id).await?;
    let caps: Value = sqlx::query_scalar(
        "UPDATE execution.adapter_profiles SET capabilities = capabilities || $2, updated_at=now()
         WHERE partner_id=$1 RETURNING capabilities",
    )
    .bind(partner_id)
    .bind(flags)
    .fetch_one(pool)
    .await?;
    crate::partners::invalidate();
    audit(
        pool,
        operator,
        partner_id,
        "partner.capabilities",
        &flags.to_string(),
    )
    .await?;
    Ok(caps)
}

pub async fn contract(
    pool: &PgPool,
    operator: &str,
    partner_id: &str,
    reference: &str,
) -> Result<()> {
    need_operator(operator)?;
    profile_exists(pool, partner_id).await?;
    crate::partner_onboarding::record_contract(pool, partner_id, reference).await?;
    audit(
        pool,
        operator,
        partner_id,
        "partner.contract",
        reference.trim(),
    )
    .await
}

/// Stage LIVE + delivery_enabled. Refused while any onboarding gap
/// remains, without a config file, or when the profile cannot send.
pub async fn go_live(pool: &PgPool, operator: &str, partner_id: &str) -> Result<Value> {
    need_operator(operator)?;
    profile_exists(pool, partner_id).await?;
    let c = load(partner_id)?;
    resolve_secrets(&c)?;
    let caps: Value = sqlx::query_scalar(
        "SELECT capabilities FROM execution.adapter_profiles WHERE partner_id=$1",
    )
    .bind(partner_id)
    .fetch_one(pool)
    .await?;
    if !Capabilities::from_json(&caps).send_or_publish {
        return Err(Error::PolicyGate("PARTNER_CANNOT_SEND"));
    }
    crate::partner_onboarding::set_stage(pool, partner_id, "LIVE").await?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE execution.adapter_profiles SET delivery_enabled=true, updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::partners::invalidate();
    audit(pool, operator, partner_id, "partner.go_live", "").await?;
    let live: bool = sqlx::query_scalar("SELECT execution.platform_contract_live($1)")
        .bind(partner_id)
        .fetch_one(pool)
        .await?;
    Ok(
        json!({"partner_id": partner_id, "stage": "LIVE", "delivery_enabled": true,
               "platform_contract_live": live,
               // Before the official launch nothing is routed or sent even so.
               "live_transmission": crate::launch::live_transmission_enabled()}),
    )
}

/// Kill switch: stop new sends now (in-flight attempts finish normally).
pub async fn suspend(pool: &PgPool, operator: &str, partner_id: &str, reason: &str) -> Result<()> {
    need_operator(operator)?;
    if reason.trim().is_empty() {
        return Err(Error::InvalidCode("REASON_REQUIRED"));
    }
    profile_exists(pool, partner_id).await?;
    sqlx::query(
        "UPDATE execution.adapter_profiles SET delivery_enabled=false, updated_at=now() WHERE partner_id=$1",
    )
    .bind(partner_id)
    .execute(pool)
    .await?;
    crate::partner_onboarding::set_stage(pool, partner_id, "TECHNICAL").await?;
    crate::partners::invalidate();
    audit(pool, operator, partner_id, "partner.suspend", reason.trim()).await
}

/// Choose a DSP's contract route: `DIRECT` (AUDENIQ's own contract with the
/// DSP) or `MERLIN` (the Merlin agreement; only for DSPs with a Merlin deal).
/// The onboarding contract requirement follows the choice (migration 0056).
pub async fn set_route(pool: &PgPool, operator: &str, code: &str, route: &str) -> Result<Value> {
    need_operator(operator)?;
    let dsp = crate::dsp_registry::Dsp::from_code(code).ok_or(Error::NotFound)?;
    let route = route.trim().to_ascii_uppercase();
    if !matches!(route.as_str(), "DIRECT" | "MERLIN") {
        return Err(Error::InvalidCode("ROUTE_UNKNOWN"));
    }
    let eligible: bool = sqlx::query_scalar(
        "SELECT merlin_eligible FROM distribution.dsp_contract_routes WHERE code=$1",
    )
    .bind(code)
    .fetch_one(pool)
    .await?;
    if route == "MERLIN" && !eligible {
        return Err(Error::PolicyGate("MERLIN_NOT_AVAILABLE_FOR_DSP"));
    }
    sqlx::query(
        "UPDATE distribution.dsp_contract_routes SET route=$2, updated_by=$3, updated_at=now() WHERE code=$1",
    )
    .bind(code)
    .bind(&route)
    .bind(operator.trim())
    .execute(pool)
    .await?;
    crate::partners::invalidate();
    audit(pool, operator, code, "partner.route", &route).await?;
    Ok(json!({"dsp": code, "platform": dsp.display_name(), "route": route}))
}

/// Mark whether a DSP has a Merlin deal (from Merlin's current deal list).
/// Turning it off while the DSP is on the Merlin route is refused.
pub async fn set_merlin_eligible(
    pool: &PgPool,
    operator: &str,
    code: &str,
    eligible: bool,
) -> Result<()> {
    need_operator(operator)?;
    crate::dsp_registry::Dsp::from_code(code).ok_or(Error::NotFound)?;
    let n = sqlx::query(
        "UPDATE distribution.dsp_contract_routes SET merlin_eligible=$2, updated_by=$3, updated_at=now()
         WHERE code=$1 AND ($2 OR route='DIRECT')",
    )
    .bind(code)
    .bind(eligible)
    .bind(operator.trim())
    .execute(pool)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("DSP_ON_MERLIN_ROUTE"));
    }
    audit(
        pool,
        operator,
        code,
        "partner.merlin_eligible",
        &eligible.to_string(),
    )
    .await
}
