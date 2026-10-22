//! Staff operations in the admin console: what used to be only in the
//! operator CLI (`audeniq-admin`), behind staff levels.
//!
//! - Levels: SUPPORT 1 < REVIEWER, OPERATOR 2 < ADMIN 3 < OWNER 4
//!   ([`StaffRole::level`]). Money needs the Finance duty (OPERATOR, ADMIN,
//!   OWNER), as the CLI did. [`OPS`] lists each operation's level.
//! - Step-up: staff changes, money, identifier ranges, partner go-live and new
//!   AI agents need a fresh authenticator code on the request
//!   ([`staff_mfa::step_up`]).
//! - The work runs through the same library functions as the CLI, on a
//!   separate login (`AUDENIQ_STAFF_OPS_DATABASE_URL`, role
//!   `audeniq_staff_ops` in deploy/grants.sql). Without it every operation
//!   answers 503 STAFF_OPS_UNAVAILABLE and the CLI stays the only way.
//!   Staff changes go through `identity.staff_set` / `staff_reset_mfa`
//!   (0146), which re-check in the database that the actor is an OWNER and
//!   not changing themselves or another OWNER.
//!
//! `GET /api/staff/ops` (what this staff member may do),
//! `GET /api/staff/ops/{area}/{op}` (reads, arguments in the query string),
//! `POST /api/staff/ops/{area}/{op}` (changes, JSON arguments, `code` for
//! step-up). Same session, MFA, CSRF and access-log rules as /api/staff.
use crate::{
    api::AppState,
    error::Error,
    identifiers::{self, IdentifierKind},
    mcp, partner_admin, partner_onboarding,
    payout_keys::KeyRing,
    pii, protected_admin,
    protected_names::{Action, Mode},
    staff::{self, Duty, Staff, StaffRole},
    staff_admin, staff_mfa,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

/// The staff operations login, when configured. Lazy: the API starts (and
/// answers 503 for these operations) even while that login is unreachable.
pub fn pool_from_env() -> crate::error::Result<Option<PgPool>> {
    match std::env::var("AUDENIQ_STAFF_OPS_DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => Ok(Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .acquire_timeout(std::time::Duration::from_secs(5))
                .connect_lazy(url.trim())?,
        )),
        _ => Ok(None),
    }
}

/// Who may run an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Level(u8),
    Finance,
}

#[derive(Debug, Clone, Copy)]
pub struct Op {
    pub area: &'static str,
    pub name: &'static str,
    pub need: Need,
    /// A change (POST); reads are GET.
    pub write: bool,
    /// Needs a fresh authenticator code.
    pub step_up: bool,
}

const fn op(area: &'static str, name: &'static str, need: Need, write: bool, step_up: bool) -> Op {
    Op {
        area,
        name,
        need,
        write,
        step_up,
    }
}
const L2: Need = Need::Level(2);
const L3: Need = Need::Level(3);
const L4: Need = Need::Level(4);
const FIN: Need = Need::Finance;

/// Every console operation, mirroring `audeniq-admin`. Deployment and
/// infrastructure commands (sandbox check, antivirus update, storage probe,
/// privacy purge, fingerprint backfill, seals, external recordings) stay in
/// the CLI.
pub const OPS: &[Op] = &[
    op("staff", "list", L3, false, false),
    op("staff", "grant", L4, true, true),
    op("staff", "revoke", L4, true, true),
    op("staff", "reset-mfa", L4, true, true),
    op("protected", "list", L2, false, false),
    op("protected", "add", L3, true, false),
    op("protected", "activate", L3, true, false),
    op("protected", "deactivate", L3, true, false),
    op("protected", "alias", L3, true, false),
    op("protected", "remove-alias", L3, true, false),
    op("protected", "grant-exception", L3, true, false),
    op("protected", "revoke-exception", L3, true, false),
    op("developer-apps", "list", L3, false, false),
    op("developer-apps", "approve", L3, true, false),
    op("developer-apps", "return", L3, true, false),
    op("ai-clients", "list", L3, false, false),
    op("ai-clients", "add", L3, true, true),
    op("ai-clients", "disable", L3, true, false),
    op("ai-limits", "list", L2, false, false),
    op("ai-limits", "set", L2, true, false),
    op("ai-limits", "clear", L2, true, false),
    op("partners", "list", L2, false, false),
    op("partners", "status", L2, false, false),
    op("partners", "config-check", L3, false, false),
    op("partners", "set-dsp", L3, true, false),
    op("partners", "probe", L3, true, false),
    op("partners", "dpid", L3, true, false),
    op("partners", "test-ern", L3, true, false),
    op("partners", "test-ack", L3, true, false),
    op("partners", "capabilities", L3, true, false),
    op("partners", "contract", L3, true, false),
    op("partners", "route", L3, true, false),
    op("partners", "merlin-eligible", L3, true, false),
    op("partners", "suspend", L3, true, false),
    op("partners", "go-live", L4, true, true),
    op("identifiers", "list", L3, false, false),
    op("identifiers", "register", L4, true, true),
    op("payouts", "list", FIN, false, false),
    op("payouts", "order", FIN, true, false),
    op("payouts", "approve", FIN, true, true),
    op("payouts", "submit", FIN, true, true),
    op("payouts", "result", FIN, true, true),
    op("payouts", "cancel", FIN, true, false),
    op("payouts", "manual", FIN, true, true),
    op("payouts", "reissue", FIN, true, true),
    op("payouts", "return", FIN, true, true),
    op("royalty", "show", FIN, false, false),
    op("royalty", "import", FIN, true, false),
    op("royalty", "match", FIN, true, false),
    op("royalty", "receipt", FIN, true, true),
    op("royalty", "post", FIN, true, true),
];

pub fn find(area: &str, name: &str) -> Option<&'static Op> {
    OPS.iter().find(|o| o.area == area && o.name == name)
}

impl Need {
    pub fn allows(self, role: StaffRole) -> bool {
        match self {
            Need::Level(n) => role.level() >= n,
            Need::Finance => role.may(Duty::Finance),
        }
    }
}

/// Why an operation did not run.
#[derive(Debug)]
pub enum OpsError {
    Api(Error),
    /// The staff operations login is not configured.
    Unavailable,
    /// The staff member's level is too low.
    Level(Need),
    /// A missing or malformed argument.
    Argument(&'static str),
    /// A rule the operation itself enforces, with its own code.
    Code(&'static str),
    /// A refusal from a CLI routine, which explains itself in English.
    Rejected(String),
}
impl From<Error> for OpsError {
    fn from(e: Error) -> Self {
        OpsError::Api(e)
    }
}
impl From<sqlx::Error> for OpsError {
    fn from(e: sqlx::Error) -> Self {
        // The staff functions (0146) raise their rule by name.
        if let sqlx::Error::Database(d) = &e {
            for code in [
                "STAFF_OWNER_REQUIRED",
                "STAFF_SELF_CHANGE",
                "STAFF_OWNER_PROTECTED",
                "STAFF_ROLE_INVALID",
                "STAFF_NOT_FOUND",
            ] {
                if d.message() == code {
                    return OpsError::Code(code);
                }
            }
        }
        OpsError::Api(Error::Database(e))
    }
}
impl From<anyhow::Error> for OpsError {
    fn from(e: anyhow::Error) -> Self {
        let e = match e.downcast::<Error>() {
            Ok(e) => return OpsError::Api(e),
            Err(e) => e,
        };
        match e.downcast::<sqlx::Error>() {
            Ok(e) => e.into(),
            Err(e) => OpsError::Rejected(e.to_string()),
        }
    }
}
type OpsResult<T> = std::result::Result<T, OpsError>;

fn error_body(status: StatusCode, code: &str, message: &str, details: Option<Value>) -> Response {
    let mut error = json!({"code": code, "message": message});
    if let Some(d) = details {
        error["details"] = d;
    }
    (status, Json(json!({ "error": error }))).into_response()
}

impl IntoResponse for OpsError {
    fn into_response(self) -> Response {
        match self {
            OpsError::Api(e) => e.into_response(),
            OpsError::Unavailable => error_body(
                StatusCode::SERVICE_UNAVAILABLE,
                "STAFF_OPS_UNAVAILABLE",
                "Staff operations are not configured on this server; use the operator CLI.",
                None,
            ),
            OpsError::Level(need) => error_body(
                StatusCode::FORBIDDEN,
                "STAFF_LEVEL_REQUIRED",
                "Your staff level cannot do this.",
                Some(match need {
                    Need::Level(n) => json!({"level": n}),
                    Need::Finance => json!({"duty": "FINANCE"}),
                }),
            ),
            OpsError::Argument(field) => error_body(
                StatusCode::BAD_REQUEST,
                "OPS_ARGUMENT_INVALID",
                "An argument is missing or malformed.",
                Some(json!({ "field": field })),
            ),
            OpsError::Code(code) => {
                let (status, message) = match code {
                    "ACCOUNT_NOT_FOUND" => (StatusCode::NOT_FOUND, "No account has this email."),
                    "STAFF_NOT_FOUND" => {
                        (StatusCode::NOT_FOUND, "This person is not active staff.")
                    }
                    "STAFF_ROLE_INVALID" => (
                        StatusCode::BAD_REQUEST,
                        "The console grants ADMIN, REVIEWER, OPERATOR or SUPPORT.",
                    ),
                    "STAFF_SELF_CHANGE" => (
                        StatusCode::FORBIDDEN,
                        "Nobody changes their own staff access.",
                    ),
                    "STAFF_OWNER_PROTECTED" => (
                        StatusCode::FORBIDDEN,
                        "Another OWNER is changed only with the operator CLI.",
                    ),
                    _ => (StatusCode::FORBIDDEN, "Only an OWNER can change staff."),
                };
                error_body(status, code, message, None)
            }
            OpsError::Rejected(message) => error_body(
                StatusCode::UNPROCESSABLE_ENTITY,
                "OPS_REJECTED",
                &message,
                None,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

struct Args<'a>(&'a Map<String, Value>);
impl Args<'_> {
    fn text(&self, k: &'static str) -> OpsResult<&str> {
        self.opt(k).ok_or(OpsError::Argument(k))
    }
    fn opt(&self, k: &str) -> Option<&str> {
        self.0
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
    }
    fn uuid(&self, k: &'static str) -> OpsResult<Uuid> {
        Uuid::parse_str(self.text(k)?).map_err(|_| OpsError::Argument(k))
    }
    fn flag(&self, k: &str) -> bool {
        match self.0.get(k) {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => s == "true",
            _ => false,
        }
    }
    fn decimal(&self, k: &'static str) -> OpsResult<Decimal> {
        let v = match self.0.get(k) {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return Err(OpsError::Argument(k)),
        };
        v.parse().map_err(|_| OpsError::Argument(k))
    }
    fn date(&self, k: &'static str) -> OpsResult<chrono::NaiveDate> {
        self.text(k)?.parse().map_err(|_| OpsError::Argument(k))
    }
    fn object(&self, k: &'static str) -> OpsResult<Value> {
        match self.0.get(k) {
            Some(v @ Value::Object(_)) => Ok(v.clone()),
            _ => Err(OpsError::Argument(k)),
        }
    }
}

async fn user_by_email(pool: &PgPool, keys: &KeyRing, email: &str) -> OpsResult<Uuid> {
    pii::user_by_email(keys, pool, email)
        .await?
        .ok_or(OpsError::Code("ACCOUNT_NOT_FOUND"))
}

async fn org_of_order(pool: &PgPool, order: Uuid) -> OpsResult<Uuid> {
    sqlx::query_scalar("SELECT org_id FROM finance.payout_orders WHERE id=$1")
        .bind(order)
        .fetch_optional(pool)
        .await?
        .ok_or(OpsError::Api(Error::NotFound))
}

/// Adds the account email next to a user id in each of `items` (the CLI
/// lists ids; the console shows people).
async fn with_emails(
    pool: &PgPool,
    keys: &KeyRing,
    v: &mut Value,
    id_field: &str,
    email_field: &str,
) -> OpsResult<()> {
    if let Some(items) = v["items"].as_array_mut() {
        for item in items {
            if let Some(id) = item[id_field]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
            {
                item[email_field] = json!(pii::user_email(keys, pool, id).await.ok());
            }
        }
    }
    Ok(())
}

const OK: fn() -> Value = || json!({"ok": true});

// ---------------------------------------------------------------------------
// Shared with the CLI
// ---------------------------------------------------------------------------

/// Payout requests in one status with their order (the CLI's `payout list`).
pub async fn payout_list(
    pool: &PgPool,
    keys: &KeyRing,
    status: &str,
) -> crate::error::Result<Value> {
    let mut rows = Value::Array(
        sqlx::query_scalar(concat!(
            "SELECT jsonb_build_object('request_id',p.id,'org_id',p.org_id,'amount',p.amount::text,'fee',p.fee_amount::text,
               'net',p.net_amount::text,'status',p.status,
               'account',p.account_snapshot - 'holder_name' || jsonb_build_object('holder_name',",
            crate::pii_ref!(
                "holder_name",
                "p.org_id",
                "p.account_snapshot->>'holder_name'",
                "p.holder_cipher"
            ),
            "),'created_at',p.created_at,
               'order_id',o.id,'order_status',o.status)
             FROM portal.payout_requests p LEFT JOIN finance.payout_orders o ON o.id=p.payout_order_id
             WHERE p.status=$1 ORDER BY p.created_at LIMIT 500"
        ))
        .bind(status.to_ascii_uppercase())
        .fetch_all(pool)
        .await?,
    );
    pii::reveal(keys, &mut rows)?;
    Ok(rows)
}

/// One royalty report's totals (the CLI's `royalty show`).
pub async fn royalty_show(pool: &PgPool, org: Uuid, report: Uuid) -> crate::error::Result<Value> {
    let value: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('report_id',r.id,'dsp_id',r.dsp_id,'status',r.status,
           'currency',r.currency,'source_hash',r.source_hash,
           'source_amount',(SELECT COALESCE(SUM(gross_amount),0)::text FROM finance.report_lines WHERE org_id=$1 AND report_id=r.id),
           'received_amount',(SELECT COALESCE(SUM(amount),0)::text FROM finance.royalty_receipts WHERE org_id=$1 AND report_id=r.id),
           'unresolved',(SELECT COUNT(*) FROM finance.report_lines WHERE org_id=$1 AND report_id=r.id AND match_status NOT IN ('AUTO','MANUAL')))
         FROM finance.royalty_reports r WHERE r.org_id=$1 AND r.id=$2",
    )
    .bind(org)
    .bind(report)
    .fetch_optional(pool)
    .await?;
    value.ok_or(Error::NotFound)
}

// ---------------------------------------------------------------------------
// Running an operation
// ---------------------------------------------------------------------------

/// Runs one operation for an authorized staff member (level and step-up
/// already checked). `operator` is the name the CLI routines record.
pub async fn run(
    pool: &PgPool,
    keys: &KeyRing,
    actor: Uuid,
    operator: &str,
    op: &Op,
    args: &Map<String, Value>,
) -> OpsResult<Value> {
    let a = Args(args);
    Ok(match (op.area, op.name) {
        ("staff", "list") => staff_admin::list_with(pool, keys).await?,
        ("staff", "grant") => {
            let target = user_by_email(pool, keys, a.text("email")?).await?;
            let role = a.text("role")?.to_ascii_uppercase();
            let mut tx = pool.begin().await?;
            let previous: Option<String> =
                sqlx::query_scalar("SELECT identity.staff_set($1,$2,$3,$4)")
                    .bind(actor)
                    .bind(target)
                    .bind(&role)
                    .bind(operator)
                    .fetch_one(&mut *tx)
                    .await?;
            crate::operations::audit(
                &mut tx,
                Some(actor),
                None,
                Some(target),
                "staff.granted",
                &format!("OPERATOR:{operator} {role}"),
                Uuid::new_v4(),
            )
            .await?;
            tx.commit().await?;
            json!({"user_id": target, "role": role, "previous": previous})
        }
        ("staff", "revoke") => {
            let target = a.uuid("user_id")?;
            let mut tx = pool.begin().await?;
            sqlx::query("SELECT identity.staff_set($1,$2,NULL,$3)")
                .bind(actor)
                .bind(target)
                .bind(operator)
                .execute(&mut *tx)
                .await?;
            crate::operations::audit(
                &mut tx,
                Some(actor),
                None,
                Some(target),
                "staff.revoked",
                &format!("OPERATOR:{operator}"),
                Uuid::new_v4(),
            )
            .await?;
            tx.commit().await?;
            OK()
        }
        ("staff", "reset-mfa") => {
            let target = a.uuid("user_id")?;
            let mut tx = pool.begin().await?;
            sqlx::query("SELECT identity.staff_reset_mfa($1,$2)")
                .bind(actor)
                .bind(target)
                .execute(&mut *tx)
                .await?;
            crate::operations::audit(
                &mut tx,
                Some(actor),
                None,
                Some(target),
                "staff.mfa.reset",
                &format!("OPERATOR:{operator}"),
                Uuid::new_v4(),
            )
            .await?;
            tx.commit().await?;
            OK()
        }

        ("protected", "list") => protected_admin::list(pool).await?,
        ("protected", "add") => {
            let id = protected_admin::add(
                pool,
                operator,
                a.text("name")?,
                Mode::parse(a.opt("mode").unwrap_or("CONTAINS")),
                Action::parse(a.opt("action").unwrap_or("BLOCK")),
                a.opt("note"),
            )
            .await?;
            json!({ "id": id })
        }
        ("protected", name @ ("activate" | "deactivate")) => {
            protected_admin::set_active(pool, operator, a.text("name")?, name == "activate")
                .await?;
            OK()
        }
        ("protected", "alias") => {
            protected_admin::add_alias(
                pool,
                operator,
                a.text("name")?,
                a.text("alias")?,
                a.flag("phrase"),
                Mode::parse(a.opt("mode").unwrap_or("CONTAINS")),
                Action::parse(a.opt("action").unwrap_or("BLOCK")),
            )
            .await?;
            OK()
        }
        ("protected", "remove-alias") => {
            protected_admin::remove_alias(pool, operator, a.text("name")?, a.text("alias")?)
                .await?;
            OK()
        }
        ("protected", "grant-exception") => {
            protected_admin::grant_exception(
                pool,
                operator,
                a.text("name")?,
                a.uuid("org_id")?,
                a.text("reason")?,
            )
            .await?;
            OK()
        }
        ("protected", "revoke-exception") => {
            protected_admin::revoke_exception(pool, operator, a.text("name")?, a.uuid("org_id")?)
                .await?;
            OK()
        }

        ("developer-apps", "list") => {
            let status = a.opt("status").map(str::to_ascii_uppercase);
            if let Some(v) = &status
                && !matches!(v.as_str(), "PRIVATE" | "REVIEW" | "PUBLIC")
            {
                return Err(OpsError::Argument("status"));
            }
            let mut v = mcp::developer::admin_list(pool, status.as_deref()).await?;
            with_emails(pool, keys, &mut v, "owner_user_id", "owner_email").await?;
            v
        }
        ("developer-apps", name @ ("approve" | "return")) => {
            let note = a.opt("note");
            if name == "return" && note.is_none() {
                return Err(OpsError::Argument("note"));
            }
            let to = mcp::developer::review(
                pool,
                operator,
                a.text("client_id")?,
                name == "approve",
                note,
            )
            .await?;
            json!({"client_id": a.text("client_id")?, "visibility": to})
        }

        ("ai-clients", "list") => mcp::login::admin_list_clients(pool).await?,
        ("ai-clients", "add") => {
            let redirects: Vec<String> = match args.get("redirect_uris") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(v)) => v
                    .iter()
                    .map(|u| u.as_str().map(str::to_string))
                    .collect::<Option<_>>()
                    .ok_or(OpsError::Argument("redirect_uris"))?,
                _ => return Err(OpsError::Argument("redirect_uris")),
            };
            let (client_id, secret) = mcp::login::admin_add_client(
                pool,
                operator,
                a.text("name")?,
                a.flag("password_grant"),
                a.flag("secret_post"),
                &redirects,
            )
            .await?;
            json!({"client_id": client_id, "client_secret": secret})
        }
        ("ai-clients", "disable") => {
            mcp::login::admin_disable_client(pool, operator, a.text("client_id")?).await?;
            OK()
        }

        ("ai-limits", "list") => {
            let mut v = mcp::public::admin_list_limits(pool).await?;
            with_emails(pool, keys, &mut v, "user_id", "email").await?;
            v
        }
        ("ai-limits", "set") => {
            let user = user_by_email(pool, keys, a.text("email")?).await?;
            let limit = match args.get("limit") {
                Some(Value::Number(n)) => n.as_i64(),
                Some(Value::String(s)) => s.trim().parse().ok(),
                _ => None,
            }
            .and_then(|n| i32::try_from(n).ok())
            .ok_or(OpsError::Argument("limit"))?;
            mcp::public::admin_set_limit(pool, operator, user, Some(limit), a.opt("note")).await?;
            OK()
        }
        ("ai-limits", "clear") => {
            let user = user_by_email(pool, keys, a.text("email")?).await?;
            mcp::public::admin_set_limit(pool, operator, user, None, None).await?;
            OK()
        }

        ("partners", "list") => partner_onboarding::list_profiles(pool).await?,
        ("partners", "status") => {
            serde_json::to_value(partner_onboarding::status(pool, a.text("partner_id")?).await?)
                .map_err(|_| Error::Internal)?
        }
        ("partners", "config-check") => partner_admin::check_configs(),
        ("partners", "set-dsp") => {
            let dsp = a
                .opt("dsp_id")
                .map(Uuid::parse_str)
                .transpose()
                .map_err(|_| OpsError::Argument("dsp_id"))?;
            let id =
                partner_onboarding::set_dsp(pool, operator, a.text("partner_id")?, dsp).await?;
            json!({ "dsp_id": id })
        }
        ("partners", "probe") => {
            partner_admin::probe(pool, operator, a.text("partner_id")?).await?
        }
        ("partners", "dpid") => {
            partner_admin::set_recipient_dpid(
                pool,
                operator,
                a.text("partner_id")?,
                a.text("dpid")?,
            )
            .await?;
            OK()
        }
        ("partners", "test-ern") => {
            partner_admin::test_ern(pool, operator, a.text("partner_id")?, a.text("xml")?).await?
        }
        ("partners", "test-ack") => {
            partner_admin::test_ack(
                pool,
                operator,
                a.text("partner_id")?,
                a.text("content")?.as_bytes(),
            )
            .await?
        }
        ("partners", "capabilities") => {
            partner_admin::set_capabilities(
                pool,
                operator,
                a.text("partner_id")?,
                &a.object("flags")?,
            )
            .await?
        }
        ("partners", "contract") => {
            partner_admin::contract(pool, operator, a.text("partner_id")?, a.text("reference")?)
                .await?;
            OK()
        }
        ("partners", "route") => {
            partner_admin::set_route(pool, operator, a.text("dsp_code")?, a.text("route")?).await?
        }
        ("partners", "merlin-eligible") => {
            partner_admin::set_merlin_eligible(
                pool,
                operator,
                a.text("dsp_code")?,
                a.flag("eligible"),
            )
            .await?;
            OK()
        }
        ("partners", "suspend") => {
            partner_admin::suspend(pool, operator, a.text("partner_id")?, a.text("reason")?)
                .await?;
            OK()
        }
        ("partners", "go-live") => {
            partner_admin::go_live(pool, operator, a.text("partner_id")?).await?
        }

        ("identifiers", "list") => identifiers::list_issuers(pool).await?,
        ("identifiers", "register") => {
            let kind =
                IdentifierKind::parse(a.text("kind")?).map_err(|_| OpsError::Argument("kind"))?;
            let id = identifiers::register_issuer(pool, operator, kind, a.text("prefix")?).await?;
            json!({ "issuer_id": id })
        }

        ("payouts", "list") => {
            payout_list(pool, keys, a.opt("status").unwrap_or("REQUESTED")).await?
        }
        ("payouts", "order") => {
            let request = a.uuid("request_id")?;
            let org: Uuid =
                sqlx::query_scalar("SELECT org_id FROM portal.payout_requests WHERE id=$1")
                    .bind(request)
                    .fetch_optional(pool)
                    .await?
                    .ok_or(OpsError::Api(Error::NotFound))?;
            let id = crate::finance::order_payout_request(pool, org, request, actor).await?;
            json!({ "order_id": id })
        }
        ("payouts", "approve") => {
            let id = a.uuid("order_id")?;
            crate::finance::approve_payout_order(pool, org_of_order(pool, id).await?, actor, id)
                .await?;
            json!({"order_id": id, "status": "APPROVED"})
        }
        ("payouts", "submit") => {
            let id = a.uuid("order_id")?;
            crate::finance::mark_payout_submitted(
                pool,
                org_of_order(pool, id).await?,
                actor,
                id,
                a.text("bank_tx")?,
            )
            .await?;
            json!({"order_id": id, "status": "SUBMITTED"})
        }
        ("payouts", "result") => {
            let id = a.uuid("order_id")?;
            let outcome = match a.text("outcome")?.to_ascii_lowercase().as_str() {
                "settled" => crate::finance::BankOutcome::Settled,
                "failed" => crate::finance::BankOutcome::Failed,
                "returned" => crate::finance::BankOutcome::Returned,
                "unknown" => crate::finance::BankOutcome::Unknown,
                _ => return Err(OpsError::Argument("outcome")),
            };
            let org = org_of_order(pool, id).await?;
            crate::finance::record_bank_result(
                pool,
                org,
                actor,
                id,
                a.opt("bank_tx"),
                outcome,
                json!({ "recorded_by": operator }),
            )
            .await?;
            json!({"order_id": id, "status": crate::finance::get_payout_status(pool, org, id).await?})
        }
        ("payouts", "cancel") => {
            let id = a.uuid("order_id")?;
            crate::finance::cancel_payout_order(pool, org_of_order(pool, id).await?, id).await?;
            json!({"order_id": id, "status": "CANCELLED"})
        }
        ("payouts", "manual") => {
            let id = a.uuid("order_id")?;
            crate::finance::record_manual_payout(
                pool,
                org_of_order(pool, id).await?,
                actor,
                id,
                a.text("bank_tx")?,
                a.object("evidence")?,
            )
            .await?;
            json!({"order_id": id, "status": "SETTLED", "manual": true})
        }
        ("payouts", "reissue") => {
            let id = a.uuid("order_id")?;
            let new =
                crate::finance::reissue_payout(pool, org_of_order(pool, id).await?, actor, id)
                    .await?;
            json!({"reissue_of": id, "order_id": new, "status": "PENDING_APPROVAL"})
        }
        ("payouts", "return") => {
            let id = a.uuid("order_id")?;
            let return_id = crate::finance::record_payout_return(
                pool,
                org_of_order(pool, id).await?,
                actor,
                id,
                crate::finance::PayoutReturnInput {
                    bank_reference: a.text("bank_reference")?,
                    returned_amount: a.decimal("amount")?,
                    refund_fee: a.flag("refund_fee"),
                    evidence: a.object("evidence")?,
                },
            )
            .await?;
            json!({"order_id": id, "return_id": return_id})
        }

        ("royalty", "show") => royalty_show(pool, a.uuid("org_id")?, a.uuid("report_id")?).await?,
        ("royalty", name) => {
            let org = a.uuid("org_id")?;
            let out = match name {
                "import" => {
                    let id = crate::royalty::ingest_report(
                        pool,
                        org,
                        a.text("dsp_id")?,
                        a.date("period_start")?,
                        a.date("period_end")?,
                        a.text("currency")?,
                        a.text("filename")?,
                        a.text("content")?,
                    )
                    .await?;
                    json!({"report_id": id, "status": "NORMALIZED"})
                }
                "match" => {
                    let report = a.uuid("report_id")?;
                    let (matched, unresolved) =
                        crate::royalty::match_report(pool, org, report).await?;
                    json!({"report_id": report, "newly_auto_matched": matched, "unresolved": unresolved})
                }
                "receipt" => {
                    let id = crate::royalty::record_receipt(
                        pool,
                        org,
                        a.uuid("report_id")?,
                        actor,
                        a.text("bank_reference")?,
                        a.decimal("amount")?,
                        a.text("currency")?,
                        a.object("evidence")?,
                    )
                    .await?;
                    json!({"receipt_id": id, "payable_posted": false})
                }
                "post" => {
                    crate::royalty_posting::post_report(
                        pool,
                        org,
                        a.uuid("report_id")?,
                        actor,
                        a.decimal("fx_rate")?,
                    )
                    .await?
                }
                _ => return Err(OpsError::Api(Error::NotFound)),
            };
            let mut tx = pool.begin().await?;
            crate::operations::audit(
                &mut tx,
                Some(actor),
                Some(org),
                None,
                &format!("royalty.{name}"),
                "STAFF_CONSOLE",
                Uuid::new_v4(),
            )
            .await?;
            tx.commit().await?;
            out
        }
        _ => return Err(OpsError::Api(Error::NotFound)),
    })
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

/// `GET /api/staff/ops`: whether operations are on and what this staff
/// member may run (the console hides the rest).
async fn h_index(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>, OpsError> {
    let st = staff::staff(&s, &h, false).await?;
    let ops: Vec<Value> = OPS
        .iter()
        .map(|o| {
            json!({
                "area": o.area, "op": o.name, "write": o.write, "step_up": o.step_up,
                "allowed": o.need.allows(st.role),
                "level": match o.need { Need::Level(n) => n, Need::Finance => 2 },
                "finance": o.need == Need::Finance,
            })
        })
        .collect();
    Ok(Json(json!({
        "available": s.staff_ops.is_some(),
        "role": st.role.as_str(),
        "level": st.role.level(),
        "ops": ops,
    })))
}

async fn authorize(
    s: &AppState,
    h: &HeaderMap,
    area: &str,
    name: &str,
    write: bool,
) -> OpsResult<(Staff, &'static Op)> {
    let st = staff::staff(s, h, write).await?;
    let op = find(area, name)
        .filter(|o| o.write == write)
        .ok_or(OpsError::Api(Error::NotFound))?;
    if !op.need.allows(st.role) {
        return Err(OpsError::Level(op.need));
    }
    Ok((st, op))
}

async fn execute(s: &AppState, st: &Staff, op: &Op, args: &Map<String, Value>) -> OpsResult<Value> {
    let pool = s.staff_ops.as_ref().ok_or(OpsError::Unavailable)?;
    // The name the CLI routines record next to each change.
    let operator = format!("staff:{}", st.actor.user);
    run(pool, &s.payout_keys, st.actor.user, &operator, op, args).await
}

async fn h_read(
    State(s): State<AppState>,
    Path((area, name)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
    h: HeaderMap,
) -> Result<Json<Value>, OpsError> {
    let (st, op) = authorize(&s, &h, &area, &name, false).await?;
    let args: Map<String, Value> = q.into_iter().map(|(k, v)| (k, Value::String(v))).collect();
    Ok(Json(execute(&s, &st, op, &args).await?))
}

async fn h_write(
    State(s): State<AppState>,
    Path((area, name)): Path<(String, String)>,
    h: HeaderMap,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, OpsError> {
    let (st, op) = authorize(&s, &h, &area, &name, true).await?;
    let mut args = match body.map(|Json(v)| v) {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m,
        Some(_) => return Err(OpsError::Argument("body")),
    };
    let code = args.remove("code");
    if s.staff_ops.is_none() {
        return Err(OpsError::Unavailable);
    }
    crate::auth::rate(&s.pool, &format!("staff-ops:{}", st.actor.user), 120).await?;
    if op.step_up {
        staff_mfa::step_up(&s, &st.actor, code.as_ref().and_then(Value::as_str)).await?;
    }
    Ok(Json(execute(&s, &st, op, &args).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/ops", get(h_index))
        .route("/api/staff/ops/{area}/{op}", get(h_read).post(h_write))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_is_unique_and_reads_need_no_code() {
        for (i, a) in OPS.iter().enumerate() {
            assert!(
                OPS[i + 1..]
                    .iter()
                    .all(|b| (a.area, a.name) != (b.area, b.name)),
                "{} {}",
                a.area,
                a.name
            );
            assert!(a.write || !a.step_up, "{} {}", a.area, a.name);
        }
    }

    #[test]
    fn levels_gate_operations() {
        let grant = find("staff", "grant").unwrap();
        assert!(grant.need.allows(StaffRole::Owner));
        assert!(!grant.need.allows(StaffRole::Admin));
        let add = find("protected", "add").unwrap();
        assert!(add.need.allows(StaffRole::Admin));
        assert!(!add.need.allows(StaffRole::Reviewer));
        let limit = find("ai-limits", "set").unwrap();
        assert!(limit.need.allows(StaffRole::Operator));
        assert!(!limit.need.allows(StaffRole::Support));
        let pay = find("payouts", "approve").unwrap();
        assert!(pay.need.allows(StaffRole::Operator));
        assert!(!pay.need.allows(StaffRole::Reviewer));
        assert!(pay.step_up);
        // Money, staff, identifier ranges and go-live always need a fresh code.
        for (area, name) in [
            ("staff", "grant"),
            ("staff", "revoke"),
            ("payouts", "manual"),
            ("royalty", "post"),
            ("identifiers", "register"),
            ("partners", "go-live"),
        ] {
            assert!(find(area, name).unwrap().step_up, "{area} {name}");
        }
    }
}
