//! 발매 후 중단·계약 종료·유통사 이전 (감사 P8, migration 0098).
//!
//! request → authority check → staff approval → per-platform tasks →
//! completion evidence:
//!   * the artist org's OWNER requests (the agreement must be signed and the
//!     release delivered), or staff file it on a rights holder's or an
//!     authority's claim; the check result is stored with the request;
//!   * a staff member other than the requester approves or rejects;
//!   * approval creates one task per platform that still has the release and
//!     a takedown job for it — a transfer's run on the agreed switch date;
//!   * a task completes on the platform's takedown confirmation, or on staff
//!     evidence for platforms without a takedown feed; the action completes
//!     when every task has (0098 trigger).
//!
//! Whether a platform really removed the release is outside this system:
//! a task records only what the platform or the operator reported.
use crate::{
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    staff::Staff,
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Takedown,
    Termination,
    Transfer,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "TAKEDOWN" => Self::Takedown,
            "TERMINATION" => Self::Termination,
            "TRANSFER" => Self::Transfer,
            _ => return Err(Error::InvalidCode("RELEASE_ACTION_KIND_INVALID")),
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Takedown => "TAKEDOWN",
            Self::Termination => "TERMINATION",
            Self::Transfer => "TRANSFER",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestInput {
    pub kind: String,
    pub reason: String,
    /// TAKEDOWN only: the platforms (partner ids) to leave; absent = all.
    #[serde(default)]
    pub partners: Option<Vec<String>>,
    /// TRANSFER: the distributor taking the release over.
    #[serde(default)]
    pub transfer_to: Option<String>,
    /// TRANSFER: the day the new distributor takes over (KST); the takedowns
    /// run then, so the release does not disappear before it is back.
    #[serde(default)]
    pub effective_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub approve: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub note: String,
    /// A registered document/image (screenshot, the platform's mail).
    #[serde(default)]
    pub evidence_asset_id: Option<Uuid>,
}

fn kst_today() -> NaiveDate {
    let kst = chrono::FixedOffset::east_opt(9 * 3600).expect("valid offset");
    chrono::Utc::now().with_timezone(&kst).date_naive()
}

fn text_ok(s: &str, max: usize) -> Result<()> {
    if s.chars().count() > max {
        return Err(Error::InvalidCode("NOTE_TOO_LONG"));
    }
    crate::text_policy::check_multiline(s)
}

fn map_open(e: sqlx::Error) -> Error {
    if matches!(&e, sqlx::Error::Database(d) if d.code().as_deref() == Some("23505")) {
        return Error::PolicyGate("RELEASE_ACTION_OPEN");
    }
    Error::Database(e)
}

async fn org_scope(c: &mut PgConnection, org: Uuid) -> Result<()> {
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *c)
        .await?;
    Ok(())
}

/// Platforms that still have the release: per partner, its latest delivered
/// package, unless that one is already taken down or being taken down.
pub async fn live_targets(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
) -> Result<Vec<(String, Uuid, Option<String>)>> {
    let rows: Vec<(String, Uuid, Option<String>)> = sqlx::query_as(
        "SELECT DISTINCT ON (j.partner_id) j.partner_id, j.package_id, b.live_status
           FROM execution.delivery_jobs j
           JOIN distribution.distribution_packages dp ON dp.id=j.package_id
           JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
           LEFT JOIN execution.live_bindings b ON b.package_id=j.package_id AND b.partner_id=j.partner_id
          WHERE j.org_id=$1 AND cr.release_id=$2 AND j.status='DELIVERED'
          ORDER BY j.partner_id, j.updated_at DESC",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|(_, _, s)| !matches!(s.as_deref(), Some("TAKEDOWN_REQUESTED" | "TAKEN_DOWN")))
        .collect())
}

/// The release may take a new action: delivered, not being edited or sent,
/// distribution not already ended, nothing else open.
async fn release_state(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<()> {
    let status: String = sqlx::query_scalar(
        "SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR UPDATE",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    if !crate::post_release::delivered_before(c, org, release).await? {
        return Err(Error::PolicyGate("RELEASE_NOT_DELIVERED"));
    }
    if status != "READY_FOR_DELIVERY" {
        // A post-release edit is open: finish or cancel it first.
        return Err(Error::PolicyGate("RELEASE_EDIT_OPEN"));
    }
    let busy: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.delivery_jobs j
           JOIN distribution.distribution_packages dp ON dp.id=j.package_id
           JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
          WHERE j.org_id=$1 AND cr.release_id=$2
            AND j.status IN ('QUEUED','LEASED','SENDING','AWAITING_RECONCILIATION'))",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *c)
    .await?;
    if busy {
        return Err(Error::PolicyGate("RELEASE_DELIVERY_IN_PROGRESS"));
    }
    ensure_distribution_open(c, release).await
}

/// After a termination or transfer was approved, the release is not sent
/// anywhere again from here.
pub async fn ensure_distribution_open(c: &mut PgConnection, release: Uuid) -> Result<()> {
    let row: Option<String> = sqlx::query_scalar(
        "SELECT status FROM portal.release_actions
          WHERE release_id=$1 AND (status IN ('REQUESTED','IN_PROGRESS')
                OR (status='COMPLETED' AND kind IN ('TERMINATION','TRANSFER')))
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(release)
    .fetch_optional(&mut *c)
    .await?;
    match row.as_deref() {
        None => Ok(()),
        Some("COMPLETED") => Err(Error::PolicyGate("RELEASE_DISTRIBUTION_ENDED")),
        Some(_) => Err(Error::PolicyGate("RELEASE_ACTION_OPEN")),
    }
}

struct Validated {
    kind: Kind,
    reason: String,
    partners: Option<Vec<String>>,
    transfer_to: Option<String>,
    effective_date: Option<NaiveDate>,
}

fn validate(i: &RequestInput) -> Result<Validated> {
    let kind = Kind::parse(i.kind.trim())?;
    let reason = i.reason.trim().to_owned();
    if reason.is_empty() {
        return Err(Error::PolicyGate("RELEASE_ACTION_REASON_REQUIRED"));
    }
    text_ok(&reason, 2000)?;
    let partners = match (&i.partners, kind) {
        (Some(p), Kind::Takedown) if !p.is_empty() => {
            let mut v: Vec<String> = p.iter().map(|s| s.trim().to_owned()).collect();
            if v.iter()
                .any(|s| !crate::partner_config::valid_partner_id(s))
            {
                return Err(Error::InvalidCode("RELEASE_ACTION_PARTNER_INVALID"));
            }
            v.sort();
            v.dedup();
            Some(v)
        }
        (Some(p), _) if !p.is_empty() => {
            // Ending the contract or moving distributor covers every platform.
            return Err(Error::InvalidCode("RELEASE_ACTION_PARTNERS_TAKEDOWN_ONLY"));
        }
        _ => None,
    };
    let (transfer_to, effective_date) = if kind == Kind::Transfer {
        let to = i
            .transfer_to
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or(Error::PolicyGate("TRANSFER_TO_REQUIRED"))?;
        if to.chars().count() > 200 {
            return Err(Error::InvalidCode("NOTE_TOO_LONG"));
        }
        crate::text_policy::check(to)?;
        let date = i
            .effective_date
            .filter(|d| *d >= kst_today())
            .ok_or(Error::PolicyGate("TRANSFER_DATE_INVALID"))?;
        (Some(to.to_owned()), Some(date))
    } else {
        (None, None)
    };
    Ok(Validated {
        kind,
        reason,
        partners,
        transfer_to,
        effective_date,
    })
}

/// The live targets this action covers; every named platform must have it.
fn covered(
    targets: Vec<(String, Uuid, Option<String>)>,
    partners: Option<&[String]>,
) -> Result<Vec<(String, Uuid, Option<String>)>> {
    if let Some(p) = partners {
        if p.iter().any(|x| !targets.iter().any(|(t, _, _)| t == x)) {
            return Err(Error::PolicyGate("RELEASE_ACTION_PARTNER_NOT_LIVE"));
        }
        return Ok(targets
            .into_iter()
            .filter(|(t, _, _)| p.contains(t))
            .collect());
    }
    if targets.is_empty() {
        return Err(Error::PolicyGate("RELEASE_ACTION_NOTHING_LIVE"));
    }
    Ok(targets)
}

#[allow(clippy::too_many_arguments)]
async fn create(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    source: &str,
    requester: Uuid,
    request_id: Uuid,
    v: Validated,
    mut authority: Value,
) -> Result<Value> {
    release_state(c, org, release).await?;
    let targets = covered(live_targets(c, org, release).await?, v.partners.as_deref())?;
    authority["targets"] = json!(
        targets
            .iter()
            .map(|(p, pkg, s)| json!({"partner_id": p, "package_id": pkg, "live_status": s}))
            .collect::<Vec<_>>()
    );
    authority["checked_at"] = json!(chrono::Utc::now());
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.release_actions(id,org_id,release_id,kind,source,reason,partners,transfer_to,effective_date,authority,requested_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id)
    .bind(org)
    .bind(release)
    .bind(v.kind.as_str())
    .bind(source)
    .bind(&v.reason)
    .bind(&v.partners)
    .bind(&v.transfer_to)
    .bind(v.effective_date)
    .bind(&authority)
    .bind(requester)
    .execute(&mut *c)
    .await
    .map_err(map_open)?;
    operations::audit(
        c,
        Some(requester),
        Some(org),
        Some(release),
        "release.action_requested",
        &format!("{}:{source}", v.kind.as_str()),
        request_id,
    )
    .await?;
    Ok(
        json!({"id": id, "kind": v.kind.as_str(), "status": "REQUESTED", "targets": authority["targets"]}),
    )
}

/// `POST /api/orgs/{org}/releases/{id}/actions`: the artist's request. Only
/// the organisation's OWNER (the contract party) may ask, and only for a
/// release delivered under a signed agreement.
pub async fn request(
    pool: &PgPool,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: RequestInput,
) -> Result<Value> {
    let v = validate(&i)?;
    let mut tx = pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let role = auth::membership(&mut tx, a, org, true).await?;
    if role != "OWNER" {
        return Err(Error::PolicyGate("RELEASE_ACTION_OWNER_REQUIRED"));
    }
    org_scope(&mut tx, org).await?;
    let agreement: Option<(Uuid, Option<Value>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, signer_identity, signed_at FROM portal.documents
          WHERE org_id=$1 AND release_id=$2 AND kind='AGREEMENT' AND status='SIGNED'
          ORDER BY signed_at DESC LIMIT 1",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    let (document, signer, signed_at) =
        agreement.ok_or(Error::PolicyGate("RELEASE_NOT_DELIVERED"))?;
    // Identity verification is recorded, not required, until an identity
    // provider is contracted (감사 4번).
    let requester = crate::consents::signer_identity(&mut tx, a.user).await?;
    let owner = crate::rights_grants::owner_party(&mut tx, org, release).await?;
    let authority = json!({
        "source": "ARTIST", "requester": requester, "requester_role": role,
        "agreement_document_id": document, "agreement_signed_at": signed_at,
        "agreement_signer": signer, "owner_party_id": owner,
    });
    let out = create(
        &mut tx, org, release, "ARTIST", a.user, a.request, v, authority,
    )
    .await?;
    tx.commit().await?;
    Ok(out)
}

/// Staff intake: a rights holder's claim, a court or agency order. The claim
/// itself is the reason; a different staff member approves.
pub async fn staff_intake(
    pool: &PgPool,
    st: &Staff,
    release: Uuid,
    i: RequestInput,
) -> Result<Value> {
    let v = validate(&i)?;
    let mut tx = pool.begin().await?;
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    org_scope(&mut tx, org).await?;
    let authority = json!({
        "source": "STAFF", "staff_user_id": st.actor.user, "staff_role": st.role.as_str(),
    });
    let out = create(
        &mut tx,
        org,
        release,
        "STAFF",
        st.actor.user,
        st.actor.request,
        v,
        authority,
    )
    .await?;
    tx.commit().await?;
    Ok(out)
}

async fn action_json(
    c: &mut PgConnection,
    release: Option<Uuid>,
    open_only: bool,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        "SELECT jsonb_build_object('id',a.id,'org_id',a.org_id,'release_id',a.release_id,'release_title',r.title,
           'kind',a.kind,'source',a.source,'status',a.status,'reason',a.reason,'partners',a.partners,
           'transfer_to',a.transfer_to,'effective_date',a.effective_date,'authority',a.authority,
           'requested_by',a.requested_by,'decided_by',a.decided_by,'decided_at',a.decided_at,
           'decision_note',a.decision_note,'completed_at',a.completed_at,'created_at',a.created_at,
           'tasks',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',t.id,'partner_id',t.partner_id,
               'package_id',t.package_id,'status',t.status,'last_error',t.last_error,
               'evidence_note',t.evidence_note,'evidence_asset_id',t.evidence_asset_id,
               'confirmed_via',t.confirmed_via,'confirmed_at',t.confirmed_at) ORDER BY t.partner_id)
             FROM portal.release_action_tasks t WHERE t.action_id=a.id),'[]'::jsonb))
           FROM portal.release_actions a JOIN catalog.releases r ON r.id=a.release_id
          WHERE ($1::uuid IS NULL OR a.release_id=$1)
            AND (NOT $2 OR a.status IN ('REQUESTED','IN_PROGRESS'))
          ORDER BY a.created_at DESC LIMIT 200",
    )
    .bind(release)
    .bind(open_only)
    .fetch_all(&mut *c)
    .await?)
}

/// `GET /api/orgs/{org}/releases/{id}/actions`
pub async fn list_for_release(pool: &PgPool, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    org_scope(&mut tx, org).await?;
    let mut rows = action_json(&mut tx, Some(release), false).await?;
    // Staff identities and internal check details stay with staff.
    for r in &mut rows {
        if let Some(o) = r.as_object_mut() {
            o.remove("authority");
            o.remove("decided_by");
            o.remove("requested_by");
        }
    }
    tx.commit().await?;
    Ok(json!({"actions": rows}))
}

/// `POST /api/orgs/{org}/releases/{id}/actions/{action}/cancel`: the
/// requester's organisation withdraws a request nobody decided yet.
pub async fn cancel(
    pool: &PgPool,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    action: Uuid,
) -> Result<Value> {
    let mut tx = pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let n = sqlx::query(
        "UPDATE portal.release_actions SET status='CANCELLED', updated_at=now()
          WHERE id=$1 AND org_id=$2 AND release_id=$3 AND source='ARTIST' AND status='REQUESTED'",
    )
    .bind(action)
    .bind(org)
    .bind(release)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("RELEASE_ACTION_NOT_CANCELLABLE"));
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.action_cancelled",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": action, "status": "CANCELLED"}))
}

/// Staff queue (`GET /api/staff/release-actions`).
pub async fn staff_list(c: &mut PgConnection, open_only: bool) -> Result<Value> {
    Ok(json!({"actions": action_json(c, None, open_only).await?}))
}

/// Enqueue the takedown job for one task (at `run_at`, KST midnight of the
/// transfer date, when given).
async fn enqueue_takedown(
    c: &mut PgConnection,
    task: Uuid,
    package: Uuid,
    partner: &str,
    run_on: Option<NaiveDate>,
    attempt: Uuid,
) -> Result<Uuid> {
    let job = operations::enqueue(
        c,
        "delivery",
        "delivery.takedown",
        &json!({"package_id": package, "partner_id": partner, "action_task_id": task}),
        &format!("release_action:{task}:{attempt}"),
        None,
    )
    .await?;
    if let Some(d) = run_on {
        sqlx::query(
            "UPDATE operations.jobs SET run_at=GREATEST(now(), ($2::date)::timestamp AT TIME ZONE 'Asia/Seoul')
              WHERE id=$1 AND status='QUEUED'",
        )
        .bind(job)
        .bind(d)
        .execute(&mut *c)
        .await?;
    }
    sqlx::query(
        "UPDATE portal.release_action_tasks SET job_id=$2, status='QUEUED', last_error=NULL, updated_at=now() WHERE id=$1",
    )
    .bind(task)
    .bind(job)
    .execute(&mut *c)
    .await?;
    Ok(job)
}

/// Approve (tasks + takedown jobs) or reject a request. The decider is never
/// the requester.
pub async fn decide(pool: &PgPool, st: &Staff, action: Uuid, d: Decision) -> Result<Value> {
    let note = d.note.trim().to_owned();
    text_ok(&note, 2000)?;
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT org_id, release_id, kind, status, requested_by, partners, effective_date
           FROM portal.release_actions WHERE id=$1 FOR UPDATE",
    )
    .bind(action)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if row.get::<String, _>("status") != "REQUESTED" {
        return Err(Error::PolicyGate("RELEASE_ACTION_ALREADY_DECIDED"));
    }
    if row.get::<Uuid, _>("requested_by") == st.actor.user {
        return Err(Error::PolicyGate("SECOND_PERSON_REQUIRED"));
    }
    let org: Uuid = row.get("org_id");
    let release: Uuid = row.get("release_id");
    let kind = Kind::parse(&row.get::<String, _>("kind"))?;
    org_scope(&mut tx, org).await?;
    if !d.approve {
        if note.is_empty() {
            return Err(Error::PolicyGate("REVIEW_NOTE_REQUIRED"));
        }
        sqlx::query(
            "UPDATE portal.release_actions SET status='REJECTED', decided_by=$2, decided_at=now(), decision_note=$3, updated_at=now()
              WHERE id=$1",
        )
        .bind(action)
        .bind(st.actor.user)
        .bind(&note)
        .execute(&mut *tx)
        .await?;
        operations::audit(
            &mut tx,
            Some(st.actor.user),
            Some(org),
            Some(release),
            "release.action_rejected",
            kind.as_str(),
            st.actor.request,
        )
        .await?;
        tx.commit().await?;
        return Ok(json!({"id": action, "status": "REJECTED"}));
    }
    // The request was checked when filed; check again now (a platform may
    // have dropped the release, or a send may be running).
    let partners: Option<Vec<String>> = row.get("partners");
    let effective: Option<NaiveDate> = row.get("effective_date");
    let busy: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.delivery_jobs j
           JOIN distribution.distribution_packages dp ON dp.id=j.package_id
           JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
          WHERE j.org_id=$1 AND cr.release_id=$2
            AND j.status IN ('QUEUED','LEASED','SENDING','AWAITING_RECONCILIATION'))",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *tx)
    .await?;
    if busy {
        return Err(Error::PolicyGate("RELEASE_DELIVERY_IN_PROGRESS"));
    }
    let targets = live_targets(&mut tx, org, release).await?;
    let targets: Vec<_> = match &partners {
        Some(p) => targets
            .into_iter()
            .filter(|(t, _, _)| p.contains(t))
            .collect(),
        None => targets,
    };
    let status = if targets.is_empty() {
        "COMPLETED"
    } else {
        "IN_PROGRESS"
    };
    sqlx::query(
        "UPDATE portal.release_actions SET status=$2, decided_by=$3, decided_at=now(), decision_note=NULLIF($4,''),
           completed_at=CASE WHEN $2='COMPLETED' THEN now() END, updated_at=now() WHERE id=$1",
    )
    .bind(action)
    .bind(status)
    .bind(st.actor.user)
    .bind(&note)
    .execute(&mut *tx)
    .await?;
    let run_on = if kind == Kind::Transfer {
        effective
    } else {
        None
    };
    let mut tasks = Vec::new();
    for (partner, package, _) in &targets {
        let task = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO portal.release_action_tasks(id,action_id,org_id,package_id,partner_id) VALUES($1,$2,$3,$4,$5)",
        )
        .bind(task)
        .bind(action)
        .bind(org)
        .bind(package)
        .bind(partner)
        .execute(&mut *tx)
        .await?;
        let job = enqueue_takedown(&mut tx, task, *package, partner, run_on, Uuid::nil()).await?;
        tasks.push(json!({"id": task, "partner_id": partner, "job_id": job}));
    }
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "release.action_approved",
        &format!("{}:{}", kind.as_str(), targets.len()),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": action, "status": status, "tasks": tasks}))
}

async fn open_task(
    c: &mut PgConnection,
    action: Uuid,
    task: Uuid,
) -> Result<(Uuid, Uuid, Uuid, String, String)> {
    let row: Option<(Uuid, Uuid, Uuid, String, String, String)> = sqlx::query_as(
        "SELECT a.org_id, a.release_id, t.package_id, t.partner_id, t.status, a.status
           FROM portal.release_action_tasks t JOIN portal.release_actions a ON a.id=t.action_id
          WHERE t.id=$1 AND t.action_id=$2 FOR UPDATE OF t",
    )
    .bind(task)
    .bind(action)
    .fetch_optional(&mut *c)
    .await?;
    let (org, release, package, partner, status, action_status) = row.ok_or(Error::NotFound)?;
    if action_status != "IN_PROGRESS" || status == "CONFIRMED" {
        return Err(Error::PolicyGate("RELEASE_ACTION_TASK_CLOSED"));
    }
    Ok((org, release, package, partner, status))
}

/// Staff evidence that a platform removed the release (taken down in the
/// platform's own channel, or confirmed outside its feed). The worker marks
/// the binding TAKEN_DOWN; the trigger confirms the task.
pub async fn record_evidence(
    pool: &PgPool,
    st: &Staff,
    action: Uuid,
    task: Uuid,
    e: Evidence,
) -> Result<Value> {
    let note = e.note.trim().to_owned();
    if note.is_empty() {
        return Err(Error::PolicyGate("REVIEW_NOTE_REQUIRED"));
    }
    text_ok(&note, 1000)?;
    let mut tx = pool.begin().await?;
    let (org, release, package, partner, _) = open_task(&mut tx, action, task).await?;
    if let Some(asset) = e.evidence_asset_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM catalog.assets WHERE org_id IN ($1,$2) AND id=$3 AND state='REGISTERED' AND kind IN ('DOCUMENT','IMAGE'))",
        )
        .bind(org)
        .bind(Uuid::nil())
        .bind(asset)
        .fetch_one(&mut *tx)
        .await?;
        if !ok {
            return Err(Error::PolicyGate("EVIDENCE_ASSET_INVALID"));
        }
    }
    sqlx::query(
        "UPDATE portal.release_action_tasks SET evidence_note=$2, evidence_asset_id=$3, evidence_by=$4, updated_at=now() WHERE id=$1",
    )
    .bind(task)
    .bind(&note)
    .bind(e.evidence_asset_id)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    let job = operations::enqueue(
        &mut tx,
        "delivery",
        "delivery.mark_taken_down",
        &json!({"package_id": package, "partner_id": partner, "staff_user_id": st.actor.user}),
        &format!("delivery.mark_taken_down:{task}:{}", Uuid::new_v4()),
        None,
    )
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "release.action_evidence",
        &format!("{partner}:{}", note.chars().take(200).collect::<String>()),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"task_id": task, "job_id": job}))
}

/// Send a failed (or not yet supported) platform's takedown again.
pub async fn retry_task(pool: &PgPool, st: &Staff, action: Uuid, task: Uuid) -> Result<Value> {
    let mut tx = pool.begin().await?;
    let (org, release, package, partner, status) = open_task(&mut tx, action, task).await?;
    if !matches!(status.as_str(), "FAILED" | "MANUAL_REQUIRED") {
        return Err(Error::PolicyGate("RELEASE_ACTION_TASK_NOT_RETRYABLE"));
    }
    let job = enqueue_takedown(&mut tx, task, package, &partner, None, Uuid::new_v4()).await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "release.action_task_retried",
        &partner,
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"task_id": task, "job_id": job}))
}

/// `GET /api/orgs/{org}/releases/{id}/actions/{action}/transfer-packet`:
/// what the new distributor needs to take the release over without losing
/// its history — the UPC and ISRCs as delivered, and each platform's own id.
pub async fn transfer_packet(
    pool: &PgPool,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    action: Uuid,
) -> Result<Value> {
    let mut tx = pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    org_scope(&mut tx, org).await?;
    let row: Option<(String, Option<String>, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT status, transfer_to, effective_date FROM portal.release_actions
          WHERE id=$1 AND org_id=$2 AND release_id=$3 AND kind='TRANSFER'",
    )
    .bind(action)
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, to, date) = row.ok_or(Error::NotFound)?;
    if !matches!(status.as_str(), "IN_PROGRESS" | "COMPLETED") {
        return Err(Error::PolicyGate("RELEASE_ACTION_NOT_APPROVED"));
    }
    let body: Option<Value> = sqlx::query_scalar(
        "SELECT cr.body FROM execution.delivery_jobs j
           JOIN distribution.distribution_packages dp ON dp.id=j.package_id
           JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
          WHERE j.org_id=$1 AND cr.release_id=$2 AND j.status='DELIVERED'
          ORDER BY j.updated_at DESC LIMIT 1",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    let body = body.ok_or(Error::PolicyGate("RELEASE_NOT_DELIVERED"))?;
    let platforms: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('partner_id',j.partner_id,'first_delivered_at',min(j.updated_at),
                 'partner_release_id',max(b.partner_release_id),'live_status',
                 (array_agg(b.live_status ORDER BY j.updated_at DESC))[1])
           FROM execution.delivery_jobs j
           JOIN distribution.distribution_packages dp ON dp.id=j.package_id
           JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
           LEFT JOIN execution.live_bindings b ON b.package_id=j.package_id AND b.partner_id=j.partner_id
          WHERE j.org_id=$1 AND cr.release_id=$2 AND j.status='DELIVERED'
          GROUP BY j.partner_id ORDER BY j.partner_id",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let tracks: Vec<Value> = body["tracks"]
        .as_array()
        .map(|t| {
            t.iter()
                .map(|t| {
                    json!({"disc_number": t["disc_number"], "track_number": t["track_number"],
                           "title": t["title"], "version": t["version"],
                           "artist": t["artist_name"], "isrc": t["isrc"]})
                })
                .collect()
        })
        .unwrap_or_default();
    tx.commit().await?;
    Ok(json!({
        "release_id": release, "title": body["release_title"], "release_type": body["release_type"],
        "upc": body["upc"], "tracks": tracks, "platforms": platforms,
        "transfer_to": to, "effective_date": date, "status": status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: &str) -> RequestInput {
        RequestInput {
            kind: kind.into(),
            reason: "계약 종료".into(),
            partners: None,
            transfer_to: None,
            effective_date: None,
        }
    }

    #[test]
    fn transfer_needs_destination_and_a_date_not_in_the_past() {
        assert!(validate(&input("TAKEDOWN")).is_ok());
        assert!(validate(&input("RESTORE")).is_err());
        let mut t = input("TRANSFER");
        assert!(matches!(
            validate(&t),
            Err(Error::PolicyGate("TRANSFER_TO_REQUIRED"))
        ));
        t.transfer_to = Some("다른 유통사".into());
        t.effective_date = Some(kst_today().pred_opt().unwrap());
        assert!(matches!(
            validate(&t),
            Err(Error::PolicyGate("TRANSFER_DATE_INVALID"))
        ));
        t.effective_date = Some(kst_today());
        assert!(validate(&t).is_ok());
        let mut r = input("TAKEDOWN");
        r.reason = "  ".into();
        assert!(validate(&r).is_err());
    }

    #[test]
    fn only_a_takedown_names_platforms() {
        let mut t = input("TERMINATION");
        t.partners = Some(vec!["spotify".into()]);
        assert!(validate(&t).is_err());
        let targets = vec![
            ("melon".to_string(), Uuid::nil(), Some("LIVE".to_string())),
            ("spotify".to_string(), Uuid::nil(), None),
        ];
        let p = vec!["spotify".to_string()];
        assert_eq!(covered(targets.clone(), Some(&p)).unwrap().len(), 1);
        let missing = vec!["genie".to_string()];
        assert!(covered(targets.clone(), Some(&missing)).is_err());
        assert_eq!(covered(targets, None).unwrap().len(), 2);
        assert!(covered(vec![], None).is_err());
    }
}
