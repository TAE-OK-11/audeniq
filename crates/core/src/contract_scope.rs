//! 계약 범위 → 배급 권한 (migration 0125).
//!
//! Staff fix the distribution scope with the agreement terms
//! (`agreement::set_terms`): territories (or Worldwide), excluded territories,
//! the contract's last day, the DSPs it covers and whether UGC / Content ID
//! platforms are allowed. Once the agreement is APPROVED or SIGNED the same
//! scope is checked everywhere a release can move towards a DSP:
//!   * staff approval — a scope that leaves no requested DSP is refused;
//!   * the DSP-neutral model (`preparation_model`) — the ERN deal carries only
//!     the territories the application, the rights grants and the contract
//!     all reach, and ends no later than the contract;
//!   * staging — a DSP outside the scope is a blocker per DSP;
//!   * right before the send — expiry, DSP and the outgoing ERN's deal
//!     territories are re-checked.
use crate::{
    dsp_registry::Dsp,
    error::{Error, Result},
    territory::Territories,
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::collections::BTreeSet;
use uuid::Uuid;

/// What staff enter. Empty `territories` (or `WORLD`) = Worldwide (the
/// application's own territories still apply); empty `dsps` = every DSP the
/// application chose.
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
    /// Where the contract licenses delivery (exclusions applied).
    pub territories: Territories,
    /// Last licensed day (inclusive).
    pub end: Option<NaiveDate>,
    /// None = every DSP the application chose.
    pub dsps: Option<BTreeSet<String>>,
    pub ugc: bool,
}

fn scope_territories(listed: &[String], excluded: &[String]) -> Result<Territories> {
    let invalid = |_| Error::InvalidCode("CONTRACT_SCOPE_TERRITORY_INVALID");
    let listed = Territories::from_codes(listed).map_err(invalid)?;
    let excluded = match Territories::from_codes(excluded).map_err(invalid)? {
        Territories::Only(x) => x,
        // Excluding "everywhere" leaves nothing.
        _ if !excluded.is_empty() => {
            return Err(Error::InvalidCode("CONTRACT_SCOPE_TERRITORY_EMPTY"));
        }
        _ => vec![],
    };
    if excluded.is_empty() {
        return Ok(listed);
    }
    listed
        .narrow(&Territories::WorldExcept(excluded))
        .ok_or(Error::InvalidCode("CONTRACT_SCOPE_TERRITORY_EMPTY"))
}

impl ContractScope {
    /// Validate staff input.
    pub fn from_input(i: &ScopeInput, today: NaiveDate) -> Result<Self> {
        if i.territories.len() > 250 || i.excluded_territories.len() > 250 || i.dsps.len() > 100 {
            return Err(Error::Invalid);
        }
        let territories = scope_territories(&i.territories, &i.excluded_territories)?;
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
        Ok(ContractScope {
            territories,
            end,
            dsps: (!dsps.is_empty()).then_some(dsps),
            ugc: i.ugc,
        })
    }

    pub fn to_json(&self) -> Value {
        let (listed, excluded): (&[String], &[String]) = match &self.territories {
            Territories::Worldwide => (&[], &[]),
            Territories::WorldExcept(x) => (&[], x),
            Territories::Only(x) => (x, &[]),
        };
        json!({
            "territories": listed,
            "worldwide": !matches!(self.territories, Territories::Only(_)),
            "excluded_territories": excluded,
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
        let listed = if v["worldwide"].as_bool().unwrap_or(true) {
            vec![]
        } else {
            strings("territories")
        };
        let end = v["end_date"]
            .as_str()
            .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d"))
            .transpose()
            .map_err(|_| Error::Internal)?;
        Ok(ContractScope {
            territories: scope_territories(&listed, &strings("excluded_territories"))
                .map_err(|_| Error::Internal)?,
            end,
            dsps: if v["all_dsps"].as_bool().unwrap_or(true) {
                None
            } else {
                Some(strings("dsps").into_iter().collect())
            },
            ugc: v["ugc"].as_bool().unwrap_or(false),
        })
    }

    /// One line for the agreement body: `전 세계`, `전 세계 (제외 지역: CN)`
    /// or `KR, JP`.
    pub fn territory_text(&self) -> String {
        match &self.territories {
            Territories::Worldwide => "전 세계".into(),
            Territories::WorldExcept(x) => format!("전 세계 (제외 지역: {})", x.join(", ")),
            Territories::Only(x) => x.join(", "),
        }
    }

    /// Why this DSP may not receive the release on `today` (empty = allowed).
    pub fn dsp_findings(&self, dsp: Dsp, today: NaiveDate) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(end) = self.end.filter(|end| today > *end) {
            out.push(("CONTRACT_EXPIRED", format!("contract ended {end}")));
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
    /// deal only when the contract is Worldwide (repeating every exclusion),
    /// never a territory the contract does not reach.
    pub fn ern_within(&self, ern: &str) -> bool {
        let Some(deals) = ern.find("<DealList>").map(|i| &ern[i..]) else {
            return true; // no DDEX deal (non-DDEX feed): checked per DSP instead
        };
        let listed = crate::transport::xml_values(deals, "TerritoryCode");
        let excluded = crate::transport::xml_values(deals, "ExcludedTerritoryCode");
        let worldwide = listed.iter().any(|t| t.eq_ignore_ascii_case("Worldwide"));
        let sent = if worldwide || (listed.is_empty() && excluded.is_empty()) {
            Territories::Worldwide
        } else if listed.is_empty() {
            Territories::WorldExcept(excluded)
        } else {
            Territories::Only(listed)
        };
        // Inside the contract when narrowing by it changes nothing.
        sent.narrow(&self.territories)
            .is_some_and(|n| n == normalize(sent))
    }
}

fn normalize(t: Territories) -> Territories {
    match t {
        Territories::WorldExcept(mut x) => {
            x.sort();
            x.dedup();
            Territories::WorldExcept(x)
        }
        Territories::Only(mut x) => {
            x.sort();
            x.dedup();
            Territories::Only(x)
        }
        t => t,
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
    fn input_validates_and_round_trips() {
        let today = day(2026, 10, 11);
        let s = ContractScope::from_input(&ScopeInput::default(), today).unwrap();
        assert!(s.territories.is_worldwide() && s.dsps.is_none() && !s.ugc);
        let s = ContractScope::from_input(
            &ScopeInput {
                territories: vec!["kr".into(), "JP".into()],
                excluded_territories: vec!["JP".into()],
                end_date: Some("2028-12-31".into()),
                dsps: vec!["D-5".into()],
                ugc: false,
            },
            today,
        )
        .unwrap();
        assert_eq!(s.territories, Territories::Only(vec!["KR".into()]));
        assert_eq!(ContractScope::from_json(&s.to_json()).unwrap(), s);
        let w = ContractScope::from_input(
            &ScopeInput {
                excluded_territories: vec!["CN".into()],
                ..Default::default()
            },
            today,
        )
        .unwrap();
        assert_eq!(w.territories, Territories::WorldExcept(vec!["CN".into()]));
        assert_eq!(ContractScope::from_json(&w.to_json()).unwrap(), w);
        assert_eq!(w.territory_text(), "전 세계 (제외 지역: CN)");
        for bad in [
            ScopeInput {
                territories: vec!["KR".into()],
                excluded_territories: vec!["KR".into()],
                ..Default::default()
            },
            ScopeInput {
                end_date: Some("2020-01-01".into()),
                ..Default::default()
            },
            ScopeInput {
                territories: vec!["Korea".into()],
                ..Default::default()
            },
            ScopeInput {
                dsps: vec!["D-999".into()],
                ..Default::default()
            },
        ] {
            assert!(ContractScope::from_input(&bad, today).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn dsp_and_ern_checks_follow_the_scope() {
        let scope = ContractScope {
            territories: Territories::Only(vec!["JP".into(), "KR".into()]),
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
        assert!(!scope.ern_within(&deal("<ExcludedTerritoryCode>CN</ExcludedTerritoryCode>")));
        let world = ContractScope {
            territories: Territories::WorldExcept(vec!["CN".into()]),
            end: None,
            dsps: None,
            ugc: true,
        };
        assert!(world.ern_within(&deal("<ExcludedTerritoryCode>CN</ExcludedTerritoryCode>")));
        assert!(world.ern_within(&deal(
            "<ExcludedTerritoryCode>CN</ExcludedTerritoryCode><ExcludedTerritoryCode>JP</ExcludedTerritoryCode>"
        )));
        assert!(world.ern_within(&deal("<TerritoryCode>KR</TerritoryCode>")));
        assert!(!world.ern_within(&deal("<TerritoryCode>Worldwide</TerritoryCode>")));
        assert!(!world.ern_within(&deal("<TerritoryCode>CN</TerritoryCode>")));
        // A feed without a DDEX deal is checked per DSP instead.
        assert!(scope.ern_within("<feed/>"));
    }
}
