//! 동의 이력·서명자 기록 (migration 0127).
//!
//! The payout-account agreements were checked only in the browser. The
//! request now names every required item, and the server keeps an
//! append-only row per acceptance (`portal.consent_records`), tied to the
//! account it was given for by a hash (never the account number). Signup
//! consents are kept by `auth::record_acceptance` (identity.terms_acceptances).
use crate::error::{Error, Result};
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;

/// The settlement terms the payout-account agreements belong to. Keep equal
/// to Studio `LEGAL_DOCS.settlement.version` (web/studio/app/src/lib/terms.ts).
pub const SETTLEMENT_TERMS_VERSION: &str = "AUD-SETTLE 2.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    PayoutAccount,
}

impl Context {
    fn code(self) -> &'static str {
        match self {
            Context::PayoutAccount => "PAYOUT_ACCOUNT",
        }
    }

    /// (item id, document version).
    pub fn required(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Context::PayoutAccount => &[
                ("settlement_terms", SETTLEMENT_TERMS_VERSION),
                ("privacy", SETTLEMENT_TERMS_VERSION),
                ("financial_info", SETTLEMENT_TERMS_VERSION),
                ("payee_accuracy", SETTLEMENT_TERMS_VERSION),
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

/// The signer as the server knows them: the account, and whether (how, when)
/// its real name was verified. The verified identity is referenced by its CI
/// hash; names and e-mail stay sealed in their own tables (0102).
pub async fn signer_identity(c: &mut PgConnection, user: Uuid) -> Result<Value> {
    type Row = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    );
    let (provider, method, ci_hash, at): Row = sqlx::query_as(
        "SELECT i.provider, i.method, i.ci_hash, i.verified_at
           FROM identity.users u LEFT JOIN portal.account_identities i ON i.user_id=u.id
          WHERE u.id=$1",
    )
    .bind(user)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(json!({
        "user_id": user, "verified": ci_hash.is_some(), "provider": provider,
        "method": method, "ci_hash": ci_hash, "verified_at": at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_required_item_and_nothing_else() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let all = s(&[
            "settlement_terms",
            "privacy",
            "financial_info",
            "payee_accuracy",
        ]);
        let items = check(Context::PayoutAccount, &all).unwrap();
        assert_eq!(items[0]["version"], SETTLEMENT_TERMS_VERSION);
        assert!(check(Context::PayoutAccount, &all[..3]).is_err());
        let mut extra = all.clone();
        extra.push("marketing".into());
        assert!(matches!(
            check(Context::PayoutAccount, &extra),
            Err(Error::InvalidCode("CONSENT_UNKNOWN"))
        ));
    }
}
