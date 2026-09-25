//! F2 Pre-submit (0-A~0-D) + Stage 1 (1-A~1-D). BLUEPRINT §§3-4.
//!
//! Legal review (§23.1) is deferred: any minority path is hard-gated to
//! human review; no e-signature or legal-representative automation runs.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    fingerprint,
    identifiers::{validate_isrc, validate_upc},
    operations,
    qc::{self, CheckStatus},
    storage::ObjectStore,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, Row};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use uuid::Uuid;

/// Consent policy version for F2: adult self-consent only. Minority and
/// third-party consent automation require legal review (§23.1) and are gated.
pub const CONSENT_POLICY_VERSION: &str = "v1-self";
/// Rule version for Stage 1 field checks (1-B).
pub const FIELD_RULE_VERSION: &str = "1";

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
fn canonical(v: &Value) -> String {
    // serde_json::Map is a BTreeMap by default: keys sort, output is stable.
    serde_json::to_string(v).expect("json serializes")
}

/// Canonical scope a consent package covers: the release draft it was
/// created against. Submit re-computes this and rejects on mismatch.
/// Substantive content a consent package covers: the full revision body minus
/// the consent's own hash (which would be circular). Deliberately excludes
/// `row_version`: optimistic-locking bumps from status transitions or earlier
/// submits must not invalidate an already-given consent — otherwise no submit
/// could ever be retried.
async fn scope_of(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<Value> {
    // Declarations are a submit-time legal act, not draft content: the
    // consent scope pins them at their default so consent stays valid
    // regardless of what the submitter later declares.
    revision_body(c, org, release, "", false, &DeclarationsInput::default()).await
}

// ---------------------------------------------------------------------------
// 0-A..0-D: pre-submit gate evaluation (read-only)
// ---------------------------------------------------------------------------

/// Evaluate pre-submit gates without mutating anything.
pub async fn presubmit(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let mut gates = BTreeSet::new();
    // 0-A: account gate — session is already authenticated; check user/membership state.
    let acct = sqlx::query(
        "SELECT u.status AS user_status, m.status AS m_status, m.role FROM identity.memberships m JOIN identity.users u ON u.id=m.user_id WHERE m.org_id=$1 AND m.user_id=$2",
    )
    .bind(org)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?;
    let (account_ok, role) = match acct {
        Some(r) => {
            let us: String = r.get("user_status");
            let ms: String = r.get("m_status");
            let role: String = r.get("role");
            if us != "ACTIVE" {
                gates.insert("ACCOUNT_ON_HOLD");
            }
            if ms != "ACTIVE" {
                gates.insert("MEMBERSHIP_NOT_ACTIVE");
            }
            if role == "VIEWER" {
                gates.insert("SUBMIT_PERMISSION_REQUIRED");
            }
            (us == "ACTIVE" && ms == "ACTIVE" && role != "VIEWER", role)
        }
        None => {
            gates.insert("MEMBERSHIP_NOT_ACTIVE");
            (false, "NONE".to_string())
        }
    };
    // 0-D: upload admit — every track asset must be REGISTERED and org-bound.
    let rel = sqlx::query(
        "SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let status: String = rel.get("status");
    if status != "DRAFT" && status != "STAGE1_CORRECTION" {
        gates.insert("RELEASE_NOT_SUBMITTABLE");
    }
    let tracks = sqlx::query(
        "SELECT t.id, t.asset_id, a.state AS astate FROM catalog.tracks t LEFT JOIN catalog.assets a ON a.org_id=t.org_id AND a.id=t.asset_id WHERE t.org_id=$1 AND t.release_id=$2 AND t.archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx).await?;
    if tracks.is_empty() {
        gates.insert("TRACK_REQUIRED");
    }
    let mut assets = Vec::new();
    for t in &tracks {
        let tid: Uuid = t.get("id");
        let aid: Option<Uuid> = t.get("asset_id");
        let astate: Option<String> = t.get("astate");
        let (admitted, code) = match (aid, astate.as_deref()) {
            (None, _) => (false, Some("AUDIO_REQUIRED")),
            (Some(_), Some("REGISTERED")) => (true, None),
            _ => (false, Some("AUDIO_NOT_ADMITTED")),
        };
        if let Some(c) = code {
            gates.insert(c);
        }
        assets.push(json!({"track_id": tid, "asset_id": aid, "admitted": admitted}));
    }
    tx.rollback().await?;
    let ready = gates.is_empty();
    Ok(json!({
        "release_id": release,
        "account": {"ok": account_ok, "role": role},
        "assets": assets,
        // 0-B: no birthdate registry in F1; minority is self-declared at submit and hard-gated.
        "minority": {"path": "UNDECLARED", "policy": "minority paths require human review (legal deferred)"},
        "gates": gates.into_iter().collect::<Vec<_>>(),
        "ready_to_submit": ready,
    }))
}

// ---------------------------------------------------------------------------
// 0-C: consent capture (adult self-consent only)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentPartyInput {
    pub party_id: Uuid,
    pub role: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentInput {
    pub parties: Vec<ConsentPartyInput>,
    pub minority_declared: bool,
    pub valid_days: Option<i64>,
}

pub async fn create_consent(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    input: ConsentInput,
) -> Result<Value> {
    if input.minority_declared {
        return Err(Error::PolicyGate("MINORITY_REVIEW_REQUIRED"));
    }
    if input.parties.is_empty() || input.parties.len() > 64 {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let mut party_ids: Vec<Uuid> = input.parties.iter().map(|p| p.party_id).collect();
    party_ids.sort();
    party_ids.dedup();
    for p in &input.parties {
        if p.role.trim().is_empty() || p.role.len() > 60 {
            return Err(Error::Invalid);
        }
    }
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM identity.parties WHERE org_id=$1 AND id=ANY($2)")
            .bind(org)
            .bind(&party_ids)
            .fetch_one(&mut *tx)
            .await?;
    if n != party_ids.len() as i64 {
        return Err(Error::Invalid);
    }
    let scope = scope_of(&mut tx, org, release).await?;
    let scope_hash = sha256_hex(&canonical(&scope));
    let valid_days = input.valid_days.unwrap_or(365).clamp(1, 3650);
    let body = json!({
        "schema_version": 1,
        "policy_version": CONSENT_POLICY_VERSION,
        "release_id": release,
        "party_ids": party_ids,
        "party_roles": input.parties.iter().map(|p| json!({"party_id": p.party_id, "role": p.role})).collect::<Vec<_>>(),
        "minority_path": "NONE",
        "consent_doc_ids": [],
        "signature_ids": [],
        "scope": scope,
        "scope_hash": scope_hash,
        "valid_until": (chrono::Utc::now() + chrono::Duration::days(valid_days)).to_rfc3339(),
    });
    let package_hash = sha256_hex(&canonical(&body));
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO catalog.consent_packages(id, org_id, revision_id, body, package_hash, policy_version) VALUES($1,$2,NULL,$3,$4,$5)")
        .bind(id).bind(org).bind(&body).bind(&package_hash).bind(CONSENT_POLICY_VERSION)
        .execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "consent.created",
        "USER_CONSENT",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({
        "consent_id": id,
        "package_hash": package_hash,
        "scope_hash": scope_hash,
        "policy_version": CONSENT_POLICY_VERSION,
    }))
}

// ---------------------------------------------------------------------------
// 1-A: submit — immutable revision + qc job, one transaction
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitInput {
    pub consent_id: Uuid,
    pub minority_declared: bool,
    pub idempotency_key: String,
    pub declarations: DeclarationsInput,
}

/// Legally-weighted self-declarations captured immutably at submit time.
/// A declaration never auto-passes: any special-content flag routes the
/// revision to human review via `special_flags`. Lying on a declaration is
/// a terms violation with an immutable audit trail — detection of lies is a
/// human/vendor process, not something this struct claims to do.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DeclarationsInput {
    /// "I own or control all rights needed to distribute this release."
    pub rights_confirmed: bool,
    /// "I am 18+ or have legal-guardian consent to distribute."
    pub adult_confirmed: bool,
    pub is_cover: bool,
    pub is_remix: bool,
    pub contains_samples: bool,
    pub ai_involved: bool,
    pub explicit_content: bool,
}

impl DeclarationsInput {
    /// Special-content flags consumed by Stage 2 (`special_flags`).
    pub fn special_flags(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.is_cover {
            out.push("COVER");
        }
        if self.is_remix {
            out.push("REMIX");
        }
        if self.contains_samples {
            out.push("SAMPLE");
        }
        if self.ai_involved {
            out.push("AI");
        }
        if self.explicit_content {
            out.push("EXPLICIT");
        }
        out
    }
}

async fn enqueue_stage1(c: &mut PgConnection, revision_id: Uuid) -> Result<()> {
    sqlx::query(
        "INSERT INTO operations.jobs(id, queue, kind, payload, pinned_revision_id, idempotency_key) VALUES($1,'qc','stage1',$2,$3,$4) ON CONFLICT(idempotency_key) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(json!({"revision_id": revision_id}))
    .bind(revision_id)
    .bind(format!("stage1:{revision_id}"))
    .execute(&mut *c)
    .await?;
    Ok(())
}

async fn revision_body(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    consent_package_hash: &str,
    minority_declared: bool,
    declarations: &DeclarationsInput,
) -> Result<Value> {
    let r = sqlx::query(
        "SELECT id, title, release_type, draft, upc FROM catalog.releases WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *c)
    .await?;
    let tracks = sqlx::query(
        "SELECT t.id, t.title, t.version, t.disc_number, t.track_number, t.artist_id, t.isrc, t.asset_id, t.parental_advisory, a.sha256 AS asha, a.kind AS akind FROM catalog.tracks t LEFT JOIN catalog.assets a ON a.org_id=t.org_id AND a.id=t.asset_id WHERE t.org_id=$1 AND t.release_id=$2 AND t.archived_at IS NULL ORDER BY t.disc_number, t.track_number",
    )
    .bind(org).bind(release).fetch_all(&mut *c).await?;
    let mut tj = Vec::new();
    for t in &tracks {
        let tid: Uuid = t.get("id");
        let credits = sqlx::query("SELECT party_id, role FROM catalog.credits WHERE org_id=$1 AND track_id=$2 ORDER BY party_id, role")
            .bind(org).bind(tid).fetch_all(&mut *c).await?;
        tj.push(json!({
            "id": tid,
            "title": t.get::<String,_>("title"),
            "version": t.get::<String,_>("version"),
            "disc_number": t.get::<i32,_>("disc_number"),
            "track_number": t.get::<i32,_>("track_number"),
            "artist_id": t.get::<Uuid,_>("artist_id"),
            "isrc": t.get::<Option<String>,_>("isrc"),
            "asset_id": t.get::<Option<Uuid>,_>("asset_id"),
            "asset_sha256": t.get::<Option<String>,_>("asha"),
            "asset_kind": t.get::<Option<String>,_>("akind"),
            "parental_advisory": t.get::<bool,_>("parental_advisory"),
            "credits": credits.iter().map(|cr| json!({"party_id": cr.get::<Uuid,_>("party_id"), "role": cr.get::<String,_>("role")})).collect::<Vec<_>>(),
        }));
    }
    Ok(json!({
        "schema_version": 1,
        "release": {
            "id": r.get::<Uuid,_>("id"),
            "title": r.get::<String,_>("title"),
            "release_type": r.get::<String,_>("release_type"),
            "draft": r.get::<Value,_>("draft"),
            "upc": r.get::<Option<String>,_>("upc"),
        },
        "tracks": tj,
        "consent_package_hash": consent_package_hash,
        "minority_declared": minority_declared,
        "declarations": {
            "rights_confirmed": declarations.rights_confirmed,
            "adult_confirmed": declarations.adult_confirmed,
            "is_cover": declarations.is_cover,
            "is_remix": declarations.is_remix,
            "contains_samples": declarations.contains_samples,
            "ai_involved": declarations.ai_involved,
            "explicit_content": declarations.explicit_content,
        },
    }))
}

/// Resolve a repeated submit onto the revision its idempotency key created.
/// Same key + same body returns the original revision (safe retry); same key +
/// changed body is a client bug and is rejected instead of forking history.
/// The caller commits the transaction.
async fn keyed_revision_response(
    tx: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    idem_key: &str,
    body_hash: &str,
    status: &str,
) -> Result<Value> {
    let row = sqlx::query(
        "SELECT id, revision, body_hash FROM catalog.application_revisions WHERE org_id=$1 AND release_id=$2 AND idempotency_key=$3",
    )
    .bind(org)
    .bind(release)
    .bind(idem_key)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Internal)?;
    if row.get::<String, _>("body_hash") != body_hash {
        return Err(Error::PolicyGate("IDEMPOTENCY_KEY_REUSED"));
    }
    let rid: Uuid = row.get("id");
    enqueue_stage1(&mut *tx, rid).await?;
    Ok(
        json!({"revision_id": rid, "revision": row.get::<i32,_>("revision"), "status": status, "deduped": true}),
    )
}

pub async fn submit(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    input: SubmitInput,
) -> Result<Value> {
    if input.minority_declared {
        return Err(Error::PolicyGate("MINORITY_REVIEW_REQUIRED"));
    }
    if !input.declarations.rights_confirmed || !input.declarations.adult_confirmed {
        return Err(Error::PolicyGate("DECLARATION_REQUIRED"));
    }
    if input.idempotency_key.trim().is_empty() || input.idempotency_key.len() > 128 {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let rel = sqlx::query("SELECT status, current_revision_id FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR UPDATE")
        .bind(org).bind(release).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
    let status: String = rel.get("status");
    let current_revision: Option<Uuid> = rel.get("current_revision_id");
    // In-flight submissions (SUBMITTED/STAGE1_RUNNING) accept only the idempotent
    // path below; anything else is not submittable.
    if !["DRAFT", "STAGE1_CORRECTION", "SUBMITTED", "STAGE1_RUNNING"].contains(&status.as_str()) {
        return Err(Error::PolicyGate("RELEASE_NOT_SUBMITTABLE"));
    }
    // 0-A re-verified server-side: ACTIVE user + ACTIVE membership + non-viewer role.
    let role: Option<String> = sqlx::query_scalar(
        "SELECT m.role FROM identity.memberships m JOIN identity.users u ON u.id=m.user_id WHERE m.org_id=$1 AND m.user_id=$2 AND m.status='ACTIVE' AND u.status='ACTIVE'",
    )
    .bind(org).bind(a.user).fetch_optional(&mut *tx).await?;
    match role.as_deref() {
        Some("OWNER") | Some("EDITOR") => {}
        _ => return Err(Error::PolicyGate("SUBMIT_PERMISSION_REQUIRED")),
    }
    // 1-A: consent package must exist and cover the current draft scope.
    let c = sqlx::query("SELECT body, package_hash, policy_version FROM catalog.consent_packages WHERE org_id=$1 AND id=$2")
        .bind(org).bind(input.consent_id).fetch_optional(&mut *tx).await?
        .ok_or(Error::PolicyGate("CONSENT_NOT_FOUND"))?;
    if c.get::<String, _>("policy_version") != CONSENT_POLICY_VERSION {
        return Err(Error::PolicyGate("CONSENT_POLICY_MISMATCH"));
    }
    // Consent expires: a stale consent package cannot authorize a new submit.
    let consent_body: Value = c.get("body");
    let valid_until = consent_body
        .get("valid_until")
        .and_then(Value::as_str)
        .unwrap_or("");
    let expired = valid_until.is_empty()
        || chrono::DateTime::parse_from_rfc3339(valid_until)
            .map(|dt| dt < chrono::Utc::now())
            .unwrap_or(true);
    if expired {
        return Err(Error::PolicyGate("CONSENT_EXPIRED"));
    }
    let scope_hash = sha256_hex(&canonical(&scope_of(&mut tx, org, release).await?));
    let consent_scope = c
        .get::<Value, _>("body")
        .get("scope_hash")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if scope_hash != consent_scope {
        return Err(Error::PolicyGate("CONSENT_SCOPE_MISMATCH"));
    }
    let consent_package_hash: String = c.get("package_hash");
    let body = revision_body(
        &mut tx,
        org,
        release,
        &consent_package_hash,
        false,
        &input.declarations,
    )
    .await?;
    let body_hash = sha256_hex(&canonical(&body));
    let idem_key = input.idempotency_key.trim().to_string();
    // Idempotency key: the same key on this release always resolves to the
    // revision created by the first submit, so a retried request can never
    // fork the application history.
    if sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM catalog.application_revisions WHERE org_id=$1 AND release_id=$2 AND idempotency_key=$3",
    )
    .bind(org)
    .bind(release)
    .bind(&idem_key)
    .fetch_optional(&mut *tx)
    .await?
    .is_some()
    {
        let v = keyed_revision_response(&mut tx, org, release, &idem_key, &body_hash, &status).await?;
        tx.commit().await?;
        return Ok(v);
    }
    // Idempotent: the current revision already captures this exact body
    // (double-submit, or resubmit with no changes). Converge, never duplicate.
    if let Some(cur) = current_revision {
        let cur_hash: Option<String> =
            sqlx::query_scalar("SELECT body_hash FROM catalog.application_revisions WHERE id=$1")
                .bind(cur)
                .fetch_optional(&mut *tx)
                .await?;
        if cur_hash.as_deref() == Some(body_hash.as_str()) {
            enqueue_stage1(&mut tx, cur).await?;
            tx.commit().await?;
            let rev: i32 = sqlx::query_scalar(
                "SELECT revision FROM catalog.application_revisions WHERE id=$1",
            )
            .bind(cur)
            .fetch_one(&s.pool)
            .await?;
            return Ok(
                json!({"revision_id": cur, "revision": rev, "status": status, "deduped": true}),
            );
        }
    }
    if status != "DRAFT" && status != "STAGE1_CORRECTION" {
        // In flight with a *changed* body: the user must wait or correct first.
        return Err(Error::PolicyGate("RELEASE_NOT_SUBMITTABLE"));
    }
    let revision: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(revision),0)+1 FROM catalog.application_revisions WHERE release_id=$1",
    )
    .bind(release)
    .fetch_one(&mut *tx)
    .await?;
    let rid = Uuid::new_v4();
    // ON CONFLICT is the backstop for two identical submits racing past the
    // pre-check above; the loser converges onto the winner's revision.
    let inserted: Option<Uuid> = sqlx::query_scalar("INSERT INTO catalog.application_revisions(id, org_id, release_id, revision, body, body_hash, consent_package_hash, created_by, idempotency_key) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(release_id, idempotency_key) DO NOTHING RETURNING id")
        .bind(rid).bind(org).bind(release).bind(revision).bind(&body).bind(&body_hash).bind(&consent_package_hash).bind(a.user).bind(&idem_key)
        .fetch_optional(&mut *tx).await?;
    let rid = match inserted {
        Some(id) => id,
        None => {
            let v = keyed_revision_response(&mut tx, org, release, &idem_key, &body_hash, &status)
                .await?;
            tx.commit().await?;
            return Ok(v);
        }
    };
    // DRAFT/STAGE1_CORRECTION -> SUBMITTED; the worker moves it to STAGE1_RUNNING.
    sqlx::query("UPDATE catalog.releases SET status='SUBMITTED', current_revision_id=$3, row_version=row_version+1 WHERE org_id=$1 AND id=$2")
        .bind(org).bind(release).bind(rid).execute(&mut *tx).await?;
    enqueue_stage1(&mut tx, rid).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "submission.created",
        "USER_SUBMIT",
        a.request,
    )
    .await?;
    operations::event(
        &mut tx,
        org,
        release,
        "submission.created",
        &format!("submit:{rid}"),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"revision_id": rid, "revision": revision, "status": "SUBMITTED", "deduped": false}))
}

/// Current submission state: release status, latest revision, its checks.
pub async fn submission_status(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let rel = sqlx::query("SELECT status, current_revision_id FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL")
        .bind(org).bind(release).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
    let status: String = rel.get("status");
    let rev_id: Option<Uuid> = rel.get("current_revision_id");
    let (revision, checks, package, verification) = match rev_id {
        Some(rid) => {
            let rev = sqlx::query("SELECT revision, body_hash FROM catalog.application_revisions WHERE org_id=$1 AND id=$2")
                .bind(org).bind(rid).fetch_optional(&mut *tx).await?;
            let checks = sqlx::query("SELECT check_code, rule_version, status, result_hash, detail, created_at FROM operations.check_results WHERE revision_id=$1 ORDER BY created_at, check_code")
                .bind(rid).fetch_all(&mut *tx).await?;
            let pkg = sqlx::query("SELECT id, package_hash, rule_version FROM distribution.validation_packages WHERE org_id=$1 AND revision_id=$2")
                .bind(org).bind(rid).fetch_optional(&mut *tx).await?;
            let ver = sqlx::query("SELECT id, package_hash, rights_epoch, body->>'decision' AS decision, body->'approved_scope' AS approved_scope FROM distribution.verification_packages WHERE org_id=$1 AND revision_id=$2")
                .bind(org).bind(rid).fetch_optional(&mut *tx).await?;
            (
                rev.map(|r| json!({"id": rid, "revision": r.get::<i32,_>("revision"), "body_hash": r.get::<String,_>("body_hash")})),
                checks.iter().map(|c| json!({
                    "check_code": c.get::<String,_>("check_code"),
                    "rule_version": c.get::<String,_>("rule_version"),
                    "status": c.get::<String,_>("status"),
                    "result_hash": c.get::<String,_>("result_hash"),
                    "detail": c.get::<Option<String>,_>("detail"),
                })).collect::<Vec<_>>(),
                pkg.map(|p| json!({"id": p.get::<Uuid,_>("id"), "package_hash": p.get::<String,_>("package_hash"), "rule_version": p.get::<String,_>("rule_version")})),
                ver.map(|v| json!({
                    "id": v.get::<Uuid,_>("id"),
                    "package_hash": v.get::<String,_>("package_hash"),
                    "decision": v.get::<Option<String>,_>("decision"),
                    "approved_scope": v.get::<Option<Value>,_>("approved_scope"),
                    "rights_epoch": v.get::<i64,_>("rights_epoch"),
                })),
            )
        }
        None => (None, vec![], None, None),
    };
    tx.rollback().await?;
    Ok(json!({
        "release_id": release,
        "status": status,
        "revision": revision,
        "checks": checks,
        "validation_package": package,
        "verification_package": verification,
    }))
}

// ---------------------------------------------------------------------------
// 1-B..1-D: Stage 1 worker
// ---------------------------------------------------------------------------

/// One check outcome staged for persistence.
struct StagedCheck {
    check_code: &'static str,
    rule_version: &'static str,
    status: CheckStatus,
    result_hash: String,
    detail: String,
}

pub struct Stage1Summary {
    pub revision_id: Uuid,
    pub status_counts: BTreeMap<&'static str, i64>,
    pub release_status: String,
    pub validation_package_id: Option<Uuid>,
    /// Some check hit a transient failure: job must be requeued (cheap — cached checks are not re-analyzed).
    pub needs_retry: bool,
}

fn field_cache_key(check_code: &str, inputs: &str) -> String {
    qc::result_hash(
        check_code,
        FIELD_RULE_VERSION,
        "fields",
        &sha256_hex(inputs),
    )
}
fn asset_cache_key(check_code: &str, asset_sha256: &str) -> String {
    qc::result_hash(check_code, qc::QC_RULE_VERSION, asset_sha256, asset_sha256)
}
/// 1-B: pure field checks over the immutable revision body.
fn field_checks(body: &Value) -> Vec<StagedCheck> {
    let mut out = Vec::new();
    let rel = &body["release"];
    let title = rel["title"].as_str().unwrap_or("");
    let rtype = rel["release_type"].as_str().unwrap_or("");
    let draft = &rel["draft"];
    let tracks: Vec<&Value> = body["tracks"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let mut push = |check_code: &'static str, status: CheckStatus, inputs: &str, detail: String| {
        out.push(StagedCheck {
            check_code,
            rule_version: FIELD_RULE_VERSION,
            status,
            result_hash: field_cache_key(check_code, inputs),
            detail,
        });
    };
    push(
        "FIELD_TITLE_MISSING",
        if title.trim().is_empty() {
            CheckStatus::CorrectionRequired
        } else {
            CheckStatus::Pass
        },
        title,
        format!("release title present={}", !title.trim().is_empty()),
    );
    let rdate = draft
        .get("release_date")
        .and_then(Value::as_str)
        .unwrap_or("");
    push(
        "FIELD_RELEASE_DATE_MISSING",
        if rdate.is_empty() {
            CheckStatus::CorrectionRequired
        } else {
            CheckStatus::Pass
        },
        rdate,
        format!("release_date present={}", !rdate.is_empty()),
    );
    let rdate_ok = rdate.is_empty() || chrono::NaiveDate::parse_from_str(rdate, "%Y-%m-%d").is_ok();
    push(
        "FIELD_RELEASE_DATE_INVALID",
        if rdate_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        rdate,
        format!("release_date format valid={rdate_ok}"),
    );
    // UPC is optional at submit but, when supplied, must be a real UPC-A.
    // A malformed UPC currently sails through review and dies at prepare.
    let upc = rel["upc"].as_str().unwrap_or("");
    let upc_ok = upc.is_empty() || validate_upc(upc).is_ok();
    push(
        "UPC_FORMAT_INVALID",
        if upc_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        upc,
        format!("upc format valid={upc_ok}"),
    );
    // Duplicate ISRC inside one release is always a data error: two tracks
    // cannot share a recording identifier.
    let mut seen_isrc: BTreeSet<&str> = BTreeSet::new();
    let mut dup_isrc: BTreeSet<&str> = BTreeSet::new();
    for t in &tracks {
        let isrc = t["isrc"].as_str().unwrap_or("");
        if !isrc.is_empty() && !seen_isrc.insert(isrc) {
            dup_isrc.insert(isrc);
        }
    }
    push(
        "ISRC_DUPLICATE",
        if dup_isrc.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        &dup_isrc.iter().cloned().collect::<Vec<_>>().join(","),
        format!(
            "duplicate isrcs={}",
            dup_isrc.iter().cloned().collect::<Vec<_>>().join(",")
        ),
    );
    // Spotify Metadata Style Guide 8.1: each track title in a product must be
    // unique; the only exception is different versions of the same track.
    // Identical title + identical version = duplicate entry, not a version.
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut dup_titles: Vec<String> = Vec::new();
    for t in &tracks {
        let title = t["title"].as_str().unwrap_or("").trim().to_lowercase();
        let version = t["version"].as_str().unwrap_or("").trim().to_lowercase();
        let key = format!("{title}\u{1f}{version}");
        let count = seen.entry(key).or_insert(0);
        *count += 1;
        if *count == 2 {
            dup_titles.push(t["title"].as_str().unwrap_or("").to_string());
        }
    }
    push(
        "TRACK_TITLE_DUPLICATE",
        if dup_titles.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        &dup_titles.join(","),
        format!("duplicate track titles={}", dup_titles.join(",")),
    );
    // Spotify Metadata Style Guide 8.2/8.4: version information ("Radio
    // Edit", "Remaster", "Original Mix"...) belongs in the version field,
    // not the title. Flag for review; a human confirms the split.
    const VERSION_TERMS: &[&str] = &[
        "radio edit",
        "extended mix",
        "extended version",
        "original mix",
        "album version",
        "original version",
        "remaster",
        "remastered",
        "acoustic version",
        "live version",
        "instrumental version",
        "sped up",
        "slowed",
        "nightcore",
    ];
    let mut version_in_title: Vec<String> = Vec::new();
    for t in &tracks {
        let title = t["title"].as_str().unwrap_or("");
        let lower = title.to_lowercase();
        if VERSION_TERMS.iter().any(|term| lower.contains(term)) {
            version_in_title.push(title.to_string());
        }
    }
    push(
        "TRACK_TITLE_HAS_VERSION_INFO",
        if version_in_title.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::ReviewRequired
        },
        &version_in_title.join(","),
        format!(
            "titles carrying version info={}",
            version_in_title.join(",")
        ),
    );
    // Spotify Metadata Style Guide 8.9: SEO terms intended to mislead or
    // game discovery get the product removed and can trigger a strike.
    // Keyword match is a tripwire for human review, not proof of spam.
    const SEO_TERMS: &[&str] = &[
        "sleep music",
        "music for sleep",
        "music for studying",
        "study music",
        "relaxing music",
        "chill beats to",
        "8d audio",
        "432hz",
        "528hz",
    ];
    let mut seo_titles: Vec<String> = Vec::new();
    for t in &tracks {
        let title = t["title"].as_str().unwrap_or("");
        let lower = title.to_lowercase();
        if SEO_TERMS.iter().any(|term| lower.contains(term)) {
            seo_titles.push(title.to_string());
        }
    }
    push(
        "TRACK_TITLE_SEO_SPAM",
        if seo_titles.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::ReviewRequired
        },
        &seo_titles.join(","),
        format!("titles with seo terms={}", seo_titles.join(",")),
    );
    // DDEX ERN requires P-line and C-line on every release. Preparation
    // rejects a missing line; catching it here keeps the fix in the
    // artist's hands at submit time instead of failing at prepare time.
    let p_line = draft.get("p_line").and_then(Value::as_str).unwrap_or("");
    push(
        "PLINE_MISSING",
        if p_line.trim().is_empty() {
            CheckStatus::CorrectionRequired
        } else {
            CheckStatus::Pass
        },
        p_line,
        format!("p_line present={}", !p_line.trim().is_empty()),
    );
    let c_line = draft.get("c_line").and_then(Value::as_str).unwrap_or("");
    push(
        "CLINE_MISSING",
        if c_line.trim().is_empty() {
            CheckStatus::CorrectionRequired
        } else {
            CheckStatus::Pass
        },
        c_line,
        format!("c_line present={}", !c_line.trim().is_empty()),
    );
    // Deezer requires at least one composer/lyricist per track; Apple
    // rejects initials/aliases. We can only verify presence of a writing
    // credit here, not the name's authenticity — that stays human review.
    const WRITER_ROLES: &[&str] = &[
        "composer",
        "writer",
        "lyricist",
        "songwriter",
        "author",
        "작사",
        "작곡",
    ];
    for t in &tracks {
        let tid = t["id"].as_str().unwrap_or("?");
        let has_writer = t["credits"]
            .as_array()
            .map(|cs| {
                cs.iter().any(|c| {
                    let role = c["role"].as_str().unwrap_or("").to_lowercase();
                    WRITER_ROLES.iter().any(|w| role.contains(w))
                })
            })
            .unwrap_or(false);
        push(
            "TRACK_WRITER_CREDIT_MISSING",
            if has_writer {
                CheckStatus::Pass
            } else {
                CheckStatus::CorrectionRequired
            },
            &format!("{tid}:{has_writer}"),
            format!("track={tid} writer_credit={has_writer}"),
        );
    }
    // Explicit content: Spotify/Apple/Deezer require the flag (never "E" in
    // the title); domestic DSPs additionally carry a legal duty to mark
    // 19세 미만 이용 불가 (청소년보호법). Whether the content is actually
    // harmful is a human judgment — this check only makes sure the marking
    // step cannot be forgotten.
    let explicit_declared = body["declarations"]["explicit_content"]
        .as_bool()
        .unwrap_or(false);
    let explicit_tracks: Vec<String> = tracks
        .iter()
        .filter(|t| t["parental_advisory"].as_bool().unwrap_or(false))
        .map(|t| t["title"].as_str().unwrap_or("?").to_string())
        .collect();
    let explicit_any = explicit_declared || !explicit_tracks.is_empty();
    push(
        "ADULT_MARKING_REVIEW",
        if explicit_any {
            CheckStatus::ReviewRequired
        } else {
            CheckStatus::Pass
        },
        &format!(
            "declared={explicit_declared} tracks={}",
            explicit_tracks.join(",")
        ),
        if explicit_any {
            "explicit content flagged: confirm DSP explicit flag + domestic 19금 marking (청소년보호법)"
                .to_string()
        } else {
            "no explicit content declared".to_string()
        },
    );
    let n = tracks.len();
    let type_ok = match rtype {
        "SINGLE" => n == 1,
        "EP" => (2..=6).contains(&n),
        "ALBUM" => n >= 7,
        _ => false,
    };
    push(
        "FIELD_RELEASE_TYPE_MISMATCH",
        if type_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        &format!("{rtype}:{n}"),
        format!("release_type={rtype} tracks={n}"),
    );
    let mut discs: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for t in &tracks {
        discs
            .entry(t["disc_number"].as_i64().unwrap_or(0))
            .or_default()
            .push(t["track_number"].as_i64().unwrap_or(0));
    }
    let mut order_ok = true;
    let mut order_inputs = String::new();
    for (d, mut v) in discs {
        v.sort_unstable();
        order_inputs.push_str(&format!("{d}:{v:?};"));
        if v != (1..=v.len() as i64).collect::<Vec<_>>() {
            order_ok = false;
        }
    }
    push(
        "TRACK_ORDER_GAP",
        if order_ok {
            CheckStatus::Pass
        } else {
            CheckStatus::CorrectionRequired
        },
        &order_inputs,
        format!("contiguous per disc={order_ok}"),
    );
    for t in &tracks {
        let tid = t["id"].as_str().unwrap_or("?");
        let credits = t["credits"].as_array().map(Vec::len).unwrap_or(0);
        push(
            "CREDIT_MISSING",
            if credits > 0 {
                CheckStatus::Pass
            } else {
                CheckStatus::CorrectionRequired
            },
            &format!("{tid}:{credits}"),
            format!("track={tid} credits={credits}"),
        );
        let isrc = t["isrc"].as_str().unwrap_or("");
        let isrc_ok = isrc.is_empty() || validate_isrc(isrc).is_ok();
        push(
            "ISRC_FORMAT_INVALID",
            if isrc_ok {
                CheckStatus::Pass
            } else {
                CheckStatus::CorrectionRequired
            },
            &format!("{tid}:{isrc}"),
            format!("track={tid} isrc={isrc}"),
        );
        let has_asset = t["asset_id"].as_str().is_some();
        push(
            "ASSET_MISSING",
            if has_asset {
                CheckStatus::Pass
            } else {
                CheckStatus::CorrectionRequired
            },
            &format!("{tid}:{has_asset}"),
            format!("track={tid} asset_attached={has_asset}"),
        );
    }
    out
}

async fn cached_status(
    pool: &PgPool,
    check_code: &str,
    rule_version: &str,
    result_hash: &str,
) -> Result<Option<CheckStatus>> {
    let s: Option<String> = sqlx::query_scalar(
        "SELECT status FROM operations.check_results WHERE check_code=$1 AND rule_version=$2 AND result_hash=$3 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(check_code)
    .bind(rule_version)
    .bind(result_hash)
    .fetch_optional(pool)
    .await?;
    Ok(s.and_then(|v| match v.as_str() {
        "PASS" => Some(CheckStatus::Pass),
        "CORRECTION_REQUIRED" => Some(CheckStatus::CorrectionRequired),
        "REVIEW_REQUIRED" => Some(CheckStatus::ReviewRequired),
        "BLOCKED" => Some(CheckStatus::Blocked),
        "TECHNICAL_RETRY" => Some(CheckStatus::TechnicalRetry),
        "NOT_APPLICABLE" => Some(CheckStatus::NotApplicable),
        _ => None,
    }))
}

/// 1-C: file QC over the revision's assets. Bytes-unchanged assets hit the
/// change cache and are never re-analyzed (ffprobe runs 0 times for them).
/// Upper bound on bytes the QC worker will download for one asset.
/// Larger objects skip analysis (per-asset TECHNICAL_RETRY) instead of
/// OOMing the worker. Overridable for tests and small deployments.
fn qc_max_bytes() -> u64 {
    std::env::var("AUDENIQ_QC_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(512 * 1024 * 1024)
}

/// Download one asset and run the fixed check contract over it. Storage and
/// local-IO failures are returned as a detail string so the caller can record
/// per-asset TECHNICAL_RETRY instead of aborting the whole Stage 1 run.
///
/// Returns the check outcomes, the measured audio duration in seconds
/// (`None` for images or when probing fails), and — for audio whose bytes
/// passed QC — the perceptual fingerprint computation result. The caller
/// persists the duration to `catalog.assets.duration_secs` (DDEX ERN needs
/// it) and the fingerprint to `catalog.asset_fingerprints` (similarity
/// detection). Fingerprinting the wrong bytes is meaningless, so audio
/// blocked by SHA256_MISMATCH yields `None` here; its fingerprint codes
/// are already covered by check_audio's NotApplicable tail.
async fn analyze_asset(
    storage: &Arc<dyn ObjectStore>,
    key: &str,
    kind: &str,
    sha256: &str,
    tmp_name: &str,
) -> std::result::Result<
    (
        Vec<qc::CheckOutcome>,
        Option<f64>,
        Option<std::result::Result<fingerprint::Fingerprint, String>>,
    ),
    String,
> {
    let size = storage
        .head(key)
        .await
        .map_err(|_| "object store unavailable".to_string())?
        .map(|m| m.size)
        .unwrap_or(-1);
    if size < 0 {
        return Err("object not found".to_string());
    }
    if size as u64 > qc_max_bytes() {
        return Err(format!(
            "object too large for analyzer: {size} bytes > {} byte limit",
            qc_max_bytes()
        ));
    }
    let bytes = storage
        .get(key)
        .await
        .map_err(|_| "object download failed".to_string())?;
    let tmp = std::env::temp_dir().join(tmp_name);
    std::fs::write(&tmp, &bytes).map_err(|_| "temp file write failed".to_string())?;
    // RAII guard: the temp file is removed on drop, even if a QC analyzer
    // panics. Prevents /tmp (512MB tmpfs) from filling up under parallel load.
    struct TempFile<'a>(&'a std::path::Path);
    impl Drop for TempFile<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }
    let _tmp_guard = TempFile(&tmp);
    let outcomes = match kind {
        "AUDIO" => qc::check_audio(&tmp, Some(sha256)),
        "IMAGE" => qc::check_image(&tmp, Some(sha256)),
        _ => Vec::new(),
    };
    // Duration is measured with a second ffprobe pass rather than parsed
    // out of check outcomes: the check contract is fixed and must not grow
    // a side channel. ~100ms on a local file, submit path only.
    let duration_secs = match kind {
        "AUDIO" => qc::probe_duration_secs(&tmp),
        _ => None,
    };
    // Perceptual fingerprint for similarity detection. Only for audio whose
    // bytes were admitted: SHA-256 still guards integrity (exact bytes),
    // the fingerprint adds similarity (same recording, different bytes).
    // Skip when QC already rejected the bytes (Blocked on SHA mismatch, or
    // CorrectionRequired on magic mismatch): fingerprinting invalid audio
    // is meaningless, and a decode failure there is permanent, not
    // transient — it must not trigger TechnicalRetry.
    let invalid = outcomes.iter().any(|o| {
        matches!(
            o.status,
            CheckStatus::Blocked | CheckStatus::CorrectionRequired
        )
    });
    let fp = match kind {
        "AUDIO" if !invalid => {
            Some(fingerprint::compute_fingerprint(&tmp).map_err(|e| format!("{e:?}")))
        }
        _ => None,
    };
    Ok((outcomes, duration_secs, fp))
}

/// Produce the two DB-backed fingerprint check outcomes for an analyzed
/// audio asset.
///
/// - `AUDIO_FINGERPRINT_FAILED`: Pass when the fingerprint computed;
///   TechnicalRetry on decode failure (transient); NotApplicable when the
///   audio is too short for a meaningful fingerprint (retrying the same
///   bytes cannot help) or when QC blocked the bytes (tail already
///   covered the code).
/// - `AUDIO_SIMILAR_TO_EXISTING`: compares the new fingerprint against
///   every other fingerprint in the org at the same algorithm version.
///   A BER at or below `SIMILAR_BER` is REVIEW_REQUIRED — similarity is a
///   human judgement, never an auto-block. Byte-identical re-uploads score
///   BER 0 and are flagged here; SHA-256 remains the integrity guard.
///
/// Codes already present in `out` (e.g. via a check_audio tail) are left
/// alone.
async fn handle_fingerprint_checks(
    pool: &PgPool,
    org: Uuid,
    aid: Uuid,
    sha256: &str,
    fp: &Option<std::result::Result<fingerprint::Fingerprint, String>>,
    to_run: &[&str],
    out: &mut Vec<StagedCheck>,
) -> Result<()> {
    fn already_emitted(out: &[StagedCheck], code: &str) -> bool {
        out.iter().any(|s| s.check_code == code)
    }
    let want_fp_failed = to_run.contains(&"AUDIO_FINGERPRINT_FAILED")
        && !already_emitted(out, "AUDIO_FINGERPRINT_FAILED");
    let want_similar = to_run.contains(&"AUDIO_SIMILAR_TO_EXISTING")
        && !already_emitted(out, "AUDIO_SIMILAR_TO_EXISTING");
    if !want_fp_failed && !want_similar {
        return Ok(());
    }
    let fp = match fp {
        Some(Ok(fp)) => fp,
        Some(Err(detail)) => {
            // Distinguish "too short to fingerprint" (deterministic) from a
            // decode failure (transient). Match on the typed PolicyGate code,
            // not the Debug rendering.
            let too_short = detail.contains(fingerprint::TOO_SHORT_CODE);
            if want_fp_failed {
                out.push(StagedCheck {
                    check_code: "AUDIO_FINGERPRINT_FAILED",
                    rule_version: qc::QC_RULE_VERSION,
                    status: if too_short {
                        CheckStatus::NotApplicable
                    } else {
                        CheckStatus::TechnicalRetry
                    },
                    result_hash: asset_cache_key("AUDIO_FINGERPRINT_FAILED", sha256),
                    detail: detail.clone(),
                });
            }
            if want_similar {
                out.push(StagedCheck {
                    check_code: "AUDIO_SIMILAR_TO_EXISTING",
                    rule_version: qc::QC_RULE_VERSION,
                    status: CheckStatus::NotApplicable,
                    result_hash: asset_cache_key("AUDIO_SIMILAR_TO_EXISTING", sha256),
                    detail: "no fingerprint available; similarity not evaluated".into(),
                });
            }
            return Ok(());
        }
        // Non-audio, or QC blocked the bytes: tails already covered these.
        None => return Ok(()),
    };
    if want_fp_failed {
        out.push(StagedCheck {
            check_code: "AUDIO_FINGERPRINT_FAILED",
            rule_version: qc::QC_RULE_VERSION,
            status: CheckStatus::Pass,
            result_hash: asset_cache_key("AUDIO_FINGERPRINT_FAILED", sha256),
            detail: format!(
                "fingerprint v{} frames={}",
                fingerprint::FINGERPRINT_VERSION,
                fp.frames.len()
            ),
        });
    }
    if fp.is_empty() {
        // Fingerprint computed but too few frames for a meaningful
        // comparison: do not pretend similarity was evaluated.
        if want_similar {
            out.push(StagedCheck {
                check_code: "AUDIO_SIMILAR_TO_EXISTING",
                rule_version: qc::QC_RULE_VERSION,
                status: CheckStatus::NotApplicable,
                result_hash: asset_cache_key("AUDIO_SIMILAR_TO_EXISTING", sha256),
                detail: "audio too short for fingerprint comparison".into(),
            });
        }
        return Ok(());
    }
    // Idempotent store: one row per asset; a re-run reuses the row.
    // asset_fingerprints is FORCE RLS: authorize this write's org in a
    // short transaction, same pattern as distribution.rs.
    let mut ftx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *ftx)
        .await?;
    sqlx::query(
        "INSERT INTO catalog.asset_fingerprints(asset_id, org_id, version, frames, duration_secs, hash)
         VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(asset_id) DO NOTHING",
    )
    .bind(aid)
    .bind(org)
    .bind(fingerprint::FINGERPRINT_VERSION)
    .bind(fp.frames.len() as i32)
    .bind(fp.duration_secs)
    .bind(fp.to_bytes())
    .execute(&mut *ftx)
    .await?;
    if want_similar {
        let hits = find_similar_assets(&mut ftx, org, aid, &fp.frames).await?;
        let (status, detail) = if hits.is_empty() {
            (CheckStatus::Pass, "no similar audio in catalog".to_string())
        } else {
            let listed: Vec<String> = hits
                .iter()
                .map(|(id, ber)| format!("{id} (BER={ber:.3})"))
                .collect();
            (
                CheckStatus::ReviewRequired,
                format!("similar to {} asset(s): {}", hits.len(), listed.join(", ")),
            )
        };
        out.push(StagedCheck {
            check_code: "AUDIO_SIMILAR_TO_EXISTING",
            rule_version: qc::QC_RULE_VERSION,
            status,
            result_hash: asset_cache_key("AUDIO_SIMILAR_TO_EXISTING", sha256),
            detail,
        });
    }
    ftx.commit().await?;
    Ok(())
}

/// Load a previously stored fingerprint for `aid`, if any. Used when the
/// only uncached check is AUDIO_SIMILAR_TO_EXISTING: the bytes are unchanged
/// (all other checks hit the cache), so we reuse the stored fingerprint
/// instead of re-downloading and re-analyzing the file.
async fn load_stored_fingerprint(
    pool: &PgPool,
    org: Uuid,
    aid: Uuid,
) -> Result<Option<fingerprint::Fingerprint>> {
    let mut rtx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *rtx)
        .await?;
    let row: Option<(Vec<u8>, f64)> = sqlx::query_as(
        "SELECT hash, duration_secs FROM catalog.asset_fingerprints WHERE asset_id=$1 AND version=$2",
    )
    .bind(aid)
    .bind(fingerprint::FINGERPRINT_VERSION)
    .fetch_optional(&mut *rtx)
    .await?;
    rtx.rollback().await?;
    match row {
        Some((bytes, duration_secs)) => {
            let mut fp =
                fingerprint::Fingerprint::from_bytes(&bytes).map_err(|_| Error::Internal)?;
            fp.duration_secs = duration_secs;
            Ok(Some(fp))
        }
        None => Ok(None),
    }
}

/// Best-similarity matches for `frames` among the org's stored
/// fingerprints at the current algorithm version, excluding `aid`
/// itself. Sorted by BER ascending, capped at 3 for the check detail.
async fn find_similar_assets(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org: Uuid,
    aid: Uuid,
    frames: &[u32],
) -> Result<Vec<(Uuid, f64)>> {
    let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT asset_id, hash FROM catalog.asset_fingerprints
          WHERE org_id=$1 AND asset_id<>$2 AND version=$3",
    )
    .bind(org)
    .bind(aid)
    .bind(fingerprint::FINGERPRINT_VERSION)
    .fetch_all(&mut **tx)
    .await?;
    let mut hits = Vec::new();
    for (other_id, hash) in rows {
        let Ok(other) = fingerprint::Fingerprint::from_bytes(&hash) else {
            continue;
        };
        if let Some(ber) = fingerprint::bit_error_rate(frames, &other.frames) {
            if ber <= fingerprint::SIMILAR_BER {
                hits.push((other_id, ber));
            }
        }
    }
    hits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(3);
    Ok(hits)
}

async fn asset_checks(
    pool: &PgPool,
    storage: &Arc<dyn ObjectStore>,
    org: Uuid,
    body: &Value,
) -> Result<Vec<StagedCheck>> {
    let mut assets: BTreeMap<String, (String, String)> = BTreeMap::new();
    if let Some(tracks) = body["tracks"].as_array() {
        for t in tracks {
            if let (Some(aid), Some(sha), Some(kind)) = (
                t["asset_id"].as_str(),
                t["asset_sha256"].as_str(),
                t["asset_kind"].as_str(),
            ) {
                assets.insert(aid.to_string(), (sha.to_string(), kind.to_string()));
            }
        }
    }
    let mut out = Vec::new();
    for (aid_str, (sha256, kind)) in &assets {
        let aid = Uuid::parse_str(aid_str).map_err(|_| Error::Internal)?;
        let row =
            sqlx::query("SELECT object_key, state FROM catalog.assets WHERE org_id=$1 AND id=$2")
                .bind(org)
                .bind(aid)
                .fetch_optional(pool)
                .await?
                .ok_or(Error::Internal)?;
        let state: String = row.get("state");
        let key: String = row.get("object_key");
        if state != "REGISTERED" {
            out.push(StagedCheck {
                check_code: "ASSET_NOT_ADMITTED",
                rule_version: qc::QC_RULE_VERSION,
                status: CheckStatus::Blocked,
                result_hash: asset_cache_key("ASSET_NOT_ADMITTED", sha256),
                detail: format!("asset state={state}"),
            });
            continue;
        }
        let expected: &[&str] = match kind.as_str() {
            "AUDIO" => qc::AUDIO_CHECK_CODES,
            "IMAGE" => qc::IMAGE_CHECK_CODES,
            _ => &[],
        };
        let mut to_run: Vec<&str> = Vec::new();
        for code in expected {
            // AUDIO_SIMILAR_TO_EXISTING is never cached: the result depends
            // on what else is in the catalog at check time, not just the
            // bytes. A byte-identical re-upload must be flagged as similar
            // to the original, even though the SHA-256 matches a cached PASS.
            if *code == "AUDIO_SIMILAR_TO_EXISTING" {
                to_run.push(code);
                continue;
            }
            let rh = asset_cache_key(code, sha256);
            match cached_status(pool, code, qc::QC_RULE_VERSION, &rh).await? {
                // A cached TECHNICAL_RETRY is transient: re-run instead of copying it.
                Some(st) if st != CheckStatus::TechnicalRetry => out.push(StagedCheck {
                    check_code: code,
                    rule_version: qc::QC_RULE_VERSION,
                    status: st,
                    result_hash: rh,
                    detail: "cache_hit".into(),
                }),
                _ => to_run.push(code),
            }
        }
        if to_run.is_empty() {
            continue;
        }
        // If the only uncached check is AUDIO_SIMILAR_TO_EXISTING and we
        // have a stored fingerprint, reuse it without re-downloading the
        // bytes. The similarity result depends on catalog state, but the
        // fingerprint itself is immutable for unchanged bytes.
        let only_similarity = to_run == ["AUDIO_SIMILAR_TO_EXISTING"];
        if only_similarity {
            if let Some(stored_fp) = load_stored_fingerprint(pool, org, aid).await? {
                let fp_opt = Some(Ok(stored_fp));
                handle_fingerprint_checks(pool, org, aid, sha256, &fp_opt, &to_run, &mut out)
                    .await?;
                // Emit the cached codes for the other checks (already in `out`
                // via the cache_hit path above); nothing more to do for this asset.
                continue;
            }
            // No stored fingerprint: fall through to analyze_asset which will
            // compute and store it.
        }
        // Unique temp name: two workers must never share an analyzer file.
        let tmp_name = format!("audeniq-qc-{}", Uuid::new_v4());
        let outcomes = match analyze_asset(storage, &key, kind.as_str(), sha256, &tmp_name).await {
            Ok((o, duration_secs, fp)) => {
                // Persist measured audio duration for the DDEX builder.
                // Only fills when unknown; never overwrites.
                if let Some(secs) = duration_secs {
                    let _ = sqlx::query(
                        "UPDATE catalog.assets SET duration_secs=$1 WHERE org_id=$2 AND id=$3 AND duration_secs IS NULL",
                    )
                    .bind(secs)
                    .bind(org)
                    .bind(aid)
                    .execute(pool)
                    .await;
                }
                // Perceptual fingerprint: store + similarity check. The two
                // fingerprint codes are DB-backed so they are produced here,
                // not in check_audio; codes already emitted via a QC tail
                // (e.g. blocked bytes) are not duplicated.
                handle_fingerprint_checks(pool, org, aid, sha256, &fp, &to_run, &mut out).await?;
                o
            }
            Err(detail) => {
                // Storage/IO failure is per-asset TECHNICAL_RETRY: other
                // assets' results still persist and the job is requeued.
                for code in &to_run {
                    out.push(StagedCheck {
                        check_code: code,
                        rule_version: qc::QC_RULE_VERSION,
                        status: CheckStatus::TechnicalRetry,
                        result_hash: asset_cache_key(code, sha256),
                        detail: detail.clone(),
                    });
                }
                continue;
            }
        };
        for o in outcomes {
            if to_run.contains(&o.check_code) {
                out.push(StagedCheck {
                    check_code: o.check_code,
                    rule_version: qc::QC_RULE_VERSION,
                    status: o.status,
                    result_hash: asset_cache_key(o.check_code, sha256),
                    detail: o.detail,
                });
            }
        }
    }
    Ok(out)
}

/// Roll up per-asset worst status into catalog.assets.qc_status.
async fn update_asset_qc(
    c: &mut PgConnection,
    org: Uuid,
    body: &Value,
    checks: &[StagedCheck],
) -> Result<()> {
    // Map asset sha256 -> worst status. asset checks are keyed by bytes hash.
    let mut worst: BTreeMap<String, CheckStatus> = BTreeMap::new();
    let rank = |s: CheckStatus| match s {
        CheckStatus::Pass | CheckStatus::NotApplicable => 0,
        CheckStatus::TechnicalRetry => 1,
        CheckStatus::ReviewRequired => 2,
        CheckStatus::CorrectionRequired => 3,
        CheckStatus::Blocked => 4,
    };
    let mut sha_of: BTreeMap<String, String> = BTreeMap::new(); // asset_id -> sha256
    if let Some(tracks) = body["tracks"].as_array() {
        for t in tracks {
            if let (Some(aid), Some(sha)) = (t["asset_id"].as_str(), t["asset_sha256"].as_str()) {
                sha_of.insert(aid.to_string(), sha.to_string());
            }
        }
    }
    for ch in checks {
        if ch.rule_version != qc::QC_RULE_VERSION {
            continue;
        }
        for (aid, sha) in &sha_of {
            if ch.result_hash == asset_cache_key(ch.check_code, sha) {
                let e = worst.entry(aid.clone()).or_insert(CheckStatus::Pass);
                if rank(ch.status) > rank(*e) {
                    *e = ch.status;
                }
            }
        }
    }
    for (aid, st) in worst {
        let qc_status = match st {
            CheckStatus::Pass | CheckStatus::NotApplicable => "PASS",
            _ => "BLOCKED",
        };
        let aid = Uuid::parse_str(&aid).map_err(|_| Error::Internal)?;
        sqlx::query("UPDATE catalog.assets SET qc_status=$3 WHERE org_id=$1 AND id=$2")
            .bind(org)
            .bind(aid)
            .bind(qc_status)
            .execute(&mut *c)
            .await?;
    }
    Ok(())
}

fn validation_package(
    revision_id: Uuid,
    body_hash: &str,
    consent_package_hash: &str,
    body: &Value,
    check_ids: &[Uuid],
) -> Value {
    let mut validated_assets = Vec::new();
    let mut claimant_parties = BTreeSet::new();
    let mut flags: BTreeSet<&str> = BTreeSet::new();
    if let Some(d) = body.get("declarations") {
        for (key, flag) in [
            ("is_cover", "COVER"),
            ("is_remix", "REMIX"),
            ("contains_samples", "SAMPLE"),
            ("ai_involved", "AI"),
            ("explicit_content", "EXPLICIT"),
        ] {
            if d.get(key).and_then(Value::as_bool).unwrap_or(false) {
                flags.insert(flag);
            }
        }
    }
    if let Some(tracks) = body["tracks"].as_array() {
        for t in tracks {
            if let (Some(aid), Some(sha)) = (t["asset_id"].as_str(), t["asset_sha256"].as_str()) {
                validated_assets.push(json!({
                    "asset_id": aid,
                    "sha256": sha,
                    "metric_hash": sha256_hex(&format!("{aid}:{sha}")),
                }));
            }
            if t.get("parental_advisory")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                flags.insert("EXPLICIT");
            }
            if let Some(credits) = t["credits"].as_array() {
                for cr in credits {
                    if let Some(p) = cr["party_id"].as_str() {
                        claimant_parties.insert(p.to_string());
                    }
                }
            }
        }
    }
    json!({
        "schema_version": 1,
        "revision_id": revision_id,
        "revision_hash": body_hash,
        "validated_assets": validated_assets,
        "special_flags": flags.into_iter().collect::<Vec<_>>(),
        "claimant_party_ids": claimant_parties.into_iter().collect::<Vec<_>>(),
        "consent_package_hash": consent_package_hash,
        "stage1_check_refs": check_ids,
        "rule_version": qc::QC_RULE_VERSION,
    })
}

/// 1-D: run Stage 1 for one revision. All checks are recorded immutably;
/// on technical retry the job is requeued and cached checks are not re-analyzed.
pub async fn run_stage1(
    pool: &PgPool,
    storage: &Arc<dyn ObjectStore>,
    revision_id: Uuid,
) -> Result<Stage1Summary> {
    let rev = sqlx::query(
        "SELECT org_id, release_id, body, body_hash, consent_package_hash FROM catalog.application_revisions WHERE id=$1",
    )
    .bind(revision_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = rev.get("org_id");
    let release: Uuid = rev.get("release_id");
    let body: Value = rev.get("body");
    let body_hash: String = rev.get("body_hash");
    let consent_package_hash: String = rev.get("consent_package_hash");

    // Idempotent completion: a previous attempt already pinned the validation
    // package (e.g. the worker crashed between commit and job completion).
    // Return its summary without re-running QC or touching the release.
    if let Some(pkg_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM distribution.validation_packages WHERE revision_id=$1",
    )
    .bind(revision_id)
    .fetch_optional(pool)
    .await?
    {
        let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
            .bind(release)
            .fetch_one(pool)
            .await?;
        return Ok(Stage1Summary {
            revision_id,
            status_counts: BTreeMap::new(),
            release_status: status,
            validation_package_id: Some(pkg_id),
            needs_retry: false,
        });
    }

    let mut checks = field_checks(&body);
    checks.extend(asset_checks(pool, storage, org, &body).await?);

    let mut tx = pool.begin().await?;
    // The worker owns Stage 1 now: SUBMITTED -> STAGE1_RUNNING (idempotent on retry).
    sqlx::query("UPDATE catalog.releases SET status='STAGE1_RUNNING', row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND status='SUBMITTED'")
        .bind(org)
        .bind(release)
        .execute(&mut *tx)
        .await?;
    let mut check_ids = Vec::new();
    for c in &checks {
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM operations.check_results WHERE revision_id=$1 AND check_code=$2 AND result_hash=$3",
        )
        .bind(revision_id)
        .bind(c.check_code)
        .bind(&c.result_hash)
        .fetch_optional(&mut *tx)
        .await?;
        let id = match existing {
            Some(id) => id,
            None => {
                sqlx::query_scalar("INSERT INTO operations.check_results(id, revision_id, check_code, rule_version, status, result_hash, detail) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING id")
                    .bind(Uuid::new_v4())
                    .bind(revision_id)
                    .bind(c.check_code)
                    .bind(c.rule_version)
                    .bind(c.status.as_db())
                    .bind(&c.result_hash)
                    .bind(&c.detail)
                    .fetch_one(&mut *tx)
                    .await?
            }
        };
        check_ids.push(id);
    }

    let mut counts: BTreeMap<&'static str, i64> = BTreeMap::new();
    for c in &checks {
        *counts.entry(c.status.as_db()).or_default() += 1;
    }
    let n = |s: &str| counts.get(s).copied().unwrap_or(0);
    let needs_retry = n("TECHNICAL_RETRY") > 0;
    // Only objective failures block: BLOCKED or CORRECTION_REQUIRED.
    // REVIEW_REQUIRED is recorded for the Stage 2 human reviewer and never
    // stops the release — uncertain calls pass through, per policy.
    let has_blocking = n("BLOCKED") > 0 || n("CORRECTION_REQUIRED") > 0;
    let mut summary = Stage1Summary {
        revision_id,
        status_counts: counts,
        release_status: String::new(),
        validation_package_id: None,
        needs_retry,
    };

    if summary.needs_retry {
        // Keep STAGE1_RUNNING; checks are recorded and the cache makes the
        // retry cheap. Caller requeues the job.
        tx.commit().await?;
        summary.release_status = "STAGE1_RUNNING".into();
        return Ok(summary);
    }

    update_asset_qc(&mut tx, org, &body, &checks).await?;

    if has_blocking {
        sqlx::query(
            "UPDATE catalog.releases SET status='STAGE1_CORRECTION', row_version=row_version+1 WHERE org_id=$1 AND id=$2",
        )
        .bind(org)
        .bind(release)
        .execute(&mut *tx)
        .await?;
        summary.release_status = "STAGE1_CORRECTION".into();
    } else {
        let pkg = validation_package(
            revision_id,
            &body_hash,
            &consent_package_hash,
            &body,
            &check_ids,
        );
        let pkg_hash = sha256_hex(&canonical(&pkg));
        let pkg_id = Uuid::new_v4();
        sqlx::query("INSERT INTO distribution.validation_packages(id, org_id, revision_id, body, package_hash, rule_version) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(pkg_id).bind(org).bind(revision_id).bind(&pkg).bind(&pkg_hash).bind(qc::QC_RULE_VERSION)
            .execute(&mut *tx)
            .await?;
        // Same-transaction handoff: the Stage 2 orchestrator owns everything
        // after STAGE1_PASSED. ON CONFLICT keeps a retried run from double-queueing.
        sqlx::query("INSERT INTO operations.jobs(id, queue, kind, payload, pinned_revision_id, idempotency_key) VALUES($1,'rights','stage2',$2,$3,$4) ON CONFLICT(idempotency_key) DO NOTHING")
            .bind(Uuid::new_v4())
            .bind(json!({"revision_id": revision_id, "validation_package_id": pkg_id, "package_hash": pkg_hash}))
            .bind(revision_id)
            .bind(format!("stage2:{revision_id}"))
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE catalog.releases SET status='STAGE1_PASSED', row_version=row_version+1 WHERE org_id=$1 AND id=$2")
            .bind(org)
            .bind(release)
            .execute(&mut *tx)
            .await?;
        summary.release_status = "STAGE1_PASSED".into();
        summary.validation_package_id = Some(pkg_id);
    }
    operations::audit(
        &mut tx,
        None,
        Some(org),
        Some(release),
        "stage1.completed",
        &summary.release_status,
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(summary)
}
