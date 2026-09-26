//! Per-DSP message presets for DDEX ERN generation.
//!
//! Structural reference: daddykev/ddex-suite (MIT)
//! `packages/ddex-builder/src/presets/` (declarative partner presets:
//! profile, required fields, validation rules, defaults) and
//! AbdelrahmanFouad/ddex-delivery-toolkit `delivery.py` PROFILES
//! (per-platform message shaping: message id strategy, deal mapping,
//! platform-specific required fields). Written from scratch in Rust; no
//! code is copied. Only the *idea* — separate per-DSP message shaping
//! from the global builder — is mirrored.
//!
//! A preset answers three questions the global builder cannot:
//! 1. How is the `MessageId` formed for this DSP?
//!    (`message_id_template`)
//! 2. When does the deal start relative to the release date?
//!    (`deal_start_offset_days`)
//! 3. Which preflight warnings are deal-breakers for this DSP?
//!    (`escalate_to_error`: rule ids promoted from warning to error)
//!
//! Resolution: every partner gets the built-in `default` preset unless
//! `execution.adapter_profiles.capabilities` (JSONB) contains a
//! `ddex_preset` object, whose fields override the defaults. The override
//! rejects unknown fields so a typo can never silently become a default.
//!
//! Real DPIDs and partner-specific values remain onboarding data (F6);
//! presets only shape the message, never invent party identifiers.

use crate::error::{Error, Result};
use chrono::NaiveDate;

/// Resolved per-DSP message preset used by the distribution pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DspMessagePreset {
    /// Stable preset id. `"default"` for the built-in preset.
    pub id: String,
    /// Template for `MessageId`. Variables: `{package}`, `{dsp}`,
    /// `{date}` (deal start as `YYYYMMDD`).
    pub message_id_template: String,
    /// Days added to the release date to obtain the deal start date.
    /// `0` = the deal starts on release day.
    pub deal_start_offset_days: i32,
    /// Preflight rule ids (e.g. `"DDEX-PREFLIGHT-RELEASE-TYPE"`) whose
    /// warnings become errors for this DSP.
    pub escalate_to_error: Vec<String>,
}

/// The built-in preset: current AUDENIQ behavior, unchanged.
pub fn default_preset() -> DspMessagePreset {
    DspMessagePreset {
        id: "default".to_string(),
        message_id_template: "AUDENIQ-ERN-{package}-{dsp}".to_string(),
        deal_start_offset_days: 0,
        escalate_to_error: Vec::new(),
    }
}

impl Default for DspMessagePreset {
    fn default() -> Self {
        default_preset()
    }
}

/// Partial preset override as stored in
/// `execution.adapter_profiles.capabilities -> 'ddex_preset'`.
/// Every field is optional; unknown fields are rejected.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PresetOverride {
    #[serde(default)]
    message_id_template: Option<String>,
    #[serde(default)]
    deal_start_offset_days: Option<i32>,
    #[serde(default)]
    escalate_to_error: Option<Vec<String>>,
}

impl DspMessagePreset {
    /// Resolve the preset for a partner. `capabilities` is the raw
    /// `adapter_profiles.capabilities` JSONB value. Missing or null
    /// `ddex_preset` yields the default preset; a present-but-malformed
    /// object is an operator config error (`Err(Error::Invalid)`), never
    /// a silent fallback.
    pub fn resolve(partner_id: &str, capabilities: &serde_json::Value) -> Result<Self> {
        let mut preset = default_preset();
        let Some(raw) = capabilities.get("ddex_preset") else {
            return Ok(preset);
        };
        if raw.is_null() {
            return Ok(preset);
        }
        let override_: PresetOverride = serde_json::from_value(raw.clone()).map_err(|e| {
            tracing::warn!(
                partner_id,
                error = %e,
                "adapter_profiles.capabilities.ddex_preset is malformed"
            );
            Error::Invalid
        })?;
        if let Some(t) = override_.message_id_template {
            // Validate eagerly so a broken template fails at resolve
            // time, not when the first message is rendered.
            preset.message_id_template = t;
            preset.render_message_id("package", "dsp", "20260101")?;
        }
        if let Some(d) = override_.deal_start_offset_days {
            preset.deal_start_offset_days = d;
        }
        if let Some(e) = override_.escalate_to_error {
            preset.escalate_to_error = e;
        }
        preset.id = format!("partner:{partner_id}");
        Ok(preset)
    }

    /// Render `message_id_template` for one message. Unknown `{...}`
    /// variables fail closed: a typo must not silently ship.
    pub fn render_message_id(
        &self,
        package_id: &str,
        dsp_id: &str,
        date_yyyymmdd: &str,
    ) -> Result<String> {
        let mut out = String::with_capacity(self.message_id_template.len() + 32);
        let mut rest = self.message_id_template.as_str();
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let Some(end) = after.find('}') else {
                return Err(Error::Invalid);
            };
            match &after[..end] {
                "package" => out.push_str(package_id),
                "dsp" => out.push_str(dsp_id),
                "date" => out.push_str(date_yyyymmdd),
                unknown => {
                    tracing::warn!(
                        template = %self.message_id_template,
                        variable = unknown,
                        "unknown message_id_template variable"
                    );
                    return Err(Error::Invalid);
                }
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        if out.trim().is_empty() {
            return Err(Error::Invalid);
        }
        Ok(out)
    }

    /// Deal start date for a release under this preset.
    pub fn deal_start_date(&self, release_date: NaiveDate) -> Result<NaiveDate> {
        release_date
            .checked_add_signed(chrono::TimeDelta::days(i64::from(
                self.deal_start_offset_days,
            )))
            .ok_or(Error::Invalid)
    }

    /// Whether this DSP treats the rule id as a deal-breaker. Applied by
    /// `ddex_validate::escalated_warnings`: an escalated warning skips
    /// just this DSP's message, it does not fail the whole batch.
    pub fn escalates(&self, rule_id: &str) -> bool {
        self.escalate_to_error.iter().any(|r| r == rule_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_preset_renders_legacy_message_id() {
        let p = default_preset();
        assert_eq!(
            p.render_message_id("pkg-1", "dsp-2", "20270101").unwrap(),
            "AUDENIQ-ERN-pkg-1-dsp-2"
        );
        assert_eq!(
            p.deal_start_date(NaiveDate::from_ymd_opt(2027, 3, 1).unwrap())
                .unwrap(),
            NaiveDate::from_ymd_opt(2027, 3, 1).unwrap()
        );
    }

    #[test]
    fn resolve_without_override_yields_default() {
        let p = DspMessagePreset::resolve("mockdsp", &json!({})).unwrap();
        assert_eq!(p, default_preset());
        let p = DspMessagePreset::resolve("mockdsp", &json!({"ddex_preset": null})).unwrap();
        assert_eq!(p, default_preset());
    }

    #[test]
    fn resolve_applies_partial_override() {
        let caps = json!({
            "ddex_preset": {
                "message_id_template": "DSP-{dsp}-{date}-{package}",
                "deal_start_offset_days": 7,
                "escalate_to_error": ["DDEX-PREFLIGHT-RELEASE-TYPE"],
            }
        });
        let p = DspMessagePreset::resolve("partner-x", &caps).unwrap();
        assert_eq!(p.id, "partner:partner-x");
        assert_eq!(
            p.render_message_id("pkg", "dsp", "20270301").unwrap(),
            "DSP-dsp-20270301-pkg"
        );
        assert_eq!(
            p.deal_start_date(NaiveDate::from_ymd_opt(2027, 3, 1).unwrap())
                .unwrap(),
            NaiveDate::from_ymd_opt(2027, 3, 8).unwrap()
        );
        assert!(p.escalates("DDEX-PREFLIGHT-RELEASE-TYPE"));
        assert!(!p.escalates("DDEX-PREFLIGHT-DATE-CHRONOLOGY"));
    }

    #[test]
    fn resolve_rejects_unknown_override_fields() {
        let caps = json!({"ddex_preset": {"message_id_templat": "x"}});
        assert!(DspMessagePreset::resolve("p", &caps).is_err());
    }

    #[test]
    fn resolve_rejects_bad_template_eagerly() {
        let caps = json!({"ddex_preset": {"message_id_template": "X-{bogus}"}});
        assert!(DspMessagePreset::resolve("p", &caps).is_err());
        let caps = json!({"ddex_preset": {"message_id_template": "   "}});
        assert!(DspMessagePreset::resolve("p", &caps).is_err());
    }

    #[test]
    fn render_rejects_unknown_variable() {
        let mut p = default_preset();
        p.message_id_template = "X-{package}-{typo}".to_string();
        assert!(p.render_message_id("a", "b", "c").is_err());
    }
}
