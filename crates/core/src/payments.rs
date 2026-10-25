//! Card payment providers (PG) for add-on services. The PG is not decided
//! yet, so the same flow runs on Toss Payments, PortOne (V2: KG이니시스,
//! NICE, KCP, 카카오페이… behind one console) or Payple, chosen by
//! `PAYMENT_PROVIDER` (`toss` | `portone` | `payple`) with that provider's
//! keys. Without a complete provider, card payment is off and add-ons are
//! paid from the settlement balance only.
//!
//! Each provider gives the browser what its payment window needs
//! ([`Provider::window`]), confirms a returned payment server-side for
//! exactly the expected amount ([`Provider::confirm`]: Toss and Payple
//! approve, PortOne is looked up), and cancels part or all of a payment
//! ([`Provider::cancel`]) when an order is cancelled or rejected.
//! Secrets never leave the server; only the client keys the window script
//! needs are sent. `PAYMENT_FIXTURE_BASE` (development/test only) points
//! every provider at one loopback stand-in.
use crate::config::env_value as env;
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{sync::OnceLock, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Toss,
    PortOne,
    Payple,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Toss => "toss",
            Kind::PortOne => "portone",
            Kind::Payple => "payple",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "toss" | "tosspayments" => Some(Kind::Toss),
            "portone" => Some(Kind::PortOne),
            "payple" => Some(Kind::Payple),
            _ => None,
        }
    }
}

pub struct Provider {
    pub kind: Kind,
    /// Public key the window script uses (Toss client key, PortOne store id,
    /// Payple client key).
    client_key: String,
    /// PortOne channel key (which PG behind PortOne).
    channel_key: String,
    secret: String,
    /// Payple partner id (cst_id) and refund key.
    cst_id: String,
    refund_key: String,
    /// API base for each call; the fixture replaces all of them.
    api: String,
    /// Payple: the demo or live window/auth host.
    payple_host: String,
    test: bool,
}

fn fixture() -> Option<String> {
    let base = env("PAYMENT_FIXTURE_BASE")?;
    let local = crate::external_http::local_http_enabled(true);
    crate::external_http::validate_url(&base, local)
        .ok()
        .map(|_| base.trim_end_matches('/').to_owned())
}

/// The configured provider, when its keys are complete.
pub fn provider() -> Option<Provider> {
    let kind = Kind::parse(&env("PAYMENT_PROVIDER")?)?;
    let fx = fixture();
    let p = match kind {
        Kind::Toss => Provider {
            kind,
            client_key: env("TOSS_CLIENT_KEY")?,
            channel_key: String::new(),
            secret: env("TOSS_SECRET_KEY")?,
            cst_id: String::new(),
            refund_key: String::new(),
            api: fx
                .clone()
                .map(|b| format!("{b}/toss"))
                .unwrap_or_else(|| "https://api.tosspayments.com".into()),
            payple_host: String::new(),
            test: false,
        },
        Kind::PortOne => Provider {
            kind,
            client_key: env("PORTONE_STORE_ID")?,
            channel_key: env("PORTONE_CHANNEL_KEY")?,
            secret: env("PORTONE_API_SECRET")?,
            cst_id: String::new(),
            refund_key: String::new(),
            api: fx
                .clone()
                .map(|b| format!("{b}/portone"))
                .unwrap_or_else(|| "https://api.portone.io".into()),
            payple_host: String::new(),
            test: false,
        },
        Kind::Payple => {
            let test = env("PAYPLE_ENV").as_deref() != Some("live");
            Provider {
                kind,
                client_key: env("PAYPLE_CLIENT_KEY")?,
                channel_key: String::new(),
                secret: env("PAYPLE_CUST_KEY")?,
                cst_id: env("PAYPLE_CST_ID")?,
                refund_key: env("PAYPLE_REFUND_KEY")?,
                api: fx
                    .clone()
                    .map(|b| format!("{b}/payple-api"))
                    .unwrap_or_else(|| {
                        if test {
                            "https://demo-api-v2.payple.kr"
                        } else {
                            "https://api-v2.payple.kr"
                        }
                        .into()
                    }),
                payple_host: fx.map(|b| format!("{b}/payple")).unwrap_or_else(|| {
                    if test {
                        "https://democpay.payple.kr"
                    } else {
                        "https://cpay.payple.kr"
                    }
                    .into()
                }),
                test,
            }
        }
    };
    Some(p)
}

fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        crate::external_http::client_builder(fixture().is_some())
            .timeout(Duration::from_secs(20))
            .build()
            .expect("HTTP client")
    })
}

fn failed() -> Error {
    Error::PolicyGate("ADDON_CARD_FAILED")
}

/// One JSON call; non-2xx is a failure (logged with the provider's code,
/// never a secret).
async fn call(
    method: reqwest::Method,
    url: &str,
    headers: &[(&str, String)],
    body: Option<Value>,
) -> Result<Value> {
    let mut r = http().request(method, url);
    for (k, v) in headers {
        r = r.header(*k, v);
    }
    if let Some(b) = body {
        r = r
            .header("content-type", "application/json")
            .body(b.to_string());
    }
    let r = r.send().await.map_err(|e| {
        tracing::warn!(error = %e, "payment provider request failed");
        failed()
    })?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        tracing::warn!(%status, code = %v["code"], "payment provider refused");
        return Err(failed());
    }
    Ok(v)
}

/// What a confirmed payment left us to cancel it later.
#[derive(Debug, Default, Clone)]
pub struct Paid {
    /// Toss paymentKey, PortOne paymentId, Payple order id.
    pub key: String,
    /// Payple: the payment date (YYYYMMDD) its cancel needs.
    pub date: String,
}

impl Provider {
    pub fn card_label(&self) -> &'static str {
        match self.kind {
            Kind::Toss => "토스페이먼츠",
            Kind::PortOne => "카드·간편결제",
            Kind::Payple => "페이플",
        }
    }

    /// Browser window parameters for one payment.
    pub fn window(
        &self,
        order_id: &str,
        amount: i64,
        order_name: &str,
        customer: &str,
        origin: &str,
    ) -> Value {
        match self.kind {
            Kind::Toss => json!({
                "provider": "toss", "client_key": self.client_key, "customer_key": customer,
                "order_id": order_id, "amount": amount, "order_name": order_name,
            }),
            Kind::PortOne => json!({
                "provider": "portone", "store_id": self.client_key, "channel_key": self.channel_key,
                "pg_payment_id": order_id, "amount": amount, "order_name": order_name,
            }),
            Kind::Payple => json!({
                "provider": "payple", "client_key": self.client_key,
                "script": format!("{}/js/v1/payment.js", self.payple_host),
                "order_id": order_id, "amount": amount, "order_name": payple_text(order_name),
                // Relative: a layer on PC and the SPA callback receives the result.
                "result_url": "/api/payments/payple/return",
                "test": self.test, "referer": origin,
            }),
        }
    }

    /// Confirm the payment the window returned. `input` is what the browser
    /// relays (Toss: paymentKey; PortOne: nothing beyond the id; Payple: the
    /// certification keys). Approves for exactly `amount` or fails.
    pub async fn confirm(
        &self,
        order_id: &str,
        amount: i64,
        input: &Value,
        origin: &str,
    ) -> Result<Paid> {
        let text = |k: &str| {
            input
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        match self.kind {
            Kind::Toss => {
                let key = text("payment_key");
                if key.is_empty() || key.len() > 200 {
                    return Err(Error::Invalid);
                }
                let v = call(
                    reqwest::Method::POST,
                    &format!("{}/v1/payments/confirm", self.api),
                    &[
                        ("authorization", self.basic()),
                        ("idempotency-key", format!("confirm-{order_id}")),
                    ],
                    Some(json!({"paymentKey": key, "orderId": order_id, "amount": amount})),
                )
                .await?;
                if v["status"] != "DONE" || v["totalAmount"].as_i64() != Some(amount) {
                    return Err(failed());
                }
                Ok(Paid {
                    key,
                    date: String::new(),
                })
            }
            Kind::PortOne => {
                // The id is ours; the payment is looked up, never trusted from the browser.
                let v = call(
                    reqwest::Method::GET,
                    &format!("{}/payments/{}", self.api, enc(order_id)),
                    &[("authorization", format!("PortOne {}", self.secret))],
                    None,
                )
                .await?;
                if v["status"] != "PAID"
                    || v["amount"]["total"].as_i64() != Some(amount)
                    || v["currency"] != "KRW"
                {
                    return Err(failed());
                }
                Ok(Paid {
                    key: order_id.to_string(),
                    date: String::new(),
                })
            }
            Kind::Payple => {
                let (auth, req) = (text("auth_key"), text("req_key"));
                if auth.is_empty() || req.is_empty() || auth.len() > 500 || req.len() > 255 {
                    return Err(Error::Invalid);
                }
                // The certification must be for this order and amount before approving.
                if text("order_id") != order_id || text("total").parse::<i64>().ok() != Some(amount)
                {
                    return Err(Error::PolicyGate("ADDON_CARD_MISMATCH"));
                }
                let v = call(
                    reqwest::Method::POST,
                    &format!("{}/api/v1/payments/cards/approval/confirm", self.api),
                    &[("referer", origin.to_string()), ("cache-control", "no-cache".into())],
                    Some(json!({"PCD_CST_ID": self.cst_id, "PCD_CUST_KEY": self.secret, "PCD_AUTH_KEY": auth, "PCD_PAY_REQKEY": req})),
                )
                .await?;
                if v["PCD_PAY_RST"] != "success"
                    || v["PCD_PAY_OID"] != order_id
                    || v["PCD_PAY_TOTAL"]
                        .as_str()
                        .and_then(|t| t.parse::<i64>().ok())
                        != Some(amount)
                {
                    tracing::warn!(code = %v["PCD_PAY_CODE"], "payple approval refused");
                    return Err(failed());
                }
                let date: String = v["PCD_PAY_TIME"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .filter(char::is_ascii_digit)
                    .take(8)
                    .collect();
                Ok(Paid {
                    key: order_id.to_string(),
                    date,
                })
            }
        }
    }

    /// Cancel `amount` of a payment (the whole payment or one order's part).
    pub async fn cancel(
        &self,
        paid: &Paid,
        amount: i64,
        reason: &str,
        idem: &str,
        origin: &str,
    ) -> Result<()> {
        let reason = if reason.trim().is_empty() {
            "부가서비스 취소"
        } else {
            reason.trim()
        };
        match self.kind {
            Kind::Toss => {
                call(
                    reqwest::Method::POST,
                    &format!("{}/v1/payments/{}/cancel", self.api, enc(&paid.key)),
                    &[
                        ("authorization", self.basic()),
                        ("idempotency-key", idem.to_string()),
                    ],
                    Some(json!({"cancelReason": reason, "cancelAmount": amount})),
                )
                .await?;
            }
            Kind::PortOne => {
                call(
                    reqwest::Method::POST,
                    &format!("{}/payments/{}/cancel", self.api, enc(&paid.key)),
                    &[("authorization", format!("PortOne {}", self.secret))],
                    Some(json!({"reason": reason, "amount": amount})),
                )
                .await?;
            }
            Kind::Payple => {
                // Partner auth for a cancel, then the cancel with the refund key.
                let auth = call(
                    reqwest::Method::POST,
                    &format!("{}/php/auth.php", self.payple_host),
                    &[("referer", origin.to_string()), ("cache-control", "no-cache".into())],
                    Some(json!({"cst_id": self.cst_id, "custKey": self.secret, "PCD_PAYCANCEL_FLAG": "Y"})),
                )
                .await?;
                if auth["result"] != "success" {
                    return Err(failed());
                }
                let v = call(
                    reqwest::Method::POST,
                    &format!("{}/php/account/api/cPayCAct.php", self.payple_host),
                    &[("referer", origin.to_string()), ("cache-control", "no-cache".into())],
                    Some(json!({
                        "PCD_CST_ID": auth["cst_id"], "PCD_CUST_KEY": auth["custKey"], "PCD_AUTH_KEY": auth["AuthKey"],
                        "PCD_REFUND_KEY": self.refund_key, "PCD_PAYCANCEL_FLAG": "Y",
                        "PCD_PAY_OID": paid.key, "PCD_PAY_DATE": paid.date, "PCD_REFUND_TOTAL": amount.to_string(),
                    })),
                )
                .await?;
                if v["PCD_PAY_RST"] != "success" {
                    tracing::warn!(code = %v["PCD_PAY_CODE"], "payple cancel refused");
                    return Err(failed());
                }
            }
        }
        Ok(())
    }

    fn basic(&self) -> String {
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{}:", self.secret))
        )
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Payple refuses some characters in goods names; keep letters, digits,
/// spaces and the allowed marks.
fn payple_text(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || " `!@#$%^*()_=-[]{};:./?·".contains(*c))
        .take(120)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payple_goods_drop_refused_marks() {
        assert_eq!(
            payple_text("뮤직비디오 배급 · 여름의 끝 <MV> & \"live\""),
            "뮤직비디오 배급 · 여름의 끝 MV  live"
        );
    }
}
