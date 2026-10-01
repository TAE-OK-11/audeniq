//! Free local reference matching. Reference audio is operator supplied, with
//! public source/attribution and a permission basis. No remote lookup, scraping,
//! API key, or inference that an unmatched recording is original or authorized.
use crate::{
    error::{Error, Result},
    fingerprint, qc,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub const CHECK_CODE: &str = "S2_EXTERNAL_RECORDING_COMPARISON";
const PAGE: i64 = 64;
const MAX_REFERENCES: usize = 10_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceMetadata {
    pub title: String,
    pub artist: String,
    pub isrc: Option<String>,
    pub source_url: String,
    pub permission_basis: String,
}

/// Import a local audio file independently of any artist/org catalog. Files
/// are streamed for SHA, decoded once, and never retained in the database.
pub async fn import(
    pool: &PgPool,
    operator: &str,
    metadata: ReferenceMetadata,
    audio: &Path,
) -> Result<Uuid> {
    for (text, limit) in [
        (operator, 200),
        (metadata.title.as_str(), 300),
        (metadata.artist.as_str(), 200),
        (metadata.source_url.as_str(), 2000),
        (metadata.permission_basis.as_str(), 2000),
    ] {
        if text.trim().is_empty() || text.chars().count() > limit {
            return Err(Error::Invalid);
        }
        crate::text_policy::check_multiline(text)?;
    }
    let url = url::Url::parse(&metadata.source_url).map_err(|_| Error::Invalid)?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::Invalid);
    }
    if let Some(isrc) = &metadata.isrc {
        crate::identifiers::validate_isrc(isrc)?;
    }
    let path = audio.to_path_buf();
    let (sha, fp) = tokio::task::spawn_blocking(move || {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let file = std::fs::File::open(&path).map_err(|_| Error::Invalid)?;
        let info = file.metadata().map_err(|_| Error::Invalid)?;
        if !info.is_file() || info.len() == 0 || info.len() > 536_870_912 {
            return Err(Error::Invalid);
        }
        let mut digest = Sha256::new();
        let mut reader = std::io::BufReader::new(file);
        let mut bytes = [0u8; 65536];
        loop {
            let n = reader.read(&mut bytes).map_err(|_| Error::Internal)?;
            if n == 0 {
                break;
            }
            digest.update(&bytes[..n]);
        }
        let metrics = qc::probe_audio_metrics(&path).ok_or(Error::Invalid)?;
        let fp = fingerprint::compute_fingerprint(&path, metrics.duration_secs)?;
        if fp.frames.len() < 128
            || fp.frames.len() > 2000
            || fp.frames.iter().collect::<BTreeSet<_>>().len() < 16
        {
            return Err(Error::InvalidCode("REFERENCE_FINGERPRINT_NOT_DISTINCT"));
        }
        Ok((hex::encode(digest.finalize()), fp))
    })
    .await
    .map_err(|_| Error::Internal)??;
    let mut tx = pool.begin().await?;
    let existing = sqlx::query(
        "SELECT id,title,artist,isrc,source_url,permission_basis FROM catalog.external_recordings WHERE source_sha256=$1 AND version=$2",
    )
    .bind(&sha)
    .bind(fingerprint::FINGERPRINT_VERSION)
    .fetch_optional(&mut *tx)
    .await?;
    // A repeated import must not silently change the attribution/evidence.
    if let Some(row) = existing {
        if row.get::<String, _>("title") != metadata.title
            || row.get::<String, _>("artist") != metadata.artist
            || row.get::<Option<String>, _>("isrc") != metadata.isrc
            || row.get::<String, _>("source_url") != metadata.source_url
            || row.get::<String, _>("permission_basis") != metadata.permission_basis
        {
            return Err(Error::InvalidCode("REFERENCE_ATTRIBUTION_CONFLICT"));
        }
        return Ok(row.get("id"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO catalog.external_recordings(id,title,artist,isrc,source_url,permission_basis,source_sha256,version,frames,hash,duration_secs,imported_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
        .bind(id).bind(&metadata.title).bind(&metadata.artist).bind(&metadata.isrc).bind(&metadata.source_url)
        .bind(&metadata.permission_basis).bind(&sha).bind(fingerprint::FINGERPRINT_VERSION)
        .bind(fp.frames.len() as i32).bind(fp.to_bytes()).bind(fp.duration_secs).bind(operator)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO operations.audit_events(id,actor_service,resource_id,action,reason_code,request_id) VALUES($1,$2,$3,'external_recording.imported',$4,$5)")
        .bind(Uuid::new_v4()).bind(format!("audeniq-admin:{operator}")).bind(id)
        .bind(json!({"source_sha256":sha,"source_url":metadata.source_url,"permission_basis":metadata.permission_basis}).to_string())
        .bind(Uuid::new_v4()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn set_active(pool: &PgPool, operator: &str, id: Uuid, active: bool) -> Result<()> {
    if operator.trim().is_empty() || operator.chars().count() > 200 {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    let changed = sqlx::query("UPDATE catalog.external_recordings SET active=$2 WHERE id=$1")
        .bind(id)
        .bind(active)
        .execute(&mut *tx)
        .await?;
    if changed.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    sqlx::query("INSERT INTO operations.audit_events(id,actor_service,resource_id,action,reason_code,request_id) VALUES($1,$2,$3,'external_recording.activation',$4,$5)")
        .bind(Uuid::new_v4()).bind(format!("audeniq-admin:{operator}")).bind(id)
        .bind(json!({"active":active}).to_string()).bind(Uuid::new_v4()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Require a long overlap (at least ~12 s and two thirds of the shorter
/// fingerprint), rather than treating a coincident 3 s phrase as a copy.
/// This intentionally detects close recordings, not cover performances.
fn long_match_ber(a: &[u32], b: &[u32]) -> Option<f64> {
    let minimum = 128.max(a.len().min(b.len()) * 2 / 3);
    if a.len() < minimum || b.len() < minimum {
        return None;
    }
    let mut best = fingerprint::NEAR_DUPLICATE_BER;
    let mut found = false;
    for offset in -(b.len() as isize - minimum as isize)..=(a.len() - minimum) as isize {
        let (ai, bi) = if offset >= 0 {
            (offset as usize, 0)
        } else {
            (0, (-offset) as usize)
        };
        let overlap = (a.len() - ai).min(b.len() - bi);
        let limit = (best * overlap as f64 * 32.0).floor() as u64;
        let mut errors = 0u64;
        for k in 0..overlap {
            errors += (a[ai + k] ^ b[bi + k]).count_ones() as u64;
            if errors > limit {
                break;
            }
        }
        if errors <= limit {
            best = errors as f64 / (overlap as f64 * 32.0);
            found = true;
            if best == 0.0 {
                return Some(best);
            }
        }
    }
    found.then_some(best)
}

pub struct Scan {
    pub status: &'static str,
    pub detail: String,
    pub epoch: i64,
}
fn report(state: &str, examined: usize, matches: Vec<Value>, missing: Vec<Uuid>) -> Scan {
    Scan {
        epoch: 0,
        status: match state {
            "NOT_CHECKED" => "NOT_APPLICABLE",
            "COMPLETED" if matches.is_empty() => "PASS",
            _ => "REVIEW_REQUIRED",
        },
        detail:
            json!({"inspection_status":state,"scope":"operator_imported_external_reference_catalog",
            "references_examined":examined,"matches":matches,"missing_fingerprint_assets":missing,
            "global_catalog_checked":false,"copyright_verdict":"NOT_DETERMINED"})
            .to_string(),
    }
}

/// Fresh comparison at Stage 2. Stored Stage 1 fingerprints avoid downloading
/// or decoding the submitted audio again. Run before release/job row locks;
/// the caller rechecks revision, asset integrity and its live lease on commit.
pub async fn scan(pool: &PgPool, org: Uuid, tracks: &[Value]) -> Result<Scan> {
    let epoch: i64 =
        sqlx::query_scalar("SELECT epoch FROM catalog.external_recording_epoch WHERE singleton")
            .fetch_one(pool)
            .await?;
    let mut scan = scan_inner(pool, org, tracks).await?;
    scan.epoch = epoch;
    let mut detail: Value = serde_json::from_str(&scan.detail).map_err(|_| Error::Internal)?;
    detail["reference_catalog_epoch"] = json!(epoch);
    scan.detail = detail.to_string();
    Ok(scan)
}

async fn scan_inner(pool: &PgPool, org: Uuid, tracks: &[Value]) -> Result<Scan> {
    let mut after = Uuid::nil();
    let mut examined = 0;
    let mut matches = Vec::new();
    let mut queries: BTreeMap<Uuid, Vec<u32>> = BTreeMap::new();
    let mut missing = Vec::new();
    let isrcs: BTreeMap<&str, Vec<&Value>> = tracks
        .iter()
        .filter_map(|t| t["isrc"].as_str().map(|isrc| (isrc, t)))
        .fold(BTreeMap::new(), |mut map, (isrc, track)| {
            map.entry(isrc).or_default().push(track);
            map
        });
    let deadline = Instant::now() + Duration::from_secs(20);
    let ids: Vec<Uuid> = tracks
        .iter()
        .filter_map(|t| t["asset_id"].as_str().and_then(|s| Uuid::parse_str(s).ok()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx)
        .await?;
    let fingerprints: Vec<(Uuid,Vec<u8>)>=sqlx::query_as("SELECT asset_id,hash FROM catalog.asset_fingerprints WHERE org_id=$1 AND version=$2 AND asset_id=ANY($3)")
        .bind(org).bind(fingerprint::FINGERPRINT_VERSION).bind(&ids).fetch_all(&mut *tx).await?;
    tx.rollback().await?;
    for (id, hash) in fingerprints {
        let fp = fingerprint::Fingerprint::from_bytes(&hash)?;
        if fp.frames.len() >= 128 {
            queries.insert(id, fp.frames);
        }
    }
    for id in ids {
        if !queries.contains_key(&id) {
            missing.push(id);
        }
    }
    let queries = std::sync::Arc::new(queries);
    loop {
        let rows=sqlx::query("SELECT id,title,artist,isrc,source_url,hash FROM catalog.external_recordings WHERE active AND version=$1 AND id>$2 ORDER BY id LIMIT $3")
            .bind(fingerprint::FINGERPRINT_VERSION).bind(after).bind(PAGE.min((MAX_REFERENCES+1-examined) as i64)).fetch_all(pool).await?;
        if rows.is_empty() {
            return Ok(report(
                if examined == 0 {
                    "NOT_CHECKED"
                } else if missing.is_empty() {
                    "COMPLETED"
                } else {
                    "INCOMPLETE"
                },
                examined,
                matches,
                missing,
            ));
        }
        if examined + rows.len() > MAX_REFERENCES || Instant::now() > deadline {
            return Ok(report("INCOMPLETE", examined, matches, missing));
        }
        after = rows.last().ok_or(Error::Internal)?.get("id");
        examined += rows.len();
        for row in &rows {
            if let Some(isrc) = row.get::<Option<String>, _>("isrc")
                && let Some(claims) = isrcs.get(isrc.as_str())
            {
                for track in claims {
                    matches.push(json!({"signal":"ISRC_CLAIM","submitted_track_id":track["id"],
                        "reference_id":row.get::<Uuid,_>("id"),"title":row.get::<String,_>("title"),
                        "artist":row.get::<String,_>("artist"),"isrc":isrc,"source_url":row.get::<String,_>("source_url")}));
                    if matches.len() >= 5 {
                        break;
                    }
                }
            }
            if matches.len() >= 5 {
                break;
            }
        }
        if !matches.is_empty() {
            return Ok(report("MATCH_FOUND", examined, matches, missing));
        }
        let queries = queries.clone();
        let page_matches=tokio::task::spawn_blocking(move || {
            let mut hits=Vec::new();
            for row in rows {
                let fp=fingerprint::Fingerprint::from_bytes(&row.get::<Vec<u8>,_>("hash"))?;
                for (asset,frames) in queries.iter() {
                    if let Some(ber)=long_match_ber(frames,&fp.frames) {
                        hits.push(json!({"signal":"AUDIO_FINGERPRINT","submitted_asset_id":asset,"reference_id":row.get::<Uuid,_>("id"),
                            "title":row.get::<String,_>("title"),"artist":row.get::<String,_>("artist"),
                            "isrc":row.get::<Option<String>,_>("isrc"),"source_url":row.get::<String,_>("source_url"),"ber":ber}));
                    }
                    if hits.len()>=5 || Instant::now()>deadline { break; }
                }
                if hits.len()>=5 || Instant::now()>deadline { break; }
            }
            Ok::<_,Error>(hits)
        }).await.map_err(|_|Error::Internal)??;
        matches.extend(page_matches);
        // A proven candidate is already a hold; cap evidence and stop work.
        if !matches.is_empty() {
            return Ok(report("MATCH_FOUND", examined, matches, missing));
        }
        if Instant::now() > deadline {
            return Ok(report("INCOMPLETE", examined, matches, missing));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn varied(n: usize) -> Vec<u32> {
        let mut s = 17u32;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                s
            })
            .collect()
    }
    #[test]
    fn long_overlap_finds_trimmed_recording_but_not_a_short_shared_phrase() {
        let frames = varied(400);
        assert_eq!(long_match_ber(&frames, &frames[40..]), Some(0.0));
        assert_eq!(long_match_ber(&frames[..80], &frames), None);
        let mut other = frames.iter().map(|x| !x).collect::<Vec<_>>();
        other[100..140].copy_from_slice(&frames[100..140]);
        assert_eq!(long_match_ber(&frames, &other), None);
    }
    #[test]
    fn absent_and_incomplete_reference_searches_never_claim_no_duplicate() {
        assert_eq!(
            report("NOT_CHECKED", 0, vec![], vec![]).status,
            "NOT_APPLICABLE"
        );
        assert_eq!(
            report("INCOMPLETE", 64, vec![], vec![]).status,
            "REVIEW_REQUIRED"
        );
        assert_eq!(report("COMPLETED", 64, vec![], vec![]).status, "PASS");
    }
}
