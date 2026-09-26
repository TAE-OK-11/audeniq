use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadInput {
    pub kind: String,
    pub size_bytes: i64,
    pub content_type: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteInput {
    pub asset_id: Uuid,
    pub expected_key: String,
}
/// Largest accepted audio master (512 MiB; about 50 min of 24-bit/48 kHz stereo WAV).
pub const MAX_AUDIO_BYTES: i64 = 512 * 1024 * 1024;
/// Largest accepted cover-art image (20 MiB).
pub const MAX_IMAGE_BYTES: i64 = 20 * 1024 * 1024;
/// Rights proof documents (PDF or scanned image).
pub const MAX_DOCUMENT_BYTES: i64 = 20 * 1024 * 1024;

/// Container the declared (kind, content type) pair promises, or `None` when
/// the pair is not accepted at all. Upload completion sniffs the real bytes
/// and refuses an object whose magic number does not match, so an MP3 or a
/// FLAC renamed to `.wav` never becomes a registered master.
pub fn expected_container(kind: &str, content_type: &str) -> Option<&'static str> {
    match (kind, content_type) {
        ("AUDIO", "audio/wav" | "audio/x-wav") => Some("WAV"),
        ("AUDIO", "audio/flac") => Some("FLAC"),
        ("IMAGE", "image/jpeg") => Some("JPEG"),
        ("IMAGE", "image/png") => Some("PNG"),
        // Rights proofs (licences, consent letters): PDF or a scan.
        ("DOCUMENT", "application/pdf") => Some("PDF"),
        ("DOCUMENT", "image/jpeg") => Some("JPEG"),
        ("DOCUMENT", "image/png") => Some("PNG"),
        _ => None,
    }
}

pub async fn issue(s: &AppState, a: &Actor, org: Uuid, i: UploadInput) -> Result<Value> {
    if expected_container(&i.kind, &i.content_type).is_none() {
        return Err(Error::InvalidCode("UPLOAD_TYPE_UNSUPPORTED"));
    }
    if i.size_bytes < 1 {
        return Err(Error::InvalidCode("UPLOAD_EMPTY"));
    }
    let (max, too_large) = match i.kind.as_str() {
        "IMAGE" => (MAX_IMAGE_BYTES, "UPLOAD_IMAGE_TOO_LARGE"),
        "DOCUMENT" => (MAX_DOCUMENT_BYTES, "UPLOAD_DOCUMENT_TOO_LARGE"),
        _ => (MAX_AUDIO_BYTES, "UPLOAD_AUDIO_TOO_LARGE"),
    };
    if i.size_bytes > max {
        return Err(Error::InvalidCode(too_large));
    }
    auth::rate(&s.pool, &format!("uploads:{}", a.user), UPLOAD_ISSUE_LIMIT).await?;
    let mut tx = s.pool.begin().await?;
    let asset = Uuid::new_v4();
    let session = Uuid::new_v4();
    let nonce = Uuid::new_v4();
    auth::create_resource(&mut tx, a, org, asset, "asset").await?;
    let key = format!("quarantine/{org}/{asset}/{nonce}");
    let stable = format!("registered/{org}/{asset}/{}", Uuid::new_v4());
    let expires: DateTime<Utc> = sqlx::query_scalar("SELECT now()+interval '15 minutes'")
        .fetch_one(&mut *tx)
        .await?;
    let grant = s
        .storage
        .presign_put(
            &key,
            i.size_bytes,
            &i.content_type,
            &nonce.to_string(),
            expires,
        )
        .await?;
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type) VALUES($1,$2,$3,$4,$5,$6)").bind(asset).bind(org).bind(i.kind).bind(stable).bind(i.size_bytes).bind(&i.content_type).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO catalog.upload_sessions(id,org_id,asset_id,expected_key,nonce,expected_bytes,content_type,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
 .bind(session).bind(org).bind(asset).bind(&key).bind(nonce).bind(i.size_bytes).bind(i.content_type).bind(expires).execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(asset),
        "upload.issued",
        "PRIVATE_QUARANTINE",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"upload_session_id":session,"asset_id":asset,"expected_key":key,"grant":grant,"qc_status":"PENDING"}),
    )
}
/// Upload issue / completion budget per user per 15-minute window. Sized for
/// a label delivering a large catalog (a 1,000-track batch plus artwork in
/// one window) while still bounding one account's storage churn.
pub const UPLOAD_ISSUE_LIMIT: i32 = 2_000;
pub const UPLOAD_COMPLETE_LIMIT: i32 = 2_000;

/// Complete an upload: verify the quarantined object, copy it to its
/// immutable key, hash it and register the asset.
///
/// The storage work (HEAD, server-side copy, streaming SHA-256 of up to
/// 512 MiB) runs with **no database connection or transaction held**. The
/// old implementation kept a transaction with row locks open across all of
/// it, so a handful of concurrent large uploads exhausted the API pool and
/// every other request failed. Correctness is kept by re-checking state in
/// short transactions around the IO:
///
/// 1. authorize + validate the session (ISSUED, unexpired, key matches);
/// 2. HEAD + copy + HEAD (no DB);
/// 3. re-check expiry on the wall clock after that IO, before hashing;
/// 4. stream the digest and sniff the container (no DB);
/// 5. lock the session and register only if it is still ISSUED: a cancel
///    during the IO wins (CONFLICT), and a concurrent duplicate completion
///    converges on `duplicate: true`.
pub async fn complete(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: CompleteInput,
) -> Result<Value> {
    auth::rate(
        &s.pool,
        &format!("upload-complete:{}", a.user),
        UPLOAD_COMPLETE_LIMIT,
    )
    .await?;
    // Phase 1: short validation transaction.
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, i.asset_id, "asset", true).await?;
    let r=sqlx::query("SELECT u.*,a.object_key,(u.expires_at>clock_timestamp()) AS valid FROM catalog.upload_sessions u JOIN catalog.assets a ON a.id=u.asset_id AND a.org_id=u.org_id WHERE u.id=$1 AND u.org_id=$2").bind(id).bind(org).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    let asset: Uuid = r.get("asset_id");
    let key: String = r.get("expected_key");
    if asset != i.asset_id || key != i.expected_key {
        return Err(Error::Forbidden);
    }
    if r.get::<String, _>("status") == "COMPLETED" {
        return Ok(
            json!({"asset_id":asset,"state":"REGISTERED","qc_status":"PENDING","duplicate":true}),
        );
    }
    if r.get::<String, _>("status") != "ISSUED" || !r.get::<bool, _>("valid") {
        return Err(Error::Conflict);
    }
    let size: i64 = r.get("expected_bytes");
    let mime: String = r.get("content_type");
    let nonce: Uuid = r.get("nonce");
    let stable: String = r.get("object_key");

    // Phase 2: storage verification and immutable copy, no DB held.
    let meta = s.storage.head(&key).await?.ok_or(Error::Conflict)?;
    if meta.size != size || meta.content_type != mime || meta.nonce != nonce.to_string() {
        return Err(Error::Conflict);
    }
    // Recheck wall clock after HEAD, before incurring a copy.
    if !session_unexpired(s, id).await? {
        return Err(Error::Conflict);
    }
    s.storage.freeze(&key, &stable, &meta.etag).await?;
    let copy = s.storage.head(&stable).await?.ok_or(Error::Storage)?;
    if copy.size != size
        || copy.content_type != mime
        || copy.nonce != nonce.to_string()
        || copy.etag != meta.etag
    {
        return Err(Error::Conflict);
    }
    // Phase 3: expiry is checked again after network IO using wall clock.
    if !session_unexpired(s, id).await? {
        return Err(Error::Conflict);
    }

    // Phase 4: content verification. The frozen copy is immutable, so
    // hashing it here binds catalog.assets.sha256 to exactly the bytes every
    // later stage reads (Stage 1 re-verifies the hash before analysis).
    // Streaming keeps API memory constant even for 512 MiB masters.
    let digest = match s.storage.digest(&stable, size as u64).await {
        Ok(d) => d,
        Err(Error::PolicyGate("OBJECT_TOO_LARGE")) => return Err(Error::Conflict),
        Err(e) => return Err(e),
    };
    if digest.size != size as u64 {
        return Err(Error::Conflict);
    }
    let kind: String = sqlx::query_scalar("SELECT kind FROM catalog.assets WHERE id=$1")
        .bind(asset)
        .fetch_one(&s.pool)
        .await?;
    let detected = crate::qc::detect_container(&digest.head);
    if expected_container(&kind, &mime) != Some(detected) {
        // Nothing is registered: the session stays ISSUED but unusable for
        // these bytes, and the user re-uploads the real master with its
        // real type.
        tracing::info!(%asset, detected, declared = %mime, "upload content mismatch");
        return Err(Error::PolicyGate("UPLOAD_CONTENT_MISMATCH"));
    }

    // Phase 5: register, only if nothing changed the session meanwhile.
    let mut tx = s.pool.begin().await?;
    // Membership/ACL may have been revoked during the IO.
    auth::authorize(&mut tx, a, org, asset, "asset", true).await?;
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM catalog.upload_sessions WHERE id=$1 AND org_id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    match status.as_deref() {
        Some("ISSUED") => {}
        Some("COMPLETED") => {
            tx.commit().await?;
            return Ok(
                json!({"asset_id":asset,"state":"REGISTERED","qc_status":"PENDING","duplicate":true}),
            );
        }
        _ => return Err(Error::Conflict),
    }
    sqlx::query("UPDATE catalog.upload_sessions SET status='COMPLETED',completed_at=clock_timestamp() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let n = sqlx::query("UPDATE catalog.assets SET state='REGISTERED',etag=$2,sha256=$3 WHERE id=$1 AND state='UPLOADING'")
        .bind(asset)
        .bind(copy.etag)
        .bind(&digest.sha256)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(asset),
        "upload.completed",
        "OBJECT_VERIFIED_QC_PENDING",
        a.request,
    )
    .await?;
    operations::event(
        &mut tx,
        org,
        asset,
        "asset.registered",
        &format!("asset:{asset}"),
    )
    .await?;
    tx.commit().await?;
    drop_quarantine(s, &key).await;
    Ok(
        json!({"asset_id":asset,"state":"REGISTERED","qc_status":"PENDING","duplicate":false,"sha256":digest.sha256,"detected_container":detected}),
    )
}
/// Wall-clock expiry check for one upload session (no lock held).
async fn session_unexpired(s: &AppState, id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT expires_at>clock_timestamp() FROM catalog.upload_sessions WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&s.pool)
    .await?
    .unwrap_or(false))
}
/// Best-effort removal of the quarantine object after complete/cancel
/// (sandbox round 2: quarantine copies were never deleted). A presigned PUT
/// cannot be revoked, so a client may still write the key until the grant
/// expires; the bucket's `quarantine/` lifecycle rule (docs/API.md) removes
/// such leftovers. Failure never fails the request: the registered copy and
/// the session state are already committed.
async fn drop_quarantine(s: &AppState, key: &str) {
    if let Err(error) = s.storage.delete(key).await {
        tracing::warn!(
            key,
            ?error,
            "quarantine object delete failed; lifecycle rule will expire it"
        );
    }
}
pub async fn get(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, id, "asset", false).await?;
    let v:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'kind',kind,'state',state,'qc_status',qc_status,'size_bytes',size_bytes,'content_type',content_type,'sha256',sha256) FROM catalog.assets WHERE org_id=$1 AND id=$2").bind(org).bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(v)
}

pub async fn status(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let row = sqlx::query("SELECT id,asset_id,status,expires_at,completed_at,(expires_at<=now()) AS expired FROM catalog.upload_sessions WHERE org_id=$1 AND id=$2")
        .bind(org).bind(id).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
    let asset: Uuid = row.get("asset_id");
    auth::authorize(&mut tx, a, org, asset, "asset", false).await?;
    let expires: DateTime<Utc> = row.get("expires_at");
    let completed: Option<DateTime<Utc>> = row.get("completed_at");
    let status: String = row.get("status");
    let expired: bool = row.get("expired");
    tx.commit().await?;
    Ok(
        json!({"upload_session_id":id,"asset_id":asset,"status":status,"expires_at":expires,"completed_at":completed,"expired":expired}),
    )
}
pub async fn cancel(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    // Authorize before acquiring upload locks, matching complete() lock order.
    let asset: Uuid = sqlx::query_scalar(
        "SELECT asset_id FROM catalog.upload_sessions WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    auth::authorize(&mut tx, a, org, asset, "asset", true).await?;
    let (status, key): (String, String) = sqlx::query_as(
        "SELECT status, expected_key FROM catalog.upload_sessions WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if status == "COMPLETED" {
        return Err(Error::Conflict);
    }
    if status == "CANCELLED" {
        return Ok(json!({"cancelled":true,"duplicate":true}));
    }
    sqlx::query("UPDATE catalog.upload_sessions SET status='CANCELLED' WHERE org_id=$1 AND id=$2")
        .bind(org)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE catalog.assets SET state='REJECTED' WHERE org_id=$1 AND id=$2 AND state='UPLOADING'")
        .bind(org).bind(asset).execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(asset),
        "upload.cancelled",
        "USER_REQUEST",
        a.request,
    )
    .await?;
    operations::event(
        &mut tx,
        org,
        asset,
        "asset.upload_cancelled",
        &format!("upload-cancel:{id}"),
    )
    .await?;
    tx.commit().await?;
    drop_quarantine(s, &key).await;
    Ok(json!({"cancelled":true,"duplicate":false}))
}
