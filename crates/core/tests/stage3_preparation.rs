//! Synthetic partner-neutral fixtures only. No DSP service/credentials.
use async_trait::async_trait;
use audeniq_core::{
    domain::{FreshnessPin, digest},
    ern::{generate_ern, validate_xml},
    error::{Error, Result},
    preparation_model::{CanonicalRelease, VerificationPackage},
    preflight::{CheckStatus, CurrentFacts, preflight},
    route_plan::plan_submissions,
    storage::{ObjectMeta, ObjectStore, UploadGrant},
};
use chrono::{DateTime, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

fn fixture(index: usize) -> (CanonicalRelease, VerificationPackage, MemoryStore) {
    let raw = [include_str!("fixtures/ern/single.json"), include_str!("fixtures/ern/ep.json"), include_str!("fixtures/ern/album.json")][index];
    let mut c: CanonicalRelease = serde_json::from_str(raw).unwrap();
    let body = json!({
        "schema_version":1,"revision_id":c.revision_id,"revision_hash":c.revision_hash,
        "decision":"PASS","rights_epoch":c.rights_epoch,
        "approved_scope":{"dsp_ids":c.approved_scope.iter().map(|s|s.dsp_id).collect::<Vec<_>>()},
        "rule_version":"1"
    });
    c.verification_package_hash = digest(&body);
    let v = VerificationPackage { id:c.verification_package_id,org_id:c.org_id,revision_id:c.revision_id,
        rights_epoch:c.rights_epoch,package_hash:c.verification_package_hash.clone(),body };
    let mut store = MemoryStore::default();
    store.files.insert(c.artwork.object_key.clone(), (b"synthetic-artwork".to_vec(), c.artwork.content_type.clone()));
    for (i,t) in c.tracks.iter().enumerate() {
        store.files.insert(t.audio.object_key.clone(), (format!("synthetic-audio-{i}").into_bytes(),t.audio.content_type.clone()));
    }
    (c,v,store)
}

fn facts(c: &CanonicalRelease, xml: &str) -> (FreshnessPin, CurrentFacts) {
    let pin = FreshnessPin { revision_id:c.revision_id,verification_hash:c.verification_package_hash.clone(),
        snapshot_id:c.snapshot_id,rights_epoch:c.rights_epoch as u64,route_contract_id:Uuid::from_u128(99),
        package_hash:hex::encode(Sha256::digest(xml.as_bytes())) };
    let current = CurrentFacts {pin:pin.clone(),hold:false,contract_active:true};
    (pin,current)
}

#[derive(Default)]
struct MemoryStore { files: BTreeMap<String,(Vec<u8>,String)>, unavailable:bool }

#[async_trait]
impl ObjectStore for MemoryStore {
    async fn presign_put(&self,_: &str,_: i64,_: &str,_: &str,_: DateTime<Utc>) -> Result<UploadGrant> { Err(Error::Storage) }
    async fn freeze(&self,_: &str,_: &str,_: &str) -> Result<()> { Err(Error::Storage) }
    async fn head(&self,key: &str) -> Result<Option<ObjectMeta>> {
        if self.unavailable {return Err(Error::Storage);}
        Ok(self.files.get(key).map(|(b,m)|ObjectMeta {size:b.len() as i64,content_type:m.clone(),nonce:String::new(),etag:String::new()}))
    }
    async fn get(&self,key: &str) -> Result<Vec<u8>> {
        if self.unavailable {return Err(Error::Storage);}
        self.files.get(key).map(|(b,_)|b.clone()).ok_or(Error::Storage)
    }
}

#[tokio::test]
async fn three_partner_neutral_fixtures_pass_all_four_checks() {
    for i in 0..3 {
        let (c,v,store) = fixture(i);
        let xml = generate_ern(&c).unwrap();
        assert_eq!(xml,generate_ern(&c).unwrap());
        assert!(xml.contains("deliveryEnabled=\"false\""));
        let (pin,current) = facts(&c,&xml);
        let report = preflight(&c,&v,&xml,&pin,&current,&store).await;
        assert!(report.passed(),"{report:?}");
        let plan = plan_submissions(&c,&v).unwrap();
        assert_eq!(plan.len(),c.approved_scope.len());
        for p in plan {assert!(!p.delivery_enabled); assert_eq!(p.audio_asset_ids.len(),c.tracks.len());}
        if let Ok(dir) = std::env::var("F4_FIXTURE_XML_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("fixture-{i}.xml")),xml).unwrap();
        }
    }
}

#[test]
fn xml_escaping_and_deterministic_order() {
    let (mut c,_,_) = fixture(2);
    let xml = generate_ern(&c).unwrap();
    assert!(xml.contains("&amp;")); assert!(xml.contains("&lt;"));
    assert!(xml.contains("&quot;")); assert!(xml.contains("&apos;"));
    c.tracks.reverse(); c.approved_scope.reverse();
    assert_eq!(xml,generate_ern(&c).unwrap());
    c.title.push('\0'); assert!(generate_ern(&c).is_err());
}

#[tokio::test]
async fn xml_and_metadata_fail_closed() {
    let (mut c,v,store) = fixture(0);
    let xml = generate_ern(&c).unwrap();
    let (pin,current) = facts(&c,&xml);
    for bad in [xml.replace("</Release>",""),xml.replace("<UPC>","<Unknown>"),xml.replace("<Title>","<Title>altered"),format!("<!DOCTYPE x [<!ENTITY x SYSTEM 'file:///etc/passwd'>]>{xml}")] {
        assert!(validate_xml(&c,&bad).is_err());
        assert_eq!(preflight(&c,&v,&bad,&pin,&current,&store).await.xml,CheckStatus::Fail);
    }
    c.title = "   ".into();
    let report = preflight(&c,&v,&xml,&pin,&current,&store).await;
    assert_eq!(report.metadata,CheckStatus::Fail); assert!(!report.passed());
}

#[tokio::test]
async fn missing_corrupt_and_unavailable_files_never_pass() {
    let (c,v,mut store) = fixture(0);
    let xml = generate_ern(&c).unwrap(); let (pin,current) = facts(&c,&xml);
    store.files.remove(&c.artwork.object_key);
    assert_eq!(preflight(&c,&v,&xml,&pin,&current,&store).await.files,CheckStatus::Fail);
    let (_,_,mut store) = fixture(0);
    store.files.get_mut(&c.tracks[0].audio.object_key).unwrap().0[0] ^= 1;
    assert_eq!(preflight(&c,&v,&xml,&pin,&current,&store).await.files,CheckStatus::Fail);
    store.unavailable=true;
    let report=preflight(&c,&v,&xml,&pin,&current,&store).await;
    assert_eq!(report.files,CheckStatus::Unknown); assert!(!report.passed());
}

#[tokio::test]
async fn freshness_and_approval_mismatch_never_pass() {
    for mutation in 0..9 {
        let (mut c,mut v,store)=fixture(0); let xml=generate_ern(&c).unwrap();
        let (pin,mut current)=facts(&c,&xml);
        match mutation {
            0=>current.hold=true,
            1=>current.contract_active=false,
            2=>current.pin.rights_epoch+=1,
            3=>current.pin.revision_id=Uuid::new_v4(),
            4=>current.pin.package_hash="f".repeat(64),
            5=>c.approved_scope[0].dsp_id=Uuid::new_v4(),
            6=>v.body["decision"]=json!("REVIEW_REQUIRED"),
            7=>v.org_id=Uuid::new_v4(),
            _=>c.revision_hash="d".repeat(64),
        }
        assert_eq!(preflight(&c,&v,&xml,&pin,&current,&store).await.rights,CheckStatus::Fail,"mutation {mutation}");
    }
}

#[test]
fn empty_approval_creates_no_routes_and_duplicate_approval_is_rejected() {
    let (mut c,mut v,_)=fixture(0);
    c.approved_scope.clear(); v.body["approved_scope"]["dsp_ids"]=json!([]);
    v.package_hash=digest(&v.body); c.verification_package_hash=v.package_hash.clone();
    assert!(plan_submissions(&c,&v).unwrap().is_empty());
    let (mut c,mut v,_)=fixture(0);
    v.body["approved_scope"]["dsp_ids"]=json!([c.approved_scope[0].dsp_id,c.approved_scope[0].dsp_id]);
    v.package_hash=digest(&v.body); c.verification_package_hash=v.package_hash.clone();
    assert!(plan_submissions(&c,&v).is_err());
}

#[test]
fn duplicate_resource_track_and_invalid_identifier_are_rejected() {
    for mutation in 0..5 {
        let (mut c,_,_)=fixture(1);
        match mutation {
            0=>c.tracks[1].id=c.tracks[0].id,
            1=>c.tracks[1].audio=c.tracks[0].audio.clone(),
            2=>c.tracks[1].track_number=c.tracks[0].track_number,
            3=>c.tracks[1].isrc="invalid".into(),
            _=>c.upc="012345678904".into(),
        }
        assert!(generate_ern(&c).is_err());
    }
}
