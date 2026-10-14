//! Original-document archive (원본 보관) and integrity anchors.
//!
//! Monthly (`audeniq-admin archive run`): every sealed document — release
//! applications, agreements, rights documents, signing records (with their
//! event chain), terms acceptances — is rendered exactly as sealed
//! (`crate::seal::render`) and uploaded once per content version to a
//! separate archive bucket (S3 or R2, ideally another provider, with object
//! lock / bucket lock: WORM). Uploaded files of those documents (PDF, images)
//! are copied byte for byte. A signed manifest lists every archived object
//! (cumulative: each month's manifest is the full index), chained to the
//! previous month's manifest.
//!
//! Daily (`archive anchor`): the head of the seal chain is written to the
//! archive bucket, so rewriting the database (seals included) is detected.
//!
//! The objects are sealed with the personal-data keyring when a key is set
//! (they hold names, signatures and identity checks); the manifest holds
//! only ids and hashes. `archive open` shows an original again (열람).
use crate::{
    error::{Error, Result},
    payout_keys::{Field, KeyRing},
    seal::{self, sha256_hex},
    storage::{ArchiveBody, ObjectStore},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{Datelike, NaiveDate, Utc};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// 상법 제33조: 영업에 관한 중요서류 10년.
const CONTRACT_YEARS: i32 = 10;
/// 전자상거래법 제6조: 계약·청약철회 기록 5년 (terms acceptances).
const CONSENT_YEARS: i32 = 5;

fn software() -> String {
    format!(
        "audeniq-core {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::var("AUDENIQ_BUILD").unwrap_or_default()
    )
    .trim()
    .chars()
    .take(120)
    .collect()
}

/// S3 Object Lock in COMPLIANCE mode per object (`ARCHIVE_OBJECT_LOCK=true`).
/// R2: leave it off and set a bucket lock rule instead.
fn object_lock() -> bool {
    std::env::var("ARCHIVE_OBJECT_LOCK").as_deref() == Ok("true")
}

fn retain_until(kind: &str, from: NaiveDate) -> NaiveDate {
    let years = if kind == "TERMS_ACCEPTANCE" {
        CONSENT_YEARS
    } else {
        CONTRACT_YEARS
    };
    from.with_year(from.year() + years)
        .unwrap_or_else(|| from + chrono::Duration::days(365 * years as i64 + 3))
}

fn lock_until(d: NaiveDate) -> Option<chrono::DateTime<Utc>> {
    object_lock().then(|| d.and_hms_opt(0, 0, 0).expect("midnight").and_utc())
}

/// Seals an object for the archive (base64 inside, so files work too).
fn protect(keys: &KeyRing, subject: Uuid, bytes: &[u8]) -> Result<(Vec<u8>, bool, Option<i16>)> {
    if !keys.ready() {
        return Ok((bytes.to_vec(), false, None));
    }
    let sealed = keys.seal_field(Field::ArchiveObject, subject, &STANDARD.encode(bytes))?;
    Ok((sealed, true, Some(keys.active_version())))
}

fn unprotect(keys: &KeyRing, subject: Uuid, stored: &[u8], encrypted: bool) -> Result<Vec<u8>> {
    if !encrypted {
        return Ok(stored.to_vec());
    }
    let text = keys.open_field(Field::ArchiveObject, subject, stored)?;
    STANDARD
        .decode(text.as_bytes())
        .map_err(|_| Error::Internal)
}

fn valid_period(p: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(&format!("{p}-01"), "%Y-%m-%d")
        .ok()
        .filter(|_| p.len() == 7)
        .ok_or(Error::Invalid)
}

pub fn previous_month(today: NaiveDate) -> String {
    let first = today.with_day(1).expect("day 1");
    let prev = first - chrono::Duration::days(1);
    format!("{:04}-{:02}", prev.year(), prev.month())
}

struct Ctx<'a> {
    pool: &'a PgPool,
    source: &'a dyn ObjectStore,
    target: &'a dyn ObjectStore,
    keys: &'a KeyRing,
}

#[allow(clippy::too_many_arguments)]
async fn put_object(
    cx: &Ctx<'_>,
    run: Uuid,
    object_kind: &str,
    doc_kind: &str,
    doc_id: Uuid,
    seal_seq: Option<i64>,
    content: &[u8],
    key: &str,
    mime: &str,
    retain: NaiveDate,
) -> Result<bool> {
    let content_sha = sha256_hex(content);
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.archive_objects
          WHERE object_kind=$1 AND doc_kind=$2 AND doc_id=$3 AND content_sha256=$4)",
    )
    .bind(object_kind)
    .bind(doc_kind)
    .bind(doc_id)
    .bind(&content_sha)
    .fetch_one(cx.pool)
    .await?;
    if exists {
        return Ok(false);
    }
    let object_id = Uuid::new_v4();
    let (stored, encrypted, key_version) = protect(cx.keys, object_id, content)?;
    let key = if encrypted {
        format!("{key}.sealed")
    } else {
        key.to_owned()
    };
    cx.target
        .put_archive(
            &key,
            ArchiveBody::Bytes(&stored),
            if encrypted {
                "application/octet-stream"
            } else {
                mime
            },
            lock_until(retain),
        )
        .await?;
    sqlx::query(
        "INSERT INTO operations.archive_objects(id,object_kind,doc_kind,doc_id,seal_seq,content_sha256,object_key,object_sha256,size_bytes,encrypted,key_version,run_id,retain_until)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(object_id)
    .bind(object_kind)
    .bind(doc_kind)
    .bind(doc_id)
    .bind(seal_seq)
    .bind(&content_sha)
    .bind(&key)
    .bind(sha256_hex(&stored))
    .bind(stored.len() as i64)
    .bind(encrypted)
    .bind(key_version)
    .bind(run)
    .bind(retain)
    .execute(cx.pool)
    .await?;
    Ok(true)
}

/// One monthly archive run. Re-running a completed month is a no-op.
pub async fn run(
    pool: &PgPool,
    source: &dyn ObjectStore,
    target: &dyn ObjectStore,
    keys: &KeyRing,
    period: &str,
    operator: &str,
) -> Result<Value> {
    crate::privacy_maintenance::owner(pool, operator).await?;
    valid_period(period)?;
    if let Some(done) = sqlx::query_scalar::<_, String>(
        "SELECT manifest_key FROM operations.archive_runs WHERE period=$1 AND status='COMPLETED'",
    )
    .bind(period)
    .fetch_optional(pool)
    .await?
    {
        return Ok(json!({"period": period, "already_completed": true, "manifest_key": done}));
    }
    // Documents from before sealing existed are sealed first, so they are archived too.
    let backfilled = seal::backfill(pool, keys).await?;
    let run = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO operations.archive_runs(id,period,operator,software) VALUES($1,$2,$3,$4)",
    )
    .bind(run)
    .bind(period)
    .bind(operator.trim())
    .bind(software())
    .execute(pool)
    .await?;
    let cx = Ctx {
        pool,
        source,
        target,
        keys,
    };
    match archive_all(&cx, run, period, operator).await {
        Ok(mut v) => {
            v["backfilled_seals"] = json!(backfilled);
            Ok(v)
        }
        Err(e) => {
            sqlx::query("UPDATE operations.archive_runs SET status='FAILED',finished_at=now(),error=$2 WHERE id=$1")
                .bind(run)
                .bind(format!("{e:?}").chars().take(500).collect::<String>())
                .execute(pool)
                .await?;
            Err(e)
        }
    }
}

async fn archive_all(cx: &Ctx<'_>, run: Uuid, period: &str, operator: &str) -> Result<Value> {
    let mut problems = Vec::new();
    let mut uploaded = 0i64;
    let mut conn = cx.pool.acquire().await?;
    // The latest seal of every document: archive it exactly as sealed.
    let sealed = sqlx::query(
        "SELECT DISTINCT ON (doc_kind, doc_id) seq, doc_kind, doc_id, content_sha256, sealed_at
           FROM operations.document_seals ORDER BY doc_kind, doc_id, seq DESC",
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in &sealed {
        let kind: String = row.get("doc_kind");
        let id: Uuid = row.get("doc_id");
        let seq: i64 = row.get("seq");
        let at: chrono::DateTime<Utc> = row.get("sealed_at");
        let Some(k) = seal::Kind::parse(&kind) else {
            continue;
        };
        let (_, doc) = match seal::render(&mut conn, k, id).await {
            Ok(v) => v,
            Err(Error::NotFound) => {
                problems.push(json!({"kind": kind, "id": id, "problem": "DOCUMENT_MISSING"}));
                continue;
            }
            Err(e) => return Err(e),
        };
        if sha256_hex(doc.as_bytes()) != row.get::<String, _>("content_sha256") {
            // Never archive a document that no longer matches its seal.
            problems.push(
                json!({"kind": kind, "id": id, "seal_seq": seq, "problem": "DOCUMENT_CHANGED"}),
            );
            continue;
        }
        let key = format!(
            "documents/{}/{}/{id}/{}.json",
            kind.to_lowercase(),
            at.year(),
            &row.get::<String, _>("content_sha256")[..16]
        );
        if put_object(
            cx,
            run,
            "DOCUMENT",
            &kind,
            id,
            Some(seq),
            doc.as_bytes(),
            &key,
            "application/json",
            retain_until(&kind, at.date_naive()),
        )
        .await?
        {
            uploaded += 1;
        }
    }
    // Uploaded files of agreements and rights documents, byte for byte.
    let files = sqlx::query(
        "SELECT d.id, d.created_at, x.object_key, x.content_type, x.size_bytes
           FROM portal.documents d JOIN catalog.assets x ON x.id=d.asset_id AND x.org_id=d.org_id
          WHERE x.state='REGISTERED'",
    )
    .fetch_all(&mut *conn)
    .await?;
    for f in &files {
        let id: Uuid = f.get("id");
        let at: chrono::DateTime<Utc> = f.get("created_at");
        let bytes = match cx.source.get(&f.get::<String, _>("object_key")).await {
            Ok(b) => b,
            Err(_) => {
                problems
                    .push(json!({"kind": "DOCUMENT_FILE", "id": id, "problem": "FILE_UNREADABLE"}));
                continue;
            }
        };
        let key = format!("files/{}/{id}/{}", at.year(), &sha256_hex(&bytes)[..32]);
        if put_object(
            cx,
            run,
            "FILE",
            "DOCUMENT_FILE",
            id,
            None,
            &bytes,
            &key,
            f.get("content_type"),
            retain_until("DOCUMENT_FILE", at.date_naive()),
        )
        .await?
        {
            uploaded += 1;
        }
    }
    drop(conn);
    let anchor = anchor_head(cx.pool, cx.target, cx.keys).await?;
    // The manifest: every archived object to date, signed and chained.
    let objects = sqlx::query(
        "SELECT o.object_kind,o.doc_kind,o.doc_id,o.seal_seq,s.hash AS seal_hash,o.content_sha256,o.object_key,
                o.object_sha256,o.size_bytes,o.encrypted,o.key_version,o.archived_at,o.retain_until
           FROM operations.archive_objects o LEFT JOIN operations.document_seals s ON s.seq=o.seal_seq
          WHERE o.purged_at IS NULL ORDER BY o.archived_at, o.object_key",
    )
    .fetch_all(cx.pool)
    .await?;
    let entries: Vec<Value> = objects
        .iter()
        .map(|o| {
            json!({
                "object_kind": o.get::<String, _>("object_kind"), "doc_kind": o.get::<String, _>("doc_kind"),
                "doc_id": o.get::<Uuid, _>("doc_id"), "seal_seq": o.get::<Option<i64>, _>("seal_seq"),
                "seal_hash": o.get::<Option<String>, _>("seal_hash"),
                "content_sha256": o.get::<String, _>("content_sha256"), "object_key": o.get::<String, _>("object_key"),
                "object_sha256": o.get::<String, _>("object_sha256"), "size_bytes": o.get::<i64, _>("size_bytes"),
                "encrypted": o.get::<bool, _>("encrypted"), "key_version": o.get::<Option<i16>, _>("key_version"),
                "archived_at": o.get::<chrono::DateTime<Utc>, _>("archived_at"),
                "retain_until": o.get::<NaiveDate, _>("retain_until"),
            })
        })
        .collect();
    let prev: Option<String> = sqlx::query_scalar(
        "SELECT manifest_sha256 FROM operations.archive_runs WHERE status='COMPLETED' ORDER BY period DESC LIMIT 1",
    )
    .fetch_optional(cx.pool)
    .await?
    .flatten();
    let manifest = json!({
        "format": "audeniq-archive-manifest-v1",
        "period": period,
        "run_id": run,
        "generated_at": Utc::now(),
        "operator": operator.trim(),
        "software": software(),
        "previous_manifest_sha256": prev,
        "seal_chain_head": anchor,
        "retention": {"contracts_years": CONTRACT_YEARS, "consents_years": CONSENT_YEARS},
        "object_count": entries.len(),
        "problems": problems,
        "objects": entries,
    });
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|_| Error::Internal)?;
    let digest = sha256_hex(&bytes);
    let manifest_key = format!("manifests/{period}.json");
    let keep = lock_until(retain_until("MANIFEST", Utc::now().date_naive()));
    cx.target
        .put_archive(
            &manifest_key,
            ArchiveBody::Bytes(&bytes),
            "application/json",
            keep,
        )
        .await?;
    let signature = signature_doc(cx.keys, "audeniq-archive-manifest-v1", &digest);
    cx.target
        .put_archive(
            &format!("manifests/{period}.sig.json"),
            ArchiveBody::Bytes(signature.to_string().as_bytes()),
            "application/json",
            keep,
        )
        .await?;
    sqlx::query(
        "UPDATE operations.archive_runs SET status='COMPLETED',finished_at=now(),uploaded=$2,total_objects=$3,
           manifest_key=$4,manifest_sha256=$5,prev_manifest_sha256=$6 WHERE id=$1",
    )
    .bind(run)
    .bind(uploaded as i32)
    .bind(entries.len() as i32)
    .bind(&manifest_key)
    .bind(&digest)
    .bind(&prev)
    .execute(cx.pool)
    .await?;
    crate::operations::audit(
        &mut *cx.pool.acquire().await?,
        None,
        None,
        None,
        "privacy.archive.completed",
        &format!(
            "OPERATOR:{} period={period} uploaded={uploaded} objects={} problems={}",
            operator.trim(),
            entries.len(),
            problems.len()
        ),
        run,
    )
    .await?;
    Ok(json!({
        "period": period, "uploaded": uploaded, "objects": entries.len(),
        "manifest_key": manifest_key, "manifest_sha256": digest, "problems": problems,
    }))
}

/// `{sha256, key_version, signature}` over `"{label}|{sha256}"`.
fn signature_doc(keys: &KeyRing, label: &str, digest: &str) -> Value {
    match keys.sign_document(format!("{label}|{digest}").as_bytes()) {
        Some((v, sig)) => {
            json!({"label": label, "sha256": digest, "key_version": v, "algorithm": "Ed25519", "signature": STANDARD.encode(sig)})
        }
        None => json!({"label": label, "sha256": digest, "signature": null}),
    }
}

/// Writes today's seal-chain head to the archive bucket (once a day).
async fn anchor_head(pool: &PgPool, target: &dyn ObjectStore, keys: &KeyRing) -> Result<Value> {
    let head =
        sqlx::query("SELECT seq, hash FROM operations.document_seals ORDER BY seq DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    let (seq, hash) = match head {
        Some(r) => (r.get::<i64, _>("seq"), r.get::<String, _>("hash")),
        None => (0, seal::GENESIS.to_string()),
    };
    let today = Utc::now().date_naive();
    let key = format!("anchors/{today}.json");
    let done: Option<i64> =
        sqlx::query_scalar("SELECT head_seq FROM operations.integrity_anchors WHERE object_key=$1")
            .bind(&key)
            .fetch_optional(pool)
            .await?;
    if done.is_none() {
        let body = json!({
            "format": "audeniq-integrity-anchor-v1", "date": today, "head_seq": seq, "head_hash": hash,
            "anchored_at": Utc::now(),
            "signature": signature_doc(keys, "audeniq-integrity-anchor-v1", &hash),
        });
        target
            .put_archive(
                &key,
                ArchiveBody::Bytes(body.to_string().as_bytes()),
                "application/json",
                lock_until(retain_until("ANCHOR", today)),
            )
            .await?;
        sqlx::query("INSERT INTO operations.integrity_anchors(id,head_seq,head_hash,object_key) VALUES($1,$2,$3,$4)")
            .bind(Uuid::new_v4())
            .bind(seq)
            .bind(&hash)
            .bind(&key)
            .execute(pool)
            .await?;
    }
    Ok(json!({"seq": seq, "hash": hash, "anchor_key": key}))
}

/// Daily: anchor the seal chain head in WORM storage.
pub async fn anchor(
    pool: &PgPool,
    target: &dyn ObjectStore,
    keys: &KeyRing,
    operator: &str,
) -> Result<Value> {
    crate::privacy_maintenance::owner(pool, operator).await?;
    anchor_head(pool, target, keys).await
}

/// Integrity check: the seal chain and every document against its seal,
/// the chain against the anchors stored in WORM (read back from the bucket),
/// and the archive objects (size and, with `deep`, content).
pub async fn verify(
    pool: &PgPool,
    target: &dyn ObjectStore,
    keys: &KeyRing,
    deep: bool,
) -> Result<Value> {
    let mut conn = pool.acquire().await?;
    let report = seal::verify(&mut conn, keys).await?;
    drop(conn);
    let mut problems: Vec<Value> = report.problems.clone();
    // Anchors: read from the bucket, not trusted from the database.
    let anchors: Vec<String> = sqlx::query_scalar(
        "SELECT object_key FROM operations.integrity_anchors ORDER BY anchored_at",
    )
    .fetch_all(pool)
    .await?;
    let mut checked_anchors = 0;
    let mut claims: Vec<(&String, i64, String)> = Vec::new();
    for key in &anchors {
        let Ok(bytes) = target.get(key).await else {
            problems.push(json!({"anchor": key, "problem": "ANCHOR_MISSING"}));
            continue;
        };
        let a: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let (Some(seq), Some(hash)) = (a["head_seq"].as_i64(), a["head_hash"].as_str()) else {
            problems.push(json!({"anchor": key, "problem": "ANCHOR_UNREADABLE"}));
            continue;
        };
        checked_anchors += 1;
        if seq != 0 {
            claims.push((key, seq, hash.to_string()));
        }
    }
    // One read for every anchored seal, not one per anchor.
    let seqs: Vec<i64> = claims.iter().map(|c| c.1).collect();
    let stored: std::collections::HashMap<i64, String> = sqlx::query_as::<_, (i64, String)>(
        "SELECT seq, hash FROM operations.document_seals WHERE seq = ANY($1)",
    )
    .bind(&seqs)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();
    for (key, seq, hash) in claims {
        if stored.get(&seq) != Some(&hash) {
            problems
                .push(json!({"anchor": key, "seq": seq, "problem": "CHAIN_DIFFERS_FROM_ANCHOR"}));
        }
    }
    let objects = sqlx::query("SELECT id,object_key,object_sha256,size_bytes FROM operations.archive_objects WHERE purged_at IS NULL")
        .fetch_all(pool)
        .await?;
    for o in &objects {
        let key: String = o.get("object_key");
        if deep {
            // Streamed hash: constant memory whatever the object size.
            let size = o.get::<i64, _>("size_bytes").max(0) as u64;
            match target.digest(&key, size).await {
                Ok(d) if d.sha256 == o.get::<String, _>("object_sha256") && d.size == size => {}
                Ok(_) | Err(Error::PolicyGate(_)) => {
                    problems.push(json!({"object": key, "problem": "ARCHIVE_OBJECT_CHANGED"}))
                }
                Err(_) => {
                    problems.push(json!({"object": key, "problem": "ARCHIVE_OBJECT_MISSING"}))
                }
            }
        } else {
            match target.head(&key).await {
                Ok(Some(m)) if m.size == o.get::<i64, _>("size_bytes") => {}
                Ok(_) | Err(_) => {
                    problems.push(json!({"object": key, "problem": "ARCHIVE_OBJECT_MISSING"}))
                }
            }
        }
    }
    Ok(json!({
        "ok": problems.is_empty(),
        "seals": report.seals, "documents": report.documents, "unsigned_seals": report.unsigned,
        "head": {"seq": report.head_seq, "hash": report.head_hash},
        "anchors_checked": checked_anchors, "archive_objects_checked": objects.len(), "deep": deep,
        "problems": problems,
    }))
}

/// 열람: an archived original, decrypted, by its object key.
pub async fn open(
    pool: &PgPool,
    target: &dyn ObjectStore,
    keys: &KeyRing,
    operator: &str,
    key: &str,
) -> Result<Vec<u8>> {
    crate::privacy_maintenance::owner(pool, operator).await?;
    let row = sqlx::query("SELECT id, object_sha256, encrypted, content_sha256 FROM operations.archive_objects WHERE object_key=$1")
        .bind(key)
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)?;
    let stored = target.get(key).await?;
    if sha256_hex(&stored) != row.get::<String, _>("object_sha256") {
        return Err(Error::PolicyGate("ARCHIVE_OBJECT_CHANGED"));
    }
    let bytes = unprotect(keys, row.get("id"), &stored, row.get("encrypted"))?;
    if sha256_hex(&bytes) != row.get::<String, _>("content_sha256") {
        return Err(Error::PolicyGate("ARCHIVE_OBJECT_CHANGED"));
    }
    crate::operations::audit(
        &mut *pool.acquire().await?,
        None,
        None,
        Some(row.get("id")),
        "privacy.archive.opened",
        &format!("OPERATOR:{}", operator.trim()),
        Uuid::new_v4(),
    )
    .await?;
    Ok(bytes)
}

/// Uploads a local file (database base backup or WAL, already encrypted by
/// the host script with `age`) to the archive bucket under `key`.
pub async fn put_backup(
    target: &dyn ObjectStore,
    path: &std::path::Path,
    key: &str,
    retain_days: i64,
) -> Result<Value> {
    if !(key.starts_with("pitr/") || key.starts_with("backups/"))
        || !(0..=3660).contains(&retain_days)
    {
        return Err(Error::Invalid);
    }
    let until = (retain_days > 0 && object_lock())
        .then(|| Utc::now() + chrono::Duration::days(retain_days));
    target
        .put_archive(
            key,
            ArchiveBody::File(path),
            "application/octet-stream",
            until,
        )
        .await?;
    Ok(json!({"key": key, "retained_days": retain_days}))
}

/// Retention over: deletes expired archive objects (PIPA 제21조 파기) and
/// marks them purged. Locked objects outlive this only until their lock ends.
pub async fn purge_expired(
    pool: &PgPool,
    target: &dyn ObjectStore,
    operator: &str,
) -> Result<Value> {
    crate::privacy_maintenance::owner(pool, operator).await?;
    let expired = sqlx::query("SELECT id, object_key FROM operations.archive_objects WHERE purged_at IS NULL AND retain_until < current_date LIMIT 500")
        .fetch_all(pool)
        .await?;
    let mut purged = 0;
    for o in &expired {
        if target
            .delete(&o.get::<String, _>("object_key"))
            .await
            .is_ok()
        {
            sqlx::query("UPDATE operations.archive_objects SET purged_at=now() WHERE id=$1")
                .bind(o.get::<Uuid, _>("id"))
                .execute(pool)
                .await?;
            purged += 1;
        }
    }
    if purged > 0 {
        crate::operations::audit(
            &mut *pool.acquire().await?,
            None,
            None,
            None,
            "privacy.archive.purged",
            &format!("OPERATOR:{} objects={purged}", operator.trim()),
            Uuid::new_v4(),
        )
        .await?;
    }
    Ok(json!({"purged": purged, "remaining_expired": expired.len() - purged}))
}

pub async fn status(pool: &PgPool) -> Result<Value> {
    let runs = sqlx::query(
        "SELECT period,status,started_at,finished_at,uploaded,total_objects,manifest_key,manifest_sha256,error
           FROM operations.archive_runs ORDER BY started_at DESC LIMIT 12",
    )
    .fetch_all(pool)
    .await?;
    let anchor = sqlx::query("SELECT head_seq,head_hash,object_key,anchored_at FROM operations.integrity_anchors ORDER BY anchored_at DESC LIMIT 1")
        .fetch_optional(pool)
        .await?;
    Ok(json!({
        "runs": runs.iter().map(|r| json!({
            "period": r.get::<String, _>("period"), "status": r.get::<String, _>("status"),
            "started_at": r.get::<chrono::DateTime<Utc>, _>("started_at"),
            "finished_at": r.get::<Option<chrono::DateTime<Utc>>, _>("finished_at"),
            "uploaded": r.get::<i32, _>("uploaded"), "objects": r.get::<i32, _>("total_objects"),
            "manifest_key": r.get::<Option<String>, _>("manifest_key"),
            "manifest_sha256": r.get::<Option<String>, _>("manifest_sha256"),
            "error": r.get::<Option<String>, _>("error"),
        })).collect::<Vec<_>>(),
        "last_anchor": anchor.map(|a| json!({
            "seq": a.get::<i64, _>("head_seq"), "hash": a.get::<String, _>("head_hash"),
            "key": a.get::<String, _>("object_key"), "at": a.get::<chrono::DateTime<Utc>, _>("anchored_at"),
        })),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_follows_the_law() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        assert_eq!(
            retain_until("AGREEMENT", d),
            NaiveDate::from_ymd_opt(2036, 10, 12).unwrap()
        );
        assert_eq!(
            retain_until("TERMS_ACCEPTANCE", d),
            NaiveDate::from_ymd_opt(2031, 10, 12).unwrap()
        );
        let leap = NaiveDate::from_ymd_opt(2028, 2, 29).unwrap();
        assert!(retain_until("AGREEMENT", leap) > leap);
    }

    #[test]
    fn periods_and_previous_month() {
        assert!(valid_period("2026-10").is_ok());
        assert!(valid_period("2026-13").is_err() && valid_period("26-10").is_err());
        assert_eq!(
            previous_month(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            "2025-12"
        );
        assert_eq!(
            previous_month(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()),
            "2026-09"
        );
    }

    #[test]
    fn archive_objects_round_trip_sealed() {
        let keys = KeyRing::test_ring();
        let id = Uuid::new_v4();
        let (stored, enc, v) = protect(&keys, id, b"\x00binary\xff").unwrap();
        assert!(enc && v == Some(2));
        assert_eq!(
            unprotect(&keys, id, &stored, true).unwrap(),
            b"\x00binary\xff"
        );
        assert!(
            unprotect(&keys, Uuid::new_v4(), &stored, true).is_err(),
            "bound to its object"
        );
    }
}
