//! Existing identifier validation and append-only assignment records. Issuance is OFF.
use crate::error::{Error, Result};
use sqlx::PgConnection;
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

#[derive(Debug, Clone, Copy)]
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

pub fn issue_identifier(_kind: IdentifierKind) -> Result<String> {
    Err(Error::PolicyGate("IDENTIFIER_ISSUANCE_OFF"))
}

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
    fn issuance_cannot_be_enabled() {
        for k in [IdentifierKind::Isrc, IdentifierKind::Upc] {
            assert!(matches!(
                issue_identifier(k),
                Err(Error::PolicyGate("IDENTIFIER_ISSUANCE_OFF"))
            ));
        }
    }
}
