//! File-drop adapter: DDEX ERN choreography (global DSPs) or a
//! partner-specific JSON/CSV feed (Korean services) over SFTP, S3 or a
//! local mount.
//!
//! Send sequence (both modes):
//! 1. stage the pinned files locally and re-verify their hashes;
//! 2. upload resources, then the message/manifest, into a fresh folder;
//! 3. upload the completion marker LAST.
//!
//! A partner only ingests a folder once its marker exists, so any failure
//! before step 3 left nothing the partner will process: the attempt is
//! closed as `Unavailable` and retried under a new folder. Only a failure
//! while writing the marker itself is `Unknown` (it may have landed) and
//! goes to reconciliation, never an automatic resend. The folder name is
//! recorded on the attempt before the upload starts, so reconciliation asks
//! about exactly that submission.
use super::ack::{self, AckStatus};
use super::http::ApiClient;
use super::manifest::{self, Extras};
use super::{StageError, check_ddex_package, now_batch_id, stage_files};
use crate::error::{Error, Result};
use crate::execution::{
    AckEvent, Capabilities, DspAdapter, InquiryOutcome, SendContext, SendOutcome, TransferPackage,
};
use crate::partner_config::{AdapterKind, Choreography, LivePolicy, PartnerConfig};
use crate::storage::ObjectStore;
use crate::transport::{FileTransport, TransportError, Upload, join};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

const MAX_ACK_BYTES: u64 = 1024 * 1024;
const MAX_ACK_FILES: usize = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ddex,
    PartnerSpec,
}

pub struct FileDropAdapter {
    partner_id: String,
    mode: Mode,
    caps: Capabilities,
    transport: Arc<dyn FileTransport>,
    storage: Arc<dyn ObjectStore>,
    config: PartnerConfig,
    status_api: Option<ApiClient>,
}

impl FileDropAdapter {
    pub fn new(
        config: PartnerConfig,
        profile_caps: Capabilities,
        storage: Arc<dyn ObjectStore>,
    ) -> Result<Self> {
        Self::with_transport(
            config.clone(),
            profile_caps,
            storage,
            crate::transport::build(&config.transport)?
                .ok_or(Error::InvalidCode("PARTNER_TRANSPORT_MISSING"))?,
        )
    }

    pub fn with_transport(
        config: PartnerConfig,
        profile_caps: Capabilities,
        storage: Arc<dyn ObjectStore>,
        transport: Arc<dyn FileTransport>,
    ) -> Result<Self> {
        let mode = match config.adapter {
            AdapterKind::Ddex => Mode::Ddex,
            AdapterKind::PartnerSpec => Mode::PartnerSpec,
            AdapterKind::HttpApi => return Err(Error::InvalidCode("PARTNER_ADAPTER_MISMATCH")),
        };
        let status_api = config.status_api.clone().map(ApiClient::new).transpose()?;
        let live_policy = match mode {
            Mode::Ddex => config.ddex.live_policy,
            Mode::PartnerSpec => config.partner_spec.live_policy,
        };
        let supported = Capabilities {
            validate_package: true,
            prepare_transfer: true,
            send_or_publish: true,
            inquire_submission: true,
            parse_ack: true,
            get_release_status: status_api.is_some() || live_policy == LivePolicy::OnAck,
            update_release: true,
            takedown: true,
            receive_royalty_report: false,
        };
        Ok(Self {
            partner_id: config.partner_id.clone(),
            mode,
            caps: super::intersect(supported, profile_caps),
            transport,
            storage,
            config,
            status_api,
        })
    }

    fn live_policy(&self) -> LivePolicy {
        match self.mode {
            Mode::Ddex => self.config.ddex.live_policy,
            Mode::PartnerSpec => self.config.partner_spec.live_policy,
        }
    }

    fn inbox(&self) -> String {
        match self.mode {
            Mode::Ddex => self.config.ddex.inbox_dir.clone(),
            Mode::PartnerSpec => self.config.partner_spec.inbox_dir.clone(),
        }
        .unwrap_or_default()
        .trim_matches('/')
        .to_string()
    }

    fn rel(&self, path: &str) -> String {
        let inbox = self.inbox();
        if inbox.is_empty() {
            path.trim_start_matches('/').to_string()
        } else {
            join(&inbox, path).trim_start_matches('/').to_string()
        }
    }

    /// Partner message id for a new submission of `upc`:
    /// batch profile `<batch>/<UPC>`, otherwise `<UPC>_<timestamp>`.
    fn new_message_id(&self, upc: &str) -> String {
        let ts = now_batch_id();
        match (self.mode, self.config.ddex.choreography) {
            (Mode::Ddex, Choreography::Batch) => format!("{ts}/{upc}"),
            _ => format!("{upc}_{ts}"),
        }
    }

    fn upc_of(message_id: &str) -> Option<&str> {
        match message_id.split_once('/') {
            Some((_, upc)) => Some(upc),
            None => message_id.split_once('_').map(|(u, _)| u),
        }
    }

    /// Completion marker path for a submission folder.
    fn marker_path(&self, message_id: &str) -> String {
        match self.mode {
            Mode::Ddex => match message_id.split_once('/') {
                Some((batch, _)) => self.rel(&format!("{batch}/BatchComplete_{batch}.xml")),
                None => self.rel(&format!("{message_id}/BatchComplete_{message_id}.xml")),
            },
            Mode::PartnerSpec => self.rel(&format!(
                "{message_id}/{}",
                self.config
                    .partner_spec
                    .complete_marker
                    .replace("{id}", message_id)
            )),
        }
    }

    fn extras(package: &TransferPackage) -> Extras {
        Extras {
            genre: package.genre.clone(),
            label: package.label.clone(),
        }
    }

    /// The message/manifest documents of one submission.
    fn documents(
        &self,
        package: &TransferPackage,
        message_id: &str,
        action: &str,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let upc = package
            .upc
            .as_deref()
            .ok_or(Error::PolicyGate("DELIVERY_UPC_MISSING"))?;
        let mut docs = Vec::new();
        match self.mode {
            Mode::Ddex => {
                if package.ern_xml.is_empty() {
                    return Err(Error::PolicyGate("DELIVERY_ERN_MISSING"));
                }
                docs.push((format!("{upc}.xml"), package.ern_xml.clone()));
            }
            Mode::PartnerSpec => {
                let p = package
                    .prepared
                    .as_deref()
                    .ok_or(Error::PolicyGate("DELIVERY_METADATA_UNAVAILABLE"))?;
                let extras = Self::extras(package);
                let map = &self.config.partner_spec.field_map;
                let formats = &self.config.partner_spec.formats;
                if formats.iter().any(|f| f == "json") {
                    let m = manifest::with_platform(
                        manifest::release_json(p, &package.files, &extras, message_id, action, map),
                        &self.partner_id,
                    );
                    docs.push((
                        "manifest.json".into(),
                        serde_json::to_vec_pretty(&m).map_err(|_| Error::Internal)?,
                    ));
                }
                if formats.iter().any(|f| f == "csv") && action == "deliver" {
                    docs.push((
                        "metadata.csv".into(),
                        manifest::release_csv(p, &package.files, &extras, map).into_bytes(),
                    ));
                }
                if formats.iter().any(|f| f == "ddex") && !package.ern_xml.is_empty() {
                    docs.push((format!("{upc}.xml"), package.ern_xml.clone()));
                }
                if docs.is_empty() {
                    return Err(Error::InvalidCode("PARTNER_FEED_FORMAT_EMPTY"));
                }
            }
        }
        Ok(docs)
    }

    /// Upload one submission: resources, documents, marker last.
    async fn submit(
        &self,
        package: &TransferPackage,
        message_id: &str,
        action: &str,
        with_files: bool,
    ) -> SendOutcome {
        let folder = self.rel(message_id);
        let docs = match self.documents(package, message_id, action) {
            Ok(d) => d,
            Err(e) => {
                return SendOutcome::Rejected {
                    code: "DELIVERY_DOCUMENT_INVALID".into(),
                    message: format!("{e}"),
                };
            }
        };
        let staged = if with_files {
            match stage_files(&self.storage, &package.files, &folder).await {
                Ok(s) => Some(s),
                Err(StageError::Storage(d)) => {
                    return SendOutcome::Unavailable {
                        detail: format!("staging from storage failed: {d}"),
                    };
                }
                Err(StageError::Integrity(d)) => {
                    return SendOutcome::Rejected {
                        code: "EXECUTION_FILE_TAMPERED".into(),
                        message: format!("local integrity check before send: {d}"),
                    };
                }
            }
        } else {
            None
        };
        let mut uploads: Vec<Upload> = staged
            .as_ref()
            .map(|s| s.uploads.clone())
            .unwrap_or_default();
        for (name, bytes) in docs {
            uploads.push(Upload::Bytes {
                bytes,
                remote: join(&folder, &name).trim_start_matches('/').to_string(),
            });
        }
        if let Err(e) = self.transport.put_all(&uploads).await {
            // No marker yet: the partner will not process this folder.
            return SendOutcome::Unavailable {
                detail: format!("upload before completion marker failed: {e}"),
            };
        }
        let marker = Upload::Bytes {
            bytes: Vec::new(),
            remote: self.marker_path(message_id),
        };
        let outcome = match self.transport.put_all(std::slice::from_ref(&marker)).await {
            Ok(()) => SendOutcome::Accepted {
                partner_message_id: message_id.to_string(),
            },
            Err(TransportError::Unreachable(d)) => SendOutcome::Unavailable {
                detail: format!("completion marker not written (connection failed): {d}"),
            },
            Err(e) => SendOutcome::Unknown {
                detail: format!("completion marker upload interrupted: {e}"),
            },
        };
        drop(staged);
        outcome
    }

    /// Candidate acknowledgement files for a submission.
    async fn ack_candidates(&self, message_id: &str) -> Vec<String> {
        let upc = Self::upc_of(message_id).unwrap_or(message_id);
        let batch = message_id.split('/').next().unwrap_or(message_id);
        let mut out = Vec::new();
        let result_dir = match self.mode {
            Mode::Ddex => self.config.ddex.ack_dir.clone(),
            Mode::PartnerSpec => self.config.partner_spec.result_dir.clone(),
        };
        if let Some(dir) = result_dir {
            let dir = dir.trim_matches('/').to_string();
            if let Ok(names) = self.transport.list(&dir).await {
                for n in names {
                    if n.contains(batch) || n.contains(upc) {
                        out.push(if dir.is_empty() {
                            n
                        } else {
                            format!("{dir}/{n}")
                        });
                    }
                }
            }
        }
        let folder = self.rel(message_id);
        if let Ok(names) = self.transport.list(&folder).await {
            for n in names {
                let l = n.to_ascii_lowercase();
                if l.starts_with("ack")
                    || l.contains("_ack")
                    || l.contains("result")
                    || l.contains("receipt")
                {
                    out.push(format!("{folder}/{n}"));
                }
            }
        }
        out.sort();
        out.dedup();
        out.truncate(MAX_ACK_FILES);
        out
    }

    /// Latest definite answer among the candidate files.
    async fn latest_answer(&self, message_id: &str) -> Result<Option<(ack::AckDoc, Vec<u8>)>> {
        let upc = Self::upc_of(message_id).unwrap_or(message_id);
        let batch = message_id.split('/').next().unwrap_or(message_id);
        let mut answer = None;
        for path in self.ack_candidates(message_id).await {
            let bytes = match self.transport.get(&path, MAX_ACK_BYTES).await {
                Ok(b) => b,
                Err(TransportError::NotFound) => continue,
                Err(TransportError::Unreachable(_)) => return Err(Error::Storage),
                Err(_) => continue,
            };
            let Some(doc) = ack::parse_document(&bytes) else {
                continue;
            };
            // Files from a shared ACK folder must name this submission.
            let in_folder = path.starts_with(&self.rel(message_id));
            let mentions = doc
                .refs
                .iter()
                .any(|r| r.contains(batch) || r.contains(upc) || r.contains(message_id));
            if !in_folder && !mentions && !path.contains(batch) {
                continue;
            }
            if doc.status != AckStatus::Pending {
                answer = Some((doc, bytes));
            }
        }
        Ok(answer)
    }

    fn outcome_of(&self, doc: &ack::AckDoc, message_id: &str) -> InquiryOutcome {
        match &doc.status {
            AckStatus::Success if self.live_policy() == LivePolicy::OnAck => InquiryOutcome::Live {
                partner_release_id: doc
                    .partner_release_id
                    .clone()
                    .unwrap_or_else(|| message_id.to_string()),
            },
            AckStatus::Success => InquiryOutcome::Accepted {
                partner_message_id: message_id.to_string(),
            },
            AckStatus::Live => InquiryOutcome::Live {
                partner_release_id: doc
                    .partner_release_id
                    .clone()
                    .unwrap_or_else(|| message_id.to_string()),
            },
            AckStatus::TakenDown => InquiryOutcome::TakenDown,
            AckStatus::Failure { code, .. } => InquiryOutcome::Rejected { code: code.clone() },
            AckStatus::Pending => InquiryOutcome::StillUnknown,
        }
    }

    /// Scan for answers without a known message id (admin/ops sweep).
    pub fn transport(&self) -> &Arc<dyn FileTransport> {
        &self.transport
    }
}

#[async_trait]
impl DspAdapter for FileDropAdapter {
    fn partner_id(&self) -> &str {
        &self.partner_id
    }

    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn plan_message_id(&self, package: &TransferPackage) -> Option<String> {
        package.upc.as_deref().map(|u| self.new_message_id(u))
    }

    async fn validate_package(&self, package: &TransferPackage) -> Result<()> {
        self.require(self.caps.validate_package, "validate_package")?;
        match self.mode {
            Mode::Ddex => check_ddex_package(package),
            Mode::PartnerSpec => {
                let p = package
                    .prepared
                    .as_deref()
                    .ok_or(Error::PolicyGate("DELIVERY_METADATA_UNAVAILABLE"))?;
                if package.upc.as_deref() != Some(p.upc.as_str()) {
                    return Err(Error::PolicyGate("DELIVERY_UPC_MISMATCH"));
                }
                // Every track file and the cover must be in the package.
                let mut want: Vec<&str> =
                    p.tracks.iter().map(|t| t.audio.sha256.as_str()).collect();
                want.push(p.artwork.sha256.as_str());
                for sha in want {
                    if !package.files.iter().any(|f| f.sha256 == sha) {
                        return Err(Error::PolicyGate("DELIVERY_FILE_MISSING"));
                    }
                }
                Ok(())
            }
        }
    }

    async fn prepare_transfer(&self, package: &TransferPackage) -> Result<Value> {
        self.require(self.caps.prepare_transfer, "prepare_transfer")?;
        let upc = package.upc.as_deref().unwrap_or("");
        Ok(json!({
            "transport": self.transport.describe(),
            "files": package.files.iter().map(|f| &f.delivery_name).collect::<Vec<_>>(),
            "document": match self.mode { Mode::Ddex => format!("{upc}.xml"), Mode::PartnerSpec => "manifest.json".into() },
        }))
    }

    async fn send_or_publish(&self, ctx: &SendContext) -> Result<SendOutcome> {
        self.require(self.caps.send_or_publish, "send_or_publish")?;
        let Some(message_id) = ctx
            .planned_message_id
            .clone()
            .or_else(|| self.plan_message_id(&ctx.package))
        else {
            return Ok(SendOutcome::Rejected {
                code: "DELIVERY_UPC_MISSING".into(),
                message: "no UPC to name the submission".into(),
            });
        };
        if !crate::transport::valid_remote_path(&message_id) {
            return Ok(SendOutcome::Rejected {
                code: "DELIVERY_MESSAGE_ID_INVALID".into(),
                message: message_id,
            });
        }
        Ok(self
            .submit(&ctx.package, &message_id, "deliver", true)
            .await)
    }

    async fn inquire_submission(&self, partner_message_id: &str) -> Result<InquiryOutcome> {
        self.require(self.caps.inquire_submission, "inquire_submission")?;
        if !crate::transport::valid_remote_path(partner_message_id) {
            return Ok(InquiryOutcome::StillUnknown);
        }
        Ok(match self.latest_answer(partner_message_id).await? {
            Some((doc, _)) => self.outcome_of(&doc, partner_message_id),
            None => InquiryOutcome::StillUnknown,
        })
    }

    async fn parse_ack(&self, payload: &[u8]) -> Result<AckEvent> {
        self.require(self.caps.parse_ack, "parse_ack")?;
        let doc = ack::parse_document(payload).ok_or(Error::Invalid)?;
        // JSON events name the submission; DDEX ACK documents mention the
        // folder in their file paths.
        let pmid = serde_json::from_slice::<Value>(payload)
            .ok()
            .and_then(|v| {
                ["partner_message_id", "delivery_id", "batch_id"]
                    .iter()
                    .find_map(|k| v.get(*k).and_then(Value::as_str).map(str::to_owned))
            })
            .or_else(|| ack::find_ddex_message_id(&doc.refs));
        match (&doc.status, pmid) {
            (AckStatus::Pending, _) => Err(Error::InvalidCode("ACK_PENDING")),
            (_, Some(p)) => ack::to_event(&doc, payload, &p).ok_or(Error::Invalid),
            (AckStatus::Live | AckStatus::TakenDown, None) if doc.partner_release_id.is_some() => {
                let event_id = doc
                    .event_id
                    .clone()
                    .unwrap_or_else(|| ack::content_event_id(payload));
                Ok(if doc.status == AckStatus::Live {
                    AckEvent::Live {
                        event_id,
                        partner_release_id: doc.partner_release_id.clone().unwrap_or_default(),
                        partner_message_id: None,
                    }
                } else {
                    AckEvent::TakedownConfirmed {
                        event_id,
                        partner_release_id: doc.partner_release_id.clone(),
                        partner_message_id: None,
                    }
                })
            }
            _ => Err(Error::Invalid),
        }
    }

    async fn get_release_status(&self, partner_release_id: &str) -> Result<InquiryOutcome> {
        self.require(self.caps.get_release_status, "get_release_status")?;
        if let Some(api) = &self.status_api {
            let path = super::http::fill(&api.config.status_path, partner_release_id, "", "");
            return Ok(
                match api.json(reqwest::Method::GET, &path, None, None).await {
                    Ok(v) => super::api::inquiry_from_status(api, &v, partner_release_id),
                    Err(super::http::HttpFailure::Rejected { status: 404, .. }) => {
                        InquiryOutcome::StillUnknown
                    }
                    Err(super::http::HttpFailure::Rejected { code, .. }) => {
                        InquiryOutcome::Rejected { code }
                    }
                    Err(_) => return Err(Error::Storage),
                },
            );
        }
        // OnAck: the successful ACK already made it live.
        Ok(InquiryOutcome::Live {
            partner_release_id: partner_release_id.to_string(),
        })
    }

    async fn update_release(&self, ctx: &SendContext, _changes: &Value) -> Result<SendOutcome> {
        self.require(self.caps.update_release, "update_release")?;
        let upc = ctx.package.upc.as_deref().unwrap_or_default();
        if upc.is_empty() {
            return Ok(SendOutcome::Rejected {
                code: "DELIVERY_UPC_MISSING".into(),
                message: "update needs the release UPC".into(),
            });
        }
        let id = self.new_message_id(upc);
        // Metadata update: the message only; resources are unchanged.
        Ok(self.submit(&ctx.package, &id, "update", false).await)
    }

    async fn takedown(&self, ctx: &SendContext) -> Result<SendOutcome> {
        self.require(self.caps.takedown, "takedown")?;
        let upc = ctx.package.upc.as_deref().unwrap_or_default();
        if upc.is_empty() {
            return Ok(SendOutcome::Rejected {
                code: "DELIVERY_UPC_MISSING".into(),
                message: "takedown needs the release UPC".into(),
            });
        }
        let id = self.new_message_id(upc);
        Ok(self.submit(&ctx.package, &id, "takedown", false).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{TResult, TransportError};
    use std::sync::Mutex;

    /// Transport that fails the n-th `put_all` call with the given error.
    struct Flaky {
        fail_on: usize,
        error: TransportError,
        calls: Mutex<Vec<Vec<String>>>,
    }

    #[async_trait]
    impl FileTransport for Flaky {
        async fn put_all(&self, uploads: &[Upload]) -> TResult<()> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(uploads.iter().map(|u| u.remote().to_string()).collect());
            if calls.len() == self.fail_on {
                Err(self.error.clone())
            } else {
                Ok(())
            }
        }
        async fn list(&self, _dir: &str) -> TResult<Vec<String>> {
            Ok(Vec::new())
        }
        async fn get(&self, _path: &str, _max: u64) -> TResult<Vec<u8>> {
            Err(TransportError::NotFound)
        }
        async fn probe(&self) -> TResult<()> {
            Ok(())
        }
        fn describe(&self) -> String {
            "flaky".into()
        }
    }

    fn adapter(fail_on: usize, error: TransportError) -> (FileDropAdapter, Arc<Flaky>) {
        let t = Arc::new(Flaky {
            fail_on,
            error,
            calls: Mutex::new(Vec::new()),
        });
        let cfg = PartnerConfig::parse(
            r#"{"partner_id":"D-5","adapter":"ddex","transport":{"kind":"sftp","host":"h","username":"u",
                "private_key":{"file":"/k"},"known_hosts":{"file":"/kh"}},"ddex":{"inbox_dir":"inbox"}}"#,
        )
        .unwrap();
        let all = Capabilities::from_json(&json!({"validate_package":true,"prepare_transfer":true,
            "send_or_publish":true,"inquire_submission":true,"parse_ack":true,"update_release":true,"takedown":true}));
        let a = FileDropAdapter::with_transport(
            cfg,
            all,
            Arc::new(crate::storage::DisabledStore),
            t.clone(),
        )
        .unwrap();
        (a, t)
    }

    fn ctx(a: &FileDropAdapter) -> SendContext {
        let package = TransferPackage {
            package_id: uuid::Uuid::nil(),
            package_hash: String::new(),
            org_id: uuid::Uuid::nil(),
            release_id: uuid::Uuid::nil(),
            ern_xml: b"<ern:NewReleaseMessage xmlns:ern=\"x\"/>".to_vec(),
            files: Vec::new(),
            upc: Some("036000291452".into()),
            prepared: None,
            genre: None,
            label: None,
        };
        SendContext {
            job_id: uuid::Uuid::nil(),
            attempt_id: uuid::Uuid::nil(),
            attempt_no: 1,
            idempotency_key: "delivery:x:1".into(),
            planned_message_id: a.plan_message_id(&package),
            package,
        }
    }

    #[tokio::test]
    async fn marker_is_written_last_at_the_batch_level() {
        let (a, t) = adapter(99, TransportError::NotFound);
        let c = ctx(&a);
        let pmid = c.planned_message_id.clone().unwrap();
        let (batch, upc) = pmid.split_once('/').unwrap();
        assert_eq!(upc, "036000291452");
        match a.send_or_publish(&c).await.unwrap() {
            SendOutcome::Accepted { partner_message_id } => assert_eq!(partner_message_id, pmid),
            o => panic!("{o:?}"),
        }
        let calls = t.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], vec![format!("inbox/{batch}/{upc}/{upc}.xml")]);
        assert_eq!(
            calls[1],
            vec![format!("inbox/{batch}/BatchComplete_{batch}.xml")]
        );
    }

    #[tokio::test]
    async fn failure_before_marker_is_a_safe_retry() {
        for e in [
            TransportError::Failed("disk full".into()),
            TransportError::Unreachable("refused".into()),
        ] {
            let (a, _) = adapter(1, e);
            assert!(matches!(
                a.send_or_publish(&ctx(&a)).await.unwrap(),
                SendOutcome::Unavailable { .. }
            ));
        }
    }

    #[tokio::test]
    async fn interrupted_marker_is_unknown_but_refused_connection_is_retryable() {
        let (a, _) = adapter(2, TransportError::Failed("broken pipe".into()));
        assert!(matches!(
            a.send_or_publish(&ctx(&a)).await.unwrap(),
            SendOutcome::Unknown { .. }
        ));
        let (a, _) = adapter(2, TransportError::Unreachable("refused".into()));
        assert!(matches!(
            a.send_or_publish(&ctx(&a)).await.unwrap(),
            SendOutcome::Unavailable { .. }
        ));
    }

    #[tokio::test]
    async fn release_by_release_profile_names_folders_by_upc() {
        let cfg = PartnerConfig::parse(
            r#"{"partner_id":"D-9","adapter":"ddex","transport":{"kind":"sftp","host":"h","username":"u",
                "private_key":{"file":"/k"},"known_hosts":{"file":"/kh"}},"ddex":{"choreography":"release_by_release"}}"#,
        )
        .unwrap();
        let t = Arc::new(Flaky {
            fail_on: 99,
            error: TransportError::NotFound,
            calls: Mutex::new(Vec::new()),
        });
        let a = FileDropAdapter::with_transport(
            cfg,
            Capabilities::from_json(&json!({"send_or_publish":true})),
            Arc::new(crate::storage::DisabledStore),
            t.clone(),
        )
        .unwrap();
        let c = ctx(&a);
        let pmid = c.planned_message_id.clone().unwrap();
        assert!(pmid.starts_with("036000291452_"));
        a.send_or_publish(&c).await.unwrap();
        let calls = t.calls.lock().unwrap().clone();
        assert_eq!(calls[1], vec![format!("{pmid}/BatchComplete_{pmid}.xml")]);
        // Capabilities the profile does not document stay off.
        assert!(!a.capabilities().takedown);
        assert!(matches!(
            a.inquire_submission(&pmid).await,
            Err(Error::Gated)
        ));
    }
}
