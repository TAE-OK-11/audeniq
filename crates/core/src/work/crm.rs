//! CRM Lite: relationships with artists, labels, DSPs, workspaces and outside
//! partners. A relationship points at the source record and adds only what
//! AUDENIQ does not know: owner, status, next contact, contacts and contact
//! history. Its name, contract status (signed agreements of the workspace, or
//! the DSP's contract route), releases and delivery numbers are read from the
//! sources. Related releases / contracts are `work.links`, notes `work.notes`,
//! tasks are tasks linked to the relationship or its record.
//!
//! Every staff role may read and edit; archiving is ADMIN only. Every change
//! is audited (`work.relationship_*`, `work.contact_*`, `work.activity_logged`).
use super::{active_staff, audit, entities, line, one_of, text};
use crate::{
    api::AppState,
    error::{Error, Result},
    staff,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::collections::HashMap;
use uuid::Uuid;

/// Display name of relationship `r`: its own name, or the source record's.
macro_rules! source_name {
    () => {
        "COALESCE(r.name, CASE r.entity_kind
           WHEN 'artist' THEN (SELECT a.name FROM catalog.artists a WHERE a.id::text=r.entity_id)
           WHEN 'label' THEN (SELECT l.name FROM catalog.labels l WHERE l.id::text=r.entity_id)
           WHEN 'dsp' THEN (SELECT d.name FROM distribution.dsp_registry d WHERE d.code=r.entity_id)
           WHEN 'org' THEN (SELECT o.name FROM identity.orgs o WHERE o.id::text=r.entity_id) END)"
    };
}
pub(crate) use source_name;

/// The workspace behind relationship `r` (for agreements and releases).
macro_rules! source_org {
    () => {
        "CASE r.entity_kind
           WHEN 'org' THEN (SELECT o.id FROM identity.orgs o WHERE o.id::text=r.entity_id)
           WHEN 'artist' THEN (SELECT a.org_id FROM catalog.artists a WHERE a.id::text=r.entity_id)
           WHEN 'label' THEN (SELECT l.org_id FROM catalog.labels l WHERE l.id::text=r.entity_id) END"
    };
}

macro_rules! rel_cols {
    () => {
        concat!(
            "r.id, r.kind, r.entity_kind, r.entity_id, r.name AS own_name, r.manual_contract_status, r.owner_id, r.status,
             r.next_contact_at, r.tags, r.created_by, r.created_at, r.updated_at, r.row_version, r.archived_at, ",
            source_name!(),
            " AS label, ",
            source_org!(),
            " AS org_id,
             (SELECT max(a.at) FROM work.relationship_activities a WHERE a.relationship_id=r.id) AS last_contact_at,
             (SELECT count(*) FROM work.tasks t WHERE t.archived_at IS NULL AND t.status NOT IN ('DONE','CANCELLED')
                AND EXISTS(SELECT 1 FROM work.links l WHERE l.subject_kind='task' AND l.subject_id=t.id AND l.archived_at IS NULL
                  AND ((l.entity_kind='relationship' AND l.entity_id=r.id::text)
                    OR (l.entity_kind=r.entity_kind AND l.entity_id=r.entity_id)))) AS open_tasks,
             (SELECT count(*) FROM work.notes n WHERE n.archived_at IS NULL
                AND ((n.target_kind='relationship' AND n.target_id=r.id::text)
                  OR (n.target_kind=r.entity_kind AND n.target_id=r.entity_id))) AS note_count,
             (SELECT jsonb_build_object('id',c.id,'name',",
            crate::pii_ref!("contact_name", "c.id", "c.name", "c.name_cipher"),
            ",'role',c.role,'email',",
            crate::pii_ref!("contact_email", "c.id", "c.email", "c.email_cipher"),
            ",'phone',",
            crate::pii_ref!("contact_phone", "c.id", "c.phone", "c.phone_cipher"),
            ")
                FROM work.relationship_contacts c WHERE c.relationship_id=r.id AND c.archived_at IS NULL
               ORDER BY c.is_primary DESC, c.created_at LIMIT 1) AS primary_contact"
        )
    };
}

pub const KINDS: &[&str] = &["ARTIST", "LABEL", "DSP", "PARTNER", "ORG"];
const STATUSES: &[&str] = &["PROSPECT", "ACTIVE", "AT_RISK", "INACTIVE"];
const CONTRACT: &[&str] = &["NONE", "NEGOTIATING", "SIGNED", "EXPIRED", "TERMINATED"];
const ACTIVITY: &[&str] = &["CALL", "EMAIL", "MEETING", "NOTE"];

struct Rel {
    id: Uuid,
    kind: String,
    entity_kind: Option<String>,
    entity_id: Option<String>,
    own_name: Option<String>,
    manual_contract: Option<String>,
    owner_id: Option<Uuid>,
    status: String,
    next_contact_at: Option<DateTime<Utc>>,
    tags: Vec<String>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    row_version: i32,
    archived_at: Option<DateTime<Utc>>,
    label: Option<String>,
    org_id: Option<Uuid>,
    last_contact_at: Option<DateTime<Utc>>,
    open_tasks: i64,
    note_count: i64,
    primary_contact: Option<Value>,
}

impl Rel {
    fn from_row(r: &PgRow) -> Self {
        Self {
            id: r.get("id"),
            kind: r.get("kind"),
            entity_kind: r.get("entity_kind"),
            entity_id: r.get("entity_id"),
            own_name: r.get("own_name"),
            manual_contract: r.get("manual_contract_status"),
            owner_id: r.get("owner_id"),
            status: r.get("status"),
            next_contact_at: r.get("next_contact_at"),
            tags: r.get("tags"),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
            row_version: r.get("row_version"),
            archived_at: r.get("archived_at"),
            label: r.get("label"),
            org_id: r.get("org_id"),
            last_contact_at: r.get("last_contact_at"),
            open_tasks: r.get("open_tasks"),
            note_count: r.get("note_count"),
            primary_contact: r.get("primary_contact"),
        }
    }
    fn json(&self, contract: Value) -> Value {
        json!({
            "id": self.id, "kind": self.kind,
            "entity": self.entity_kind.as_ref().map(|k| json!({"kind": k, "id": self.entity_id})),
            "name": self.label, "own_name": self.own_name, "missing": self.entity_kind.is_some() && self.label.is_none(),
            "owner_id": self.owner_id, "status": self.status, "next_contact_at": self.next_contact_at,
            "contact_due": self.archived_at.is_none() && self.next_contact_at.is_some_and(|d| d <= Utc::now()),
            "last_contact_at": self.last_contact_at, "tags": self.tags, "contract": contract,
            "open_tasks": self.open_tasks, "note_count": self.note_count, "primary_contact": self.primary_contact,
            "org_id": self.org_id, "created_by": self.created_by, "created_at": self.created_at,
            "updated_at": self.updated_at, "row_version": self.row_version, "archived_at": self.archived_at,
        })
    }
}

/// Contract status from the sources: signed / pending distribution agreements
/// of the workspace, the DSP's contract route and onboarding, or (without a
/// source record) what staff track by hand.
async fn contracts(c: &mut PgConnection, rels: &[Rel]) -> Result<HashMap<Uuid, Value>> {
    let orgs: Vec<Uuid> = rels.iter().filter_map(|r| r.org_id).collect();
    let agreements: HashMap<Uuid, (i64, i64, Option<DateTime<Utc>>)> = sqlx::query(
        "SELECT org_id,
                count(*) FILTER (WHERE status IN ('SIGNED','APPROVED')) AS signed,
                count(*) FILTER (WHERE status IN ('AWAITING_DOCUMENTS','REVIEW','PREPARED','NEEDS')) AS pending,
                max(updated_at) AS last_at
           FROM portal.documents WHERE kind='AGREEMENT' AND org_id = ANY($1) GROUP BY org_id",
    )
    .bind(&orgs)
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(|r| {
        (
            r.get("org_id"),
            (r.get("signed"), r.get("pending"), r.get("last_at")),
        )
    })
    .collect();
    let dsps: Vec<String> = rels
        .iter()
        .filter(|r| r.entity_kind.as_deref() == Some("dsp"))
        .filter_map(|r| r.entity_id.clone())
        .collect();
    let dsp: HashMap<String, (bool, Option<String>, Option<String>)> = sqlx::query(
        "SELECT x.code, execution.platform_contract_live(x.code) AS live,
                (SELECT o.stage FROM execution.partner_readiness(x.code) o) AS stage,
                (SELECT cr.route FROM distribution.dsp_contract_routes cr WHERE cr.code=x.code) AS route
           FROM unnest($1::text[]) AS x(code)",
    )
    .bind(&dsps)
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(|r| {
        (
            r.get("code"),
            (r.get("live"), r.get("stage"), r.get("route")),
        )
    })
    .collect();
    let mut out = HashMap::new();
    for r in rels {
        let v = match (r.entity_kind.as_deref(), r.entity_id.as_ref()) {
            (Some("dsp"), Some(code)) => {
                let (live, stage, route) = dsp.get(code).cloned().unwrap_or((false, None, None));
                json!({
                    "status": if live { "SIGNED" } else if stage.is_some() { "NEGOTIATING" } else { "NONE" },
                    "source": "dsp_contract", "route": route, "onboarding_stage": stage,
                })
            }
            (Some(_), _) => {
                let (signed, pending, at) = r
                    .org_id
                    .and_then(|o| agreements.get(&o).cloned())
                    .unwrap_or((0, 0, None));
                json!({
                    "status": if signed > 0 { "SIGNED" } else if pending > 0 { "NEGOTIATING" } else { "NONE" },
                    "source": "agreements", "signed": signed, "pending": pending, "updated_at": at,
                })
            }
            _ => {
                json!({"status": r.manual_contract.as_deref().unwrap_or("NONE"), "source": "manual"})
            }
        };
        out.insert(r.id, v);
    }
    Ok(out)
}

async fn load(c: &mut PgConnection, id: Uuid, lock: bool) -> Result<Rel> {
    let q = if lock {
        sqlx::query(concat!(
            "SELECT ",
            rel_cols!(),
            " FROM work.relationships r WHERE r.id=$1 FOR UPDATE OF r"
        ))
    } else {
        sqlx::query(concat!(
            "SELECT ",
            rel_cols!(),
            " FROM work.relationships r WHERE r.id=$1"
        ))
    };
    let r = q
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Rel::from_row(&r))
}

fn tags(v: &[String]) -> Result<Vec<String>> {
    if v.len() > 8 {
        return Err(Error::InvalidCode("TOO_MANY_TAGS"));
    }
    let mut out = Vec::new();
    for t in v {
        let t = line(t, 30)?;
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// List / detail
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub owner: Option<Uuid>,
    /// due (next contact passed) · week
    pub contact: Option<String>,
    pub q: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

pub async fn list(s: &AppState, h: &HeaderMap, q: ListQuery) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let kind = q.kind.as_deref().filter(|v| !v.is_empty());
    if let Some(k) = kind {
        one_of(k, KINDS, "KIND_UNKNOWN")?;
    }
    let status = q.status.as_deref().filter(|v| !v.is_empty());
    if let Some(v) = status {
        one_of(v, STATUSES, "STATUS_UNKNOWN")?;
    }
    let contact = q.contact.as_deref().filter(|v| !v.is_empty());
    if let Some(v) = contact {
        one_of(v, &["due", "week"], "CONTACT_FILTER_UNKNOWN")?;
    }
    let term =
        q.q.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| line(t, 80))
            .transpose()?;
    let mut c = s.pool.acquire().await?;
    let rows = sqlx::query(concat!(
        "SELECT ", rel_cols!(), " FROM work.relationships r
          WHERE ($1 OR r.archived_at IS NULL) AND (NOT $1 OR r.archived_at IS NOT NULL)
            AND ($2::text IS NULL OR r.kind=$2) AND ($3::text IS NULL OR r.status=$3) AND ($4::uuid IS NULL OR r.owner_id=$4)
            AND ($5::text IS NULL OR ($5='due' AND r.next_contact_at <= now()) OR ($5='week' AND r.next_contact_at < now() + interval '7 days'))
            AND ($6::text IS NULL OR ", source_name!(), " ILIKE '%'||$6||'%' OR $6 = ANY(r.tags))
          ORDER BY (r.next_contact_at IS NULL), r.next_contact_at, r.updated_at DESC LIMIT 500"
    ))
    .bind(q.archived)
    .bind(kind)
    .bind(status)
    .bind(q.owner)
    .bind(contact)
    .bind(term)
    .fetch_all(&mut *c)
    .await?;
    let rels: Vec<Rel> = rows.iter().map(Rel::from_row).collect();
    let contract = contracts(&mut c, &rels).await?;
    let counts: HashMap<String, i64> = sqlx::query(
        "SELECT kind, count(*) AS n FROM work.relationships WHERE archived_at IS NULL GROUP BY kind",
    )
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(|r| (r.get("kind"), r.get("n")))
    .collect();
    let mut out = json!({
        "items": rels.iter().map(|r| r.json(contract.get(&r.id).cloned().unwrap_or(Value::Null))).collect::<Vec<_>>(),
        "counts": counts,
    });
    crate::pii::reveal(&s.payout_keys, &mut out)?;
    Ok(out)
}

pub async fn detail(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    let r = load(&mut c, id, false).await?;
    let contract = contracts(&mut c, std::slice::from_ref(&r)).await?;
    let contacts: Vec<Value> = sqlx::query_scalar(concat!(
        "SELECT jsonb_build_object('id',c.id,'name',",
            crate::pii_ref!("contact_name", "c.id", "c.name", "c.name_cipher"),
            ",'role',c.role,'email',",
            crate::pii_ref!("contact_email", "c.id", "c.email", "c.email_cipher"),
            ",'phone',",
            crate::pii_ref!("contact_phone", "c.id", "c.phone", "c.phone_cipher"),
        ",'is_primary',c.is_primary,'created_at',c.created_at,'updated_at',c.updated_at)
           FROM work.relationship_contacts c WHERE c.relationship_id=$1 AND c.archived_at IS NULL ORDER BY c.is_primary DESC, c.created_at"
    ))
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    let activities: Vec<Value> = sqlx::query_scalar(concat!(
        "SELECT jsonb_build_object('id',a.id,'kind',a.kind,'summary',a.summary,'at',a.at,'by_user',a.by_user,
                'contact', (SELECT ",
        crate::pii_ref!("contact_name", "c.id", "c.name", "c.name_cipher"),
        " FROM work.relationship_contacts c WHERE c.id=a.contact_id))
           FROM work.relationship_activities a WHERE a.relationship_id=$1 ORDER BY a.at DESC, a.created_at DESC LIMIT 200"
    ))
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    // Related records staff linked by hand (releases, contracts, …)
    let links = sqlx::query(
        "SELECT id, entity_kind, entity_id FROM work.links
          WHERE subject_kind='relationship' AND subject_id=$1 AND archived_at IS NULL ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    let refs: Vec<entities::Ref> = links
        .iter()
        .map(|l| entities::Ref {
            kind: l.get("entity_kind"),
            id: l.get("entity_id"),
        })
        .collect();
    let labels = entities::resolve(&mut c, &refs).await?;
    let linked: Vec<Value> = links
        .iter()
        .zip(refs.iter())
        .map(|(l, rf)| {
            let mut v = labels.get(rf).cloned().unwrap_or(Value::Null);
            v["link_id"] = json!(l.get::<Uuid, _>("id"));
            v
        })
        .collect();
    // From the source: the workspace's releases and agreements
    let (releases, agreements): (Vec<Value>, Vec<Value>) = match r.org_id {
        Some(org) => (
            sqlx::query_scalar(
                "SELECT jsonb_build_object('id',id,'title',title,'status',status,'created_at',created_at)
                   FROM catalog.releases WHERE org_id=$1 AND archived_at IS NULL ORDER BY created_at DESC LIMIT 12",
            )
            .bind(org)
            .fetch_all(&mut *c)
            .await?,
            sqlx::query_scalar(
                "SELECT jsonb_build_object('id',d.id,'title',d.title,'status',d.status,'release_title',r.title,'updated_at',d.updated_at)
                   FROM portal.documents d LEFT JOIN catalog.releases r ON r.id=d.release_id
                  WHERE d.org_id=$1 AND d.kind='AGREEMENT' ORDER BY d.updated_at DESC LIMIT 12",
            )
            .bind(org)
            .fetch_all(&mut *c)
            .await?,
        ),
        None => (Vec::new(), Vec::new()),
    };
    let mut out = json!({
        "relationship": r.json(contract.get(&r.id).cloned().unwrap_or(Value::Null)),
        "contacts": contacts,
        "activities": activities,
        "links": linked,
        "releases": releases,
        "agreements": agreements,
    });
    crate::pii::reveal(&s.payout_keys, &mut out)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Create / update / archive
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub kind: String,
    #[serde(default)]
    pub entity_kind: Option<String>,
    #[serde(default)]
    pub entity_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub owner_id: Option<Uuid>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub next_contact_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub manual_contract_status: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn create(s: &AppState, h: &HeaderMap, i: CreateInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    one_of(&i.kind, KINDS, "KIND_UNKNOWN")?;
    let status = i.status.as_deref().unwrap_or("ACTIVE");
    one_of(status, STATUSES, "STATUS_UNKNOWN")?;
    let entity = match (i.entity_kind.as_deref(), i.entity_id.as_deref()) {
        (Some(k), Some(id)) if !k.is_empty() && !id.is_empty() => {
            one_of(k, &["artist", "label", "dsp", "org"], "ENTITY_KIND_UNKNOWN")?;
            let r = entities::parse(k, id)?;
            if i.kind != k.to_uppercase() {
                return Err(Error::InvalidCode("KIND_MISMATCH"));
            }
            Some(r)
        }
        _ => None,
    };
    // A name (and a hand-tracked contract status) only without a source record.
    let name = match (&entity, i.name.as_deref()) {
        (None, Some(n)) => {
            let n = line(n, 120)?;
            if n.is_empty() {
                return Err(Error::InvalidCode("NAME_REQUIRED"));
            }
            Some(n)
        }
        (None, None) => return Err(Error::InvalidCode("NAME_REQUIRED")),
        (Some(_), Some(n)) if !n.trim().is_empty() => {
            return Err(Error::InvalidCode("NAME_FROM_SOURCE"));
        }
        (Some(_), _) => None,
    };
    let manual = match (&entity, i.manual_contract_status.as_deref()) {
        (None, Some(v)) => {
            one_of(v, CONTRACT, "CONTRACT_STATUS_UNKNOWN")?;
            Some(v.to_string())
        }
        (Some(_), Some(_)) => return Err(Error::InvalidCode("CONTRACT_FROM_SOURCE")),
        _ => None,
    };
    let tg = tags(&i.tags)?;
    let mut tx = s.pool.begin().await?;
    if let Some(r) = &entity {
        entities::require(&mut tx, r).await?;
    }
    active_staff(&mut tx, i.owner_id).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.relationships(id,kind,entity_kind,entity_id,name,manual_contract_status,owner_id,status,next_contact_at,tags,created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id).bind(&i.kind).bind(entity.as_ref().map(|r| r.kind.clone())).bind(entity.as_ref().map(|r| r.id.clone()))
    .bind(&name).bind(&manual).bind(i.owner_id).bind(status).bind(i.next_contact_at).bind(&tg).bind(st.actor.user)
    .execute(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.code().as_deref() == Some("23505") => Error::InvalidCode("RELATIONSHIP_EXISTS"),
        _ => e.into(),
    })?;
    audit(&mut tx, &st, Some(id), "work.relationship_created", &i.kind).await?;
    tx.commit().await?;
    detail(s, h, id).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateInput {
    pub row_version: i32,
    #[serde(default, deserialize_with = "super::tasks::double_option")]
    pub owner_id: Option<Option<Uuid>>,
    pub status: Option<String>,
    #[serde(default, deserialize_with = "super::tasks::double_option")]
    pub next_contact_at: Option<Option<DateTime<Utc>>>,
    pub tags: Option<Vec<String>>,
    pub name: Option<String>,
    pub manual_contract_status: Option<String>,
}

pub async fn update(s: &AppState, h: &HeaderMap, id: Uuid, i: UpdateInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let mut r = load(&mut tx, id, true).await?;
    if r.archived_at.is_some() {
        return Err(Error::PolicyGate("RELATIONSHIP_ARCHIVED"));
    }
    if r.row_version != i.row_version {
        return Err(Error::Conflict);
    }
    let mut changed: Vec<&str> = Vec::new();
    if let Some(v) = i.owner_id
        && v != r.owner_id
    {
        active_staff(&mut tx, v).await?;
        r.owner_id = v;
        changed.push("OWNER");
    }
    if let Some(v) = &i.status
        && *v != r.status
    {
        one_of(v, STATUSES, "STATUS_UNKNOWN")?;
        r.status = v.clone();
        changed.push("STATUS");
    }
    if let Some(v) = i.next_contact_at
        && v != r.next_contact_at
    {
        r.next_contact_at = v;
        changed.push("NEXT_CONTACT");
    }
    if let Some(v) = &i.tags {
        r.tags = tags(v)?;
        changed.push("TAGS");
    }
    if let Some(v) = &i.name {
        if r.entity_kind.is_some() {
            return Err(Error::InvalidCode("NAME_FROM_SOURCE"));
        }
        let v = line(v, 120)?;
        if v.is_empty() {
            return Err(Error::InvalidCode("NAME_REQUIRED"));
        }
        r.own_name = Some(v);
        changed.push("NAME");
    }
    if let Some(v) = &i.manual_contract_status {
        if r.entity_kind.is_some() {
            return Err(Error::InvalidCode("CONTRACT_FROM_SOURCE"));
        }
        one_of(v, CONTRACT, "CONTRACT_STATUS_UNKNOWN")?;
        r.manual_contract = Some(v.clone());
        changed.push("CONTRACT");
    }
    if !changed.is_empty() {
        sqlx::query(
            "UPDATE work.relationships SET owner_id=$2,status=$3,next_contact_at=$4,tags=$5,name=$6,manual_contract_status=$7,
                    updated_at=now(),row_version=row_version+1 WHERE id=$1",
        )
        .bind(id)
        .bind(r.owner_id)
        .bind(&r.status)
        .bind(r.next_contact_at)
        .bind(&r.tags)
        .bind(&r.own_name)
        .bind(&r.manual_contract)
        .execute(&mut *tx)
        .await?;
        audit(
            &mut tx,
            &st,
            Some(id),
            "work.relationship_updated",
            &changed.join(","),
        )
        .await?;
    }
    tx.commit().await?;
    detail(s, h, id).await
}

pub async fn archive(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if !st.role.is_admin() {
        return Err(Error::Forbidden);
    }
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query(
        "UPDATE work.relationships SET archived_at=now(), archived_by=$2, row_version=row_version+1 WHERE id=$1 AND archived_at IS NULL",
    )
    .bind(id)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    if n.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.relationship_archived",
        "ARCHIVE",
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"archived": id}))
}

// ---------------------------------------------------------------------------
// Contacts, activities, links
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactInput {
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
    /// update only: archive the contact
    #[serde(default)]
    pub archived: bool,
}

struct Contact {
    name: String,
    role: String,
    email: Option<String>,
    phone: Option<String>,
}

/// Name, email and phone sealed for contact `id` (AAD binds the contact).
type SealedContact = (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);
fn seal_contact(
    keys: &crate::payout_keys::KeyRing,
    id: Uuid,
    v: &Contact,
) -> Result<SealedContact> {
    use crate::payout_keys::Field;
    Ok((
        keys.seal_field(Field::ContactName, id, &v.name)?,
        crate::pii::seal_opt(keys, Field::ContactEmail, id, v.email.as_deref())?,
        crate::pii::seal_opt(keys, Field::ContactPhone, id, v.phone.as_deref())?,
    ))
}

fn contact(i: &ContactInput) -> Result<Contact> {
    let name = line(&i.name, 80)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("NAME_REQUIRED"));
    }
    let email = i
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            let ok = e.len() <= 254
                && e.split_once('@').is_some_and(|(a, b)| {
                    !a.is_empty() && !b.is_empty() && !e.chars().any(char::is_whitespace)
                });
            if ok {
                Ok(e.to_string())
            } else {
                Err(Error::InvalidCode("EMAIL_INVALID"))
            }
        })
        .transpose()?;
    let phone = i
        .phone
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let ok = (4..=32).contains(&p.len())
                && p.chars().all(|c| c.is_ascii_digit() || "+() -".contains(c));
            if ok {
                Ok(p.to_string())
            } else {
                Err(Error::InvalidCode("PHONE_INVALID"))
            }
        })
        .transpose()?;
    Ok(Contact {
        name,
        role: line(&i.role, 80)?,
        email,
        phone,
    })
}

async fn live_rel(c: &mut PgConnection, id: Uuid) -> Result<()> {
    let archived: Option<Option<DateTime<Utc>>> =
        sqlx::query_scalar("SELECT archived_at FROM work.relationships WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *c)
            .await?;
    match archived {
        None => Err(Error::NotFound),
        Some(Some(_)) => Err(Error::PolicyGate("RELATIONSHIP_ARCHIVED")),
        Some(None) => Ok(()),
    }
}

async fn touch(c: &mut PgConnection, id: Uuid) -> Result<()> {
    sqlx::query(
        "UPDATE work.relationships SET updated_at=now(), row_version=row_version+1 WHERE id=$1",
    )
    .bind(id)
    .execute(&mut *c)
    .await?;
    Ok(())
}

pub async fn add_contact(s: &AppState, h: &HeaderMap, id: Uuid, i: ContactInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let v = contact(&i)?;
    let mut tx = s.pool.begin().await?;
    live_rel(&mut tx, id).await?;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.relationship_contacts WHERE relationship_id=$1 AND archived_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if n >= 30 {
        return Err(Error::InvalidCode("TOO_MANY_CONTACTS"));
    }
    if i.is_primary {
        sqlx::query("UPDATE work.relationship_contacts SET is_primary=false, updated_at=now() WHERE relationship_id=$1 AND is_primary")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let cid = Uuid::new_v4();
    let (name, email, phone) = seal_contact(&s.payout_keys, cid, &v)?;
    sqlx::query(
        "INSERT INTO work.relationship_contacts(id,relationship_id,name_cipher,role,email_cipher,phone_cipher,is_primary,created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(cid)
    .bind(id)
    .bind(&name)
    .bind(&v.role)
    .bind(&email)
    .bind(&phone)
    .bind(i.is_primary || n == 0)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    touch(&mut tx, id).await?;
    audit(&mut tx, &st, Some(id), "work.contact_added", "CONTACT").await?;
    tx.commit().await?;
    detail(s, h, id).await
}

pub async fn edit_contact(
    s: &AppState,
    h: &HeaderMap,
    cid: Uuid,
    i: ContactInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let rel: Uuid = sqlx::query_scalar(
        "SELECT relationship_id FROM work.relationship_contacts WHERE id=$1 AND archived_at IS NULL FOR UPDATE",
    )
    .bind(cid)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    live_rel(&mut tx, rel).await?;
    if i.archived {
        sqlx::query("UPDATE work.relationship_contacts SET archived_at=now(), archived_by=$2, is_primary=false, updated_at=now() WHERE id=$1")
            .bind(cid)
            .bind(st.actor.user)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, &st, Some(rel), "work.contact_archived", "CONTACT").await?;
    } else {
        let v = contact(&i)?;
        if i.is_primary {
            sqlx::query("UPDATE work.relationship_contacts SET is_primary=false, updated_at=now() WHERE relationship_id=$1 AND is_primary AND id<>$2")
                .bind(rel)
                .bind(cid)
                .execute(&mut *tx)
                .await?;
        }
        let (name, email, phone) = seal_contact(&s.payout_keys, cid, &v)?;
        sqlx::query(
            "UPDATE work.relationship_contacts SET name=NULL, name_cipher=$2, role=$3, email=NULL, email_cipher=$4,
               phone=NULL, phone_cipher=$5, is_primary=$6, updated_at=now() WHERE id=$1",
        )
        .bind(cid)
        .bind(&name)
        .bind(&v.role)
        .bind(&email)
        .bind(&phone)
        .bind(i.is_primary)
        .execute(&mut *tx)
        .await?;
        audit(&mut tx, &st, Some(rel), "work.contact_updated", "CONTACT").await?;
    }
    touch(&mut tx, rel).await?;
    tx.commit().await?;
    detail(s, h, rel).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityInput {
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub contact_id: Option<Uuid>,
    /// optionally schedule the next contact at the same time
    #[serde(default, deserialize_with = "super::tasks::double_option")]
    pub next_contact_at: Option<Option<DateTime<Utc>>>,
}

pub async fn log_activity(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    i: ActivityInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    one_of(&i.kind, ACTIVITY, "ACTIVITY_UNKNOWN")?;
    let summary = text(&i.summary, 2000)?;
    if summary.is_empty() {
        return Err(Error::InvalidCode("SUMMARY_REQUIRED"));
    }
    let at = i.at.unwrap_or_else(Utc::now);
    if at > Utc::now() + chrono::Duration::minutes(5) {
        return Err(Error::InvalidCode("ACTIVITY_IN_FUTURE"));
    }
    let mut tx = s.pool.begin().await?;
    live_rel(&mut tx, id).await?;
    if let Some(c) = i.contact_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM work.relationship_contacts WHERE id=$1 AND relationship_id=$2)",
        )
        .bind(c)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !ok {
            return Err(Error::InvalidCode("CONTACT_NOT_FOUND"));
        }
    }
    sqlx::query(
        "INSERT INTO work.relationship_activities(id,relationship_id,kind,summary,contact_id,at,by_user) VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(&i.kind)
    .bind(&summary)
    .bind(i.contact_id)
    .bind(at)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    if let Some(next) = i.next_contact_at {
        sqlx::query("UPDATE work.relationships SET next_contact_at=$2 WHERE id=$1")
            .bind(id)
            .bind(next)
            .execute(&mut *tx)
            .await?;
    }
    touch(&mut tx, id).await?;
    audit(&mut tx, &st, Some(id), "work.activity_logged", &i.kind).await?;
    tx.commit().await?;
    detail(s, h, id).await
}

pub async fn link(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    i: super::tasks::LinkInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let r = entities::parse(&i.entity_kind, &i.entity_id)?;
    if r.kind == "relationship" && r.id == id.to_string() {
        return Err(Error::InvalidCode("LINK_SELF"));
    }
    let mut tx = s.pool.begin().await?;
    live_rel(&mut tx, id).await?;
    entities::require(&mut tx, &r).await?;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.links WHERE subject_kind='relationship' AND subject_id=$1 AND archived_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if n >= 50 {
        return Err(Error::InvalidCode("TOO_MANY_LINKS"));
    }
    let added = sqlx::query(
        "INSERT INTO work.links(id,subject_kind,subject_id,entity_kind,entity_id,created_by) VALUES($1,'relationship',$2,$3,$4,$5)
         ON CONFLICT (subject_kind, subject_id, entity_kind, entity_id) WHERE archived_at IS NULL DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(&r.kind)
    .bind(&r.id)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    if added.rows_affected() > 0 {
        touch(&mut tx, id).await?;
        audit(&mut tx, &st, Some(id), "work.relationship_linked", &r.kind).await?;
    }
    tx.commit().await?;
    detail(s, h, id).await
}

pub async fn unlink(s: &AppState, h: &HeaderMap, id: Uuid, link: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    live_rel(&mut tx, id).await?;
    let kind: Option<String> = sqlx::query_scalar(
        "UPDATE work.links SET archived_at=now(), archived_by=$3
          WHERE id=$1 AND subject_kind='relationship' AND subject_id=$2 AND archived_at IS NULL RETURNING entity_kind",
    )
    .bind(link)
    .bind(id)
    .bind(st.actor.user)
    .fetch_optional(&mut *tx)
    .await?;
    let kind = kind.ok_or(Error::NotFound)?;
    touch(&mut tx, id).await?;
    audit(&mut tx, &st, Some(id), "work.relationship_unlinked", &kind).await?;
    tx.commit().await?;
    detail(s, h, id).await
}

/// The relationship of a source record, for screens that show one (DSP ops).
pub(crate) async fn by_entities(
    keys: &crate::payout_keys::KeyRing,
    c: &mut PgConnection,
    kind: &str,
    ids: &[String],
) -> Result<HashMap<String, Value>> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        rel_cols!(),
        " FROM work.relationships r WHERE r.archived_at IS NULL AND r.entity_kind=$1 AND r.entity_id = ANY($2)"
    ))
    .bind(kind)
    .bind(ids)
    .fetch_all(&mut *c)
    .await?;
    let rels: Vec<Rel> = rows.iter().map(Rel::from_row).collect();
    let contract = contracts(c, &rels).await?;
    rels.iter()
        .filter_map(|r| {
            r.entity_id.clone().map(|e| {
                let mut v = r.json(contract.get(&r.id).cloned().unwrap_or(Value::Null));
                crate::pii::reveal(keys, &mut v).map(|()| (e, v))
            })
        })
        .collect()
}

async fn h_list(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list(&s, &h, q).await?))
}
async fn h_create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<CreateInput>,
) -> Result<Json<Value>> {
    Ok(Json(create(&s, &h, i).await?))
}
async fn h_detail(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(detail(&s, &h, id).await?))
}
async fn h_update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<UpdateInput>,
) -> Result<Json<Value>> {
    Ok(Json(update(&s, &h, id, i).await?))
}
async fn h_archive(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(archive(&s, &h, id).await?))
}
async fn h_add_contact(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ContactInput>,
) -> Result<Json<Value>> {
    Ok(Json(add_contact(&s, &h, id, i).await?))
}
async fn h_edit_contact(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ContactInput>,
) -> Result<Json<Value>> {
    Ok(Json(edit_contact(&s, &h, id, i).await?))
}
async fn h_activity(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ActivityInput>,
) -> Result<Json<Value>> {
    Ok(Json(log_activity(&s, &h, id, i).await?))
}
async fn h_link(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<super::tasks::LinkInput>,
) -> Result<Json<Value>> {
    Ok(Json(link(&s, &h, id, i).await?))
}
async fn h_unlink(
    State(s): State<AppState>,
    Path((id, link_id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(unlink(&s, &h, id, link_id).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/relationships", get(h_list).post(h_create))
        .route(
            "/api/staff/work/relationships/{id}",
            get(h_detail).post(h_update),
        )
        .route(
            "/api/staff/work/relationships/{id}/archive",
            post(h_archive),
        )
        .route(
            "/api/staff/work/relationships/{id}/contacts",
            post(h_add_contact),
        )
        .route(
            "/api/staff/work/relationship-contacts/{id}",
            post(h_edit_contact),
        )
        .route(
            "/api/staff/work/relationships/{id}/activities",
            post(h_activity),
        )
        .route("/api/staff/work/relationships/{id}/links", post(h_link))
        .route(
            "/api/staff/work/relationships/{id}/links/{link}/archive",
            post(h_unlink),
        )
}
