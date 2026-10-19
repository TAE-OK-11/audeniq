// cargo bench --locked -p audeniq-core --bench backend_resources
// The reference below preserves the pre-optimization package validation and
// hashing algorithms from b3bcdfe. Both versions use the same dependencies.
use audeniq_core::domain;
use serde::Serialize;
use serde_json::{Value, json};
use std::hint::black_box;
mod support;
use support::{allocations, compare};

mod reference {
    use super::*;
    struct HashWriter(sha2::Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            sha2::Digest::update(&mut self.0, bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    pub fn hash<T: Serialize + ?Sized>(value: &T) -> String {
        use sha2::Digest;
        let mut writer = HashWriter(sha2::Sha256::new());
        serde_json::to_writer(&mut writer, value).unwrap();
        audeniq_core::hex::encode(writer.0.finalize())
    }
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
                uuid::Uuid::parse_str(s).is_ok()
            }
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }
    pub fn valid(name: &str, body: &Value) -> bool {
        let Some(schema) = domain::contract_schema(name) else {
            return false;
        };
        let validator = jsonschema::validator_for(&schema).unwrap();
        if !validator.is_valid(body) || !check(body, "") {
            return false;
        }
        let field = match name {
            "ConsentPackageV1" => Some("consent_package_hash"),
            "VerificationPackageV1" => Some("verification_package_hash"),
            _ => None,
        };
        if let Some(field) = field {
            let mut copy = body.clone();
            let claimed = copy.as_object_mut().unwrap().remove(field).unwrap();
            if claimed != hash(&copy) {
                return false;
            }
        }
        true
    }
}

fn consent(n: usize) -> Value {
    let id = "00000000-0000-4000-8000-000000000001";
    let mut body = json!({
        "schema_version": 1, "party_id": id, "acting_org_id": id, "document_revision": id,
        "consent_policy_version": "legal-v1", "lawful_signer_ids": vec![id; n],
        "signer_verification_refs": vec![id; n], "bound_application_revision": id
    });
    body["consent_package_hash"] = json!(reference::hash(&body));
    body
}
fn verification(n: usize) -> Value {
    let id = "00000000-0000-4000-8000-000000000001";
    let mut body = json!({
        "schema_version": 1, "validation_hash": "a".repeat(64), "rights_epoch": 1,
        "approved_scope": ["WORLDWIDE"], "approved_dsp_ids": vec![id; n], "blocked_scope": [],
        "evidence_ids": vec![id; n], "split_plan_snapshot_id": id, "catalog_match_refs": vec![id; n],
        "check_matrix_refs": vec![id; n], "policy_versions": ["v1"]
    });
    body["verification_package_hash"] = json!(reference::hash(&body));
    body
}

fn main() {
    // Initialize jsonschema's process-wide support before isolating the
    // bounded cost of our four retained embedded validators.
    assert!(reference::valid("ConsentPackageV1", &consent(1)));
    let cache = allocations(|| {
        for name in [
            "ConsentPackageV1",
            "ValidationPackageV1",
            "VerificationPackageV1",
            "PreparationPackageV1",
        ] {
            assert!(domain::validate_package(name, &Value::Null).is_err());
        }
    });
    println!(
        "four_schema_cache: allocations={} peak_payload_bytes={} retained_payload_bytes={}",
        cache.0, cache.1, cache.2
    );
    for (label, name, body, n) in [
        ("consent/1", "ConsentPackageV1", consent(1), 500),
        ("consent/1000", "ConsentPackageV1", consent(1000), 100),
        (
            "verification/16",
            "VerificationPackageV1",
            verification(16),
            500,
        ),
        (
            "verification/2048",
            "VerificationPackageV1",
            verification(2048),
            30,
        ),
    ] {
        assert!(reference::valid(name, &body));
        domain::validate_package(name, &body).unwrap();
        assert_eq!(reference::hash(&body), domain::sha256_json(&body));
        compare(
            label,
            n,
            || assert!(reference::valid(black_box(name), black_box(&body))),
            || domain::validate_package(black_box(name), black_box(&body)).unwrap(),
        );
    }
    for n in [1, 2000] {
        let body = json!({"assets": (0..n).map(|i| json!({
            "index": i, "title": "음원 제목\\\"\n", "sha256": "b".repeat(64), "duration": 210.5
        })).collect::<Vec<_>>()});
        assert_eq!(reference::hash(&body), domain::sha256_json(&body));
        compare(
            &format!("hash/{n}-assets"),
            if n == 1 { 10000 } else { 100 },
            || {
                black_box(reference::hash(black_box(&body)));
            },
            || {
                black_box(domain::sha256_json(black_box(&body)));
            },
        );
    }
    println!(
        "Peak payload excludes fixture, warm cache, stack, allocator overhead and other processes; it is not RSS."
    );
}
