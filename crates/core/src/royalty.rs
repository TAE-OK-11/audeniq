//! Normalized royalty import and conservative matching.
//! Real partner schemas must be normalized before import. Import and matching
//! never confirm receipt of money or authorize payout.

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
    let mut records = crate::royalty_csv::records(content)
        .map_err(Error::InvalidCode)?
        .into_iter();
    let header = records.next().ok_or(Error::Invalid)?;
    let cols: Vec<String> = header.iter().map(|s| s.trim().to_owned()).collect();
    let mut col_pos = std::collections::HashMap::new();
    for (i, col) in cols.iter().enumerate() {
        if col.is_empty() || col_pos.insert(col.as_str(), i).is_some() {
            return Err(Error::InvalidCode("INVALID_REPORT_HEADER"));
        }
    }
    for required in ["isrc", "quantity", "gross_amount"] {
        if !col_pos.contains_key(required) {
            return Err(Error::InvalidCode("INVALID_REPORT_HEADER"));
        }
    }
    let mut lines = Vec::new();
    for (n, row) in records.enumerate() {
        if row.iter().all(|s| s.trim().is_empty()) {
            continue;
        }
        if row.len() != cols.len() {
            return Err(Error::InvalidCode("INVALID_REPORT_COLUMNS"));
        }
        let get = |name: &str| -> Option<String> {
            col_pos
                .get(name)
                .map(|&i| row[i].trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        let parse_dec = |name: &str| -> Result<rust_decimal::Decimal> {
            get(name)
                .ok_or(Error::InvalidCode("REPORT_AMOUNT_REQUIRED"))?
                .parse()
                .map_err(|_| Error::InvalidCode("INVALID_REPORT_AMOUNT"))
        };
        let quantity = parse_dec("quantity")?;
        let gross_amount = parse_dec("gross_amount")?;
        let currency = get("currency");
        if currency
            .as_ref()
            .is_some_and(|c| c.len() != 3 || !c.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err(Error::InvalidCode("INVALID_CURRENCY"));
        }
        // Reports spell ISRCs as 'usabc2600001' or 'US-ABC-26-00001' too.
        let isrc = get("isrc")
            .map(|i| {
                i.chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
                    .to_ascii_uppercase()
            })
            .filter(|i| !i.is_empty());
        let dsp_track_id = get("dsp_track_id");
        let raw = cols
            .iter()
            .cloned()
            .zip(row.into_iter().map(serde_json::Value::String))
            .collect();
        lines.push(ReportLine {
            line_no: n as i32,
            isrc,
            dsp_track_id,
            quantity: Some(quantity),
            gross_amount: Some(gross_amount),
            currency,
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
    if dsp_id.trim().is_empty()
        || dsp_id.len() > 120
        || filename.trim().is_empty()
        || filename.len() > 500
    {
        return Err(Error::Invalid);
    }
    let lines = parse_csv(content)?;
    if lines.is_empty() {
        return Err(Error::Invalid);
    }
    if lines
        .iter()
        .any(|l| l.currency.as_deref().is_some_and(|c| c != currency))
    {
        return Err(Error::InvalidCode("MIXED_REPORT_CURRENCY"));
    }
    let source_hash = crate::domain::sha256_hex(content);
    let report_id = Uuid::new_v4();

    let mut tx = pool.begin().await?;
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO finance.royalty_reports
         (id,org_id,dsp_id,period_start,period_end,source_filename,source_hash,currency,status,raw_ref)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,'NORMALIZED',$9)
         ON CONFLICT(org_id,source_hash) DO NOTHING RETURNING id",
    )
    .bind(report_id).bind(org_id).bind(dsp_id).bind(period_start).bind(period_end)
    .bind(filename).bind(&source_hash).bind(currency)
    .bind(serde_json::json!({"format":"normalized-csv-v1","source_content":content}))
    .fetch_optional(&mut *tx).await?;
    if inserted.is_none() {
        return Err(Error::Conflict);
    }
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

/// Only an unambiguous single catalog track becomes AUTO. Unresolved lines
/// prevent MATCHED. Report locking serializes matching and candidate evidence.
pub async fn match_report(pool: &PgPool, org_id: Uuid, report_id: Uuid) -> Result<(i64, i64)> {
    let mut tx = pool.begin().await?;
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM finance.royalty_reports WHERE id=$1 AND org_id=$2 FOR UPDATE",
    )
    .bind(report_id)
    .bind(org_id)
    .fetch_optional(&mut *tx)
    .await?;
    if status.ok_or(Error::NotFound)? == "POSTED" {
        return Err(Error::PolicyGate("REPORT_ALREADY_POSTED"));
    }
    // Only an ISRC that names exactly one catalog track matches automatically.
    // The same recording on a single and an album is normal (INV-09): those
    // lines are resolved by the report's UPC below, or stay for review.
    let by_isrc = sqlx::query(
        "WITH candidates AS (
           SELECT rl.id, MIN(t.release_id::text)::uuid AS release_id
           FROM finance.report_lines rl JOIN catalog.tracks t
             ON t.org_id=rl.org_id AND t.isrc=rl.isrc
           WHERE rl.report_id=$1 AND rl.org_id=$2 AND rl.match_status='UNMATCHED'
           GROUP BY rl.id HAVING COUNT(*)=1
         )
         UPDATE finance.report_lines rl SET match_status='AUTO',
           matched_release_id=c.release_id,
           match_evidence=jsonb_build_object('isrc',rl.isrc,'matched_at',now()::text)
         FROM candidates c WHERE rl.id=c.id AND rl.org_id=$2",
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
           AND rl.match_status IN ('UNMATCHED','AMBIGUOUS')
           AND p.isrc=rl.isrc
           AND p.upc=ltrim(btrim(rl.raw->>'upc'),'0')",
    )
    .bind(report_id)
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let updated = (by_isrc + by_product) as i64;
    sqlx::query(
        "WITH ambiguous AS (
           SELECT rl.id FROM finance.report_lines rl JOIN catalog.tracks t
             ON t.org_id=rl.org_id AND t.isrc=rl.isrc
           WHERE rl.report_id=$1 AND rl.org_id=$2 AND rl.match_status='UNMATCHED'
           GROUP BY rl.id HAVING COUNT(*)>1
         )
         UPDATE finance.report_lines rl SET match_status='AMBIGUOUS',
           match_evidence=jsonb_build_object('reason','MULTIPLE_ISRC_CANDIDATES','detected_at',now()::text)
         FROM ambiguous a WHERE rl.id=a.id AND rl.org_id=$2",
    ).bind(report_id).bind(org_id).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO finance.royalty_match_candidates
         (id,org_id,report_line_id,release_id,dsp_id,binding_ref,evidence)
         SELECT gen_random_uuid(),rl.org_id,rl.id,t.release_id,r.dsp_id,
                jsonb_build_object('track_id',t.id),
                jsonb_build_object('reason','MULTIPLE_ISRC_CANDIDATES','isrc',rl.isrc)
         FROM finance.report_lines rl JOIN catalog.tracks t ON t.org_id=rl.org_id AND t.isrc=rl.isrc
         JOIN finance.royalty_reports r ON r.id=rl.report_id AND r.org_id=rl.org_id
         WHERE rl.report_id=$1 AND rl.org_id=$2 AND rl.match_status='AMBIGUOUS'
           AND NOT EXISTS(SELECT 1 FROM finance.royalty_match_candidates c
             WHERE c.org_id=$2 AND c.report_line_id=rl.id AND c.binding_ref->>'track_id'=t.id::text)",
    ).bind(report_id).bind(org_id).execute(&mut *tx).await?;
    let unresolved: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM finance.report_lines
         WHERE report_id=$1 AND org_id=$2 AND match_status NOT IN ('AUTO','MANUAL')",
    )
    .bind(report_id)
    .bind(org_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("UPDATE finance.royalty_reports SET status=$3 WHERE id=$1 AND org_id=$2")
        .bind(report_id)
        .bind(org_id)
        .bind(if unresolved == 0 {
            "MATCHED"
        } else {
            "PARTIALLY_MATCHED"
        })
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((updated, unresolved))
}

/// Immutable bank receipt evidence, independently of catalog matching.
/// Recording a receipt does not create payables or mark the report POSTED.
#[allow(clippy::too_many_arguments)]
pub async fn record_receipt(
    pool: &PgPool,
    org: Uuid,
    report: Uuid,
    actor: Uuid,
    reference: &str,
    amount: rust_decimal::Decimal,
    currency: &str,
    evidence: serde_json::Value,
) -> Result<Uuid> {
    if reference.trim().is_empty()
        || reference != reference.trim()
        || reference.len() > 200
        || amount <= rust_decimal::Decimal::ZERO
        || currency.len() != 3
        || !currency.bytes().all(|c| c.is_ascii_uppercase())
        || evidence.as_object().is_none_or(|e| e.is_empty())
    {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    // All receipt writers serialize per org, including reused references across reports.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 19))")
        .bind(org)
        .execute(&mut *tx)
        .await?;
    let report_currency: Option<String> = sqlx::query_scalar(
        "SELECT currency::text FROM finance.royalty_reports WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(report)
    .fetch_optional(&mut *tx)
    .await?;
    if report_currency.ok_or(Error::NotFound)? != currency {
        return Err(Error::PolicyGate("RECEIPT_CURRENCY_MISMATCH"));
    }
    type Receipt = (Uuid, Uuid, rust_decimal::Decimal, String, serde_json::Value);
    let existing: Option<Receipt> = sqlx::query_as(
        "SELECT id,report_id,amount,currency::text,evidence FROM finance.royalty_receipts
         WHERE org_id=$1 AND bank_reference=$2",
    )
    .bind(org)
    .bind(reference)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((id, r, a, c, e)) = existing {
        if r != report || a != amount || c != currency || e != evidence {
            return Err(Error::Conflict);
        }
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO finance.royalty_receipts
         (id,org_id,report_id,bank_reference,amount,currency,evidence,recorded_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(org)
    .bind(report)
    .bind(reference)
    .bind(amount)
    .bind(currency)
    .bind(evidence)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::parse_csv;
    use rust_decimal::Decimal;
    #[test]
    fn quoted_fields_and_negative_adjustments() {
        let rows=parse_csv("isrc,dsp_track_id,quantity,gross_amount,currency,note\r\nUSABC2600001,\"id,1\",-2,-1.25,USD,\"refund\r\nline\"\r\n").unwrap();
        assert_eq!(rows[0].dsp_track_id.as_deref(), Some("id,1"));
        assert_eq!(rows[0].gross_amount, Some(Decimal::new(-125, 2)));
        assert_eq!(rows[0].raw["note"], "refund\r\nline");
    }
    #[test]
    fn corrupt_columns_headers_amounts_and_currency_are_rejected() {
        for input in [
            "isrc,quantity,gross_amount\nx,1",
            "isrc,quantity,gross_amount\nx,1,2,3",
            "isrc,quantity,gross_amount,quantity\nx,1,2,3",
            "isrc,quantity,gross_amount\nx,,1",
            "isrc,quantity,gross_amount,currency\nx,1,2,usd",
        ] {
            assert!(parse_csv(input).is_err(), "{input:?}");
        }
    }
}
