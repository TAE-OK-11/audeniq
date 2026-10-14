//! Distribution territories chosen in the Studio wizard (`draft.territories`).
//!
//! The draft value is one of
//! - `["WORLD"]` — everywhere,
//! - `["WORLD", "-JP", "-CN"]` — everywhere except the listed territories,
//! - `["KR", "JP"]` — only the listed territories.
//!
//! It is frozen in the submitted revision (what the artist signed and staff
//! reviewed) and becomes the DDEX deal territory: `TerritoryCode Worldwide`,
//! `ExcludedTerritoryCode*` or `TerritoryCode*`.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// ISO 3166-1 alpha-2 codes accepted by DDEX ERN 3.8.2 (`avs:IsoTerritoryCode`).
pub const ISO_CODES: &[&str] = &[
    "AD", "AE", "AF", "AG", "AI", "AL", "AM", "AN", "AO", "AQ", "AR", "AS", "AT", "AU", "AW", "AX",
    "AZ", "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BL", "BM", "BN", "BO", "BQ", "BR",
    "BS", "BT", "BV", "BW", "BY", "BZ", "CA", "CC", "CD", "CF", "CG", "CH", "CI", "CK", "CL", "CM",
    "CN", "CO", "CR", "CS", "CU", "CV", "CW", "CX", "CY", "CZ", "DE", "DJ", "DK", "DM", "DO", "DZ",
    "EC", "EE", "EG", "EH", "ER", "ES", "ET", "FI", "FJ", "FK", "FM", "FO", "FR", "GA", "GB", "GD",
    "GE", "GF", "GG", "GH", "GI", "GL", "GM", "GN", "GP", "GQ", "GR", "GS", "GT", "GU", "GW", "GY",
    "HK", "HM", "HN", "HR", "HT", "HU", "ID", "IE", "IL", "IM", "IN", "IO", "IQ", "IR", "IS", "IT",
    "JE", "JM", "JO", "JP", "KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ", "LA",
    "LB", "LC", "LI", "LK", "LR", "LS", "LT", "LU", "LV", "LY", "MA", "MC", "MD", "ME", "MF", "MG",
    "MH", "MK", "ML", "MM", "MN", "MO", "MP", "MQ", "MR", "MS", "MT", "MU", "MV", "MW", "MX", "MY",
    "MZ", "NA", "NC", "NE", "NF", "NG", "NI", "NL", "NO", "NP", "NR", "NU", "NZ", "OM", "PA", "PE",
    "PF", "PG", "PH", "PK", "PL", "PM", "PN", "PR", "PS", "PT", "PW", "PY", "QA", "RE", "RO", "RS",
    "RU", "RW", "SA", "SB", "SC", "SD", "SE", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN", "SO",
    "SR", "SS", "ST", "SV", "SX", "SY", "SZ", "TC", "TD", "TF", "TG", "TH", "TJ", "TK", "TL", "TM",
    "TN", "TO", "TR", "TT", "TV", "TW", "TZ", "UA", "UG", "UM", "US", "UY", "UZ", "VA", "VC", "VE",
    "VG", "VI", "VN", "VU", "WF", "WS", "YE", "YT", "ZA", "ZM", "ZW",
];

pub const INVALID: &str = "TERRITORIES_INVALID";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "codes", rename_all = "snake_case")]
pub enum Territories {
    #[default]
    Worldwide,
    /// Everywhere except these (sorted, distinct, never empty).
    WorldExcept(Vec<String>),
    /// Only these (sorted, distinct, never empty).
    Only(Vec<String>),
}

impl Territories {
    pub fn is_worldwide(&self) -> bool {
        *self == Territories::Worldwide
    }

    /// Parse `draft.territories`. Missing means worldwide (drafts from
    /// before the territory step); an empty list (the "only these" mode with
    /// nothing picked) and anything malformed are refused — never widened
    /// to worldwide.
    pub fn from_draft(draft: &Value) -> Result<Territories> {
        let list = match draft.get("territories") {
            None | Some(Value::Null) => return Ok(Territories::Worldwide),
            Some(Value::Array(a)) => a,
            Some(_) => return Err(Error::PolicyGate(INVALID)),
        };
        if list.is_empty() || list.len() > ISO_CODES.len() + 1 {
            return Err(Error::PolicyGate(INVALID));
        }
        let mut world = false;
        let mut excluded = Vec::new();
        let mut included = Vec::new();
        for v in list {
            let v = v.as_str().ok_or(Error::PolicyGate(INVALID))?;
            if v == "WORLD" {
                world = true;
            } else if let Some(code) = v.strip_prefix('-') {
                excluded.push(iso(code)?);
            } else {
                included.push(iso(v)?);
            }
        }
        let norm = |mut v: Vec<String>| {
            v.sort();
            v.dedup();
            v
        };
        match (world, excluded.is_empty(), included.is_empty()) {
            (true, true, true) => Ok(Territories::Worldwide),
            (true, false, true) => {
                let excluded = norm(excluded);
                // Excluding every territory leaves nothing to deliver.
                if excluded.len() >= ISO_CODES.len() {
                    return Err(Error::PolicyGate(INVALID));
                }
                Ok(Territories::WorldExcept(excluded))
            }
            (false, true, false) => Ok(Territories::Only(norm(included))),
            _ => Err(Error::PolicyGate(INVALID)),
        }
    }

    /// Whether a platform delivery reaches `code` (an ISO alpha-2 code).
    pub fn includes(&self, code: &str) -> bool {
        match self {
            Territories::Worldwide => true,
            Territories::WorldExcept(x) => !x.iter().any(|c| c == code),
            Territories::Only(x) => x.iter().any(|c| c == code),
        }
    }

    /// Where both reach: the application's territories narrowed by a rights
    /// grant or the agreement's scope. `None` when nothing is left.
    pub fn narrow(&self, allowed: &Territories) -> Option<Territories> {
        use Territories::*;
        let minus = |a: &[String], b: &[String]| -> Vec<String> {
            a.iter().filter(|c| !b.contains(c)).cloned().collect()
        };
        let out = match (self, allowed) {
            (t, Worldwide) => t.clone(),
            (Worldwide, t) => t.clone(),
            (WorldExcept(a), WorldExcept(b)) => {
                let mut x: Vec<String> = a.iter().chain(b).cloned().collect();
                x.sort();
                x.dedup();
                if x.len() >= ISO_CODES.len() {
                    return None;
                }
                WorldExcept(x)
            }
            (WorldExcept(x), Only(s)) | (Only(s), WorldExcept(x)) => Only(minus(s, x)),
            (Only(a), Only(b)) => Only(a.iter().filter(|c| b.contains(c)).cloned().collect()),
        };
        match &out {
            Only(x) if x.is_empty() => None,
            _ => Some(out),
        }
    }

    /// Codes in the form staff and grants use: `WORLD`/`WW`/`WORLDWIDE` or
    /// ISO alpha-2 (any case). Empty or a world marker means Worldwide.
    pub fn from_codes(list: &[String]) -> Result<Territories> {
        let mut codes = Vec::new();
        for raw in list {
            let c = raw.trim().to_ascii_uppercase();
            if matches!(c.as_str(), "WORLD" | "WW" | "WORLDWIDE") {
                return Ok(Territories::Worldwide);
            }
            codes.push(iso(&c)?);
        }
        codes.sort();
        codes.dedup();
        Ok(if codes.is_empty() {
            Territories::Worldwide
        } else {
            Territories::Only(codes)
        })
    }

    /// One line for manifests and staff screens: `Worldwide`,
    /// `Worldwide excluding JP CN` or `KR JP`.
    pub fn summary(&self) -> String {
        match self {
            Territories::Worldwide => "Worldwide".into(),
            Territories::WorldExcept(x) => format!("Worldwide excluding {}", x.join(" ")),
            Territories::Only(x) => x.join(" "),
        }
    }
}

fn iso(code: &str) -> Result<String> {
    if ISO_CODES.binary_search(&code).is_ok() {
        Ok(code.to_string())
    } else {
        Err(Error::PolicyGate(INVALID))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Result<Territories> {
        Territories::from_draft(&json!({ "territories": v }))
    }

    #[test]
    fn codes_are_sorted_for_binary_search() {
        assert!(ISO_CODES.windows(2).all(|w| w[0] < w[1]));
        assert!(ISO_CODES.contains(&"KR") && ISO_CODES.contains(&"US"));
    }

    #[test]
    fn draft_forms() {
        assert_eq!(
            Territories::from_draft(&json!({})).unwrap(),
            Territories::Worldwide
        );
        assert_eq!(parse(json!(["WORLD"])).unwrap(), Territories::Worldwide);
        assert_eq!(
            parse(json!(["WORLD", "-JP", "-CN", "-JP"])).unwrap(),
            Territories::WorldExcept(vec!["CN".into(), "JP".into()])
        );
        assert_eq!(
            parse(json!(["KR", "JP"])).unwrap(),
            Territories::Only(vec!["JP".into(), "KR".into()])
        );
        let t = parse(json!(["WORLD", "-JP"])).unwrap();
        assert!(t.includes("KR") && !t.includes("JP"));
        assert_eq!(t.summary(), "Worldwide excluding JP");
    }

    #[test]
    fn malformed_is_refused() {
        for v in [
            json!("WORLD"),
            json!([]),
            json!(["WORLD", "KR"]),
            json!(["KR", "-JP"]),
            json!(["-JP"]),
            json!(["XX"]),
            json!(["kr"]),
            json!([1]),
            json!(["WORLD", "-WORLD"]),
        ] {
            assert!(parse(v.clone()).is_err(), "{v}");
        }
    }

    #[test]
    fn narrowing_keeps_only_what_both_reach() {
        let only = |c: &[&str]| Territories::Only(c.iter().map(|s| s.to_string()).collect());
        let except =
            |c: &[&str]| Territories::WorldExcept(c.iter().map(|s| s.to_string()).collect());
        let w = Territories::Worldwide;
        assert_eq!(w.narrow(&w), Some(w.clone()));
        assert_eq!(w.narrow(&only(&["KR"])), Some(only(&["KR"])));
        assert_eq!(
            only(&["JP", "KR", "US"]).narrow(&only(&["JP", "KR"])),
            Some(only(&["JP", "KR"]))
        );
        assert_eq!(only(&["US"]).narrow(&only(&["KR"])), None);
        assert_eq!(
            except(&["CN"]).narrow(&only(&["CN", "KR"])),
            Some(only(&["KR"]))
        );
        assert_eq!(
            only(&["CN", "KR"]).narrow(&except(&["CN"])),
            Some(only(&["KR"]))
        );
        assert_eq!(
            except(&["JP"]).narrow(&except(&["CN"])),
            Some(except(&["CN", "JP"]))
        );
        assert_eq!(
            Territories::from_codes(&["kr".into(), "JP".into(), "KR".into()]).unwrap(),
            only(&["JP", "KR"])
        );
        assert_eq!(
            Territories::from_codes(&["WW".into()]).unwrap(),
            Territories::Worldwide
        );
        assert!(Territories::from_codes(&["Korea".into()]).is_err());
    }

    #[test]
    fn worldwide_is_skipped_when_serialized_in_structs() {
        // The tag form keeps any non-default value self-describing.
        assert_eq!(
            serde_json::to_value(Territories::Only(vec!["KR".into()])).unwrap(),
            json!({"mode":"only","codes":["KR"]})
        );
    }
}
