//! Strict, versioned handoff DTOs; no opaque JSON payload that could carry contract originals.
use crate::{
    domain,
    error::{Error, Result},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct SchemaV1;
impl TryFrom<u8> for SchemaV1 {
    type Error = &'static str;
    fn try_from(v: u8) -> std::result::Result<Self, Self::Error> {
        if v == 1 {
            Ok(Self)
        } else {
            Err("unsupported schema version")
        }
    }
}
impl From<SchemaV1> for u8 {
    fn from(_: SchemaV1) -> u8 {
        1
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentPackageV1 {
    pub schema_version: SchemaV1,
    pub party_id: Uuid,
    pub acting_org_id: Uuid,
    pub document_revision: Uuid,
    pub consent_policy_version: String,
    pub lawful_signer_ids: Vec<Uuid>,
    pub signer_verification_refs: Vec<Uuid>,
    pub consent_package_hash: String,
    pub bound_application_revision: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedAsset {
    pub asset_id: Uuid,
    pub sha256: String,
    pub metric_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationPackageV1 {
    pub schema_version: SchemaV1,
    pub revision_id: Uuid,
    pub revision_hash: String,
    pub validated_assets: Vec<ValidatedAsset>,
    pub special_flags: Vec<String>,
    pub claimant_party_ids: Vec<Uuid>,
    pub consent_package_hash: String,
    pub stage1_check_refs: Vec<Uuid>,
    pub rule_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationPackageV1 {
    pub schema_version: SchemaV1,
    pub validation_hash: String,
    pub rights_epoch: u64,
    pub approved_scope: Vec<String>,
    pub approved_dsp_ids: Vec<Uuid>,
    pub blocked_scope: Vec<String>,
    pub evidence_ids: Vec<Uuid>,
    pub split_plan_snapshot_id: Uuid,
    pub catalog_match_refs: Vec<Uuid>,
    pub check_matrix_refs: Vec<Uuid>,
    pub policy_versions: Vec<String>,
    pub verification_package_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DspPackageDigest {
    pub dsp_id: Uuid,
    pub package_digest: String,
    pub asset_manifest_ref: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationPackageV1 {
    pub schema_version: SchemaV1,
    pub snapshot_id: Uuid,
    pub snapshot_hash: String,
    pub verification_package_hash: String,
    pub identifier_refs: Vec<Uuid>,
    pub route_id: Uuid,
    pub contract_id: Uuid,
    pub fee_schedule_id: Uuid,
    pub dsp_packages: Vec<DspPackageDigest>,
    pub submit_not_before: DateTime<Utc>,
    pub submit_deadline: DateTime<Utc>,
    pub consumer_release_at: DateTime<Utc>,
    pub preflight_ref: Uuid,
    pub queued_job_refs: Vec<Uuid>,
}
macro_rules! validate {($($t:ident),*)=>{$(impl $t{pub fn validate(&self)->Result<()>{let body=serde_json::to_value(self).map_err(|_|Error::Invalid)?;domain::validate_package(stringify!($t),&body)}})*}}
validate!(
    ConsentPackageV1,
    ValidationPackageV1,
    VerificationPackageV1,
    PreparationPackageV1
);
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn consent() -> serde_json::Value {
        let mut body = json!({"schema_version":1,"party_id":Uuid::new_v4(),"acting_org_id":Uuid::new_v4(),"document_revision":Uuid::new_v4(),"consent_policy_version":"legal-review-pending","lawful_signer_ids":[Uuid::new_v4()],"signer_verification_refs":[Uuid::new_v4()],"bound_application_revision":Uuid::new_v4()});
        let hash = domain::digest(&body);
        body["consent_package_hash"] = json!(hash);
        body
    }
    #[test]
    fn valid_consent_contract_and_integrity() {
        let v = consent();
        let dto: ConsentPackageV1 = serde_json::from_value(v.clone()).unwrap();
        dto.validate().unwrap();
        let mut changed = v;
        changed["acting_org_id"] = json!(Uuid::new_v4());
        assert!(domain::validate_package("ConsentPackageV1", &changed).is_err());
    }
    #[test]
    fn pii_unknown_fields_versions_and_missing_fields_rejected() {
        for field in [
            "passport",
            "contract_original",
            "r2_secret",
            "guardian_family_document",
        ] {
            let mut v = consent();
            v[field] = json!("never-forward");
            assert!(serde_json::from_value::<ConsentPackageV1>(v.clone()).is_err());
            assert!(domain::validate_package("ConsentPackageV1", &v).is_err());
        }
        let mut v = consent();
        v["schema_version"] = json!(2);
        assert!(serde_json::from_value::<ConsentPackageV1>(v).is_err());
        let mut v = consent();
        v.as_object_mut().unwrap().remove("lawful_signer_ids");
        assert!(serde_json::from_value::<ConsentPackageV1>(v).is_err());
    }
    #[test]
    fn validation_package_no_implicit_rights_or_identifier_issue() {
        let v = json!({"schema_version":1,"revision_id":Uuid::new_v4(),"revision_hash":"a".repeat(64),"validated_assets":[{"asset_id":Uuid::new_v4(),"sha256":"b".repeat(64),"metric_hash":"c".repeat(64)}],"special_flags":[],"claimant_party_ids":[Uuid::new_v4()],"consent_package_hash":"d".repeat(64),"stage1_check_refs":[Uuid::new_v4()],"rule_version":"v1"});
        let d: ValidationPackageV1 = serde_json::from_value(v.clone()).unwrap();
        d.validate().unwrap();
        for key in ["rights_approved", "issued_isrc", "ddex", "ocr_text"] {
            let mut wrong = v.clone();
            wrong[key] = json!(true);
            assert!(serde_json::from_value::<ValidationPackageV1>(wrong.clone()).is_err());
            assert!(domain::validate_package("ValidationPackageV1", &wrong).is_err());
        }
    }
    #[test]
    fn generated_states_are_exact_and_axes_separate() {
        use crate::states::*;
        assert!(
            ApplicationPipelineStatus::DRAFT
                .transition(ApplicationPipelineStatus::READY_FOR_DELIVERY)
                .is_err()
        );
        assert!(
            DeliveryJobStatus::SENT_UNKNOWN
                .transition(DeliveryJobStatus::QUEUED)
                .is_err()
        );
        assert!(
            DspLiveStatus::IN_REVIEW
                .transition(DspLiveStatus::LIVE)
                .is_ok()
        );
        assert!(serde_json::from_str::<ApplicationPipelineStatus>("\"ACCEPTED\"").is_err());
    }
}
