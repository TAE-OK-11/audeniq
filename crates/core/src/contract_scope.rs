//! 계약 범위 → 배급 권한 (감사 2번, migration 0092).
//!
//! Staff fix the distribution scope with the agreement terms
//! (`agreement::set_terms`): territories (or Worldwide), excluded territories,
//! the contract's last day, the DSPs it covers and whether UGC / Content ID
//! platforms are allowed. Once the agreement is APPROVED or SIGNED the same
//! scope is checked everywhere a release can move towards a DSP:
//!   * staff approval — a scope that leaves no requested DSP is refused;
//!   * the DSP-neutral model (`preparation_model`) — the ERN deal carries only
//!     the contract's territories, exclusions and end date;
//!   * staging — a DSP outside the scope is a blocker per DSP;
//!   * E-1 right before the send — expiry, DSP and the outgoing ERN's deal
//!     territories are re-checked.
use crate::{
    dsp_registry::Dsp,
    error::{Error, Result},
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::collections::BTreeSet;
use uuid::Uuid;

const WORLD: &[&str] = &["WORLD", "WW", "WORLDWIDE"];

/// What staff enter. Empty `territories` (or `WORLD`) = the application's
/// territories; empty `dsps` = every DSP the application chose.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeInput {
    #[serde(default)]
    pub territories: Vec<String>,
    #[serde(default)]
    pub excluded_territories: Vec<String>,
    /// The contract's last day (inclusive), `YYYY-MM-DD`.
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub dsps: Vec<String>,
    /// UGC / Content ID platforms (fingerprinting user uploads). Off unless
    /// agreed: the agreement body excludes UGC rights management by default.
    #[serde(default)]
    pub ugc: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractScope {
    /// None = Worldwide.
    pub territories: Option<BTreeSet<String>>,
    pub excluded: BTreeSet<String>,
    /// Last licensed day (inclusive).
    pub end: Option<NaiveDate>,
    /// None = every DSP the application chose.
    pub dsps: Option<BTreeSet<String>>,
    pub ugc: bool,
}

fn code(raw: &str) -> Result<Option<String>> {
    let c = raw.trim().to_ascii_uppercase();
    if WORLD.contains(&c.as_str()) {
        return Ok(None);
    }
    if c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase()) {
        Ok(Some(c))
    } else {
        Err(Error::InvalidCode("CONTRACT_SCOPE_TERRITORY_INVALID"))
    }
}

fn codes(list: &[String]) -> Result<Option<BTreeSet<String>>> {
    let mut out = BTreeSet::new();
    for t in list {
        match code(t)? {
            None => return Ok(None),
            Some(c) => {
                out.insert(c);
            }
        }
    }
    Ok((!out.is_empty()).then_some(out))
}

impl ContractScope {
    /// Validate staff input. `application` is the application's own
    /// territories (None = Worldwide), used when staff leave them empty.
    pub fn from_input(
        i: &ScopeInput,
        application: Option<BTreeSet<String>>,
        today: NaiveDate,
    ) -> Result<Self> {
        let territories = if i.territories.is_empty() {
            application
        } else {
            codes(&i.territories)?
        };
        let excluded = codes(&i.excluded_territories)?.unwrap_or_default();
        if excluded.len() > 250 || territories.as_ref().is_some_and(|t| t.len() > 250) {
            return Err(Error::Invalid);
        }
        let end = match i
            .end_date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => None,
            Some(d) => {
                let d = NaiveDate::parse_from_str(d, "%Y-%m-%d")
                    .map_err(|_| Error::InvalidCode("CONTRACT_SCOPE_END_INVALID"))?;
                if d < today {
                    return Err(Error::InvalidCode("CONTRACT_SCOPE_END_INVALID"));
                }
                Some(d)
            }
        };
        let mut dsps = BTreeSet::new();
        for c in &i.dsps {
            let dsp =
                Dsp::from_code(c.trim()).ok_or(Error::InvalidCode("CONTRACT_SCOPE_DSP_UNKNOWN"))?;
            dsps.insert(dsp.code().to_owned());
        }
        let scope = ContractScope {
            territories,
            excluded,
            end,
            dsps: (!dsps.is_empty()).then_some(dsps),
            ugc: i.ugc,
        };
        if scope.effective_territories().is_some_and(|t| t.is_empty()) {
            return Err(Error::InvalidCode("CONTRACT_SCOPE_TERRITORY_EMPTY"));
        }
        Ok(scope)
    }

    /// Territories after exclusions (None = Worldwide minus `excluded`).
    pub fn effective_territories(&self) -> Option<BTreeSet<String>> {
        self.territories
            .as_ref()
            .map(|t| t.difference(&self.excluded).cloned().collect())
    }

    pub fn to_json(&self) -> Value {
        json!({
            "territories": self.territories.as_ref().map(|t| t.iter().collect::<Vec<_>>()).unwrap_or_default(),
            "worldwide": self.territories.is_none(),
            "excluded_territories": self.excluded.iter().collect::<Vec<_>>(),
            "end_date": self.end.map(|d| d.format("%Y-%m-%d").to_string()),
            "dsps": self.dsps.as_ref().map(|d| d.iter().collect::<Vec<_>>()).unwrap_or_default(),
            "all_dsps": self.dsps.is_none(),
            "ugc": self.ugc,
        })
    }

    /// The stored form (`agreement_terms.scope`).
    pub fn from_json(v: &Value) -> Result<Self> {
        let strings = |k: &str| -> Vec<String> {
            v[k].as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let territories = if v["worldwide"].as_bool().unwrap_or(true) {
            None
        } else {
            codes(&strings("territories"))?
        };
        let end = v["end_date"]
            .as_str()
            .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d"))
            .transpose()
            .map_err(|_| Error::Internal)?;
        Ok(ContractScope {
            territories,
            excluded: codes(&strings("excluded_territories"))?.unwrap_or_default(),
            end,
            dsps: if v["all_dsps"].as_bool().unwrap_or(true) {
                None
            } else {
                Some(strings("dsps").into_iter().collect())
            },
            ugc: v["ugc"].as_bool().unwrap_or(false),
        })
    }

    /// Why this DSP may not receive the release on `today` (empty = allowed).
    pub fn dsp_findings(&self, dsp: Dsp, today: NaiveDate) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if self.end.is_some_and(|end| today > end) {
            out.push((
                "CONTRACT_EXPIRED",
                format!("contract ended {}", self.end.unwrap()),
            ));
        }
        if self.dsps.as_ref().is_some_and(|d| !d.contains(dsp.code())) {
            out.push((
                "CONTRACT_SCOPE_DSP",
                format!("{} is not covered by the agreement", dsp.code()),
            ));
        }
        if !self.ugc && dsp.spec().content_id {
            out.push((
                "CONTRACT_SCOPE_UGC",
                format!(
                    "{} is a UGC / Content ID platform; the agreement excludes UGC",
                    dsp.code()
                ),
            ));
        }
        out
    }

    /// The DealList of an outgoing ERN stays inside the scope: a Worldwide
    /// deal only when the contract is Worldwide, never a territory the
    /// contract excludes or does not list.
    pub fn ern_within(&self, ern: &str) -> bool {
        let Some(deals) = ern.find("<DealList>").map(|i| &ern[i..]) else {
            return true; // no DDEX deal (non-DDEX feed): checked per DSP instead
        };
        let listed = crate::transport::xml_values(deals, "TerritoryCode");
        let excluded = crate::transport::xml_values(deals, "ExcludedTerritoryCode");
        let allowed = self.effective_territories();
        for t in &listed {
            if WORLD.contains(&t.to_ascii_uppercase().as_str()) {
                // Worldwide is fine only for a Worldwide contract whose
                // exclusions the ERN repeats.
                if allowed.is_some() || !self.excluded.iter().all(|e| excluded.contains(e)) {
                    return false;
                }
            } else if self.excluded.contains(t) || allowed.as_ref().is_some_and(|a| !a.contains(t))
            {
                return false;
            }
        }
        if listed.is_empty() && !excluded.is_empty() {
            // Worldwide minus `excluded`: needs a Worldwide contract and every
            // contract exclusion repeated.
            return allowed.is_none() && self.excluded.iter().all(|e| excluded.contains(e));
        }
        true
    }
}

/// The scope of the release's APPROVED or SIGNED agreement, if staff set one.
/// Read through `execution.agreement_scope` (the worker has no portal grants).
pub async fn load(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<Option<ContractScope>> {
    let v: Option<Value> = sqlx::query_scalar("SELECT execution.agreement_scope($1,$2)")
        .bind(org)
        .bind(release)
        .fetch_one(&mut *c)
        .await?;
    v.filter(|v| v.is_object())
        .map(|v| ContractScope::from_json(&v))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn input_defaults_to_the_application_and_round_trips() {
        let today = day(2026, 10, 11);
        let s = ContractScope::from_input(&ScopeInput::default(), None, today).unwrap();
        assert!(s.territories.is_none() && s.dsps.is_none() && !s.ugc);
        let s = ContractScope::from_input(
            &ScopeInput {
                territories: vec!["kr".into(), "JP".into()],
                excluded_territories: vec!["JP".into()],
                end_date: Some("2028-12-31".into()),
                dsps: vec!["D-5".into()],
                ugc: false,
            },
            None,
            today,
        )
        .unwrap();
        assert_eq!(s.effective_territories(), Some(["KR".to_string()].into()));
        assert_eq!(ContractScope::from_json(&s.to_json()).unwrap(), s);
        assert!(
            ContractScope::from_input(
                &ScopeInput {
                    territories: vec!["KR".into()],
                    excluded_territories: vec!["KR".into()],
                    ugc: true,
                    ..Default::default()
                },
                None,
                today
            )
            .is_err()
        );
        assert!(
            ContractScope::from_input(
                &ScopeInput {
                    end_date: Some("2020-01-01".into()),
                    ugc: true,
                    ..Default::default()
                },
                None,
                today
            )
            .is_err()
        );
    }

    #[test]
    fn dsp_and_ern_checks_follow_the_scope() {
        let scope = ContractScope {
            territories: Some(["KR".to_string(), "JP".to_string()].into()),
            excluded: BTreeSet::new(),
            end: Some(day(2027, 12, 31)),
            dsps: Some(["D-5".to_string()].into()),
            ugc: true,
        };
        let d5 = Dsp::from_code("D-5").unwrap();
        assert!(scope.dsp_findings(d5, day(2027, 6, 1)).is_empty());
        assert_eq!(
            scope.dsp_findings(d5, day(2028, 1, 1))[0].0,
            "CONTRACT_EXPIRED"
        );
        let other = Dsp::ALL
            .iter()
            .copied()
            .find(|d| d.code() != "D-5")
            .unwrap();
        assert!(
            scope
                .dsp_findings(other, day(2027, 6, 1))
                .iter()
                .any(|f| f.0 == "CONTRACT_SCOPE_DSP")
        );
        let deal = |t: &str| {
            format!(
                "<DealList><ReleaseDeal><Deal><DealTerms>{t}</DealTerms></Deal></ReleaseDeal></DealList>"
            )
        };
        assert!(scope.ern_within(&deal("<TerritoryCode>KR</TerritoryCode>")));
        assert!(!scope.ern_within(&deal("<TerritoryCode>Worldwide</TerritoryCode>")));
        assert!(!scope.ern_within(&deal("<TerritoryCode>US</TerritoryCode>")));
        let world = ContractScope {
            territories: None,
            excluded: ["CN".to_string()].into(),
            end: None,
            dsps: None,
            ugc: true,
        };
        assert!(world.ern_within(&deal("<ExcludedTerritoryCode>CN</ExcludedTerritoryCode>")));
        assert!(!world.ern_within(&deal("<TerritoryCode>Worldwide</TerritoryCode>")));
    }
}
