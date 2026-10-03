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
        // WAV (like ALAC/AIFF/WavPack/TTA below) is converted to a FLAC
        // master at completion; FLAC is stored as uploaded.
        ("AUDIO", "audio/wav" | "audio/x-wav") => Some("WAV"),
        ("AUDIO", "audio/flac") => Some("FLAC"),
        // ALAC in an .m4a: converted to FLAC losslessly at completion; AAC
        // (lossy) in the same container is refused there.
        ("AUDIO", "audio/mp4" | "audio/x-m4a") => Some("M4A"),
        ("AUDIO", "audio/aiff" | "audio/x-aiff") => Some("AIFF"),
        ("AUDIO", "audio/wavpack" | "audio/x-wavpack") => Some("WAVPACK"),
        ("AUDIO", "audio/tta" | "audio/x-tta") => Some("TTA"),
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
    auth::rate(&s.pool, &format!("uploads:{}", a.user), 60).await?;
    let mut tx = s.pool.begin().await?;
    let asset = Uuid::new_v4();
    let session = Uuid::new_v4();
    let nonce = Uuid::new_v4();
    auth::create_resource(&mut tx, a, org, asset, "asset").await?;
    let key = format!("quarantine/{org}/{asset}/{nonce}");
    let stable = format!("registered/{org}/{asset}/{}", Uuid::new_v4());
    let expires: DateTime<Utc> = sqlx::query_scalar("SELECT now()+interval '10 minutes'")
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
pub async fn complete(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: CompleteInput,
) -> Result<Value> {
    auth::rate(&s.pool, &format!("upload-complete:{}", a.user), 60).await?;
    // Shed excess completions before they occupy DB connections/row locks.
    // The client retries completion (503 + Retry-After), not the R2 upload.
    let _upload_slot = s
        .upload_slots
        .try_acquire()
        .map_err(|_| Error::UploadBusy)?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, i.asset_id, "asset", true).await?;
    let r=sqlx::query("SELECT u.*,a.object_key,a.kind,(u.expires_at>clock_timestamp()) AS valid FROM catalog.upload_sessions u JOIN catalog.assets a ON a.id=u.asset_id AND a.org_id=u.org_id WHERE u.id=$1 AND u.org_id=$2 FOR UPDATE OF u,a").bind(id).bind(org).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
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
    let kind: String = r.get("kind");
    let mime: String = r.get("content_type");
    let container = expected_container(&kind, &mime).ok_or(Error::Invalid)?;
    // Scanning and normalization are bounded for every kind, including FLAC.
    let slot = s
        .transcode_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::UploadBusy)?;
    let meta = s.storage.head(&key).await?.ok_or(Error::Conflict)?;
    let size: i64 = r.get("expected_bytes");
    let nonce: Uuid = r.get("nonce");
    if meta.size != size || meta.content_type != mime || meta.nonce != nonce.to_string() {
        return Err(Error::Conflict);
    }
    // Freeze into quarantine, never the registered namespace. A cancelled or
    // failed completion leaves no object that any reader can serve or analyze.
    let frozen = format!("quarantine/{org}/{asset}/frozen-{}", Uuid::new_v4());
    let valid: bool = sqlx::query_scalar(
        "SELECT expires_at>clock_timestamp() FROM catalog.upload_sessions WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !valid {
        return Err(Error::Conflict);
    }
    s.storage.freeze(&key, &frozen, &meta.etag).await?;
    let copy = s.storage.head(&frozen).await?.ok_or(Error::Storage)?;
    if copy.size != size
        || copy.content_type != mime
        || copy.nonce != nonce.to_string()
        || copy.etag != meta.etag
    {
        return Err(Error::Conflict);
    }
    let prepared = prepare(
        s,
        org,
        asset,
        &frozen,
        size,
        &nonce.to_string(),
        &kind,
        &mime,
        container,
        slot,
    )
    .await?;
    finish_session(&mut tx, id).await?;
    sqlx::query("INSERT INTO catalog.asset_safety(asset_id,org_id,source_key,source_sha256,safe_key,safe_sha256,rule_version) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(asset).bind(org).bind(&frozen).bind(&prepared.source_sha256).bind(&prepared.key)
        .bind(&prepared.sha256).bind(crate::upload_safety::RULE_VERSION).execute(&mut *tx).await?;
    sqlx::query("UPDATE catalog.assets SET state='REGISTERED',object_key=$2,content_type=$3,size_bytes=$4,etag=$5,sha256=$6 WHERE id=$1")
        .bind(asset).bind(&prepared.key).bind(&prepared.mime).bind(prepared.size)
        .bind(&prepared.etag).bind(&prepared.sha256).execute(&mut *tx).await?;
    if let Some(provenance) = &prepared.provenance {
        sqlx::query("INSERT INTO catalog.asset_provenance(asset_id,org_id,source_sha256,master_sha256,rule_version,body) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(asset).bind(org).bind(&prepared.source_sha256).bind(&prepared.sha256)
            .bind(crate::provenance::RULE_VERSION).bind(provenance).execute(&mut *tx).await?;
    }
    if kind == "AUDIO" {
        queue_analysis(&mut tx, org, asset).await?;
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(asset),
        "upload.completed",
        "MALWARE_CHECKED_AND_NORMALIZED",
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
    // The original signed PUT stays as a single-use lock until lifecycle
    // expiry. The frozen evidence remains private and is never downloadable.
    Ok(
        json!({"asset_id":asset,"state":"REGISTERED","qc_status":"PENDING","duplicate":false,
        "sha256":prepared.sha256,"detected_container":if kind=="AUDIO" { "FLAC" } else { container },
        "converted_from":if kind=="AUDIO" { Some(if container=="M4A" { "ALAC" } else { container }) } else { None },
        "safety_status":"VERIFIED"}),
    )
}

async fn finish_session(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid) -> Result<()> {
    // Check after ALL network IO and conversion, not before hashing/decoding.
    let n = sqlx::query("UPDATE catalog.upload_sessions SET status='COMPLETED',completed_at=clock_timestamp() WHERE id=$1 AND status='ISSUED' AND expires_at>clock_timestamp()")
        .bind(id).execute(&mut **tx).await?.rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    Ok(())
}
struct Prepared {
    key: String,
    size: i64,
    etag: String,
    mime: String,
    sha256: String,
    source_sha256: String,
    provenance: Option<serde_json::Value>,
}

#[allow(clippy::too_many_arguments)]
async fn prepare(
    s: &AppState,
    org: Uuid,
    asset: Uuid,
    frozen: &str,
    size: i64,
    nonce: &str,
    kind: &str,
    mime: &str,
    container: &'static str,
    slot: tokio::sync::OwnedSemaphorePermit,
) -> Result<Prepared> {
    let workspace = crate::upload_safety::Workspace::new()?;
    let src = workspace.0.join("source");
    let dst = workspace.0.join(if kind == "AUDIO" {
        "safe.flac"
    } else if mime == "application/pdf" {
        "safe.pdf"
    } else {
        "safe.image"
    });
    let digest = s.storage.download_to(frozen, &src, size as u64).await?;
    if digest.size != size as u64 {
        return Err(Error::Conflict);
    }
    if crate::qc::detect_container(&digest.head) != container {
        return Err(Error::PolicyGate("UPLOAD_CONTENT_MISMATCH"));
    }
    // No decoder / metadata reader runs before the whole source is scanned.
    crate::upload_safety::scan(&src, digest.size).await?;
    let (kind, mime) = (kind.to_string(), mime.to_string());
    let (workspace, dst, provenance, slot) = tokio::task::spawn_blocking(move || {
        // Guards outlive a cancelled HTTP future, including each decoder.
        let provenance = if kind == "AUDIO" {
            Some(crate::provenance::inspect_audio(&src))
        } else if kind == "IMAGE" {
            Some(crate::provenance::inspect(&src))
        } else {
            None
        };
        if provenance
            .as_ref()
            .is_some_and(|p| p["inspection_status"] != "COMPLETED")
        {
            return Err(Error::UploadBusy);
        }
        if kind == "AUDIO" {
            crate::lossless::to_flac(&src, &dst, container).map_err(|code| match code {
                "UPLOAD_CONVERSION_UNAVAILABLE" | "UPLOAD_CONVERSION_TIMEOUT" => Error::UploadBusy,
                _ => Error::PolicyGate(code),
            })?;
        } else {
            crate::upload_safety::sanitize(&src, &dst, &mime, &workspace.0)?;
        }
        Ok::<_, Error>((workspace, dst, provenance, slot))
    })
    .await
    .map_err(|_| Error::Internal)??;
    let safe_size = tokio::fs::metadata(&dst)
        .await
        .map_err(|_| Error::Storage)?
        .len();
    if safe_size == 0 || safe_size > MAX_AUDIO_BYTES as u64 {
        return Err(Error::PolicyGate("UPLOAD_AUDIO_TOO_LARGE"));
    }
    crate::upload_safety::scan(&dst, safe_size).await?;
    let sha256 = crate::qc::sha256_file(&dst)?;
    let safe_mime = if kind_is_audio(container) {
        "audio/flac"
    } else {
        mime_from_container(container)
    };
    let key = format!("registered/{org}/{asset}/{}", Uuid::new_v4());
    s.storage.put_file(&key, &dst, safe_mime, nonce).await?;
    let meta = s.storage.head(&key).await?.ok_or(Error::Storage)?;
    if meta.size != safe_size as i64 || meta.content_type != safe_mime || meta.nonce != nonce {
        return Err(Error::Storage);
    }
    drop((workspace, slot));
    Ok(Prepared {
        key,
        size: safe_size as i64,
        etag: meta.etag,
        mime: safe_mime.into(),
        sha256,
        source_sha256: digest.sha256,
        provenance,
    })
}

fn kind_is_audio(container: &str) -> bool {
    matches!(
        container,
        "WAV" | "FLAC" | "M4A" | "AIFF" | "WAVPACK" | "TTA"
    )
}
fn mime_from_container(container: &str) -> &'static str {
    match container {
        "JPEG" => "image/jpeg",
        "PNG" => "image/png",
        "PDF" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Queue the pre-submission analysis of a registered audio master
/// (`submission::precheck_asset`): the single full download that hashes the
/// bytes and runs QC before the artist submits.
async fn queue_analysis(tx: &mut sqlx::PgConnection, org: Uuid, asset: Uuid) -> Result<()> {
    operations::enqueue(
        tx,
        "qc",
        "asset.analyze",
        &json!({"org_id": org, "asset_id": asset}),
        &format!("asset.analyze:{asset}"),
        None,
    )
    .await?;
    Ok(())
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
    let status: String = sqlx::query_scalar(
        "SELECT status FROM catalog.upload_sessions WHERE org_id=$1 AND id=$2 FOR UPDATE",
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
    // The quarantine object is not deleted: it is the single-use lock for
    // the upload URL (signed If-None-Match: *). The bucket lifecycle rule
    // removes quarantine/ objects after a day.
    Ok(json!({"cancelled":true,"duplicate":false}))
}
