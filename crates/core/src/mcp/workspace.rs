//! Workspace tools that read state or make small, reversible changes:
//! notifications, inquiries, rights proofs, file checks, delivery and
//! Content ID state, smart-link checks, signing requests, members,
//! infringement claims, split invites, CSV exports and draft deletion.
//! Each one calls the Studio's own service function with the token's actor,
//! so authorization and rules are exactly the Studio's.
use super::{
    oauth::Principal,
    tools::{ToolError, tool_error, uuid_arg},
};
use crate::{api::AppState, auth, catalog, error::Error, portal};
use serde_json::{Value, json};
use uuid::Uuid;

type ToolResult = std::result::Result<Value, ToolError>;

/// The tools this module answers ([`call`]).
pub(super) const TOOLS: [&str; 13] = [
    "mark_notifications_read",
    "close_inquiry",
    "attach_rights_proof",
    "get_file_checks",
    "get_delivery_status",
    "get_content_id_status",
    "check_smart_link",
    "list_signing_requests",
    "list_members",
    "list_infringement_claims",
    "resend_split_invite",
    "export_csv",
    "delete_release_draft",
];

pub(super) async fn call(s: &AppState, p: &Principal, name: &str, args: &Value) -> ToolResult {
    match name {
        "mark_notifications_read" => read_notifications(s, p, args).await,
        "close_inquiry" => close_inquiry(s, p, args).await,
        "attach_rights_proof" => Box::pin(attach_proof(s, p, args)).await,
        "get_file_checks" => file_checks(s, p, args).await,
        "get_delivery_status" => delivery(s, p, args).await,
        "get_content_id_status" => content_id(s, p, args).await,
        "check_smart_link" => smart_link_check(s, p, args).await,
        "list_signing_requests" => signing_requests(s, p, args).await,
        "list_members" => members(s, p).await,
        "list_infringement_claims" => infringement(s, p, args).await,
        "resend_split_invite" => resend_split(s, p, args).await,
        "export_csv" => export_csv(s, p, args).await,
        "delete_release_draft" => delete_draft(s, p, args).await,
        _ => Err(tool_error("UNKNOWN_TOOL", "Unknown tool.")),
    }
}

/// Longest CSV returned inline (the edge caps responses; larger exports are
/// downloaded in the Studio).
const CSV_MAX_BYTES: usize = 256 * 1024;

fn studio(s: &AppState, path: &str) -> String {
    format!("{}{path}", super::issuer(s))
}

fn view(mut v: Value, name: &str) -> Value {
    if !v.is_object() {
        v = json!({"items": v});
    }
    v["view"] = json!(name);
    v
}

/// `mark_notifications_read`: given ids, or all.
async fn read_notifications(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let ids: Vec<Uuid> = args["notification_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str()?.parse().ok())
        .collect();
    let all = args["all"].as_bool().unwrap_or(false);
    if !all && ids.is_empty() {
        return Err(tool_error(
            "INVALID_ARGUMENTS",
            "Give notification_ids or all: true.",
        ));
    }
    let input = portal::ReadInput { ids, all };
    let v = portal::read_notifications(s, &p.actor, p.org, input).await?;
    Ok(view(v, "notifications_read"))
}

/// `close_inquiry`.
async fn close_inquiry(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let id = uuid_arg(args, "inquiry_id")?;
    portal::close_inquiry(s, &p.actor, p.org, id).await?;
    let v = portal::get_inquiry(s, &p.actor, p.org, id).await?;
    Ok(view(v, "inquiry"))
}

/// `attach_rights_proof`: a registered DOCUMENT or IMAGE asset becomes the
/// proof of a rights document, which then goes to AUDENIQ's review.
async fn attach_proof(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let asset_id = uuid_arg(args, "asset_id")?;
    let file_name = args["file_name"].as_str().unwrap_or("").trim().to_string();
    if args.get("document_id").is_none() {
        // A new proof for a release (the Studio's "add a rights document").
        let input = portal::DocumentInput {
            release_id: uuid_arg(args, "release_id")?,
            title: args["title"].as_str().unwrap_or("").trim().to_string(),
            body: String::new(),
            asset_id: Some(asset_id),
            file_name,
            electronic: None,
        };
        let mut v = view(
            portal::create_document(s, &p.actor, p.org, input).await?,
            "document_proof",
        );
        v["note"] = json!(
            "AUDENIQ reviews the proof; get_account_context (view documents) shows the outcome."
        );
        return Ok(v);
    }
    let id = uuid_arg(args, "document_id")?;
    // The tool never shows a version to conflict with: use the current one,
    // and retry once if another change landed in between.
    for attempt in 0..2 {
        let row_version: i64 = sqlx::query_scalar(
            "SELECT row_version FROM portal.documents WHERE org_id=$1 AND id=$2",
        )
        .bind(p.org)
        .bind(id)
        .fetch_optional(&s.pool)
        .await
        .map_err(Error::from)?
        .ok_or(Error::NotFound)?;
        let input = portal::ProofInput {
            asset_id,
            file_name: file_name.clone(),
            row_version,
        };
        match portal::attach_proof(s, &p.actor, p.org, id, input).await {
            Err(Error::Conflict) if attempt == 0 => continue,
            r => {
                let mut v = view(r?, "document_proof");
                v["note"] = json!(
                    "AUDENIQ reviews the proof; get_account_context (view documents) shows the outcome."
                );
                return Ok(v);
            }
        }
    }
    Err(Error::Conflict.into())
}

/// `get_file_checks`: the checks of one uploaded file.
async fn file_checks(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let asset = uuid_arg(args, "asset_id")?;
    let v = crate::uploads::checks(s, &p.actor, p.org, asset).await?;
    Ok(view(v, "file_checks"))
}

/// `get_delivery_status`: per-platform delivery of a release.
async fn delivery(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let release = uuid_arg(args, "release_id")?;
    let mut tx = s.pool.begin().await.map_err(Error::from)?;
    auth::authorize(&mut tx, &p.actor, p.org, release, "release", false).await?;
    tx.commit().await.map_err(Error::from)?;
    let mut v = crate::delivery_staging::release_delivery_view(&s.pool, p.org, release).await?;
    v = view(v, "delivery");
    v["release_id"] = json!(release);
    Ok(v)
}

/// `get_content_id_status`.
async fn content_id(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let release = uuid_arg(args, "release_id")?;
    let mut v = crate::platform_requests::content_id(s, &p.actor, p.org, release).await?;
    v = view(v, "content_id");
    v["release_id"] = json!(release);
    Ok(v)
}

/// `check_smart_link`: verdicts for links without saving them.
async fn smart_link_check(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let release = uuid_arg(args, "release_id")?;
    let links = args["links"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
        .collect();
    let input = crate::smart_links::CheckInput { links };
    let v = crate::smart_links::check(s, &p.actor, p.org, release, input).await?;
    Ok(view(v, "smart_link_check"))
}

/// `list_signing_requests`: status only; signing happens in the signer's
/// own link, never here.
async fn signing_requests(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let release_id = match args.get("release_id") {
        Some(_) => Some(uuid_arg(args, "release_id")?),
        None => None,
    };
    let q = crate::signing::ListQuery { release_id };
    let mut v = view(
        crate::signing::list(s, &p.actor, p.org, q).await?,
        "signing_requests",
    );
    v["studio_url"] = json!(studio(s, "/rights"));
    Ok(v)
}

/// `list_members`: e-mails are masked unless the caller owns the workspace.
async fn members(s: &AppState, p: &Principal) -> ToolResult {
    let mut v = view(crate::members::list(s, &p.actor, p.org).await?, "members");
    v["note"] = json!("Inviting members and changing roles is done in AUDENIQ Studio.");
    Ok(v)
}

/// `list_infringement_claims` for one release.
async fn infringement(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let release = uuid_arg(args, "release_id")?;
    let mut v = view(
        crate::infringement::for_release(s, &p.actor, p.org, release).await?,
        "infringement_claims",
    );
    v["release_id"] = json!(release);
    Ok(v)
}

/// `resend_split_invite`: e-mails a collaborator's invite again.
async fn resend_split(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let release = uuid_arg(args, "release_id")?;
    let share = uuid_arg(args, "share_id")?;
    let v = crate::splits::resend(s, &p.actor, p.org, release, share).await?;
    Ok(view(v, "split_invite_resent"))
}

/// `export_csv`: statements or royalty reports as CSV text.
async fn export_csv(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let kind = args["kind"].as_str().unwrap_or("");
    let csv = match kind {
        "statements" => portal::statements_csv(s, &p.actor, p.org).await?,
        "royalty_reports" => {
            let period = portal::Period {
                from: args["from"].as_str().map(str::to_owned),
                to: args["to"].as_str().map(str::to_owned),
            };
            portal::reports_csv(s, &p.actor, p.org, period).await?
        }
        _ => {
            return Err(tool_error(
                "INVALID_ARGUMENTS",
                "kind must be statements or royalty_reports.",
            ));
        }
    };
    if csv.len() > CSV_MAX_BYTES {
        return Err(tool_error(
            "EXPORT_TOO_LARGE",
            &format!(
                "This export is too large to return here. Download it in AUDENIQ Studio: {}",
                studio(s, "/settlement")
            ),
        ));
    }
    Ok(json!({
        "view": "csv_export",
        "kind": kind,
        "rows": csv.lines().count().saturating_sub(1),
        "csv": csv,
    }))
}

/// `delete_release_draft`: drafts only; submitted releases are withdrawn.
async fn delete_draft(s: &AppState, p: &Principal, args: &Value) -> ToolResult {
    let release = uuid_arg(args, "release_id")?;
    let mut tx = s.pool.begin().await.map_err(Error::from)?;
    auth::authorize(&mut tx, &p.actor, p.org, release, "release", true).await?;
    let (row_version, status): (i64, String) = sqlx::query_as(
        "SELECT row_version, status FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(p.org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await
    .map_err(Error::from)?
    .ok_or(Error::NotFound)?;
    tx.commit().await.map_err(Error::from)?;
    if status != "DRAFT" {
        return Err(tool_error(
            "RELEASE_NOT_DRAFT",
            "Only drafts can be deleted. A submitted release is withdrawn instead.",
        ));
    }
    catalog::archive(
        s,
        &p.actor,
        p.org,
        catalog::Kind::Release,
        release,
        row_version,
    )
    .await?;
    Ok(json!({"view": "release_deleted", "release_id": release, "deleted": true}))
}
