//! Partner REST API adapter: create submission → upload files → commit,
//! then status polling. Paths, id/status pointers and the status vocabulary
//! come from `http_api` in the partner config, so a contract's API maps
//! onto this flow by configuration.
//!
//! No-duplicate rule: the create call carries our idempotency key
//! (`Idempotency-Key`), and nothing is processed partner-side before the
//! commit call. Failures before commit close the attempt as `Unavailable`;
//! only a commit whose answer is lost is `Unknown`, reconciled through the
//! idempotency-key lookup.
use super::http::{ApiClient, HttpFailure, fill};
use super::manifest::{self, Extras};
use super::{StageError, stage_files};
use crate::error::{Error, Result};
use crate::execution::{
    AckEvent, Capabilities, DspAdapter, InquiryOutcome, SendContext, SendOutcome, TransferPackage,
};
use crate::partner_config::PartnerConfig;
use crate::storage::ObjectStore;
use crate::transport::Upload;
use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct HttpApiAdapter {
    partner_id: String,
    caps: Capabilities,
    api: ApiClient,
    storage: Arc<dyn ObjectStore>,
    config: PartnerConfig,
}

/// Map a partner status document to an inquiry outcome.
pub fn inquiry_from_status(api: &ApiClient, v: &Value, id: &str) -> InquiryOutcome {
    let (status, release_id) = api.status_of(v);
    match status.as_str() {
        "ACCEPTED" => InquiryOutcome::Accepted {
            partner_message_id: id.to_string(),
        },
        "LIVE" => InquiryOutcome::Live {
            partner_release_id: release_id.unwrap_or_else(|| id.to_string()),
        },
        "TAKEN_DOWN" => InquiryOutcome::TakenDown,
        "PENDING" => InquiryOutcome::StillUnknown,
        other => InquiryOutcome::Rejected {
            code: other.trim_start_matches("REJECTED:").to_string(),
        },
    }
}

impl HttpApiAdapter {
    pub fn new(
        config: PartnerConfig,
        profile_caps: Capabilities,
        storage: Arc<dyn ObjectStore>,
    ) -> Result<Self> {
        let api = ApiClient::new(
            config
                .http_api
                .clone()
                .ok_or(Error::InvalidCode("PARTNER_HTTP_API_MISSING"))?,
        )?;
        let supported = Capabilities {
            validate_package: true,
            prepare_transfer: true,
            send_or_publish: true,
            inquire_submission: true,
            parse_ack: true,
            get_release_status: true,
            update_release: true,
            takedown: true,
            receive_royalty_report: false,
        };
        Ok(Self {
            partner_id: config.partner_id.clone(),
            caps: super::intersect(supported, profile_caps),
            api,
            storage,
            config,
        })
    }

    fn body(
        &self,
        package: &TransferPackage,
        action: &str,
        key: &str,
    ) -> Result<(Vec<u8>, &'static str)> {
        if self.api.config.send_ddex_xml {
            if package.ern_xml.is_empty() {
                return Err(Error::PolicyGate("DELIVERY_ERN_MISSING"));
            }
            return Ok((package.ern_xml.clone(), "application/xml"));
        }
        let p = package
            .prepared
            .as_deref()
            .ok_or(Error::PolicyGate("DELIVERY_METADATA_UNAVAILABLE"))?;
        let extras = Extras {
            genre: package.genre.clone(),
            label: package.label.clone(),
        };
        let m = manifest::with_platform(
            manifest::release_json(
                p,
                &package.files,
                &extras,
                key,
                action,
                &self.config.partner_spec.field_map,
            ),
            &self.partner_id,
        );
        Ok((
            serde_json::to_vec(&m).map_err(|_| Error::Internal)?,
            "application/json",
        ))
    }

    fn not_received(detail: String) -> SendOutcome {
        SendOutcome::Unavailable { detail }
    }
}

#[async_trait]
impl DspAdapter for HttpApiAdapter {
    fn partner_id(&self) -> &str {
        &self.partner_id
    }

    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    async fn validate_package(&self, package: &TransferPackage) -> Result<()> {
        self.require(self.caps.validate_package, "validate_package")?;
        if self.api.config.send_ddex_xml {
            super::check_ddex_package(package)
        } else if package.prepared.is_none() {
            Err(Error::PolicyGate("DELIVERY_METADATA_UNAVAILABLE"))
        } else {
            Ok(())
        }
    }

    async fn prepare_transfer(&self, package: &TransferPackage) -> Result<Value> {
        self.require(self.caps.prepare_transfer, "prepare_transfer")?;
        Ok(json!({"endpoint": self.api.config.base_url, "files": package.files.len()}))
    }

    async fn send_or_publish(&self, ctx: &SendContext) -> Result<SendOutcome> {
        self.require(self.caps.send_or_publish, "send_or_publish")?;
        let key = ctx.idempotency_key.as_str();
        let body = match self.body(&ctx.package, "deliver", key) {
            Ok(b) => b,
            Err(e) => {
                return Ok(SendOutcome::Rejected {
                    code: "DELIVERY_DOCUMENT_INVALID".into(),
                    message: format!("{e}"),
                });
            }
        };
        // 1. create
        let created = match self
            .api
            .send(
                Method::POST,
                &self.api.config.create_path,
                Some((body.0, body.1)),
                Some(key),
            )
            .await
        {
            Ok((_, b)) => serde_json::from_slice::<Value>(&b).unwrap_or(Value::Null),
            Err(HttpFailure::NotReceived(d)) => return Ok(Self::not_received(d)),
            Err(HttpFailure::Rejected { code, status }) => {
                return Ok(SendOutcome::Rejected {
                    code,
                    message: format!("create refused (HTTP {status})"),
                });
            }
            // A create whose answer is lost is harmless: nothing is
            // processed before commit, and the next attempt creates anew.
            Err(HttpFailure::Unknown(d)) => return Ok(Self::not_received(d)),
        };
        let Some(id) = created
            .pointer(&self.api.config.id_pointer)
            .and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
        else {
            return Ok(Self::not_received("create response has no id".into()));
        };
        // 2. files
        let staged = match stage_files(&self.storage, &ctx.package.files, "").await {
            Ok(s) => s,
            Err(StageError::Storage(d)) => return Ok(Self::not_received(d)),
            Err(StageError::Integrity(d)) => {
                return Ok(SendOutcome::Rejected {
                    code: "EXECUTION_FILE_TAMPERED".into(),
                    message: d,
                });
            }
        };
        for (u, f) in staged.uploads.iter().zip(&ctx.package.files) {
            let Upload::File { local, .. } = u else {
                continue;
            };
            let path = fill(&self.api.config.file_path, &id, &f.delivery_name, key);
            match self
                .api
                .send_file(
                    Method::PUT,
                    &path,
                    local,
                    &f.sha256,
                    &f.content_type,
                    Some(key),
                )
                .await
            {
                Ok(_) => {}
                Err(HttpFailure::Rejected { code, status }) => {
                    return Ok(SendOutcome::Rejected {
                        code,
                        message: format!("file {} refused (HTTP {status})", f.delivery_name),
                    });
                }
                Err(HttpFailure::NotReceived(d) | HttpFailure::Unknown(d)) => {
                    return Ok(Self::not_received(format!(
                        "file upload before commit: {d}"
                    )));
                }
            }
        }
        drop(staged);
        // 3. commit — the only step that makes the partner act.
        let path = fill(&self.api.config.commit_path, &id, "", key);
        Ok(
            match self.api.send(Method::POST, &path, None, Some(key)).await {
                Ok(_) => SendOutcome::Accepted {
                    partner_message_id: id,
                },
                Err(HttpFailure::NotReceived(d)) => Self::not_received(d),
                Err(HttpFailure::Rejected { code, status }) => SendOutcome::Rejected {
                    code,
                    message: format!("commit refused (HTTP {status})"),
                },
                Err(HttpFailure::Unknown(d)) => SendOutcome::Unknown { detail: d },
            },
        )
    }

    async fn inquire_submission(&self, partner_message_id: &str) -> Result<InquiryOutcome> {
        self.require(self.caps.inquire_submission, "inquire_submission")?;
        let path = fill(&self.api.config.status_path, partner_message_id, "", "");
        match self.api.json(Method::GET, &path, None, None).await {
            Ok(v) => Ok(inquiry_from_status(&self.api, &v, partner_message_id)),
            Err(HttpFailure::Rejected { status: 404, .. }) => Ok(InquiryOutcome::StillUnknown),
            Err(HttpFailure::Rejected { code, .. }) => Ok(InquiryOutcome::Rejected { code }),
            Err(_) => Err(Error::Storage),
        }
    }

    async fn inquire_by_idempotency(&self, idempotency_key: &str) -> Result<InquiryOutcome> {
        let path = fill(&self.api.config.lookup_path, "", "", idempotency_key);
        match self.api.json(Method::GET, &path, None, None).await {
            Ok(v) => {
                // Either the submission itself or a list with one item.
                let doc = v
                    .get("items")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first().cloned())
                    .unwrap_or(v);
                let Some(id) = doc.pointer(&self.api.config.id_pointer).and_then(|x| {
                    x.as_str()
                        .map(str::to_owned)
                        .or_else(|| x.as_i64().map(|n| n.to_string()))
                }) else {
                    return Ok(InquiryOutcome::StillUnknown);
                };
                Ok(match inquiry_from_status(&self.api, &doc, &id) {
                    InquiryOutcome::StillUnknown => InquiryOutcome::Accepted {
                        partner_message_id: id,
                    },
                    o => o,
                })
            }
            Err(HttpFailure::Rejected { status: 404, .. }) => Ok(InquiryOutcome::StillUnknown),
            Err(_) => Ok(InquiryOutcome::StillUnknown),
        }
    }

    async fn parse_ack(&self, payload: &[u8]) -> Result<AckEvent> {
        self.require(self.caps.parse_ack, "parse_ack")?;
        let doc = super::ack::parse_document(payload).ok_or(Error::Invalid)?;
        let v: Value = serde_json::from_slice(payload).map_err(|_| Error::Invalid)?;
        let id = ["partner_message_id", "submission_id", "id"]
            .iter()
            .find_map(|k| {
                v.get(*k).and_then(|x| {
                    x.as_str()
                        .map(str::to_owned)
                        .or_else(|| x.as_i64().map(|n| n.to_string()))
                })
            });
        match id {
            Some(id) => {
                super::ack::to_event(&doc, payload, &id).ok_or(Error::InvalidCode("ACK_PENDING"))
            }
            None => Err(Error::Invalid),
        }
    }

    async fn get_release_status(&self, partner_release_id: &str) -> Result<InquiryOutcome> {
        self.require(self.caps.get_release_status, "get_release_status")?;
        self.inquire_submission(partner_release_id).await
    }

    async fn update_release(&self, ctx: &SendContext, _changes: &Value) -> Result<SendOutcome> {
        self.require(self.caps.update_release, "update_release")?;
        let Some(id) = ctx.planned_message_id.clone() else {
            return Ok(SendOutcome::Rejected {
                code: "UPDATE_TARGET_UNKNOWN".into(),
                message: "no partner submission id".into(),
            });
        };
        let body = match self.body(&ctx.package, "update", &ctx.idempotency_key) {
            Ok(b) => b,
            Err(e) => {
                return Ok(SendOutcome::Rejected {
                    code: "DELIVERY_DOCUMENT_INVALID".into(),
                    message: format!("{e}"),
                });
            }
        };
        let path = fill(&self.api.config.update_path, &id, "", &ctx.idempotency_key);
        Ok(
            match self
                .api
                .send(Method::PUT, &path, Some(body), Some(&ctx.idempotency_key))
                .await
            {
                Ok(_) => SendOutcome::Accepted {
                    partner_message_id: id,
                },
                Err(HttpFailure::NotReceived(d)) => SendOutcome::Unavailable { detail: d },
                Err(HttpFailure::Rejected { code, .. }) => SendOutcome::Rejected {
                    code,
                    message: "update refused".into(),
                },
                Err(HttpFailure::Unknown(d)) => SendOutcome::Unknown { detail: d },
            },
        )
    }

    async fn takedown(&self, ctx: &SendContext) -> Result<SendOutcome> {
        self.require(self.caps.takedown, "takedown")?;
        let Some(id) = ctx.planned_message_id.clone() else {
            return Ok(SendOutcome::Rejected {
                code: "TAKEDOWN_TARGET_UNKNOWN".into(),
                message: "no partner submission id".into(),
            });
        };
        let path = fill(
            &self.api.config.takedown_path,
            &id,
            "",
            &ctx.idempotency_key,
        );
        Ok(
            match self
                .api
                .send(Method::POST, &path, None, Some(&ctx.idempotency_key))
                .await
            {
                Ok(_) => SendOutcome::Accepted {
                    partner_message_id: id,
                },
                Err(HttpFailure::NotReceived(d)) => SendOutcome::Unavailable { detail: d },
                Err(HttpFailure::Rejected { code, .. }) => SendOutcome::Rejected {
                    code,
                    message: "takedown refused".into(),
                },
                Err(HttpFailure::Unknown(d)) => SendOutcome::Unknown { detail: d },
            },
        )
    }
}
