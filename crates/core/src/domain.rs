use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;
use uuid::Uuid;
pub fn state_contract() -> &'static Value {
    static CONTRACT: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CONTRACT.get_or_init(|| {
        serde_json::from_str(include_str!("../../../config/states.json"))
            .expect("embedded contract")
    })
}
pub fn transition(axis: &str, old: &str, next: &str) -> Result<()> {
    let c = state_contract();
    let values = c["axes"][axis].as_array().ok_or(Error::Invalid)?;
    let belongs = |s: &str| values.iter().any(|v| v == s);
    if belongs(old)
        && belongs(next)
        && c["transitions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v[0] == old && v[1] == next)
    {
        Ok(())
    } else {
        Err(Error::Conflict)
    }
}

#[cfg(test)]
mod drift_tests {
    #[test]
    fn database_edges_match_rust_contract() {
        let contract = super::state_contract();
        // Edges are added by later forward-only migrations too; the DB set is
        // their union and must equal the Rust contract exactly.
        let ddl = [
            include_str!("../../../migrations/0002_state_contract.sql"),
            include_str!("../../../migrations/0032_recoverable_corrections.sql"),
            include_str!("../../../migrations/0044_staff_portal.sql"),
            include_str!("../../../migrations/0046_identifier_reissue.sql"),
            include_str!("../../../migrations/0049_release_review_queue.sql"),
            include_str!("../../../migrations/0050_release_withdraw.sql"),
        ]
        .concat();
        let edges = contract["transitions"].as_array().unwrap();
        assert_eq!(
            ddl.matches("INSERT INTO operations.allowed_transitions")
                .count(),
            edges.len()
        );
        for pair in edges {
            let old = pair[0].as_str().unwrap();
            let next = pair[1].as_str().unwrap();
            let (axis, _) = contract["axes"]
                .as_object()
                .unwrap()
                .iter()
                .find(|(_, values)| values.as_array().unwrap().iter().any(|v| v == old))
                .unwrap();
            let expected = format!("VALUES ('{axis}','{old}','{next}')");
            assert!(ddl.contains(&expected), "missing DB edge: {expected}");
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReturnTo {
    PreSubmit,
    S1,
    S2,
    S3Prep,
    Exec,
    Post,
    Finance,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FreshnessPin {
    pub revision_id: Uuid,
    pub verification_hash: String,
    pub snapshot_id: Uuid,
    pub rights_epoch: u64,
    pub route_contract_id: Uuid,
    pub package_hash: String,
}
pub fn freshness_guard(
    pin: &FreshnessPin,
    current: &FreshnessPin,
    hold: bool,
    contract_active: bool,
) -> std::result::Result<(), ReturnTo> {
    if hold
        || !contract_active
        || pin.rights_epoch != current.rights_epoch
        || pin.route_contract_id != current.route_contract_id
    {
        return Err(ReturnTo::S2);
    }
    if pin.revision_id != current.revision_id {
        return Err(ReturnTo::PreSubmit);
    }
    if pin.verification_hash != current.verification_hash {
        return Err(ReturnTo::S2);
    }
    if pin.snapshot_id != current.snapshot_id || pin.package_hash != current.package_hash {
        return Err(ReturnTo::S3Prep);
    }
    Ok(())
}
pub fn contract_schema(name: &str) -> Option<Value> {
    let raw = match name {
        "ConsentPackageV1" => include_str!("../../../config/contracts/ConsentPackageV1.json"),
        "ValidationPackageV1" => include_str!("../../../config/contracts/ValidationPackageV1.json"),
        "VerificationPackageV1" => {
            include_str!("../../../config/contracts/VerificationPackageV1.json")
        }
        "PreparationPackageV1" => {
            include_str!("../../../config/contracts/PreparationPackageV1.json")
        }
        _ => return None,
    };
    serde_json::from_str(raw).ok()
}

fn contract_validator(name: &str) -> Result<&'static jsonschema::Validator> {
    // Only four immutable, embedded schemas can enter this cache. Package
    // bodies and validation results are always checked afresh.
    static CONSENT: OnceLock<Option<jsonschema::Validator>> = OnceLock::new();
    static VALIDATION: OnceLock<Option<jsonschema::Validator>> = OnceLock::new();
    static VERIFICATION: OnceLock<Option<jsonschema::Validator>> = OnceLock::new();
    static PREPARATION: OnceLock<Option<jsonschema::Validator>> = OnceLock::new();
    let slot = match name {
        "ConsentPackageV1" => &CONSENT,
        "ValidationPackageV1" => &VALIDATION,
        "VerificationPackageV1" => &VERIFICATION,
        "PreparationPackageV1" => &PREPARATION,
        _ => return Err(Error::Invalid),
    };
    slot.get_or_init(|| jsonschema::validator_for(&contract_schema(name)?).ok())
        .as_ref()
        .ok_or(Error::Internal)
}
/// SHA-256 of deterministically key-sorted JSON. Versioned internal canonical format, not an assertion of JCS.
pub fn digest(value: &Value) -> String {
    sha256_json(value)
}

/// Hex SHA-256 of raw bytes.
pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    use sha2::Digest;
    crate::hex::encode(sha2::Sha256::digest(bytes))
}

/// `io::Write` sink that feeds bytes straight into SHA-256.
struct HashWriter(sha2::Sha256);
impl std::io::Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        sha2::Digest::update(&mut self.0, buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Hex SHA-256 of `serde_json::to_string(value)` without building the
/// string: the serializer streams into the hasher (packages and snapshots
/// are hashed on every freeze and every delivery attempt).
pub fn sha256_json<T: Serialize + ?Sized>(value: &T) -> String {
    use sha2::Digest;
    let mut w = HashWriter(sha2::Sha256::new());
    serde_json::to_writer(&mut w, value).expect("json serializes");
    crate::hex::encode(w.0.finalize())
}

/// Serialize a borrowed object, omitting only its top-level self-hash.
/// Iteration and nested values match cloning the object and removing the key.
struct WithoutField<'a> {
    object: &'a serde_json::Map<String, Value>,
    field: &'a str,
}
impl Serialize for WithoutField<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let len = self.object.len() - usize::from(self.object.contains_key(self.field));
        let mut map = serializer.serialize_map(Some(len))?;
        for (key, value) in self.object {
            if key != self.field {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

#[cfg(test)]
mod hash_tests {
    use super::*;

    #[test]
    fn streamed_hash_equals_buffered_hash() {
        use sha2::{Digest, Sha256};
        let v = serde_json::json!({"b": [1, 2, {"c": "한글"}], "a": null, "z": 1.5});
        let buffered = crate::hex::encode(Sha256::digest(serde_json::to_string(&v).unwrap()));
        assert_eq!(super::sha256_json(&v), buffered);
    }

    #[test]
    fn borrowed_hash_matches_cloned_json_at_boundaries_and_with_escaped_strings() {
        for n in [0, 1, 63, 64, 4095, 4096, 4097, 16384] {
            let value = serde_json::json!({
                "verification_package_hash": "b".repeat(64),
                "array": (0..n).map(|i| i % 17).collect::<Vec<_>>(),
                "text": format!("한글\\\"\n\t{}", "x".repeat(n)),
                "nested": [{"verification_package_hash": "nested", "bool": true, "null": null, "float": -1.25}]
            });
            let mut expected = value.clone();
            expected
                .as_object_mut()
                .unwrap()
                .remove("verification_package_hash");
            let view = WithoutField {
                object: value.as_object().unwrap(),
                field: "verification_package_hash",
            };
            assert_eq!(
                sha256_json(&view),
                sha256_hex(serde_json::to_vec(&expected).unwrap())
            );
        }
    }

    #[test]
    fn borrowed_hash_omits_only_top_level_field_and_preserves_body() {
        let body = serde_json::json!({
            "consent_package_hash": "claimed",
            "nested": {"consent_package_hash": "nested", "text": "한글"},
            "array": [1, false, null]
        });
        let mut expected = body.clone();
        expected
            .as_object_mut()
            .unwrap()
            .remove("consent_package_hash");
        for field in ["consent_package_hash", "missing"] {
            let view = WithoutField {
                object: body.as_object().unwrap(),
                field,
            };
            let expected = if field == "missing" { &body } else { &expected };
            assert_eq!(sha256_json(&view), digest(expected));
            assert_eq!(serde_json::to_value(&view).unwrap(), *expected);
        }
        assert_eq!(body["consent_package_hash"], "claimed");
        assert_eq!(body["nested"]["consent_package_hash"], "nested");
    }
}
pub fn validate_package(name: &str, body: &Value) -> Result<()> {
    let validator = contract_validator(name)?;
    if !validator.is_valid(body) {
        return Err(Error::Invalid);
    }
    // The report's illustrative id/string schemas are tightened here, without renaming fields.
    fn check(v: &Value, key: &str) -> bool {
        match v {
            Value::Object(m) => m.iter().all(|(k, v)| check(v, k)),
            Value::Array(a) => a.iter().all(|v| check(v, key)),
            Value::String(s)
                if key.ends_with("_hash") || key == "sha256" || key == "package_digest" =>
            {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }
            Value::String(s)
                if key.ends_with("_id")
                    || key.ends_with("_ids")
                    || matches!(key, "bound_application_revision" | "snapshot_id") =>
            {
                Uuid::parse_str(s).is_ok()
            }
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }
    if !check(body, "") {
        return Err(Error::Invalid);
    }
    let hash_field = match name {
        "ConsentPackageV1" => Some("consent_package_hash"),
        "VerificationPackageV1" => Some("verification_package_hash"),
        _ => None,
    };
    if let Some(field) = hash_field {
        let object = body.as_object().ok_or(Error::Invalid)?;
        let claimed = object.get(field).ok_or(Error::Invalid)?;
        if claimed != &sha256_json(&WithoutField { object, field }) {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_schemas_are_reused_across_threads_and_unknown_names_rejected() {
        for name in [
            "ConsentPackageV1",
            "ValidationPackageV1",
            "VerificationPackageV1",
            "PreparationPackageV1",
        ] {
            let expected = contract_validator(name).unwrap() as *const _ as usize;
            let tasks: Vec<_> = (0..8)
                .map(|_| {
                    std::thread::spawn(move || {
                        contract_validator(name).unwrap() as *const _ as usize
                    })
                })
                .collect();
            for task in tasks {
                assert_eq!(task.join().unwrap(), expected);
            }
        }
        assert!(matches!(
            validate_package("unknown", &Value::Null),
            Err(Error::Invalid)
        ));
    }

    #[test]
    fn cached_verification_rechecks_content_ids_and_hashes() {
        let id = "00000000-0000-4000-8000-000000000001";
        let mut body = serde_json::json!({
            "schema_version": 1, "validation_hash": "a".repeat(64), "rights_epoch": 1,
            "approved_scope": ["WORLDWIDE"], "approved_dsp_ids": [id], "blocked_scope": [],
            "evidence_ids": [id], "split_plan_snapshot_id": id, "catalog_match_refs": [id],
            "check_matrix_refs": [id], "policy_versions": ["v1"]
        });
        body["verification_package_hash"] = Value::String(digest(&body));
        validate_package("VerificationPackageV1", &body).unwrap();
        validate_package("VerificationPackageV1", &body).unwrap();
        let original = body.clone();
        for (field, value) in [
            ("rights_epoch", serde_json::json!(2)),
            ("approved_dsp_ids", serde_json::json!(["bad-id"])),
            ("validation_hash", serde_json::json!("A".repeat(64))),
            (
                "verification_package_hash",
                serde_json::json!("f".repeat(64)),
            ),
            ("passport", serde_json::json!("secret")),
        ] {
            let mut tampered = original.clone();
            tampered[field] = value;
            assert!(
                matches!(
                    validate_package("VerificationPackageV1", &tampered),
                    Err(Error::Invalid)
                ),
                "{field}"
            );
        }
        assert_eq!(body, original);
        body.as_object_mut()
            .unwrap()
            .remove("verification_package_hash");
        assert!(matches!(
            validate_package("VerificationPackageV1", &body),
            Err(Error::Invalid)
        ));
    }

    #[test]
    fn cached_preparation_accepts_valid_package_and_rejects_invalid_fields() {
        let id = "00000000-0000-4000-8000-000000000001";
        let body = serde_json::json!({
            "schema_version": 1, "snapshot_id": id, "snapshot_hash": "a".repeat(64),
            "verification_package_hash": "b".repeat(64), "identifier_refs": [id],
            "route_id": id, "contract_id": id, "fee_schedule_id": id,
            "dsp_packages": [{"dsp_id": id, "package_digest": "c".repeat(64), "asset_manifest_ref": id}],
            "submit_not_before": "2026-10-01T00:00:00Z", "submit_deadline": "2026-10-02T00:00:00Z",
            "consumer_release_at": "2026-10-03T00:00:00Z", "preflight_ref": id, "queued_job_refs": [id]
        });
        validate_package("PreparationPackageV1", &body).unwrap();
        let dto: crate::packages::PreparationPackageV1 =
            serde_json::from_value(body.clone()).unwrap();
        dto.validate().unwrap();
        for (field, value) in [
            ("snapshot_id", serde_json::json!("invalid")),
            ("snapshot_hash", serde_json::json!("a".repeat(63))),
            ("dsp_packages", serde_json::json!([{"dsp_id": id}])),
            ("schema_version", serde_json::json!(2)),
        ] {
            let mut changed = body.clone();
            changed[field] = value;
            assert!(
                matches!(
                    validate_package("PreparationPackageV1", &changed),
                    Err(Error::Invalid)
                ),
                "{field}"
            );
        }
    }
    #[test]
    fn deny_implicit_progress() {
        assert!(transition("application_pipeline_status", "DRAFT", "READY_FOR_DELIVERY").is_err());
        assert!(transition("delivery_job_status", "SENT_UNKNOWN", "QUEUED").is_err());
        assert!(transition("dsp_live_status", "NOT_SUBMITTED", "LIVE").is_err());
        assert!(transition("application_pipeline_status", "SUBMITTED", "STAGE1_RUNNING").is_ok());
    }
    #[test]
    fn all_allowed_edges_match_axes() {
        let c = state_contract();
        for pair in c["transitions"].as_array().unwrap() {
            let a = pair[0].as_str().unwrap();
            let b = pair[1].as_str().unwrap();
            assert!(
                c["axes"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .any(|axis| transition(axis, a, b).is_ok())
            );
        }
    }
    #[test]
    fn versions_and_pii_rejected() {
        for name in [
            "ConsentPackageV1",
            "ValidationPackageV1",
            "VerificationPackageV1",
            "PreparationPackageV1",
        ] {
            assert!(
                validate_package(
                    name,
                    &serde_json::json!({"schema_version":2,"passport":"secret"})
                )
                .is_err()
            );
        }
    }
    #[test]
    fn stale_revision_and_epoch() {
        let a = FreshnessPin {
            revision_id: Uuid::new_v4(),
            verification_hash: "a".repeat(64),
            snapshot_id: Uuid::new_v4(),
            rights_epoch: 1,
            route_contract_id: Uuid::new_v4(),
            package_hash: "b".repeat(64),
        };
        let mut b = a.clone();
        b.rights_epoch = 2;
        assert_eq!(freshness_guard(&a, &b, false, true), Err(ReturnTo::S2));
        b = a.clone();
        b.revision_id = Uuid::new_v4();
        assert_eq!(
            freshness_guard(&a, &b, false, true),
            Err(ReturnTo::PreSubmit)
        );
    }
}

/// Today's calendar date in Korea (KST, UTC+9). Ages and street-date lead
/// times are Korean dates; the UTC date is a day behind until 09:00 KST.
pub fn today_kst() -> chrono::NaiveDate {
    let kst = chrono::FixedOffset::east_opt(9 * 3600).expect("valid offset");
    chrono::Utc::now().with_timezone(&kst).date_naive()
}

/// `"-1234567"` → `"-1,234,567"`: an integer's digits grouped by thousands.
pub fn group_digits(n: &str) -> String {
    let (sign, digits) = n.strip_prefix('-').map_or(("", n), |d| ("-", d));
    let mut out = String::with_capacity(n.len() + digits.len() / 3);
    out.push_str(sign);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod group_digits_tests {
    use super::group_digits;

    #[test]
    fn groups_by_thousands_and_keeps_the_sign_outside() {
        assert_eq!(group_digits("0"), "0");
        assert_eq!(group_digits("999"), "999");
        assert_eq!(group_digits("1000"), "1,000");
        assert_eq!(group_digits("1234567"), "1,234,567");
        assert_eq!(group_digits("-150000"), "-150,000");
    }
}
