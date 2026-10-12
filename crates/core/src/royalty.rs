//! Virtual royalty report ingestion and matching.
//!
//! Synthetic royalty reports (CSV) from DSPs are ingested into
//! `finance.royalty_reports` / `finance.report_lines`, then matched
//! against the catalog by ISRC. This is the test-only path until real
//! partner reports arrive; the tables and constraints are production.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};

/// One parsed line from a royalty report CSV.
#[derive(Debug, Clone)]
pub struct ReportLine {
    pub line_no: i32,
    pub isrc: Option<String>,
    pub dsp_track_id: Option<String>,
    pub quantity: Option<rust_decimal::Decimal>,
    pub gross_amount: Option<rust_decimal::Decimal>,
    pub currency: Option<String>,
    pub raw: serde_json::Value,
}

/// Parse a royalty report CSV. Expected header:
/// `isrc,dsp_track_id,quantity,gross_amount,currency`
pub fn parse_csv(content: &str) -> Result<Vec<ReportLine>> {
    let mut lines = Vec::new();
    let mut rows = content.lines();
    let header = rows.next().ok_or(Error::Invalid)?;
    let cols: Vec<&str> = header.split(',').collect();
    // Simpler: build a map once.
    let col_pos: std::collections::HashMap<&str, usize> = cols
        .iter()
        .enumerate()
        .map(|(i, c)| (c.trim(), i))
        .collect();
    let get = |row: &[&str], name: &str| -> Option<String> {
        col_pos
            .get(name)
            .and_then(|&i| row.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    for required in ["isrc", "quantity", "gross_amount"] {
        if !col_pos.contains_key(required) {
            return Err(Error::Invalid);
        }
    }
    for (n, row) in rows.enumerate() {
        if row.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = row.split(',').collect();
        let parse_dec = |name: &str| -> Result<Option<rust_decimal::Decimal>> {
            match get(&fields, name) {
                None => Ok(None),
                Some(s) => s
                    .parse::<rust_decimal::Decimal>()
                    .map(Some)
                    .map_err(|_| Error::Invalid),
            }
        };
        let quantity = parse_dec("quantity")?;
        let gross_amount = parse_dec("gross_amount")?;
        if quantity.is_some_and(|q| q < rust_decimal::Decimal::ZERO) {
            return Err(Error::Invalid);
        }
        if gross_amount.is_some_and(|a| a < rust_decimal::Decimal::ZERO) {
            return Err(Error::Invalid);
        }
        let mut raw = serde_json::Map::new();
        for (i, c) in cols.iter().enumerate() {
            raw.insert(
                c.trim().to_string(),
                serde_json::Value::String(fields.get(i).unwrap_or(&"").trim().to_string()),
            );
        }
        lines.push(ReportLine {
            line_no: n as i32,
            // Reports spell ISRCs as 'usabc2600001' or 'US-ABC-26-00001' too.
            isrc: get(&fields, "isrc")
                .map(|i| {
                    i.chars()
                        .filter(|c| c.is_ascii_alphanumeric())
                        .collect::<String>()
                        .to_ascii_uppercase()
                })
                .filter(|i| !i.is_empty()),
            dsp_track_id: get(&fields, "dsp_track_id"),
            quantity,
            gross_amount,
            currency: get(&fields, "currency"),
            raw: serde_json::Value::Object(raw),
        });
    }
    Ok(lines)
}

/// Report lines written per INSERT statement during ingest.
const LINE_BATCH: usize = 5_000;

/// Ingest a royalty report. Returns the report id.
///
/// Rejects duplicates by content hash (UNIQUE(org_id, source_hash)).
#[allow(clippy::too_many_arguments)]
pub async fn ingest_report(
    pool: &PgPool,
    org_id: Uuid,
    dsp_id: &str,
    period_start: chrono::NaiveDate,
    period_end: chrono::NaiveDate,
    currency: &str,
    filename: &str,
    content: &str,
) -> Result<Uuid> {
    if period_end < period_start {
        return Err(Error::Invalid);
    }
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(Error::Invalid);
    }
    let lines = parse_csv(content)?;
    if lines.is_empty() {
        return Err(Error::Invalid);
    }
    let source_hash = crate::domain::sha256_hex(content);
    let report_id = Uuid::new_v4();

    let mut tx = pool.begin().await?;
    // Duplicate detection first for a clean error.
    let dup: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM finance.royalty_reports WHERE org_id=$1 AND source_hash=$2",
    )
    .bind(org_id)
    .bind(&source_hash)
    .fetch_optional(&mut *tx)
    .await?;
    if dup.is_some() {
        return Err(Error::Conflict);
    }
    sqlx::query(
        "INSERT INTO finance.royalty_reports
         (id, org_id, dsp_id, period_start, period_end, source_filename, source_hash, currency, status)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,'RECEIVED')",
    )
    .bind(report_id)
    .bind(org_id)
    .bind(dsp_id)
    .bind(period_start)
    .bind(period_end)
    .bind(filename)
    .bind(&source_hash)
    .bind(currency)
    .execute(&mut *tx)
    .await?;
    // Batched: a DSP report has tens of thousands of lines, and one INSERT
    // per line was one database round trip each.
    for chunk in lines.chunks(LINE_BATCH) {
        let mut line_no = Vec::with_capacity(chunk.len());
        let mut isrc = Vec::with_capacity(chunk.len());
        let mut dsp_track_id = Vec::with_capacity(chunk.len());
        let mut quantity = Vec::with_capacity(chunk.len());
        let mut gross_amount = Vec::with_capacity(chunk.len());
        let mut line_currency = Vec::with_capacity(chunk.len());
        let mut raw = Vec::with_capacity(chunk.len());
        for l in chunk {
            line_no.push(l.line_no);
            isrc.push(l.isrc.as_deref());
            dsp_track_id.push(l.dsp_track_id.as_deref());
            quantity.push(l.quantity);
            gross_amount.push(l.gross_amount);
            line_currency.push(l.currency.as_deref().unwrap_or(currency));
            raw.push(&l.raw);
        }
        sqlx::query(
            "INSERT INTO finance.report_lines
             (id, org_id, report_id, line_no, isrc, dsp_track_id, quantity, gross_amount, currency, raw)
             SELECT gen_random_uuid(), $1, $2, l.*
               FROM unnest($3::int4[], $4::text[], $5::text[], $6::numeric[], $7::numeric[], $8::text[], $9::jsonb[]) AS l",
        )
        .bind(org_id)
        .bind(report_id)
        .bind(&line_no)
        .bind(&isrc)
        .bind(&dsp_track_id)
        .bind(&quantity)
        .bind(&gross_amount)
        .bind(&line_currency)
        .bind(&raw)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(report_id)
}

/// Match report lines against the catalog by ISRC.
/// Lines with a catalog hit become AUTO; the rest stay UNMATCHED.
/// Returns (auto_matched, still_unmatched).
pub async fn match_report(pool: &PgPool, org_id: Uuid, report_id: Uuid) -> Result<(i64, i64)> {
    let mut tx = pool.begin().await?;
    // Confirm the report belongs to this org.
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM finance.royalty_reports WHERE id=$1 AND org_id=$2")
            .bind(report_id)
            .bind(org_id)
            .fetch_optional(&mut *tx)
            .await?;
    if exists.is_none() {
        return Err(Error::NotFound);
    }
    let by_isrc = sqlx::query(
        "UPDATE finance.report_lines rl SET
           match_status='AUTO',
           matched_release_id=t.release_id,
           match_evidence=jsonb_build_object('isrc', rl.isrc, 'matched_at', now()::text)
         -- Only an ISRC that belongs to exactly one release matches
         -- automatically (the same recording on a single and an album is
         -- normal); the rest stay UNMATCHED for review (INV-09).
         FROM (SELECT isrc, (array_agg(release_id))[1] AS release_id FROM catalog.tracks
                WHERE org_id=$2 AND isrc IS NOT NULL
                GROUP BY isrc HAVING count(DISTINCT release_id)=1) t
         WHERE rl.report_id=$1 AND rl.org_id=$2
           AND rl.match_status='UNMATCHED'
           AND rl.isrc IS NOT NULL
           AND t.isrc=rl.isrc",
    )
    .bind(report_id)
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    // A recording on both a single and an album is ambiguous by ISRC alone,
    // and splits are per release. DSP reports name the product too: a line
    // carrying a UPC matches the one release with that UPC and that ISRC.
    // The release's UPC is the one the artist entered or the one Stage 3
    // issued (identifier ledger, RLS-scoped to this org). EAN-13 and UPC-A
    // spell the same number with a leading zero, so compare without them.
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org_id.to_string())
        .execute(&mut *tx)
        .await?;
    let by_product = sqlx::query(
        "UPDATE finance.report_lines rl SET
           match_status='AUTO',
           matched_release_id=p.release_id,
           match_evidence=jsonb_build_object('isrc', rl.isrc, 'upc', rl.raw->>'upc', 'matched_at', now()::text)
         FROM (SELECT DISTINCT r.id AS release_id, t.isrc, ltrim(COALESCE(r.upc, ia.identifier),'0') AS upc
                 FROM catalog.releases r
                 JOIN catalog.tracks t ON t.org_id=r.org_id AND t.release_id=r.id AND t.isrc IS NOT NULL
                 LEFT JOIN distribution.identifier_assignments ia
                   ON ia.org_id=r.org_id AND ia.release_id=r.id AND ia.kind='UPC' AND ia.status='ASSIGNED'
                WHERE r.org_id=$2 AND COALESCE(r.upc, ia.identifier) IS NOT NULL) p
         WHERE rl.report_id=$1 AND rl.org_id=$2
           AND rl.match_status='UNMATCHED'
           AND p.isrc=rl.isrc
           AND p.upc=ltrim(btrim(rl.raw->>'upc'),'0')",
    )
    .bind(report_id)
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let updated = (by_isrc + by_product) as i64;
    let unmatched: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM finance.report_lines WHERE report_id=$1 AND match_status='UNMATCHED'",
    )
    .bind(report_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("UPDATE finance.royalty_reports SET status='MATCHED' WHERE id=$1")
        .bind(report_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((updated, unmatched))
}
