//! Document seals: tamper evidence for electronic documents.
//!
//! When a release application is submitted, an agreement or rights document
//! is signed, a signing request is signed or declined, or terms are accepted,
//! the same transaction seals the document:
//!
//! - its content is rendered as canonical JSON by PostgreSQL (`jsonb` text
//!   is ordered and stable) from fixed fields of the record — the same
//!   function renders it again later for verification and for the archive;
//! - `content_sha256` of that JSON, the event and the time are signed with the
//!   server's Ed25519 document key (derived from the keyring; public keys are
//!   published at `/api/document-keys`), and
//! - each seal stores the hash of the previous one (one chain, serialized
//!   with an advisory lock), so editing or deleting a seal breaks the chain.
//!
//! `verify` re-renders every sealed document and checks content, signatures
//! and the chain; the chain head is anchored in external WORM storage
//! (`crate::archive`), so even a rewritten chain is detected.
use crate::{
    error::{Error, Result},
    payout_keys::KeyRing,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Application,
    Agreement,
    RightsDocument,
    SigningRequest,
    TermsAcceptance,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Application => "APPLICATION",
            Self::Agreement => "AGREEMENT",
            Self::RightsDocument => "RIGHTS_DOCUMENT",
            Self::SigningRequest => "SIGNING_REQUEST",
            Self::TermsAcceptance => "TERMS_ACCEPTANCE",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "APPLICATION" => Self::Application,
            "AGREEMENT" => Self::Agreement,
            "RIGHTS_DOCUMENT" => Self::RightsDocument,
            "SIGNING_REQUEST" => Self::SigningRequest,
            "TERMS_ACCEPTANCE" => Self::TermsAcceptance,
            _ => return None,
        })
    }
}

/// The canonical document: fixed fields only, so later columns (review
/// status, row versions) never change a sealed document's content.
fn render_sql(kind: Kind) -> &'static str {
    match kind {
        Kind::Application => {
            "SELECT a.org_id, (jsonb_build_object(
               'kind','APPLICATION','id',a.id,'org_id',a.org_id,'release_id',a.release_id,
               'application_no',a.application_no,'form',a.form,'content_hash',a.content_hash,
               'signer_name',a.signer_name,'signer_role',a.signer_role,'agreements',a.agreements,
               'signature',a.signature,'client_submitted_at',a.client_submitted_at,
               'submitted_by',a.submitted_by,'received_at',a.received_at,
               'revision',(SELECT jsonb_build_object('id',r.id,'revision',r.revision,'body',r.body,
                    'body_hash',r.body_hash,'consent_package_hash',r.consent_package_hash,'created_at',r.created_at)
                  FROM catalog.application_revisions r
                 WHERE r.org_id=a.org_id AND r.release_id=a.release_id AND r.created_at<=a.received_at
                 ORDER BY r.revision DESC LIMIT 1))
             -- The server's original (0127) joins only when recorded, so
             -- applications sealed before it keep their content hash.
             || CASE WHEN a.server_hash IS NULL THEN '{}'::jsonb ELSE jsonb_build_object(
                  'content',a.content,'server_hash',a.server_hash,'signer_identity',a.signer_identity) END)::text AS doc
             FROM portal.release_applications a WHERE a.id=$1"
        }
        Kind::Agreement | Kind::RightsDocument => {
            "SELECT d.org_id, jsonb_build_object(
               'kind',d.kind,'id',d.id,'org_id',d.org_id,'release_id',d.release_id,'title',d.title,
               'version',d.version,'body',d.body,'signer_name',d.signer_name,'signature',d.signature,
               'signed_by',d.signed_by,'signed_at',d.signed_at,'electronic_record',d.electronic_record,
               'agreement_terms',d.agreement_terms,'confirmations',d.confirmations,
               'file',(SELECT jsonb_build_object('asset_id',x.id,'file_name',d.file_name,'sha256',x.sha256,
                    'size_bytes',x.size_bytes,'content_type',x.content_type)
                  FROM catalog.assets x WHERE x.id=d.asset_id))::text AS doc
             FROM portal.documents d WHERE d.id=$1"
        }
        Kind::SigningRequest => {
            "SELECT q.org_id, jsonb_build_object(
               'kind','SIGNING_REQUEST','id',q.id,'org_id',q.org_id,'release_id',q.release_id,
               'document_no',q.document_no,'form',q.form,'document_kind',q.document_kind,'title',q.title,
               'body',q.body,'body_hash',q.body_hash,'rights_holder',q.rights_holder,
               'signer_name',q.signer_name,'signer_role',q.signer_role,'channel',q.channel,
               'status',q.status,'identity',q.identity,'signature',q.signature,'consents',q.consents,
               'signed_at',q.signed_at,'decline_reason',q.decline_reason,'document_id',q.document_id,
               'certificate_hash',q.certificate_hash,'created_by',q.created_by,'created_at',q.created_at,
               'events',(SELECT COALESCE(jsonb_agg(jsonb_build_object('event',e.event,'at',e.at,
                    'actor_user_id',e.actor_user_id,'ip_hash',e.ip_hash,'user_agent',e.user_agent,
                    'detail',e.detail,'prev_hash',e.prev_hash,'hash',e.hash) ORDER BY e.id),'[]'::jsonb)
                  FROM portal.signing_events e WHERE e.request_id=q.id))::text AS doc
             FROM portal.signing_requests q WHERE q.id=$1"
        }
        Kind::TermsAcceptance => {
            "SELECT NULL::uuid AS org_id, jsonb_build_object(
               'kind','TERMS_ACCEPTANCE','id',t.id,'user_id',t.user_id,'doc',t.doc,'version',t.version,
               'channel',t.channel,'accepted_at',t.accepted_at)::text AS doc
             FROM identity.terms_acceptances t WHERE t.id=$1"
        }
    }
}

/// The document as sealed and archived: `(org, canonical JSON)`.
pub async fn render(c: &mut PgConnection, kind: Kind, id: Uuid) -> Result<(Option<Uuid>, String)> {
    let row = sqlx::query(render_sql(kind))
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    Ok((row.get("org_id"), row.get("doc")))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    crate::hex::encode(Sha256::digest(bytes))
}

/// What the document key signs and the chain hashes.
pub fn message(kind: &str, id: Uuid, event: &str, content: &str, at: &str, prev: &str) -> String {
    format!("audeniq-seal-v1|{kind}|{id}|{event}|{content}|{at}|{prev}")
}

fn at_text(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Seals the document inside the caller's transaction.
pub async fn seal(
    c: &mut PgConnection,
    keys: &KeyRing,
    kind: Kind,
    id: Uuid,
    event: &str,
) -> Result<()> {
    let (org, doc) = render(c, kind, id).await?;
    let content = sha256_hex(doc.as_bytes());
    // One chain: seals are written one at a time.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('operations.document_seals',0))")
        .execute(&mut *c)
        .await?;
    let prev: String = sqlx::query_scalar(
        "SELECT COALESCE((SELECT hash FROM operations.document_seals ORDER BY seq DESC LIMIT 1),$1)",
    )
    .bind(GENESIS)
    .fetch_one(&mut *c)
    .await?;
    // Microsecond precision, as stored.
    let at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT date_trunc('microseconds', clock_timestamp())")
            .fetch_one(&mut *c)
            .await?;
    let msg = message(kind.as_str(), id, event, &content, &at_text(at), &prev);
    let hash = sha256_hex(msg.as_bytes());
    let signed = keys.sign_document(msg.as_bytes());
    sqlx::query(
        "INSERT INTO operations.document_seals(doc_kind,doc_id,org_id,event,content_sha256,sealed_at,signature,key_version,prev_hash,hash)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(kind.as_str())
    .bind(id)
    .bind(org)
    .bind(event)
    .bind(&content)
    .bind(at)
    .bind(signed.as_ref().map(|(_, s)| s.clone()))
    .bind(signed.map(|(v, _)| v))
    .bind(&prev)
    .bind(&hash)
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// Seals documents that were signed or submitted before sealing existed
/// (event `BACKFILL`: the seal proves the content from now on, not from the
/// original signing). Runs before every monthly archive, so every original
/// is archived. Returns how many were sealed.
pub async fn backfill(pool: &sqlx::PgPool, keys: &KeyRing) -> Result<i64> {
    let rows = sqlx::query(
        "SELECT 'APPLICATION' AS kind, a.id, a.received_at AS at FROM portal.release_applications a
          WHERE NOT EXISTS(SELECT 1 FROM operations.document_seals s WHERE s.doc_kind='APPLICATION' AND s.doc_id=a.id)
         UNION ALL
         SELECT CASE WHEN d.kind='AGREEMENT' THEN 'AGREEMENT' ELSE 'RIGHTS_DOCUMENT' END, d.id, d.signed_at
           FROM portal.documents d
          WHERE d.status='SIGNED' AND NOT EXISTS(SELECT 1 FROM operations.document_seals s
                WHERE s.doc_kind IN ('AGREEMENT','RIGHTS_DOCUMENT') AND s.doc_id=d.id)
         UNION ALL
         SELECT 'SIGNING_REQUEST', q.id, q.updated_at FROM portal.signing_requests q
          WHERE q.status IN ('SIGNED','DECLINED') AND NOT EXISTS(SELECT 1 FROM operations.document_seals s
                WHERE s.doc_kind='SIGNING_REQUEST' AND s.doc_id=q.id)
         ORDER BY at NULLS FIRST, id",
    )
    .fetch_all(pool)
    .await?;
    let mut sealed = 0;
    for row in rows {
        let Some(kind) = Kind::parse(row.get("kind")) else {
            continue;
        };
        let mut tx = pool.begin().await?;
        seal(&mut tx, keys, kind, row.get("id"), "BACKFILL").await?;
        tx.commit().await?;
        sealed += 1;
    }
    Ok(sealed)
}

/// Public document keys, for anyone verifying seals or archive manifests.
pub fn public_keys(keys: &KeyRing) -> Value {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let items: Vec<Value> = keys
        .document_public_keys()
        .into_iter()
        .map(|(v, k)| json!({"version": v, "algorithm": "Ed25519", "public_key": STANDARD.encode(k)}))
        .collect();
    json!({"keys": items, "message_format": "audeniq-seal-v1|kind|id|event|content_sha256|sealed_at|prev_hash"})
}

#[derive(Default, Debug)]
pub struct Report {
    pub seals: i64,
    pub documents: i64,
    pub unsigned: i64,
    pub problems: Vec<Value>,
    pub head_seq: i64,
    pub head_hash: String,
}

impl Report {
    pub fn json(&self) -> Value {
        json!({
            "ok": self.problems.is_empty(),
            "seals": self.seals,
            "documents": self.documents,
            "unsigned": self.unsigned,
            "head": {"seq": self.head_seq, "hash": self.head_hash},
            "problems": self.problems,
        })
    }
}

/// Checks the whole chain (links, hashes, signatures) and re-renders the
/// latest sealed version of every document against its seal.
pub async fn verify(c: &mut PgConnection, keys: &KeyRing) -> Result<Report> {
    let mut r = Report {
        head_hash: GENESIS.into(),
        ..Default::default()
    };
    let rows = sqlx::query(
        "SELECT seq,doc_kind,doc_id,event,content_sha256,sealed_at,signature,key_version,prev_hash,hash
           FROM operations.document_seals ORDER BY seq",
    )
    .fetch_all(&mut *c)
    .await?;
    let mut prev = GENESIS.to_string();
    let mut latest: std::collections::BTreeMap<(String, Uuid), (i64, String)> = Default::default();
    for row in &rows {
        let seq: i64 = row.get("seq");
        let kind: String = row.get("doc_kind");
        let id: Uuid = row.get("doc_id");
        let content: String = row.get("content_sha256");
        let at: chrono::DateTime<chrono::Utc> = row.get("sealed_at");
        let stored_prev: String = row.get("prev_hash");
        let hash: String = row.get("hash");
        let msg = message(
            &kind,
            id,
            row.get("event"),
            &content,
            &at_text(at),
            &stored_prev,
        );
        if stored_prev != prev {
            r.problems
                .push(json!({"seq": seq, "problem": "CHAIN_BROKEN"}));
        }
        if sha256_hex(msg.as_bytes()) != hash {
            r.problems
                .push(json!({"seq": seq, "problem": "SEAL_ALTERED"}));
        }
        match (
            row.get::<Option<Vec<u8>>, _>("signature"),
            row.get::<Option<i16>, _>("key_version"),
        ) {
            (Some(sig), Some(v)) => {
                if !keys.verify_document(v, msg.as_bytes(), &sig) {
                    r.problems.push(
                        json!({"seq": seq, "problem": "SIGNATURE_INVALID", "key_version": v}),
                    );
                }
            }
            _ => r.unsigned += 1,
        }
        latest.insert((kind, id), (seq, content));
        prev = hash.clone();
        r.seals += 1;
        r.head_seq = seq;
        r.head_hash = hash;
    }
    for ((kind, id), (seq, content)) in &latest {
        let Some(k) = Kind::parse(kind) else { continue };
        r.documents += 1;
        match render(c, k, *id).await {
            Ok((_, doc)) if sha256_hex(doc.as_bytes()) == *content => {}
            Ok(_) => r
                .problems
                .push(json!({"seq": seq, "kind": kind, "id": id, "problem": "DOCUMENT_CHANGED"})),
            Err(Error::NotFound) => r
                .problems
                .push(json!({"seq": seq, "kind": kind, "id": id, "problem": "DOCUMENT_MISSING"})),
            Err(e) => return Err(e),
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signed_message_names_every_part() {
        let id = Uuid::nil();
        let m = message(
            "AGREEMENT",
            id,
            "SIGNED",
            "ab",
            "2026-10-12T00:00:00.000000Z",
            GENESIS,
        );
        assert!(m.starts_with("audeniq-seal-v1|AGREEMENT|00000000-0000-0000-0000-000000000000|SIGNED|ab|2026-10-12T00:00:00.000000Z|0000"));
    }

    #[test]
    fn document_keys_sign_and_verify_per_version() {
        let keys = KeyRing::test_ring();
        let (v, sig) = keys.sign_document(b"x").unwrap();
        assert_eq!(v, 2);
        assert!(keys.verify_document(2, b"x", &sig));
        assert!(!keys.verify_document(2, b"y", &sig));
        assert!(
            !keys.verify_document(1, b"x", &sig),
            "versions have distinct keys"
        );
        assert_eq!(keys.document_public_keys().len(), 2);
    }
}
