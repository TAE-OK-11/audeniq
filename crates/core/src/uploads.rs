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
    let conversion_slot = if matches!(container, "M4A" | "AIFF" | "WAVPACK" | "TTA") {
        Some(
            s.transcode_slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| Error::UploadBusy)?,
        )
    } else {
        None
    };
    let meta = s.storage.head(&key).await?.ok_or(Error::Conflict)?;
    let size: i64 = r.get("expected_bytes");
    let nonce: Uuid = r.get("nonce");
    if meta.size != size || meta.content_type != mime || meta.nonce != nonce.to_string() {
        return Err(Error::Conflict);
    }
    let stable: String = r.get("object_key");
    // Recheck wall clock after HEAD and lock waits, before incurring a copy.
    let valid: bool = sqlx::query_scalar(
        "SELECT expires_at>clock_timestamp() FROM catalog.upload_sessions WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !valid {
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
    if let Some(slot) = conversion_slot {
        let flac = convert_lossless(
            s,
            org,
            asset,
            &stable,
            size,
            &nonce.to_string(),
            container,
            slot,
        )
        .await?;
        finish_session(&mut tx, id).await?;
        sqlx::query("UPDATE catalog.assets SET state='REGISTERED',object_key=$2,content_type='audio/flac',size_bytes=$3,etag=$4,sha256=$5 WHERE id=$1")
            .bind(asset)
            .bind(&flac.key)
            .bind(flac.size)
            .bind(&flac.etag)
            .bind(&flac.sha256)
            .execute(&mut *tx)
            .await?;
        operations::audit(
            &mut tx,
            Some(a.user),
            Some(org),
            Some(asset),
            "upload.completed",
            "LOSSLESS_CONVERTED_TO_FLAC_QC_PENDING",
            a.request,
        )
        .await?;
        queue_analysis(&mut tx, org, asset).await?;
        operations::event(
            &mut tx,
            org,
            asset,
            "asset.registered",
            &format!("asset:{asset}"),
        )
        .await?;
        tx.commit().await?;
        // The quarantine object is not deleted: it is the single-use lock for
        // the upload URL (signed If-None-Match: *). The bucket lifecycle rule
        // removes quarantine/ objects after a day.
        // The frozen upload was only the conversion source.
        drop_quarantine(s, &stable).await;
        return Ok(
            json!({"asset_id":asset,"state":"REGISTERED","qc_status":"PENDING","duplicate":false,"sha256":flac.sha256,"detected_container":"FLAC","converted_from":if container == "M4A" { "ALAC" } else { container }}),
        );
    }
    // Audio: completion reads only the first bytes (content sniff). The
    // frozen copy is immutable (ETag pinned below), and the `asset.analyze`
    // job downloads it once to hash it and run QC, so the master is not
    // transferred twice. Covers and documents are small: hashed here.
    let (head, sha256) = if kind == "AUDIO" {
        (
            s.storage
                .read_prefix(&stable, crate::storage::HEAD_SNIFF_BYTES)
                .await?,
            None,
        )
    } else {
        let digest = match s.storage.digest(&stable, size as u64).await {
            Ok(d) => d,
            Err(Error::PolicyGate("OBJECT_TOO_LARGE")) => return Err(Error::Conflict),
            Err(e) => return Err(e),
        };
        if digest.size != size as u64 {
            return Err(Error::Conflict);
        }
        (digest.head, Some(digest.sha256))
    };
    let detected = crate::qc::detect_container(&head);
    if expected_container(&kind, &mime) != Some(detected) {
        // Nothing is registered: the transaction rolls back, the session
        // stays unusable for these bytes, and the user re-uploads the real
        // master with its real type.
        tracing::info!(%asset, detected, declared = %mime, "upload content mismatch");
        return Err(Error::PolicyGate("UPLOAD_CONTENT_MISMATCH"));
    }
    finish_session(&mut tx, id).await?;
    sqlx::query("UPDATE catalog.assets SET state='REGISTERED',etag=$2,sha256=$3 WHERE id=$1")
        .bind(asset)
        .bind(copy.etag)
        .bind(&sha256)
        .execute(&mut *tx)
        .await?;
    if kind == "AUDIO" {
        queue_analysis(&mut tx, org, asset).await?;
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
    // The quarantine object is not deleted: it is the single-use lock for
    // the upload URL (signed If-None-Match: *). The bucket lifecycle rule
    // removes quarantine/ objects after a day.
    Ok(
        json!({"asset_id":asset,"state":"REGISTERED","qc_status":"PENDING","duplicate":false,"sha256":sha256,"detected_container":detected}),
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
struct ConvertedMaster {
    key: String,
    size: i64,
    etag: String,
    sha256: String,
}

/// Lossless upload → FLAC master. Downloads the frozen, etag-pinned source,
/// verifies identical decoded PCM with SHA-256, and stores the
/// FLAC under a new registered key with the server's key and returns what
/// the asset row must record. One conversion at a time per API process.
#[allow(clippy::too_many_arguments)]
async fn convert_lossless(
    s: &AppState,
    org: Uuid,
    asset: Uuid,
    frozen: &str,
    size: i64,
    nonce: &str,
    container: &'static str,
    slot: tokio::sync::OwnedSemaphorePermit,
) -> Result<ConvertedMaster> {
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let dir = std::env::temp_dir();
    let attempt = Uuid::new_v4();
    let src = Temp(dir.join(format!("audeniq-lossless-{attempt}.source")));
    let dst = Temp(dir.join(format!("audeniq-lossless-{attempt}.flac")));
    let digest = match s.storage.download_to(frozen, &src.0, size as u64).await {
        Ok(d) => d,
        Err(Error::PolicyGate("OBJECT_TOO_LARGE")) => return Err(Error::Conflict),
        Err(e) => return Err(e),
    };
    if digest.size != size as u64 {
        return Err(Error::Conflict);
    }
    if crate::qc::detect_container(&digest.head) != container {
        return Err(Error::PolicyGate("UPLOAD_CONTENT_MISMATCH"));
    }
    let (sha256, dst, _slot) = tokio::task::spawn_blocking(move || {
        // Blocking tasks outlive a cancelled HTTP future. Move the actual
        // guards and permit in so cleanup cannot race the decoder or retry.
        let _src = src;
        let _slot = slot;
        crate::lossless::to_flac(&_src.0, &dst.0, container).map_err(|code| match code {
            "UPLOAD_CONVERSION_UNAVAILABLE" | "UPLOAD_CONVERSION_TIMEOUT" => Error::UploadBusy,
            _ => Error::PolicyGate(code),
        })?;
        let sha = crate::qc::sha256_file(&dst.0)?;
        Ok::<_, Error>((sha, dst, _slot))
    })
    .await
    .map_err(|_| Error::Internal)??;
    let flac_size = tokio::fs::metadata(&dst.0)
        .await
        .map_err(|_| Error::Internal)?
        .len() as i64;
    if flac_size > MAX_AUDIO_BYTES {
        return Err(Error::InvalidCode("UPLOAD_AUDIO_TOO_LARGE"));
    }
    let key = format!("registered/{org}/{asset}/{}", Uuid::new_v4());
    s.storage
        .put_file(&key, &dst.0, "audio/flac", nonce)
        .await?;
    let meta = s.storage.head(&key).await?.ok_or(Error::Storage)?;
    if meta.size != flac_size || meta.content_type != "audio/flac" || meta.nonce != nonce {
        return Err(Error::Storage);
    }
    Ok(ConvertedMaster {
        key,
        size: flac_size,
        etag: meta.etag,
        sha256,
    })
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
