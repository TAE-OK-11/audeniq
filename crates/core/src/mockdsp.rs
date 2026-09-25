//! F5 local test partner: MockDSP.
//!
//! Reproduces the partner behaviors BLUEPRINT 16.3 requires for contract
//! testing: ACCEPT, REJECT, TIMEOUT, UNKNOWN, WEBHOOK DUPLICATE and
//! DELAYED LIVE. No real contract is assumed; every byte is synthetic.
//!
//! The mock records every wire call by idempotency key so tests can prove
//! the zero-duplicate-send invariant. An `Unknown` send still creates the
//! partner-side submission (the response was lost, not the request), keyed
//! by idempotency key so `inquire_by_idempotency` can reconcile it — the
//! same correlation a real partner integration would use.

use crate::error::{Error, Result};
use crate::execution::{
    AckEvent, Capabilities, DspAdapter, InquiryOutcome, SendContext, SendOutcome, TransferPackage,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::Digest as _;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Scripted partner behavior per test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MockBehavior {
    /// Immediate accept with a fresh partner message id.
    #[default]
    Accept,
    /// Immediate reject with the given code.
    Reject { code: String },
    /// The wire call never returns: the send state is unknowable.
    Timeout,
    /// The response is lost but the partner DID create the submission;
    /// reconcilable via `inquire_by_idempotency`.
    Unknown,
    /// Accept, then `get_release_status` reports ingesting until
    /// `polls_before_live` polls have happened.
    DelayedLive { polls_before_live: u32 },
    /// The next `remaining` sends are refused before processing (partner
    /// outage, nothing created); later sends are accepted.
    Unavailable { remaining: u32 },
}

impl MockBehavior {
    /// Parse a `MOCKDSP_BEHAVIOR` spec: `accept`, `reject:<CODE>`, `timeout`,
    /// `unknown`, `delayed_live:<N>`, `unavailable:<N>` (first N sends fail).
    pub fn from_spec(spec: &str) -> Option<Self> {
        let (kind, arg) = match spec.trim().split_once(':') {
            Some((k, a)) => (k, Some(a)),
            None => (spec.trim(), None),
        };
        Some(match (kind.to_ascii_lowercase().as_str(), arg) {
            ("accept", None) => Self::Accept,
            ("reject", Some(code)) if !code.is_empty() => Self::Reject { code: code.into() },
            ("timeout", None) => Self::Timeout,
            ("unknown", None) => Self::Unknown,
            ("delayed_live", Some(n)) => Self::DelayedLive {
                polls_before_live: n.parse().ok()?,
            },
            ("unavailable", Some(n)) => Self::Unavailable {
                remaining: n.parse().ok()?,
            },
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ReceivedSend {
    pub idempotency_key: String,
    pub attempt_no: i32,
    pub package_hash: String,
    /// SHA-256 of the transfer document bytes the adapter actually received.
    pub ern_sha256: String,
}

#[derive(Debug, Clone)]
struct SubmissionState {
    partner_message_id: String,
    idempotency_key: String,
    accepted: bool,
    live: bool,
}

struct Inner {
    behavior: MockBehavior,
    capabilities: Capabilities,
    received: Vec<ReceivedSend>,
    seen_keys: HashSet<String>,
    submissions: HashMap<String, SubmissionState>, // by partner_message_id
    by_key: HashMap<String, String>,               // idempotency_key -> partner_message_id
    live_polls: HashMap<String, u32>,              // partner_message_id -> polls so far
    events_emitted: u64,
}

/// Count one status check toward the DelayedLive go-live threshold. Other
/// behaviors report live on the first check.
fn advance_live_poll(inner: &mut Inner, pmid: &str) {
    let threshold = match inner.behavior {
        MockBehavior::DelayedLive { polls_before_live } => polls_before_live,
        _ => 1,
    };
    let count = {
        let polls = inner.live_polls.entry(pmid.to_string()).or_insert(0);
        *polls += 1;
        *polls
    };
    if count >= threshold
        && let Some(s) = inner.submissions.get_mut(pmid)
    {
        s.live = true;
    }
}

#[derive(Clone)]
pub struct MockDsp {
    inner: Arc<Mutex<Inner>>,
}

impl MockDsp {
    pub fn new(behavior: MockBehavior) -> Self {
        Self::with_capabilities(
            behavior,
            Capabilities {
                validate_package: true,
                prepare_transfer: true,
                send_or_publish: true,
                inquire_submission: true,
                parse_ack: true,
                get_release_status: true,
                update_release: true,
                takedown: true,
                receive_royalty_report: false,
            },
        )
    }

    pub fn with_capabilities(behavior: MockBehavior, capabilities: Capabilities) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                behavior,
                capabilities,
                received: Vec::new(),
                seen_keys: HashSet::new(),
                submissions: HashMap::new(),
                by_key: HashMap::new(),
                live_polls: HashMap::new(),
                events_emitted: 0,
            })),
        }
    }

    pub fn set_behavior(&self, behavior: MockBehavior) {
        self.inner.lock().unwrap().behavior = behavior;
    }

    /// Every wire call the mock has seen, in order.
    pub fn received(&self) -> Vec<ReceivedSend> {
        self.inner.lock().unwrap().received.clone()
    }

    /// How many times each idempotency key was seen. The F5 invariant is
    /// that no key is ever seen twice.
    pub fn key_counts(&self) -> HashMap<String, usize> {
        let inner = self.inner.lock().unwrap();
        let mut counts = HashMap::new();
        for r in &inner.received {
            *counts.entry(r.idempotency_key.clone()).or_insert(0) += 1;
        }
        counts
    }

    /// Build a synthetic webhook payload for a submission event. Tests feed
    /// the bytes to `execution::ingest_ack`; feeding the same bytes twice
    /// exercises the webhook-duplicate path.
    pub fn emit_webhook(&self, partner_message_id: &str, kind: &str) -> Vec<u8> {
        let mut inner = self.inner.lock().unwrap();
        inner.events_emitted += 1;
        let event_id = format!("mock-evt-{}", inner.events_emitted);
        let payload = match kind {
            "live" => json!({
                "event_id": event_id,
                "type": "live",
                "partner_release_id": format!("mock-rel-{partner_message_id}"),
            }),
            "takedown_confirmed" => json!({
                "event_id": event_id,
                "type": "takedown_confirmed",
            }),
            "rejected" => json!({
                "event_id": event_id,
                "type": "rejected",
                "partner_message_id": partner_message_id,
                "code": "MOCK_REJECT",
            }),
            _ => json!({
                "event_id": event_id,
                "type": "accepted",
                "partner_message_id": partner_message_id,
            }),
        };
        serde_json::to_vec(&payload).unwrap()
    }

    fn fresh_message_id(inner: &Inner) -> String {
        format!(
            "mock-msg-{}-{}",
            inner.submissions.len() + 1,
            Uuid::new_v4().simple()
        )
    }
}

#[async_trait]
impl DspAdapter for MockDsp {
    fn partner_id(&self) -> &str {
        "mockdsp"
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.lock().unwrap().capabilities
    }

    async fn validate_package(&self, package: &TransferPackage) -> Result<()> {
        self.require(self.capabilities().validate_package, "validate_package")?;
        if package.ern_xml.is_empty() {
            return Err(Error::PolicyGate("MOCK_EMPTY_TRANSFER"));
        }
        Ok(())
    }

    async fn prepare_transfer(&self, _package: &TransferPackage) -> Result<Value> {
        self.require(self.capabilities().prepare_transfer, "prepare_transfer")?;
        Ok(json!({"mock": "prepared"}))
    }

    async fn send_or_publish(&self, ctx: &SendContext) -> Result<SendOutcome> {
        self.require(self.capabilities().send_or_publish, "send_or_publish")?;
        let mut inner = self.inner.lock().unwrap();
        // The mock records duplicates instead of rejecting them: the test
        // asserts the count, which is the F5 zero-duplicate-send proof.
        inner.seen_keys.insert(ctx.idempotency_key.clone());
        inner.received.push(ReceivedSend {
            idempotency_key: ctx.idempotency_key.clone(),
            attempt_no: ctx.attempt_no,
            package_hash: ctx.package.package_hash.clone(),
            ern_sha256: hex::encode(sha2::Sha256::digest(&ctx.package.ern_xml)),
        });
        let pmid = Self::fresh_message_id(&inner);
        let state = SubmissionState {
            partner_message_id: pmid.clone(),
            idempotency_key: ctx.idempotency_key.clone(),
            accepted: false,
            live: false,
        };
        if let MockBehavior::Unavailable { remaining } = inner.behavior {
            if remaining > 0 {
                inner.behavior = MockBehavior::Unavailable {
                    remaining: remaining - 1,
                };
                return Ok(SendOutcome::Unavailable {
                    detail: "mock partner unavailable (503)".to_string(),
                });
            }
        }
        match inner.behavior.clone() {
            MockBehavior::Accept
            | MockBehavior::DelayedLive { .. }
            | MockBehavior::Unavailable { .. } => {
                let mut state = state;
                state.accepted = true;
                inner.submissions.insert(pmid.clone(), state);
                inner
                    .by_key
                    .insert(ctx.idempotency_key.clone(), pmid.clone());
                Ok(SendOutcome::Accepted {
                    partner_message_id: pmid,
                })
            }
            MockBehavior::Reject { code } => Ok(SendOutcome::Rejected {
                code,
                message: "mock rejection".to_string(),
            }),
            MockBehavior::Timeout => {
                // The partner may or may not have processed it; the worker
                // must treat the state as unknowable. Record both variants:
                // the submission exists partner-side (worst case for
                // duplicate-send analysis).
                let mut state = state;
                state.accepted = true;
                inner.submissions.insert(pmid.clone(), state);
                inner.by_key.insert(ctx.idempotency_key.clone(), pmid);
                Ok(SendOutcome::Timeout)
            }
            MockBehavior::Unknown => {
                let mut state = state;
                state.accepted = true;
                inner.submissions.insert(pmid.clone(), state);
                inner.by_key.insert(ctx.idempotency_key.clone(), pmid);
                Ok(SendOutcome::Unknown {
                    detail: "mock lost the response".to_string(),
                })
            }
        }
    }

    async fn inquire_submission(&self, partner_message_id: &str) -> Result<InquiryOutcome> {
        self.require(self.capabilities().inquire_submission, "inquire_submission")?;
        let mut inner = self.inner.lock().unwrap();
        // DelayedLive counts every status check — submission inquiry or
        // release-status poll — toward the go-live threshold, modelling a
        // partner whose release flips live after N checks on either endpoint.
        if matches!(inner.behavior, MockBehavior::DelayedLive { .. }) {
            advance_live_poll(&mut inner, partner_message_id);
        }
        match inner.submissions.get(partner_message_id) {
            Some(s) if s.live => Ok(InquiryOutcome::Live {
                partner_release_id: format!("mock-rel-{partner_message_id}"),
            }),
            Some(s) if s.accepted => Ok(InquiryOutcome::Accepted {
                partner_message_id: partner_message_id.to_string(),
            }),
            Some(_) => Ok(InquiryOutcome::Rejected {
                code: "MOCK_REJECTED".to_string(),
            }),
            None => Ok(InquiryOutcome::StillUnknown),
        }
    }

    async fn parse_ack(&self, payload: &[u8]) -> Result<AckEvent> {
        self.require(self.capabilities().parse_ack, "parse_ack")?;
        let v: Value = serde_json::from_slice(payload).map_err(|_| Error::Invalid)?;
        let event_id = v
            .get("event_id")
            .and_then(Value::as_str)
            .ok_or(Error::Invalid)?
            .to_string();
        match v.get("type").and_then(Value::as_str) {
            Some("accepted") => Ok(AckEvent::Accepted {
                event_id,
                partner_message_id: v
                    .get("partner_message_id")
                    .and_then(Value::as_str)
                    .ok_or(Error::Invalid)?
                    .to_string(),
            }),
            Some("rejected") => Ok(AckEvent::Rejected {
                event_id,
                partner_message_id: v
                    .get("partner_message_id")
                    .and_then(Value::as_str)
                    .ok_or(Error::Invalid)?
                    .to_string(),
                code: v
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("MOCK_REJECT")
                    .to_string(),
            }),
            Some("live") => Ok(AckEvent::Live {
                event_id,
                partner_release_id: v
                    .get("partner_release_id")
                    .and_then(Value::as_str)
                    .ok_or(Error::Invalid)?
                    .to_string(),
            }),
            Some("takedown_confirmed") => Ok(AckEvent::TakedownConfirmed { event_id }),
            _ => Err(Error::Invalid),
        }
    }

    async fn get_release_status(&self, partner_release_id: &str) -> Result<InquiryOutcome> {
        self.require(self.capabilities().get_release_status, "get_release_status")?;
        let mut inner = self.inner.lock().unwrap();
        // partner_release_id = "mock-rel-{pmid}"
        let pmid = partner_release_id
            .strip_prefix("mock-rel-")
            .unwrap_or(partner_release_id);
        advance_live_poll(&mut inner, pmid);
        if inner.submissions.get(pmid).map(|s| s.live).unwrap_or(false) {
            Ok(InquiryOutcome::Live {
                partner_release_id: partner_release_id.to_string(),
            })
        } else {
            Ok(InquiryOutcome::Accepted {
                partner_message_id: pmid.to_string(),
            })
        }
    }

    async fn update_release(&self, ctx: &SendContext, _changes: &Value) -> Result<SendOutcome> {
        self.require(self.capabilities().update_release, "update_release")?;
        let mut inner = self.inner.lock().unwrap();
        inner.received.push(ReceivedSend {
            idempotency_key: format!("{}:update", ctx.idempotency_key),
            attempt_no: ctx.attempt_no,
            package_hash: ctx.package.package_hash.clone(),
            ern_sha256: hex::encode(sha2::Sha256::digest(&ctx.package.ern_xml)),
        });
        Ok(SendOutcome::Accepted {
            partner_message_id: format!("mock-update-{}", Uuid::new_v4().simple()),
        })
    }

    async fn takedown(&self, ctx: &SendContext) -> Result<SendOutcome> {
        self.require(self.capabilities().takedown, "takedown")?;
        let mut inner = self.inner.lock().unwrap();
        inner.received.push(ReceivedSend {
            idempotency_key: format!("{}:takedown", ctx.idempotency_key),
            attempt_no: ctx.attempt_no,
            package_hash: ctx.package.package_hash.clone(),
            ern_sha256: hex::encode(sha2::Sha256::digest(&ctx.package.ern_xml)),
        });
        // Mark every accepted submission taken down partner-side.
        for s in inner.submissions.values_mut() {
            if s.idempotency_key
                .starts_with(&ctx.idempotency_key[..ctx.idempotency_key.rfind(':').unwrap_or(0)])
            {
                s.live = false;
            }
        }
        Ok(SendOutcome::Accepted {
            partner_message_id: format!("mock-takedown-{}", Uuid::new_v4().simple()),
        })
    }

    async fn inquire_by_idempotency(&self, idempotency_key: &str) -> Result<InquiryOutcome> {
        let inner = self.inner.lock().unwrap();
        match inner
            .by_key
            .get(idempotency_key)
            .and_then(|p| inner.submissions.get(p))
        {
            Some(s) if s.accepted => Ok(InquiryOutcome::Accepted {
                partner_message_id: s.partner_message_id.clone(),
            }),
            Some(_) => Ok(InquiryOutcome::Rejected {
                code: "MOCK_REJECTED".to_string(),
            }),
            None => Ok(InquiryOutcome::StillUnknown),
        }
    }
}

#[cfg(test)]
mod spec_tests {
    use super::MockBehavior;

    #[test]
    fn behavior_specs_parse() {
        assert_eq!(
            MockBehavior::from_spec("accept"),
            Some(MockBehavior::Accept)
        );
        assert_eq!(
            MockBehavior::from_spec("unavailable:3"),
            Some(MockBehavior::Unavailable { remaining: 3 })
        );
        assert_eq!(
            MockBehavior::from_spec("reject:BAD_ART"),
            Some(MockBehavior::Reject {
                code: "BAD_ART".into()
            })
        );
        assert_eq!(
            MockBehavior::from_spec("timeout"),
            Some(MockBehavior::Timeout)
        );
        assert_eq!(MockBehavior::from_spec("unavailable:x"), None);
        assert_eq!(MockBehavior::from_spec("explode"), None);
    }
}
