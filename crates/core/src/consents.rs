//! 동의 이력 (감사 4번, migration 0094).
//!
//! Studio showed the signup and payout-account agreements but the server
//! never saw them. Each context now has a fixed list of required items and
//! the document version they point to; the request must name every item and
//! the server keeps an append-only row per acceptance.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;

/// The settlement terms the payout-account agreements belong to
/// (Studio `LEGAL_DOCS.settlement.version`).
pub const SETTLEMENT_TERMS_VERSION: &str = "AUD-SETTLE 2.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Signup,
    PayoutAccount,
}

impl Context {
    fn code(self) -> &'static str {
        match self {
            Context::Signup => "SIGNUP",
            Context::PayoutAccount => "PAYOUT_ACCOUNT",
        }
    }

    /// (item id, document version). A `None` version means the text has no
    /// versioned original in this system yet (the privacy policy): the
    /// acceptance is still recorded, with no version claimed.
    pub fn required(self) -> &'static [(&'static str, Option<&'static str>)] {
        match self {
            Context::Signup => &[
                ("terms", Some(crate::agreement::TERMS_VERSION)),
                ("privacy", None),
            ],
            Context::PayoutAccount => &[
                ("settlement_terms", Some(SETTLEMENT_TERMS_VERSION)),
                ("privacy", Some(SETTLEMENT_TERMS_VERSION)),
                ("financial_info", Some(SETTLEMENT_TERMS_VERSION)),
                ("payee_accuracy", Some(SETTLEMENT_TERMS_VERSION)),
            ],
        }
    }
}

/// Every required item must be given; unknown items are refused.
pub fn check(context: Context, given: &[String]) -> Result<Value> {
    let required = context.required();
    if given
        .iter()
        .any(|g| !required.iter().any(|(id, _)| id == g))
    {
        return Err(Error::InvalidCode("CONSENT_UNKNOWN"));
    }
    if required
        .iter()
        .any(|(id, _)| !given.iter().any(|g| g == id))
    {
        return Err(Error::PolicyGate("CONSENT_REQUIRED"));
    }
    Ok(Value::Array(
        required
            .iter()
            .map(|(id, version)| json!({"id": id, "version": version}))
            .collect(),
    ))
}

pub async fn record(
    c: &mut PgConnection,
    user: Uuid,
    org: Option<Uuid>,
    context: Context,
    items: &Value,
    subject_hash: Option<&str>,
    request: Uuid,
) -> Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.consent_records(id,user_id,org_id,context,items,subject_hash,request_id)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(user)
    .bind(org)
    .bind(context.code())
    .bind(items)
    .bind(subject_hash)
    .bind(request)
    .execute(&mut *c)
    .await?;
    Ok(id)
}

/// The signer as the server knows them: the account, and its verified real
/// name when identity verification ran (account::verify_identity).
pub async fn signer_identity(c: &mut PgConnection, user: Uuid) -> Result<Value> {
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT u.email, i.real_name, i.provider, i.verified_at
           FROM identity.users u LEFT JOIN portal.account_identities i ON i.user_id=u.id
          WHERE u.id=$1",
    )
    .bind(user)
    .fetch_optional(&mut *c)
    .await?;
    let (email, name, provider, at) = row.ok_or(Error::NotFound)?;
    Ok(json!({
        "user_id": user, "email": email,
        "verified": name.is_some(), "real_name": name, "provider": provider, "verified_at": at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_required_item_and_nothing_else() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let items = check(Context::Signup, &s(&["terms", "privacy"])).unwrap();
        assert_eq!(items[0]["version"], crate::agreement::TERMS_VERSION);
        assert!(items[1]["version"].is_null());
        assert!(check(Context::Signup, &s(&["terms"])).is_err());
        assert!(check(Context::Signup, &s(&["terms", "privacy", "marketing"])).is_err());
        assert!(
            check(
                Context::PayoutAccount,
                &s(&[
                    "settlement_terms",
                    "privacy",
                    "financial_info",
                    "payee_accuracy"
                ])
            )
            .is_ok()
        );
    }
}
