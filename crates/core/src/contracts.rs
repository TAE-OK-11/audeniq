//! F0 contracts for F2–F7. These types validate data, never perform rights approval, delivery or payment.
use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveRange {
    pub starts_at: DateTime<Utc>,
    pub ends_at_exclusive: Option<DateTime<Utc>>,
    pub indefinite: bool,
}
impl EffectiveRange {
    pub fn validate(&self) -> Result<()> {
        if self.indefinite != self.ends_at_exclusive.is_none()
            || self.ends_at_exclusive.is_some_and(|e| e <= self.starts_at)
        {
            Err(Error::Invalid)
        } else {
            Ok(())
        }
    }
    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        self.validate().is_ok()
            && at >= self.starts_at
            && self.ends_at_exclusive.is_none_or(|e| at < e)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Subject {
    Release(Uuid),
    Track(Uuid),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantAtom {
    pub id: Uuid,
    pub org_id: Uuid,
    pub subject: Subject,
    pub grantor_party_id: Uuid,
    pub grantee_party_id: Uuid,
    pub right_type: String,
    pub territory_set: BTreeSet<String>,
    pub use_set: BTreeSet<String>,
    pub effective: EffectiveRange,
    pub exclusive: bool,
    pub sublicensable: bool,
    pub parent_grant_id: Option<Uuid>,
    pub contract_revision_id: Uuid,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revision: u32,
}
impl GrantAtom {
    pub fn validate_structure(&self) -> Result<()> {
        self.effective.validate()?;
        if self.revision == 0
            || self.grantor_party_id == self.grantee_party_id
            || self.territory_set.is_empty()
            || self.use_set.is_empty()
            || self.right_type.is_empty()
            || self
                .territory_set
                .iter()
                .any(|s| s.len() != 2 || !s.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitLine {
    pub payee_id: Uuid,
    pub payee_party_id: Uuid,
    pub share_bps: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitPlan {
    pub id: Uuid,
    pub org_id: Uuid,
    pub release_id: Uuid,
    pub scope_key: String,
    pub effective: EffectiveRange,
    pub contract_revision_id: Uuid,
    pub lines: Vec<SplitLine>,
    pub body_hash: String,
}
impl SplitPlan {
    pub fn validate_structure(&self) -> Result<()> {
        self.effective.validate()?;
        let unique: BTreeSet<_> = self.lines.iter().map(|l| l.payee_id).collect();
        if unique.len() != self.lines.len()
            || self
                .lines
                .iter()
                .map(|l| u32::from(l.share_bps))
                .sum::<u32>()
                != 10000
            || self.scope_key.is_empty()
        {
            Err(Error::Invalid)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RouteKind {
    Direct,
    Merlin,
    Limbo,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteContract {
    pub id: Uuid,
    pub org_id: Uuid,
    pub dsp_id: Uuid,
    pub kind: RouteKind,
    pub contract_id: Uuid,
    pub endpoint_id: Uuid,
    pub fee_schedule_id: Uuid,
    pub profile_version: String,
    pub adapter_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionPackage {
    pub id: Uuid,
    pub org_id: Uuid,
    pub snapshot_id: Uuid,
    pub route: RouteContract,
    pub operation: DeliveryOperation,
    pub package_digest: String,
    pub manifest_hash: String,
    pub immutable_bytes_ref: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryOperation {
    NewRelease,
    Update,
    Takedown,
    Migration,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PayoutStatus {
    Draft,
    AwaitingApproval,
    Approved,
    SubmittedUnknown,
    Confirmed,
    Rejected,
    OnHold,
}
impl PayoutStatus {
    pub fn automatic_resubmission_allowed(&self) -> bool {
        false
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerLine {
    pub account_id: Uuid,
    pub currency: String,
    pub debit_minor: i128,
    pub credit_minor: i128,
}
/// Integer minor units are exact, never binary floating point. Currency scale must be pinned by finance policy.
pub fn validate_balanced(lines: &[LedgerLine]) -> Result<()> {
    if lines.len() < 2 {
        return Err(Error::Invalid);
    }
    let currency = &lines[0].currency;
    let mut balance = 0i128;
    for l in lines {
        if &l.currency != currency
            || currency.len() != 3
            || l.debit_minor < 0
            || l.credit_minor < 0
            || (l.debit_minor > 0) == (l.credit_minor > 0)
        {
            return Err(Error::Invalid);
        }
        balance = balance
            .checked_add(l.debit_minor)
            .and_then(|n| n.checked_sub(l.credit_minor))
            .ok_or(Error::Invalid)?;
    }
    if balance != 0 {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interval_is_half_open() {
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .to_utc();
        let end = start + chrono::Duration::days(1);
        let r = EffectiveRange {
            starts_at: start,
            ends_at_exclusive: Some(end),
            indefinite: false,
        };
        assert!(r.contains(start));
        assert!(!r.contains(end));
    }
    #[test]
    fn money_balanced_and_currency_separate() {
        let a = LedgerLine {
            account_id: Uuid::new_v4(),
            currency: "KRW".into(),
            debit_minor: 100,
            credit_minor: 0,
        };
        let mut b = LedgerLine {
            account_id: Uuid::new_v4(),
            currency: "KRW".into(),
            debit_minor: 0,
            credit_minor: 100,
        };
        assert!(validate_balanced(&[a.clone(), b.clone()]).is_ok());
        b.currency = "USD".into();
        assert!(validate_balanced(&[a, b]).is_err());
        assert!(!PayoutStatus::SubmittedUnknown.automatic_resubmission_allowed());
    }
}
