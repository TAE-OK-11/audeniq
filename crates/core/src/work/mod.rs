//! AUDENIQ internal work module (`/api/staff/work/*`). Design and the
//! duplicate check against existing features: docs/WORK_MANAGEMENT_PLAN.md.
//!
//! Phase 1: the general task engine (tasks, subtasks, reviewer, recurrence,
//! links to any AUDENIQ entity), internal notes with @mentions, edit history
//! and object-store attachments.
//!
//! Every call goes through [`staff::staff`] (ACTIVE staff role + staff access
//! log). Reads are open to every staff role, like the rest of the staff
//! portal; the few sensitive writes are decided in [`permit`]. Every mutation
//! is audited as `work.*`. Nothing is deleted: rows are archived.
use crate::{
    api::AppState,
    error::{Error, Result},
    operations,
    staff::{self, Staff, StaffRole},
    text_policy,
};
use axum::{Json, Router, extract::State, http::HeaderMap, routing::get};
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub mod channels;
pub mod crm;
pub mod dsp_ops;
pub mod entities;
pub mod exceptions;
pub mod inbox;
pub mod notes;
pub mod sla;
pub mod tasks;

/// Sensitive work actions and who may take them.
pub(crate) enum Act {
    /// Archive a task: its creator, its assignee or ADMIN.
    ArchiveTask {
        created_by: Uuid,
        assignee: Option<Uuid>,
    },
    /// Approve or send back a task in review: its reviewer or ADMIN.
    DecideReview { reviewer: Option<Uuid> },
    /// Edit a note: only its author.
    EditNote { author: Uuid },
    /// Archive a note: its author or ADMIN.
    ArchiveNote { author: Uuid },
    /// Rename or archive a channel: its creator or ADMIN (system channels: ADMIN).
    ManageChannel { created_by: Option<Uuid> },
}

pub(crate) fn permit(st: &Staff, act: Act) -> Result<()> {
    let me = st.actor.user;
    let admin = st.role == StaffRole::Admin;
    let ok = match act {
        Act::ArchiveTask {
            created_by,
            assignee,
        } => admin || created_by == me || assignee == Some(me),
        Act::DecideReview { reviewer } => admin || reviewer == Some(me),
        Act::EditNote { author } => author == me,
        Act::ArchiveNote { author } => admin || author == me,
        Act::ManageChannel { created_by } => admin || created_by == Some(me),
    };
    if ok { Ok(()) } else { Err(Error::Forbidden) }
}

pub(crate) fn one_of(v: &str, set: &[&str], code: &'static str) -> Result<()> {
    if set.contains(&v) {
        Ok(())
    } else {
        Err(Error::InvalidCode(code))
    }
}

/// One trimmed line of at most `max` characters.
pub(crate) fn line(s: &str, max: usize) -> Result<String> {
    let t = s.trim();
    if t.chars().count() > max {
        return Err(Error::InvalidCode("TEXT_TOO_LONG"));
    }
    text_policy::check(t)?;
    Ok(t.to_string())
}

/// Multi-line text of at most `max` characters.
pub(crate) fn text(s: &str, max: usize) -> Result<String> {
    let t = s.trim();
    if t.chars().count() > max {
        return Err(Error::InvalidCode("TEXT_TOO_LONG"));
    }
    text_policy::check_multiline(t)?;
    Ok(t.to_string())
}

/// Assignees, reviewers and mentioned people must be ACTIVE staff.
pub(crate) async fn active_staff(c: &mut PgConnection, user: Option<Uuid>) -> Result<()> {
    let Some(u) = user else { return Ok(()) };
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM identity.staff_members sm JOIN identity.users u ON u.id=sm.user_id
          WHERE sm.user_id=$1 AND sm.status='ACTIVE' AND u.status='ACTIVE')",
    )
    .bind(u)
    .fetch_one(&mut *c)
    .await?;
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidCode("NOT_STAFF"))
    }
}

pub(crate) async fn audit(
    c: &mut PgConnection,
    st: &Staff,
    resource: Option<Uuid>,
    action: &str,
    reason: &str,
) -> Result<()> {
    operations::audit(
        c,
        Some(st.actor.user),
        None,
        resource,
        action,
        reason,
        st.actor.request,
    )
    .await
}

/// Active staff for assignee / reviewer pickers and @mentions.
pub async fn people(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let rows = sqlx::query(
        "SELECT sm.user_id, u.email, sm.role FROM identity.staff_members sm JOIN identity.users u ON u.id=sm.user_id
          WHERE sm.status='ACTIVE' AND u.status='ACTIVE' ORDER BY u.email",
    )
    .fetch_all(&s.pool)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let email: String = r.get("email");
            let name = email.split('@').next().unwrap_or("").to_string();
            json!({"user_id": r.get::<Uuid,_>("user_id"), "email": email, "name": name, "role": r.get::<String,_>("role")})
        })
        .collect();
    Ok(json!({ "items": items }))
}

async fn h_people(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(people(&s, &h).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/people", get(h_people))
        .merge(entities::routes())
        .merge(tasks::routes())
        .merge(notes::routes())
        .merge(sla::routes())
        .merge(exceptions::routes())
        .merge(inbox::routes())
        .merge(crm::routes())
        .merge(channels::routes())
        .merge(dsp_ops::routes())
}
