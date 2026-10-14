//! The org's codes at a glance: every submitted release the member can read
//! with its UPC and each track's ISRC (the active assignment, else the code
//! written on the catalog row), flagged when the code is from a virtual (test)
//! range. Also as CSV for a spreadsheet.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::Result,
    identifiers::{IdentifierKind, is_virtual},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::get,
};
use serde_json::{Value, json};
use uuid::Uuid;

const MAX_RELEASES: i64 = 1000;

pub async fn list(s: &AppState, a: &Actor, org: Uuid) -> Result<Vec<Value>> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx)
        .await?;
    let mut items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
                  'release_id', r.id, 'title', r.title, 'artist', r.draft->>'artist',
                  'release_date', r.draft->>'release_date', 'status', r.status,
                  'upc', COALESCE((SELECT ia.identifier FROM distribution.identifier_assignments ia
                                    WHERE ia.org_id=r.org_id AND ia.release_id=r.id AND ia.kind='UPC' AND ia.status='ASSIGNED'
                                    ORDER BY ia.created_at DESC LIMIT 1), r.upc),
                  'tracks', COALESCE((SELECT jsonb_agg(jsonb_build_object(
                        'track_id', t.id, 'title', t.title, 'disc', t.disc_number, 'number', t.track_number,
                        'isrc', COALESCE((SELECT ia.identifier FROM distribution.identifier_assignments ia
                                           WHERE ia.org_id=t.org_id AND ia.track_id=t.id AND ia.kind='ISRC' AND ia.status='ASSIGNED'
                                           ORDER BY ia.created_at DESC LIMIT 1), t.isrc))
                        ORDER BY t.disc_number, t.track_number)
                      FROM catalog.tracks t WHERE t.org_id=r.org_id AND t.release_id=r.id AND t.archived_at IS NULL), '[]'::jsonb))
           FROM catalog.releases r
          WHERE r.org_id=$1 AND r.archived_at IS NULL AND r.status NOT IN ('DRAFT','SUPERSEDED')
            AND EXISTS(SELECT 1 FROM identity.resource_acl acl WHERE acl.org_id=r.org_id AND acl.resource_id=r.id
                        AND acl.principal_party_id=$2 AND acl.action='read' AND acl.revoked_at IS NULL
                        AND acl.starts_at<=now() AND (acl.ends_at IS NULL OR acl.ends_at>now()))
          ORDER BY r.draft->>'release_date' DESC NULLS LAST, r.title LIMIT $3",
    )
    .bind(org)
    .bind(a.party)
    .bind(MAX_RELEASES)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    for it in &mut items {
        let upc_virtual = it["upc"]
            .as_str()
            .is_some_and(|v| is_virtual(IdentifierKind::Upc, v));
        it["upc_virtual"] = json!(upc_virtual);
        if let Some(tracks) = it["tracks"].as_array_mut() {
            for t in tracks {
                let v = t["isrc"]
                    .as_str()
                    .is_some_and(|v| is_virtual(IdentifierKind::Isrc, v));
                t["isrc_virtual"] = json!(v);
            }
        }
    }
    Ok(items)
}

fn cell(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    // Spreadsheet formula injection: a leading =,+,-,@ is shown as text.
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

pub async fn csv(s: &AppState, a: &Actor, org: Uuid) -> Result<String> {
    let items = list(s, a, org).await?;
    let mut out = String::from(
        "\u{feff}release_id,release,artist,release_date,status,upc,disc,track,track_title,isrc\r\n",
    );
    for r in &items {
        let head = [
            &r["release_id"],
            &r["title"],
            &r["artist"],
            &r["release_date"],
            &r["status"],
            &r["upc"],
        ]
        .map(cell)
        .join(",");
        let tracks = r["tracks"].as_array().cloned().unwrap_or_default();
        if tracks.is_empty() {
            out.push_str(&format!("{head},,,,\r\n"));
        }
        for t in tracks {
            let tail = [&t["disc"], &t["number"], &t["title"], &t["isrc"]]
                .map(cell)
                .join(",");
            out.push_str(&format!("{head},{tail}\r\n"));
        }
    }
    Ok(out)
}

async fn h_list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(json!({"items": list(&s, &a, org).await?})))
}
async fn h_csv(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<axum::response::Response> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    crate::portal::csv_response(csv(&s, &a, org).await?, "codes.csv")
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/codes", get(h_list))
        .route("/api/orgs/{org}/codes.csv", get(h_csv))
}
