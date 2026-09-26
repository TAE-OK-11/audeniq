//! Identifier validation, the append-only assignment ledger, and issuance of
//! missing UPC/ISRC codes from the active issuer (migration 0041). Until real
//! ranges are registered the active issuers are VIRTUAL: test-only codes that
//! execution never sends to a real partner.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

/// Strict normalized ISRC (12 ASCII characters, without presentation hyphens).
/// A syntactically valid ISRC is not proof of ownership or issuance authority.
pub fn validate_isrc(value: &str) -> Result<()> {
    let b = value.as_bytes();
    if b.len() != 12
        || !b[..2].iter().all(u8::is_ascii_uppercase)
        || !b[2..5]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        || !b[5..].iter().all(u8::is_ascii_digit)
    {
        return Err(Error::Invalid);
    }
    // Placeholders, never issued: an all-zero registrant or designation code
    // (sandbox round 2: 'ZZ0000000000' was accepted and delivered). "ZZ"
    // itself is a real prefix (issued directly by the International ISRC
    // Agency), so the country code alone is not rejected.
    if &b[2..5] == b"000" || &b[7..] == b"00000" {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// UPC-A only: 12 digits including a correct GS1 modulo-10 check digit.
/// EAN-13 is intentionally not silently accepted as UPC-A.
pub fn validate_upc(value: &str) -> Result<()> {
    let b = value.as_bytes();
    if b.len() != 12 || !b.iter().all(u8::is_ascii_digit) {
        return Err(Error::Invalid);
    }
    let sum: u32 = b
        .iter()
        .enumerate()
        .map(|(i, c)| u32::from(c - b'0') * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    if !sum.is_multiple_of(10) || value == "000000000000" {
        return Err(Error::Invalid);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierKind {
    Isrc,
    Upc,
}

impl IdentifierKind {
    fn label(self) -> &'static str {
        match self {
            Self::Isrc => "ISRC",
            Self::Upc => "UPC",
        }
    }

    fn validate(self, value: &str) -> Result<()> {
        match self {
            Self::Isrc => validate_isrc(value),
            Self::Upc => validate_upc(value),
        }
    }
}

/// Codes from the virtual (test) ranges of migration 0041: UPC number system
/// 2 (GS1 restricted circulation) and ISRC country XX (ISO user-assigned).
/// Neither is valid for retail distribution, whoever wrote it.
pub fn is_virtual(kind: IdentifierKind, value: &str) -> bool {
    match kind {
        IdentifierKind::Upc => value.starts_with('2'),
        IdentifierKind::Isrc => value.starts_with("XX"),
    }
}

impl IdentifierKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_uppercase().as_str() {
            "UPC" => Ok(Self::Upc),
            "ISRC" => Ok(Self::Isrc),
            _ => Err(Error::Invalid),
        }
    }
}

/// Issuers and how far each range has been used (operator view).
pub async fn list_issuers(pool: &PgPool) -> Result<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('kind',i.kind,'mode',i.mode,'prefix',i.prefix,'active',i.active,'created_at',i.created_at,
                'issued',COALESCE((SELECT jsonb_object_agg(c.scope,c.last_value) FROM distribution.identifier_counters c WHERE c.issuer_id=i.id),'{}'::jsonb))
         FROM distribution.identifier_issuers i ORDER BY i.kind, i.created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(json!(rows))
}

/// Registers the company's real range (GS1 company prefix for UPC, ISRC
/// registrant code such as `KR-A1B`) and makes it the active issuer of its
/// kind. The previous issuer (the VIRTUAL test range) stops issuing; codes
/// already assigned never change. Runs with the owner role (audeniq-admin).
pub async fn register_issuer(
    pool: &PgPool,
    operator: &str,
    kind: IdentifierKind,
    prefix: &str,
) -> Result<Uuid> {
    let prefix: String = prefix
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let b = prefix.as_bytes();
    let valid = match kind {
        // GS1 company prefixes for UPC-A are 6 to 10 digits.
        IdentifierKind::Upc => (6..=10).contains(&b.len()) && b.iter().all(u8::is_ascii_digit),
        IdentifierKind::Isrc => {
            b.len() == 5
                && b[..2].iter().all(u8::is_ascii_uppercase)
                && b[2..]
                    .iter()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        }
    };
    if operator.trim().is_empty() || !valid || is_virtual(kind, &prefix) {
        return Err(Error::Invalid);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE distribution.identifier_issuers SET active=false WHERE kind=$1 AND active")
        .bind(kind.label())
        .execute(&mut *tx)
        .await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO distribution.identifier_issuers(id,kind,mode,prefix) VALUES($1,$2,'REGISTERED',$3)
         ON CONFLICT(kind,prefix) DO UPDATE SET active=true RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(kind.label())
    .bind(&prefix)
    .fetch_one(&mut *tx)
    .await?;
    crate::operations::audit(
        &mut tx,
        None,
        None,
        Some(id),
        "identifier_issuer.registered",
        &format!("OPERATOR:{} {} {prefix}", operator.trim(), kind.label()),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

fn upc_check_digit(body: &str) -> u32 {
    let sum: u32 = body
        .bytes()
        .enumerate()
        .map(|(i, c)| u32::from(c - b'0') * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    (10 - sum % 10) % 10
}

/// Code number `n` of an issuer range.
/// UPC-A: prefix + zero-padded item reference to 11 digits + check digit.
/// ISRC: 5-character registrant prefix + two-digit year (`scope`) + 5-digit
/// designation code.
pub fn compose(kind: IdentifierKind, prefix: &str, scope: &str, n: i64) -> Result<String> {
    let exhausted = Error::PolicyGate("IDENTIFIER_RANGE_EXHAUSTED");
    let value = match kind {
        IdentifierKind::Upc => {
            let width = 11usize
                .checked_sub(prefix.len())
                .filter(|w| *w > 0)
                .ok_or(Error::Invalid)?;
            if n < 1 || n >= 10i64.pow(width as u32) {
                return Err(exhausted);
            }
            let body = format!("{prefix}{n:0width$}");
            format!("{body}{}", upc_check_digit(&body))
        }
        IdentifierKind::Isrc => {
            if !(1..=99_999).contains(&n) {
                return Err(exhausted);
            }
            format!("{prefix}{scope}{n:05}")
        }
    };
    kind.validate(&value)?;
    Ok(value)
}

async fn assigned(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    track: Option<Uuid>,
) -> Result<Option<String>> {
    Ok(match track {
        Some(t) => sqlx::query_scalar(
            "SELECT identifier FROM distribution.identifier_assignments WHERE org_id=$1 AND track_id=$2 AND kind='ISRC'",
        )
        .bind(org)
        .bind(t)
        .fetch_optional(&mut *c)
        .await?,
        None => sqlx::query_scalar(
            "SELECT identifier FROM distribution.identifier_assignments WHERE org_id=$1 AND release_id=$2 AND kind='UPC'",
        )
        .bind(org)
        .bind(release)
        .fetch_optional(&mut *c)
        .await?,
    })
}

/// The code already recorded for this target (a release's UPC, a track's
/// ISRC), or the next code of the active issuer, recorded in the ledger as
/// ISSUED (registered range) or VIRTUAL (test range). A target keeps its code
/// forever, across retries and resubmissions. `c` must be a transaction with
/// `app.org_id` set to `org`: the ledger is RLS-protected.
pub async fn issue_or_reuse(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    track: Option<Uuid>,
    revision: Uuid,
    kind: IdentifierKind,
) -> Result<String> {
    if matches!(kind, IdentifierKind::Isrc) != track.is_some() {
        return Err(Error::Invalid);
    }
    if let Some(v) = assigned(c, org, release, track).await? {
        return Ok(v);
    }
    let issuer: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, mode, prefix FROM distribution.identifier_issuers WHERE kind=$1 AND active",
    )
    .bind(kind.label())
    .fetch_optional(&mut *c)
    .await?;
    let (issuer_id, mode, prefix) = issuer.ok_or(Error::PolicyGate("IDENTIFIER_ISSUANCE_OFF"))?;
    let scope = match kind {
        IdentifierKind::Isrc => chrono::Utc::now().format("%y").to_string(),
        IdentifierKind::Upc => String::new(),
    };
    let source = if mode == "VIRTUAL" {
        "VIRTUAL"
    } else {
        "ISSUED"
    };
    // A number can already be taken by a code an artist supplied (the ledger
    // is globally unique per code): skip it and take the next one.
    for _ in 0..20 {
        let n: i64 = sqlx::query_scalar(
            "INSERT INTO distribution.identifier_counters(issuer_id,scope,last_value) VALUES($1,$2,1)
             ON CONFLICT(issuer_id,scope) DO UPDATE SET last_value=identifier_counters.last_value+1
             RETURNING last_value",
        )
        .bind(issuer_id)
        .bind(&scope)
        .fetch_one(&mut *c)
        .await?;
        let value = compose(kind, &prefix, &scope, n)?;
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO distribution.identifier_assignments(id,org_id,release_id,track_id,revision_id,kind,identifier,source)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(release)
        .bind(track)
        .bind(revision)
        .bind(kind.label())
        .bind(&value)
        .bind(source)
        .fetch_optional(&mut *c)
        .await?;
        if inserted.is_some() {
            return Ok(value);
        }
        // A concurrent issue for the same target won: use its code.
        if let Some(v) = assigned(c, org, release, track).await? {
            return Ok(v);
        }
    }
    Err(Error::PolicyGate("IDENTIFIER_RANGE_EXHAUSTED"))
}

#[derive(Debug, Clone, Copy)]
pub struct ExistingAssignment<'a> {
    pub org_id: Uuid,
    pub release_id: Uuid,
    pub track_id: Option<Uuid>,
    pub revision_id: Uuid,
    pub kind: IdentifierKind,
    pub value: &'a str,
}

/// Caller owns authorization and transaction. Global uniqueness serializes competing
/// assignments; a conflict is never reassigned. Exact same target is idempotent.
/// Batch version of [`record_existing`]: one revision-membership check per
/// distinct (org, release, revision) triple and a single multi-row INSERT
/// for all assignments, instead of 2N round trips. Semantics are identical:
/// exact-target retries are idempotent (the existing row id is returned), a
/// cross-target conflict is a permanent [`Error::Conflict`], and the whole
/// batch aborts on the first conflict — the caller rolls the transaction
/// back, so no partial batch ever commits.
pub async fn record_existing_batch(
    c: &mut PgConnection,
    assignments: &[ExistingAssignment<'_>],
) -> Result<Vec<Uuid>> {
    use std::collections::{HashMap, HashSet};
    if assignments.is_empty() {
        return Ok(Vec::new());
    }
    for a in assignments {
        a.kind.validate(a.value)?;
        if matches!(a.kind, IdentifierKind::Isrc) != a.track_id.is_some() {
            return Err(Error::Invalid);
        }
    }
    // The membership check is loop-invariant per triple; run it once each.
    let triples: HashSet<(Uuid, Uuid, Uuid)> = assignments
        .iter()
        .map(|a| (a.org_id, a.release_id, a.revision_id))
        .collect();
    for (org_id, release_id, revision_id) in &triples {
        let belongs: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM catalog.application_revisions WHERE org_id=$1 AND release_id=$2 AND id=$3)",
        )
        .bind(org_id)
        .bind(release_id)
        .bind(revision_id)
        .fetch_one(&mut *c)
        .await?;
        if !belongs {
            return Err(Error::Conflict);
        }
    }
    // Single multi-row INSERT; rows that lose a uniqueness race are skipped
    // by ON CONFLICT DO NOTHING and resolved per row below (rare path).
    let mut qb = sqlx::QueryBuilder::new(
        "INSERT INTO distribution.identifier_assignments(id,org_id,release_id,track_id,revision_id,kind,identifier) ",
    );
    let ids: Vec<Uuid> = assignments.iter().map(|_| Uuid::new_v4()).collect();
    qb.push_values(assignments.iter().zip(ids.iter()), |mut b, (a, id)| {
        b.push_bind(id)
            .push_bind(a.org_id)
            .push_bind(a.release_id)
            .push_bind(a.track_id)
            .push_bind(a.revision_id)
            .push_bind(a.kind.label())
            .push_bind(a.value);
    });
    qb.push(" ON CONFLICT DO NOTHING RETURNING id, kind, identifier, track_id");
    let inserted: Vec<(Uuid, String, String, Option<Uuid>)> =
        qb.build_query_as().fetch_all(&mut *c).await?;
    // (kind, identifier, track_id) -> row id, for the rows this batch won.
    let won: HashMap<(&str, &str, Option<Uuid>), Uuid> = inserted
        .iter()
        .map(|(id, kind, identifier, track_id)| {
            ((kind.as_str(), identifier.as_str(), *track_id), *id)
        })
        .collect();
    let mut out = Vec::with_capacity(assignments.len());
    for a in assignments {
        if let Some(won_id) = won.get(&(a.kind.label(), a.value, a.track_id)) {
            out.push(*won_id);
            continue;
        }
        // Lost the race: exact-target retry is idempotent, anything else is
        // a permanent cross-target conflict. (Separate statement sees the
        // committed winner after a concurrent INSERT wait.)
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM distribution.identifier_assignments WHERE kind=$1 AND identifier=$2 AND org_id=$3 AND release_id=$4 AND track_id IS NOT DISTINCT FROM $5",
        )
        .bind(a.kind.label())
        .bind(a.value)
        .bind(a.org_id)
        .bind(a.release_id)
        .bind(a.track_id)
        .fetch_optional(&mut *c)
        .await?;
        match existing {
            Some(existing_id) => out.push(existing_id),
            None => return Err(Error::Conflict),
        }
    }
    Ok(out)
}
pub async fn record_existing(c: &mut PgConnection, a: &ExistingAssignment<'_>) -> Result<Uuid> {
    a.kind.validate(a.value)?;
    if matches!(a.kind, IdentifierKind::Isrc) != a.track_id.is_some() {
        return Err(Error::Invalid);
    }
    let belongs: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog.application_revisions WHERE org_id=$1 AND release_id=$2 AND id=$3)"
    ).bind(a.org_id).bind(a.release_id).bind(a.revision_id).fetch_one(&mut *c).await?;
    if !belongs {
        return Err(Error::Conflict);
    }
    let id = Uuid::new_v4();
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO distribution.identifier_assignments(id,org_id,release_id,track_id,revision_id,kind,identifier) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING RETURNING id"
    ).bind(id).bind(a.org_id).bind(a.release_id).bind(a.track_id).bind(a.revision_id)
        .bind(a.kind.label()).bind(a.value).fetch_optional(&mut *c).await?;
    if let Some(id) = inserted {
        return Ok(id);
    }
    // Separate statement sees the committed winner after a concurrent INSERT wait.
    sqlx::query_scalar(
        "SELECT id FROM distribution.identifier_assignments WHERE kind=$1 AND identifier=$2 AND org_id=$3 AND release_id=$4 AND track_id IS NOT DISTINCT FROM $5"
    ).bind(a.kind.label()).bind(a.value).bind(a.org_id).bind(a.release_id).bind(a.track_id)
        .fetch_optional(c).await?.ok_or(Error::Conflict)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placeholder_isrcs_are_rejected() {
        for bad in [
            "ZZ0000000000",
            "USABC2600000",
            "US0002600001",
            "KRA0026000001",
        ] {
            assert!(validate_isrc(bad).is_err(), "{bad}");
        }
        for good in [
            "ZZA012600001",
            "USABC2600001",
            "KRA302612345",
            "QZES82600001",
        ] {
            assert!(validate_isrc(good).is_ok(), "{good}");
        }
    }

    #[test]
    fn normalized_isrc_only() {
        for s in ["USAAA2600001", "KRA1Z2600002"] {
            assert!(validate_isrc(s).is_ok());
        }
        for s in [
            "",
            "US-AAA-26-00001",
            "usAAA2600001",
            "1SAAA2600001",
            "USAAA260000１",
            " USAAA2600001",
            "USAAA2600001\n",
        ] {
            assert!(validate_isrc(s).is_err(), "{s:?}");
        }
    }
    #[test]
    fn upc_checksum_and_leading_zero() {
        for s in ["012345678905", "036000291452"] {
            assert!(validate_upc(s).is_ok());
        }
        for s in [
            "",
            "012345678904",
            "0036000291452",
            "03600029145",
            "000000000000",
            "03600029145X",
            "036000291452\n",
        ] {
            assert!(validate_upc(s).is_err(), "{s:?}");
        }
    }
    #[test]
    fn virtual_ranges() {
        assert!(is_virtual(IdentifierKind::Upc, "200000000011"));
        assert!(!is_virtual(IdentifierKind::Upc, "036000291452"));
        assert!(is_virtual(IdentifierKind::Isrc, "XXAUD2600001"));
        assert!(!is_virtual(IdentifierKind::Isrc, "KRA402600001"));
    }
    #[test]
    fn composed_codes_are_valid_and_bounded() {
        // Virtual ranges from migration 0041.
        assert_eq!(
            compose(IdentifierKind::Upc, "2", "", 1).unwrap(),
            "200000000011"
        );
        assert_eq!(
            compose(IdentifierKind::Isrc, "XXAUD", "26", 1).unwrap(),
            "XXAUD2600001"
        );
        // A registered GS1 prefix fills the remaining item-reference digits.
        let upc = compose(IdentifierKind::Upc, "0812345", "", 42).unwrap();
        assert_eq!(&upc[..11], "08123450042");
        assert!(validate_upc(&upc).is_ok());
        for (kind, prefix, scope, n) in [
            (IdentifierKind::Upc, "2", "", 0),
            (IdentifierKind::Upc, "2", "", 10_000_000_000),
            (IdentifierKind::Upc, "0812345678", "", 10),
            (IdentifierKind::Isrc, "XXAUD", "26", 100_000),
        ] {
            assert!(matches!(
                compose(kind, prefix, scope, n),
                Err(Error::PolicyGate("IDENTIFIER_RANGE_EXHAUSTED"))
            ));
        }
        // 11-digit prefix leaves no item reference.
        assert!(compose(IdentifierKind::Upc, "08123456789", "", 1).is_err());
    }
}
