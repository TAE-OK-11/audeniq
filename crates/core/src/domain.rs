use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
/// SHA-256 of deterministically key-sorted JSON. Versioned internal canonical format, not an assertion of JCS.
pub fn digest(value: &Value) -> String {
    sha256_json(value)
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
    hex::encode(w.0.finalize())
}

#[cfg(test)]
mod hash_tests {
    #[test]
    fn streamed_hash_equals_buffered_hash() {
        use sha2::{Digest, Sha256};
        let v = serde_json::json!({"b": [1, 2, {"c": "한글"}], "a": null, "z": 1.5});
        let buffered = hex::encode(Sha256::digest(serde_json::to_string(&v).unwrap()));
        assert_eq!(super::sha256_json(&v), buffered);
    }
}
pub fn validate_package(name: &str, body: &Value) -> Result<()> {
    let schema = contract_schema(name).ok_or(Error::Invalid)?;
    let validator = jsonschema::validator_for(&schema).map_err(|_| Error::Internal)?;
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
        let mut copy = body.clone();
        let claimed = copy
            .as_object_mut()
            .unwrap()
            .remove(field)
            .ok_or(Error::Invalid)?;
        if claimed != digest(&copy) {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
