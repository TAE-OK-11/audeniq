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
    /// Ask for a resumable (multipart) upload. Only audio larger than one
    /// part uses it; everything else still gets a single signed PUT.
    #[serde(default)]
    pub resumable: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartsInput {
    pub part_numbers: Vec<i32>,
}
/// Part size of a resumable upload (512 MiB = 32 parts). R2 needs equal
/// parts except the last; S3 and OCI need at least 5 MiB.
pub const PART_SIZE: i64 = 16 * 1024 * 1024;
/// A resumable session stays open a day: long enough to resume after a
/// dropped connection or a closed laptop, short enough for quarantine.
const RESUMABLE_HOURS: i64 = 24;
/// Part URLs live this long; the device asks for more as it goes.
const PART_URL_MINUTES: i64 = 15;
/// Part URLs per request.
const PARTS_PER_REQUEST: usize = 8;

fn part_count(size: i64, part_size: i64) -> i64 {
    (size + part_size - 1) / part_size
}
fn part_len(size: i64, part_size: i64, part: i64) -> i64 {
    (size - (part - 1) * part_size).min(part_size)
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
        // Every lossless input, including FLAC, is normalized to a FLAC
        // master at completion to remove source metadata and attached files.
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
    let asset = Uuid::new_v4();
    let session = Uuid::new_v4();
    let nonce = Uuid::new_v4();
    let key = format!("quarantine/{org}/{asset}/{nonce}");
    if i.resumable && i.kind == "AUDIO" && i.size_bytes > PART_SIZE {
        return issue_resumable(s, a, org, i, asset, session, nonce, key).await;
    }
    let mut tx = s.pool.begin().await?;
    auth::create_resource(&mut tx, a, org, asset, "asset").await?;
    let stable = format!("registered/{org}/{asset}/{}", Uuid::new_v4());
    // The signed PUT URL lives 10 minutes. Completion stays open 30 minutes
    // longer (`expires_at + 30 minutes`, here and in `complete`): the object
    // can only exist if it was PUT through this grant before the URL expired,
    // and a client waiting out capacity (503) must not lose an uploaded file.
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
/// Resumable upload: the store's multipart upload at the quarantine key.
/// The device asks for part URLs as it goes ([`part_urls`]) and, after a
/// reload, for the parts already stored ([`stored_parts`]).
#[allow(clippy::too_many_arguments)]
async fn issue_resumable(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    i: UploadInput,
    asset: Uuid,
    session: Uuid,
    nonce: Uuid,
    key: String,
) -> Result<Value> {
    // Membership is checked before anything is created in the store.
    {
        // One autocommit statement: its FOR SHARE locks end with it, as they
        // did at the rollback of a transaction around it.
        auth::membership(&mut *s.pool.acquire().await?, a, org, true).await?;
    }
    // Network call outside any transaction. If the rows below fail, the
    // bucket's abort-incomplete-multipart lifecycle rule drops the upload.
    let upload_id = s
        .storage
        .create_multipart(&key, &i.content_type, &nonce.to_string())
        .await?;
    let mut tx = s.pool.begin().await?;
    auth::create_resource(&mut tx, a, org, asset, "asset").await?;
    let stable = format!("registered/{org}/{asset}/{}", Uuid::new_v4());
    let expires: DateTime<Utc> = sqlx::query_scalar("SELECT now() + make_interval(hours => $1)")
        .bind(RESUMABLE_HOURS as i32)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type) VALUES($1,$2,$3,$4,$5,$6)").bind(asset).bind(org).bind(&i.kind).bind(stable).bind(i.size_bytes).bind(&i.content_type).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO catalog.upload_sessions(id,org_id,asset_id,expected_key,nonce,expected_bytes,content_type,expires_at,multipart_upload_id,part_size) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(session).bind(org).bind(asset).bind(&key).bind(nonce).bind(i.size_bytes).bind(&i.content_type).bind(expires).bind(&upload_id).bind(PART_SIZE)
        .execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(asset),
        "upload.issued",
        "PRIVATE_QUARANTINE_MULTIPART",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({
        "upload_session_id": session, "asset_id": asset, "expected_key": key,
        "multipart": {"part_size": PART_SIZE, "part_count": part_count(i.size_bytes, PART_SIZE)},
        "expires_at": expires, "qc_status": "PENDING",
    }))
}

struct OpenMultipart {
    key: String,
    upload_id: String,
    size: i64,
    part_size: i64,
    expires: DateTime<Utc>,
}

/// An open resumable session the caller may write to.
async fn open_multipart(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    write: bool,
) -> Result<OpenMultipart> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, write).await?;
    let r = sqlx::query(
        "SELECT asset_id, expected_key, multipart_upload_id, part_size, expected_bytes, expires_at, status,
                (expires_at > now()) AS open
           FROM catalog.upload_sessions WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    auth::authorize(&mut tx, a, org, r.get("asset_id"), "asset", write).await?;
    tx.rollback().await?;
    let (Some(upload_id), Some(part_size)) = (
        r.get::<Option<String>, _>("multipart_upload_id"),
        r.get::<Option<i64>, _>("part_size"),
    ) else {
        return Err(Error::PolicyGate("UPLOAD_NOT_RESUMABLE"));
    };
    if r.get::<String, _>("status") != "ISSUED" || !r.get::<bool, _>("open") {
        return Err(Error::Conflict);
    }
    Ok(OpenMultipart {
        key: r.get("expected_key"),
        upload_id,
        size: r.get("expected_bytes"),
        part_size,
        expires: r.get("expires_at"),
    })
}

/// Signed PUT URLs for the next parts (each bound to its exact size).
pub async fn part_urls(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: PartsInput,
) -> Result<Value> {
    if i.part_numbers.is_empty() || i.part_numbers.len() > PARTS_PER_REQUEST {
        return Err(Error::InvalidCode("UPLOAD_PARTS_INVALID"));
    }
    auth::rate(&s.pool, &format!("upload-parts:{}", a.user), 2000).await?;
    let m = open_multipart(s, a, org, id, true).await?;
    let count = part_count(m.size, m.part_size);
    let expires = (Utc::now() + chrono::Duration::minutes(PART_URL_MINUTES)).min(m.expires);
    let mut parts = Vec::with_capacity(i.part_numbers.len());
    for n in i.part_numbers {
        if !(1..=count).contains(&i64::from(n)) {
            return Err(Error::InvalidCode("UPLOAD_PARTS_INVALID"));
        }
        let size = part_len(m.size, m.part_size, i64::from(n));
        let g = s
            .storage
            .presign_part(&m.key, &m.upload_id, n, size, expires)
            .await?;
        parts.push(json!({"part_number": n, "size_bytes": size, "grant": g}));
    }
    Ok(json!({"parts": parts}))
}

/// Parts already stored (where a resumed upload continues).
pub async fn stored_parts(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let m = open_multipart(s, a, org, id, false).await?;
    let count = part_count(m.size, m.part_size);
    let listed = s.storage.list_parts(&m.key, &m.upload_id).await?;
    // Only parts of the expected size count as done; a wrong one is re-sent.
    let done: Vec<Value> = listed
        .iter()
        .filter(|p| {
            (1..=count).contains(&i64::from(p.number))
                && p.size == part_len(m.size, m.part_size, i64::from(p.number))
        })
        .map(|p| json!({"part_number": p.number, "size_bytes": p.size}))
        .collect();
    Ok(
        json!({"part_size": m.part_size, "part_count": count, "parts": done, "expires_at": m.expires}),
    )
}

/// Finish a multipart upload before completion's checks: every part must be
/// stored with its expected size. A retried completion whose upload already
/// finished finds the object and skips this.
async fn finish_multipart(
    s: &AppState,
    key: &str,
    upload_id: &str,
    size: i64,
    part_size: i64,
) -> Result<()> {
    if s.storage.head(key).await?.is_some() {
        return Ok(());
    }
    let count = part_count(size, part_size);
    let mut parts = s.storage.list_parts(key, upload_id).await?;
    parts.sort_by_key(|p| p.number);
    let complete = parts.len() as i64 == count
        && parts.iter().enumerate().all(|(i, p)| {
            i64::from(p.number) == i as i64 + 1 && p.size == part_len(size, part_size, i as i64 + 1)
        });
    if !complete {
        return Err(Error::PolicyGate("UPLOAD_PARTS_INCOMPLETE"));
    }
    s.storage.complete_multipart(key, upload_id, &parts).await
}

/// Time one completion step and, when it fails, log which step, how long it
/// took and the error code under the request id (BE-012: the storage error
/// could not be traced to a step). No keys or URLs are logged.
async fn stage<T>(
    request: Uuid,
    name: &'static str,
    f: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let started = std::time::Instant::now();
    let out = f.await;
    if let Err(e) = &out {
        tracing::warn!(
            request_id = %request,
            upload_stage = name,
            elapsed_ms = started.elapsed().as_millis() as u64,
            error = %e,
            "upload_stage_failed"
        );
    }
    out
}

pub async fn complete(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: CompleteInput,
) -> Result<Value> {
    // Shed excess completions before they occupy DB connections/row locks.
    // The client retries completion (503 + Retry-After), not the R2 upload.
    // Both slots come before the rate limit: a completion refused for
    // capacity was never processed and must not use up the account's quota
    // (100-album test BE-011: busy retries exhausted it, the session expired).
    let _upload_slot = s
        .upload_slots
        .try_acquire()
        .map_err(|_| Error::UploadBusy)?;
    let started = std::time::Instant::now();
    auth::rate(&s.pool, &format!("upload-complete:{}", a.user), 60).await?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, i.asset_id, "asset", true).await?;
    let r=sqlx::query("SELECT u.*,a.object_key,a.kind,s.antivirus_status,(u.expires_at+interval '30 minutes'>clock_timestamp()) AS valid FROM catalog.upload_sessions u JOIN catalog.assets a ON a.id=u.asset_id AND a.org_id=u.org_id LEFT JOIN catalog.asset_safety s ON s.asset_id=a.id AND s.org_id=a.org_id AND s.safe_key=a.object_key AND s.safe_sha256=a.sha256 WHERE u.id=$1 AND u.org_id=$2 FOR UPDATE OF u,a").bind(id).bind(org).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
    let asset: Uuid = r.get("asset_id");
    let key: String = r.get("expected_key");
    if asset != i.asset_id || key != i.expected_key {
        return Err(Error::Forbidden);
    }
    if r.get::<String, _>("status") == "COMPLETED" {
        let status = r
            .get::<Option<String>, _>("antivirus_status")
            .ok_or(Error::PolicyGate("UPLOAD_REINSPECTION_REQUIRED"))?;
        if s.antivirus_enabled && status != "SCANNED" {
            return Err(Error::PolicyGate("UPLOAD_REINSPECTION_REQUIRED"));
        }
        return Ok(
            json!({"asset_id":asset,"state":"REGISTERED","qc_status":"PENDING","duplicate":true,"safety_status":crate::upload_safety::safety_status(&status)}),
        );
    }
    if r.get::<String, _>("status") != "ISSUED" || !r.get::<bool, _>("valid") {
        return Err(Error::Conflict);
    }
    let kind: String = r.get("kind");
    let mime: String = r.get("content_type");
    let container = expected_container(&kind, &mime).ok_or(Error::Invalid)?;
    let size: i64 = r.get("expected_bytes");
    let nonce: Uuid = r.get("nonce");
    // Never keep a DB transaction (and the session row lock) open across
    // storage calls, scans or decoding. Cancellation and concurrent
    // completion are checked again under a fresh row lock; the download is
    // conditional on the ETag seen here.
    let multipart: Option<(String, i64)> = r
        .get::<Option<String>, _>("multipart_upload_id")
        .zip(r.get::<Option<i64>, _>("part_size"));
    tx.commit().await?;
    if let Some((upload_id, part_size)) = &multipart {
        stage(
            a.request,
            "multipart",
            finish_multipart(s, &key, upload_id, size, *part_size),
        )
        .await?;
    }
    let meta = stage(a.request, "head", s.storage.head(&key))
        .await?
        .ok_or(Error::Conflict)?;
    if meta.size != size || meta.content_type != mime || meta.nonce != nonce.to_string() {
        return Err(Error::Conflict);
    }
    // Read the quarantine object directly, pinned to the ETag seen above
    // (If-Match). It used to be copied to a "frozen" key first; that copy
    // took as long as the download itself for large masters. The signed PUT
    // is single use (If-None-Match), so the object cannot change, and the
    // source stays private in quarantine/ until the lifecycle rule removes it.
    // Nothing is written outside quarantine/ before the checks pass.
    let prepared = stage(
        a.request,
        "prepare",
        Box::pin(prepare(
            s,
            org,
            asset,
            &key,
            &meta.etag,
            size,
            &nonce.to_string(),
            &kind,
            &mime,
            container,
        )),
    )
    .await?;
    // Registering may still fail (cancelled or expired meanwhile, DB error):
    // then the converted master in `registered/` is orphaned — remove it.
    let registered = prepared.key.clone();
    let out: Result<Value> = Box::pin(async {
        let mut tx = s.pool.begin().await?;
        auth::authorize(&mut tx, a, org, asset, "asset", true).await?;
        let status: String = sqlx::query_scalar(
            "SELECT status FROM catalog.upload_sessions WHERE id=$1 AND org_id=$2 FOR UPDATE",
        )
        .bind(id)
        .bind(org)
        .fetch_one(&mut *tx)
        .await?;
        if status == "COMPLETED" {
            tx.commit().await?;
            let _ = s.storage.delete(&prepared.key).await;
            crate::upload_safety::require_policy(&s.pool, org, asset, s.antivirus_enabled).await?;
            let recorded: String = sqlx::query_scalar(
                "SELECT antivirus_status FROM catalog.asset_safety WHERE asset_id=$1 AND org_id=$2",
            )
            .bind(asset)
            .bind(org)
            .fetch_one(&s.pool)
            .await?;
            return Ok(
                json!({"asset_id":asset,"state":"REGISTERED","qc_status":"PENDING","duplicate":true,"safety_status":crate::upload_safety::safety_status(&recorded)}),
            );
        }
        if status != "ISSUED" {
            return Err(Error::Conflict);
        }
        finish_session(&mut tx, id).await?;
        let antivirus_status = crate::upload_safety::antivirus_status(s.antivirus_enabled);
        sqlx::query("INSERT INTO catalog.asset_safety(asset_id,org_id,source_key,source_sha256,safe_key,safe_sha256,rule_version,antivirus_status) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(asset).bind(org).bind(&key).bind(&prepared.source_sha256).bind(&prepared.key)
            .bind(&prepared.sha256).bind(crate::upload_safety::RULE_VERSION).bind(antivirus_status).execute(&mut *tx).await?;
        sqlx::query("UPDATE catalog.assets SET state='REGISTERED',object_key=$2,content_type=$3,size_bytes=$4,etag=$5,sha256=$6 WHERE id=$1")
            .bind(asset).bind(&prepared.key).bind(&prepared.mime).bind(prepared.size)
            .bind(&prepared.etag).bind(&prepared.sha256).execute(&mut *tx).await?;
        if let Some(provenance) = &prepared.provenance {
            sqlx::query("INSERT INTO catalog.asset_provenance(asset_id,org_id,source_sha256,master_sha256,rule_version,body) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(asset).bind(org).bind(&prepared.source_sha256).bind(&prepared.sha256)
                .bind(crate::provenance::RULE_VERSION).bind(provenance).execute(&mut *tx).await?;
        }
        if kind == "AUDIO" {
            // The early answer measured during the conversion. The worker's
            // asset.analyze still stores the results and the fingerprint
            // (and keeps this first answer).
            if let Some(answer) = prepared.qc_answer {
                sqlx::query("UPDATE catalog.assets SET qc_status=$3 WHERE org_id=$1 AND id=$2 AND qc_status='PENDING'")
                    .bind(org)
                    .bind(asset)
                    .bind(answer)
                    .execute(&mut *tx)
                    .await?;
            }
            queue_analysis(&mut tx, org, asset).await?;
        }
        operations::audit(
            &mut tx,
            Some(a.user),
            Some(org),
            Some(asset),
            "upload.completed",
            if s.antivirus_enabled {
                "MALWARE_CHECKED_AND_NORMALIZED"
            } else {
                "NORMALIZED_AV_DISABLED"
            },
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
        tracing::info!(
            request_id = %a.request,
            kind = %kind,
            bytes = size,
            download_ms = prepared.timing[0],
            queue_ms = prepared.timing[1],
            convert_ms = prepared.timing[2],
            store_ms = prepared.timing[3],
            qc_answer = prepared.qc_answer.unwrap_or("PENDING"),
            total_ms = started.elapsed().as_millis() as u64,
            "upload_completed"
        );
        // The original signed PUT stays as a single-use lock until lifecycle
        // expiry: private, never downloadable, removed by the lifecycle rule.
        Ok(
            json!({"asset_id":asset,"state":"REGISTERED","qc_status":prepared.qc_answer.unwrap_or("PENDING"),"duplicate":false,
            "sha256":prepared.sha256,"detected_container":if kind=="AUDIO" { "FLAC" } else { container },
            "converted_from":if kind=="AUDIO" { Some(if container=="M4A" { "ALAC" } else { container }) } else { None },
            "safety_status":crate::upload_safety::safety_status(antivirus_status)}),
        )
    })
    .await;
    if out.is_err() {
        let _ = s.storage.delete(&registered).await;
    }
    out
}

async fn finish_session(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid) -> Result<()> {
    // Check after ALL network IO and conversion, not before hashing/decoding.
    let n = sqlx::query("UPDATE catalog.upload_sessions SET status='COMPLETED',completed_at=clock_timestamp() WHERE id=$1 AND status='ISSUED' AND expires_at+interval '30 minutes'>clock_timestamp()")
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
    /// Audio: `qc_status` answered from the conversion's own analysis
    /// (`PASS`/`BLOCKED`), so the review shows at once. `None`: pending
    /// until the worker's `asset.analyze`.
    qc_answer: Option<&'static str>,
    /// Milliseconds: download, waiting for the conversion slot, conversion
    /// (and checks), storing the result. Logged with `upload_completed`.
    timing: [u64; 4],
}

#[allow(clippy::too_many_arguments)]
async fn prepare(
    s: &AppState,
    org: Uuid,
    asset: Uuid,
    source: &str,
    etag: &str,
    size: i64,
    nonce: &str,
    kind: &str,
    mime: &str,
    container: &'static str,
) -> Result<Prepared> {
    let mut clock = std::time::Instant::now();
    let mut lap = || {
        let ms = clock.elapsed().as_millis() as u64;
        clock = std::time::Instant::now();
        ms
    };
    let workspace = crate::upload_safety::Workspace::new()?;
    let src = workspace.0.join("source");
    let dst = workspace.0.join(if kind == "AUDIO" {
        "safe.flac"
    } else if mime == "application/pdf" {
        "safe.pdf"
    } else {
        "safe.image"
    });
    let digest = s
        .storage
        .download_version_to(source, &src, size as u64, etag)
        .await?;
    if digest.size != size as u64 {
        return Err(Error::Conflict);
    }
    if crate::qc::detect_container(&digest.head) != container {
        return Err(Error::PolicyGate("UPLOAD_CONTENT_MISMATCH"));
    }
    // When enabled, scan the whole source before any decoder / metadata reader.
    crate::upload_safety::scan(&src, digest.size, s.antivirus_enabled).await?;
    let download_ms = lap();
    // One conversion at a time (CPU and temp disk), taken only now: the
    // download above and the upload below run without it. Completions wait
    // in line (FIFO) instead of bouncing to the client with UPLOAD_BUSY.
    let slot = tokio::time::timeout(s.transcode_wait, s.transcode_slots.clone().acquire_owned())
        .await
        .map_err(|_| Error::UploadBusy)?
        .map_err(|_| Error::UploadBusy)?;
    let queue_ms = lap();
    let (kind, mime) = (kind.to_string(), mime.to_string());
    let (workspace, dst, provenance, sha256, qc_answer) = tokio::task::spawn_blocking(move || {
        // Guards outlive a cancelled HTTP future, including each decoder.
        // Convert / sanitize first: a bad file gets its precise 4xx here
        // instead of a metadata-reader failure answered as "busy, retry".
        // Audio: the engine measures the audio for QC in the same decode
        // that makes the FLAC master, for an immediate review answer.
        let analysis = if kind == "AUDIO" {
            crate::lossless::to_flac_analyzed(&src, &dst, container).map_err(|code| match code {
                "UPLOAD_CONVERSION_UNAVAILABLE" | "UPLOAD_CONVERSION_TIMEOUT" => Error::UploadBusy,
                _ => Error::PolicyGate(code),
            })?
        } else {
            crate::upload_safety::sanitize(&src, &dst, &mime, &workspace.0)?;
            None
        };
        let provenance = if kind == "AUDIO" {
            Some(crate::provenance::inspect_audio(&src))
        } else if kind == "IMAGE" {
            Some(crate::provenance::inspect(&src))
        } else {
            None
        };
        if let Some(p) = provenance
            .as_ref()
            .filter(|p| p["inspection_status"] != "COMPLETED")
        {
            return Err(provenance_error(p));
        }
        // Hash on the blocking pool, not a runtime thread (up to 512 MiB).
        let sha256 = crate::qc::sha256_file(&dst)?;
        // The draft's QC answer from that analysis (the worker still stores
        // the results and fingerprint after downloading the master).
        let qc_answer = match (analysis, &provenance) {
            (Some(a), Some(p)) => crate::submission::upload_qc_answer(&dst, &sha256, a, p),
            _ => None,
        };
        // The single transcode slot covers decoding only: the AV scan of the
        // result and the upload to storage below no longer hold it, so other
        // completions are not serialized on network time.
        drop(slot);
        Ok::<_, Error>((workspace, dst, provenance, sha256, qc_answer))
    })
    .await
    .map_err(|_| Error::Internal)??;
    let convert_ms = lap();
    let safe_size = tokio::fs::metadata(&dst)
        .await
        .map_err(|_| Error::Storage)?
        .len();
    if safe_size == 0 || safe_size > MAX_AUDIO_BYTES as u64 {
        return Err(Error::PolicyGate("UPLOAD_AUDIO_TOO_LARGE"));
    }
    crate::upload_safety::scan(&dst, safe_size, s.antivirus_enabled).await?;
    let safe_mime = if kind_is_audio(container) {
        "audio/flac"
    } else {
        mime_from_container(container)
    };
    let key = format!("registered/{org}/{asset}/{}", Uuid::new_v4());
    s.storage.put_file(&key, &dst, safe_mime, nonce).await?;
    let meta = match s.storage.head(&key).await {
        Ok(Some(m))
            if m.size == safe_size as i64 && m.content_type == safe_mime && m.nonce == nonce =>
        {
            m
        }
        Ok(_) => {
            let _ = s.storage.delete(&key).await;
            return Err(Error::Storage);
        }
        Err(e) => {
            let _ = s.storage.delete(&key).await;
            return Err(e);
        }
    };
    // Removing the source and FLAC copies is file-system work: off the runtime.
    tokio::task::spawn_blocking(move || drop(workspace));
    Ok(Prepared {
        key,
        size: safe_size as i64,
        etag: meta.etag,
        mime: safe_mime.into(),
        sha256,
        source_sha256: digest.sha256,
        provenance,
        qc_answer,
        timing: [download_ms, queue_ms, convert_ms, lap()],
    })
}

/// A metadata reader that rejected an already converted/sanitized file
/// found the file itself unreadable (422); anything else (reader missing,
/// sandbox unavailable, timeout) is capacity, retried as 503.
fn provenance_error(report: &serde_json::Value) -> Error {
    let reason = report["reason"].as_str().unwrap_or_default();
    if reason.contains("rejected the file") || reason.contains("invalid report") {
        Error::PolicyGate("UPLOAD_CONTENT_MISMATCH")
    } else {
        Error::UploadBusy
    }
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

/// Checks never shown to the artist from the pre-submission analysis: the
/// similarity result can name other organizations' assets, and the
/// provenance detail is the raw metadata report (its status is enough).
const PRIVATE_DETAIL: &[&str] = &["AUDIO_SIMILAR_TO_EXISTING", "AUDIO_AI_PROVENANCE"];

/// `GET /api/orgs/{org}/assets/{id}/checks`: the pre-submission QC of an
/// uploaded track, check by check (what the worker's asset.analyze stored),
/// so the studio can say which track needs fixing and why before the
/// release is submitted. Empty while the analysis is pending.
pub async fn checks(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, id, "asset", false).await?;
    let (kind, qc_status, sha256): (String, String, Option<String>) = sqlx::query_as(
        "SELECT kind, qc_status, sha256 FROM catalog.assets WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let Some(sha) = sha256.filter(|_| kind == "AUDIO") else {
        tx.commit().await?;
        return Ok(json!({"asset_id": id, "qc_status": qc_status, "checks": []}));
    };
    let aid = id.to_string();
    let codes: Vec<&str> = crate::qc::AUDIO_CHECK_CODES
        .iter()
        .copied()
        .filter(|c| *c != "AUDIO_SIMILAR_TO_EXISTING")
        .collect();
    let hashes: Vec<String> = codes
        .iter()
        .map(|c| crate::submission::asset_check_cache_key(c, &aid, &sha))
        .collect();
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT r.check_code, r.status, r.detail
           FROM unnest($1::text[], $2::text[]) AS k(check_code, result_hash)
           JOIN operations.asset_qc_results r
             ON r.check_code=k.check_code AND r.result_hash=k.result_hash AND r.rule_version=$3",
    )
    .bind(&codes)
    .bind(&hashes)
    .bind(crate::qc::QC_RULE_VERSION)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut checks: Vec<Value> = rows
        .into_iter()
        .map(|(code, status, detail)| {
            let detail = if PRIVATE_DETAIL.contains(&code.as_str()) {
                None
            } else {
                detail.map(|d| d.chars().take(300).collect::<String>())
            };
            json!({"code": code, "status": status, "detail": detail})
        })
        .collect();
    // Contract order, so the studio lists problems the way QC finds them.
    checks.sort_by_key(|c| {
        crate::qc::AUDIO_CHECK_CODES
            .iter()
            .position(|k| Some(*k) == c["code"].as_str())
    });
    Ok(json!({"asset_id": id, "qc_status": qc_status, "checks": checks}))
}

/// A stored image (cover art and its small preview) for display, after the
/// same checks as delivery: registered, sanitized at upload (and scanned when
/// the scanner is on), and the bytes still matching their recorded hash.
pub struct StoredImage {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
    pub sha256: String,
}

/// Reads the registered image `id` of `org`; the caller authorizes.
pub async fn stored_image(s: &AppState, org: Uuid, id: Uuid) -> Result<StoredImage> {
    let row = sqlx::query(
        "SELECT a.object_key, a.content_type, a.size_bytes, a.sha256
           FROM catalog.assets a
           JOIN catalog.asset_safety sf ON sf.asset_id=a.id AND sf.org_id=a.org_id AND sf.safe_key=a.object_key
                AND sf.safe_sha256=a.sha256 AND sf.rule_version='1' AND (NOT $3 OR sf.antivirus_status='SCANNED')
          WHERE a.org_id=$1 AND a.id=$2 AND a.kind='IMAGE' AND a.state='REGISTERED'
            AND a.content_type IN ('image/jpeg','image/png')",
    )
    .bind(org)
    .bind(id)
    .bind(s.antivirus_enabled)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)?;
    let size: i64 = row.get("size_bytes");
    if size <= 0 || size > MAX_IMAGE_BYTES {
        return Err(Error::NotFound);
    }
    let sha256: String = row
        .get::<Option<String>, _>("sha256")
        .ok_or(Error::NotFound)?;
    let bytes = s.storage.get(&row.get::<String, _>("object_key")).await?;
    if bytes.len() as i64 != size || crate::domain::sha256_hex(&bytes) != sha256 {
        tracing::warn!(asset = %id, "stored image does not match its recorded hash");
        return Err(Error::NotFound);
    }
    let content_type = if row.get::<String, _>("content_type") == "image/png" {
        "image/png"
    } else {
        "image/jpeg"
    };
    Ok(StoredImage {
        bytes,
        content_type,
        sha256,
    })
}

/// `GET /api/orgs/{org}/assets/{id}/image`: a member with read access to the
/// asset sees the image (cover art kept on the server, shown in the studio
/// once the release is saved or submitted).
pub async fn image(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<StoredImage> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, id, "asset", false).await?;
    tx.commit().await?;
    stored_image(s, org, id).await
}

/// The HTTP response for a [`StoredImage`]: content-addressed, so cached
/// privately for a year and answered 304 to a matching `If-None-Match`.
pub fn image_response(img: StoredImage, h: &axum::http::HeaderMap) -> axum::response::Response {
    use axum::http::{HeaderValue, StatusCode, header};
    use axum::response::IntoResponse;
    let etag = format!("\"{}\"", img.sha256);
    let fresh = h
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag));
    let mut res = if fresh {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        img.bytes.into_response()
    };
    let hd = res.headers_mut();
    hd.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(img.content_type),
    );
    if let Ok(v) = HeaderValue::from_str(&etag) {
        hd.insert(header::ETAG, v);
    }
    hd.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    hd.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    hd.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    res
}

pub async fn status(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let row = sqlx::query("SELECT id,asset_id,status,expires_at,completed_at,(expires_at<=now()) AS expired,expires_at+interval '30 minutes' AS complete_by FROM catalog.upload_sessions WHERE org_id=$1 AND id=$2")
        .bind(org).bind(id).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
    let asset: Uuid = row.get("asset_id");
    auth::authorize(&mut tx, a, org, asset, "asset", false).await?;
    let expires: DateTime<Utc> = row.get("expires_at");
    let completed: Option<DateTime<Utc>> = row.get("completed_at");
    let status: String = row.get("status");
    let expired: bool = row.get("expired");
    let complete_by: DateTime<Utc> = row.get("complete_by");
    tx.commit().await?;
    Ok(
        json!({"upload_session_id":id,"asset_id":asset,"status":status,"expires_at":expires,"completed_at":completed,"expired":expired,"complete_by":complete_by}),
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
    let (status, key, upload_id): (String, String, Option<String>) = sqlx::query_as(
        "SELECT status, expected_key, multipart_upload_id FROM catalog.upload_sessions WHERE org_id=$1 AND id=$2 FOR UPDATE",
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
    // removes quarantine/ objects after a day. An unfinished multipart
    // upload is aborted (its part URLs stop working); the lifecycle rule
    // also drops one this call could not reach.
    if let Some(upload_id) = upload_id
        && let Err(e) = s.storage.abort_multipart(&key, &upload_id).await
    {
        tracing::warn!(error = %e, "multipart abort failed; lifecycle rule will drop it");
    }
    Ok(json!({"cancelled":true,"duplicate":false}))
}
