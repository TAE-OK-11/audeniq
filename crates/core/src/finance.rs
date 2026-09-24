//! F7 finance ledger core (BLUEPRINT §8, INV-09), partner-independent part.
//!
//! Double-entry ledger with immutable evidence rows: [`post_transaction`]
//! enforces one currency per transaction and Σdebit = Σcredit in NUMERIC
//! (rust_decimal) before anything is written; [`reverse_transaction`] is the
//! only correction path (offsetting entry, never UPDATE/DELETE).
//!
//! Deliberately NOT here (F6, partner-dependent): report parsers and the
//! auto-matcher. Anything posted with an ambiguous match (`MANUAL` /
//! `UNMATCHED`) is rejected by [`post_transaction`] per INV-09.
//!
//! Policy notes from BLUEPRINT §8, enforced below:
//! - amounts are NUMERIC; the applied FX rate, fee policy, contract and tax
//!   rule versions are pinned on each transaction.
//! - split snapshots are append-only: a contract change creates a new row
//!   with a later `valid_from`; past windows are never recomputed.
//! - payouts are manual by default (`auto_payout_enabled=false`), carry a
//!   unique idempotency key, and an unclear bank response stays
//!   `SUBMITTED_UNKNOWN` (never auto-retried).
//! - finance holds are scoped (ISRC/RELEASE/PARTY) and never blanket-expanded.

use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::str::FromStr;
use uuid::Uuid;

/// Finance rule version pinned on checks written by this module.
pub const FINANCE_RULE_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySide {
    Debit,
    Credit,
}

impl EntrySide {
    fn as_str(self) -> &'static str {
        match self {
            EntrySide::Debit => "DEBIT",
            EntrySide::Credit => "CREDIT",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LedgerEntryInput<'a> {
    pub account: &'a str,
    pub side: EntrySide,
    pub amount: Decimal,
    /// Defaults to the transaction currency. A different currency is
    /// rejected: one transaction, one currency (BLUEPRINT §8).
    pub currency: Option<&'a str>,
    pub party_id: Option<Uuid>,
    pub isrc: Option<&'a str>,
    pub split_snapshot_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct PostTransaction<'a> {
    pub transaction_code: &'a str,
    pub currency: &'a str,
    pub entries: Vec<LedgerEntryInput<'a>>,
    /// `AUTO` only. `MANUAL`/`UNMATCHED` (or anything else non-empty) is
    /// rejected: ambiguous matches never reach the ledger (INV-09).
    pub match_status: Option<&'a str>,
    pub fx_rate: Option<Decimal>,
    pub fee_policy_version: Option<&'a str>,
    pub contract_version: Option<&'a str>,
    pub tax_rule_version: Option<&'a str>,
    pub source_ref: Value,
    pub description: &'a str,
    pub created_by: Option<Uuid>,
}

fn valid_currency(c: &str) -> bool {
    c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase())
}

/// Map a Postgres unique-violation into a policy gate; anything else stays a DB error.
fn map_unique_violation(e: sqlx::Error, code: &'static str) -> Error {
    if matches!(&e, sqlx::Error::Database(d) if d.code().as_deref() == Some("23505")) {
        return Error::PolicyGate(code);
    }
    Error::Database(e)
}

/// Post a balanced double-entry transaction. Returns the transaction id.
pub async fn post_transaction(pool: &PgPool, org: Uuid, t: PostTransaction<'_>) -> Result<Uuid> {
    if t.transaction_code.trim().is_empty() {
        return Err(Error::PolicyGate("TRANSACTION_CODE_REQUIRED"));
    }
    if !valid_currency(t.currency) {
        return Err(Error::PolicyGate("INVALID_CURRENCY"));
    }
    if t.entries.is_empty() {
        return Err(Error::PolicyGate("EMPTY_TRANSACTION"));
    }
    if matches!(t.match_status, Some(ms) if ms != "AUTO") {
        return Err(Error::PolicyGate("AMBIGUOUS_MATCH_NO_POST"));
    }
    if matches!(t.fx_rate, Some(fx) if fx <= Decimal::ZERO) {
        return Err(Error::PolicyGate("INVALID_FX_RATE"));
    }
    let mut debit = Decimal::ZERO;
    let mut credit = Decimal::ZERO;
    let mut saw_debit = false;
    let mut saw_credit = false;
    for e in &t.entries {
        if e.account.trim().is_empty() {
            return Err(Error::PolicyGate("ACCOUNT_REQUIRED"));
        }
        if e.amount < Decimal::ZERO {
            return Err(Error::PolicyGate("NEGATIVE_AMOUNT"));
        }
        let ec = e.currency.unwrap_or(t.currency);
        if !valid_currency(ec) {
            return Err(Error::PolicyGate("INVALID_CURRENCY"));
        }
        if ec != t.currency {
            return Err(Error::PolicyGate("MIXED_CURRENCY"));
        }
        match e.side {
            EntrySide::Debit => {
                debit += e.amount;
                saw_debit = true;
            }
            EntrySide::Credit => {
                credit += e.amount;
                saw_credit = true;
            }
        }
    }
    if !saw_debit || !saw_credit {
        return Err(Error::PolicyGate("UNBALANCED_TRANSACTION"));
    }
    if debit != credit {
        return Err(Error::PolicyGate("UNBALANCED_TRANSACTION"));
    }

    let mut tx = pool.begin().await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO finance.ledger_transactions(id,org_id,transaction_code,currency,fx_rate,fee_policy_version,contract_version,tax_rule_version,source_ref,description,created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id)
    .bind(org)
    .bind(t.transaction_code)
    .bind(t.currency)
    .bind(t.fx_rate)
    .bind(t.fee_policy_version)
    .bind(t.contract_version)
    .bind(t.tax_rule_version)
    .bind(&t.source_ref)
    .bind(t.description)
    .bind(t.created_by)
    .execute(&mut *tx)
    .await?;
    for e in &t.entries {
        sqlx::query(
            "INSERT INTO finance.ledger_entries(id,org_id,transaction_id,account,side,amount,currency,party_id,isrc,split_snapshot_id)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(id)
        .bind(e.account)
        .bind(e.side.as_str())
        .bind(e.amount)
        .bind(e.currency.unwrap_or(t.currency))
        .bind(e.party_id)
        .bind(e.isrc)
        .bind(e.split_snapshot_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// Reverse a posted transaction with an offsetting entry. The original row is
/// never mutated (immutable trigger); the reversal references it via
/// `reversal_of`. Returns the new transaction id.
pub async fn reverse_transaction(
    pool: &PgPool,
    org: Uuid,
    actor: Option<Uuid>,
    transaction_id: Uuid,
    reason: &str,
) -> Result<Uuid> {
    if reason.trim().is_empty() {
        return Err(Error::PolicyGate("REVERSAL_REASON_REQUIRED"));
    }
    let already: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM finance.ledger_transactions WHERE org_id=$1 AND reversal_of=$2)",
    )
    .bind(org)
    .bind(transaction_id)
    .fetch_one(pool)
    .await?;
    if already {
        return Err(Error::PolicyGate("ALREADY_REVERSED"));
    }
    type TxHead = (
        String,
        String,
        Option<Decimal>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let head: Option<TxHead> =
        sqlx::query_as(
            "SELECT transaction_code,currency,fx_rate,fee_policy_version,contract_version,tax_rule_version
             FROM finance.ledger_transactions WHERE org_id=$1 AND id=$2",
        )
        .bind(org)
        .bind(transaction_id)
        .fetch_optional(pool)
        .await?;
    let (code, currency, fx_rate, fee_v, contract_v, tax_v) = head.ok_or(Error::NotFound)?;
    type EntryRow = (
        String,
        String,
        Decimal,
        Option<Uuid>,
        Option<String>,
        Option<Uuid>,
    );
    let rows: Vec<EntryRow> = sqlx::query_as(
        "SELECT account,side,amount,party_id,isrc,split_snapshot_id
             FROM finance.ledger_entries WHERE org_id=$1 AND transaction_id=$2",
    )
    .bind(org)
    .bind(transaction_id)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Err(Error::PolicyGate("EMPTY_TRANSACTION"));
    }

    let mut tx = pool.begin().await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO finance.ledger_transactions(id,org_id,transaction_code,currency,fx_rate,fee_policy_version,contract_version,tax_rule_version,reversal_of,source_ref,description,created_by)
         VALUES($1,$2,'REVERSAL',$3,$4,$5,$6,$7,$8,'{}',$9,$10)",
    )
    .bind(id)
    .bind(org)
    .bind(&currency)
    .bind(fx_rate)
    .bind(fee_v)
    .bind(contract_v)
    .bind(tax_v)
    .bind(transaction_id)
    .bind(format!("reversal of {transaction_id} ({code}): {reason}"))
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    for (account, side, amount, party_id, isrc, split_id) in &rows {
        let flipped = if side == "DEBIT" { "CREDIT" } else { "DEBIT" };
        sqlx::query(
            "INSERT INTO finance.ledger_entries(id,org_id,transaction_id,account,side,amount,currency,party_id,isrc,split_snapshot_id)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(id)
        .bind(account)
        .bind(flipped)
        .bind(amount)
        .bind(&currency)
        .bind(party_id)
        .bind(isrc)
        .bind(split_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// One recipient line of a commercial split snapshot. Shares are basis points;
/// the total must be exactly 10000.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitLine {
    pub party_id: Uuid,
    pub party_name: String,
    pub share_bps: i64,
}

/// Record a new commercial split snapshot. Append-only: a contract change
/// creates a new row with a later `valid_from`; past windows are never
/// recomputed or rewritten.
pub async fn apply_split_snapshot(
    pool: &PgPool,
    org: Uuid,
    release_id: Uuid,
    valid_from: DateTime<Utc>,
    lines: &[SplitLine],
    contract_version: &str,
    source_ref: Value,
) -> Result<Uuid> {
    if lines.is_empty() {
        return Err(Error::PolicyGate("SPLIT_LINES_REQUIRED"));
    }
    if contract_version.trim().is_empty() {
        return Err(Error::PolicyGate("CONTRACT_VERSION_REQUIRED"));
    }
    let total: i64 = lines.iter().map(|l| l.share_bps).sum();
    if total != 10_000 {
        return Err(Error::PolicyGate("SPLIT_SHARES_MUST_SUM_10000"));
    }
    if lines.iter().any(|l| l.share_bps < 0) {
        return Err(Error::PolicyGate("NEGATIVE_SPLIT_SHARE"));
    }
    let body = serde_json::to_value(lines).map_err(|_| Error::Invalid)?;
    let id = Uuid::new_v4();
    let r = sqlx::query(
        "INSERT INTO finance.commercial_split_snapshots(id,org_id,release_id,valid_from,lines,contract_version,source_ref)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(org)
    .bind(release_id)
    .bind(valid_from)
    .bind(&body)
    .bind(contract_version)
    .bind(&source_ref)
    .execute(pool)
    .await
    .map_err(|e| map_unique_violation(e, "OVERLAPPING_SPLIT_WINDOW"))?;
    debug_assert_eq!(r.rows_affected(), 1);
    Ok(id)
}

/// The split snapshot effective at `at`: the row with the greatest
/// `valid_from <= at`. Returns None when no snapshot covers `at`.
pub async fn effective_split(
    pool: &PgPool,
    org: Uuid,
    release_id: Uuid,
    at: DateTime<Utc>,
) -> Result<Option<(Uuid, Vec<SplitLine>, String)>> {
    let row: Option<(Uuid, Value, String)> = sqlx::query_as(
        "SELECT id,lines,contract_version FROM finance.commercial_split_snapshots
         WHERE org_id=$1 AND release_id=$2 AND valid_from <= $3
         ORDER BY valid_from DESC LIMIT 1",
    )
    .bind(org)
    .bind(release_id)
    .bind(at)
    .fetch_optional(pool)
    .await?;
    match row {
        None => Ok(None),
        Some((id, lines, cv)) => {
            let parsed: Vec<SplitLine> =
                serde_json::from_value(lines).map_err(|_| Error::Invalid)?;
            Ok(Some((id, parsed, cv)))
        }
    }
}

#[derive(Debug, Clone)]
pub struct PayoutOrderInput<'a> {
    pub idempotency_key: &'a str,
    pub payee_party_id: Uuid,
    pub amount: Decimal,
    pub currency: &'a str,
    pub created_by: Option<Uuid>,
}

/// Create a payout order (manual approval by default). The idempotency key
/// makes retries safe: an existing key returns the existing order id instead
/// of creating a duplicate payout.
pub async fn create_payout_order(
    pool: &PgPool,
    org: Uuid,
    p: PayoutOrderInput<'_>,
) -> Result<(Uuid, bool)> {
    if p.idempotency_key.trim().is_empty() {
        return Err(Error::PolicyGate("IDEMPOTENCY_KEY_REQUIRED"));
    }
    if p.amount <= Decimal::ZERO {
        return Err(Error::PolicyGate("INVALID_PAYOUT_AMOUNT"));
    }
    if !valid_currency(p.currency) {
        return Err(Error::PolicyGate("INVALID_CURRENCY"));
    }
    if !is_payable(pool, org, "PARTY", &p.payee_party_id.to_string()).await? {
        return Err(Error::PolicyGate("PAYEE_ON_HOLD"));
    }
    if let Some(existing) = existing_payout(pool, org, p.idempotency_key).await? {
        return Ok((existing, false));
    }
    let id = Uuid::new_v4();
    let r = sqlx::query(
        "INSERT INTO finance.payout_orders(id,org_id,idempotency_key,payee_party_id,amount,currency,created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(org)
    .bind(p.idempotency_key)
    .bind(p.payee_party_id)
    .bind(p.amount)
    .bind(p.currency)
    .bind(p.created_by)
    .execute(pool)
    .await;
    let won_race = match r {
        Ok(_) => true,
        Err(e) => {
            let dup =
                matches!(&e, sqlx::Error::Database(d) if d.code().as_deref() == Some("23505"));
            if !dup {
                return Err(Error::Database(e));
            }
            false
        }
    };
    if won_race {
        return Ok((id, true));
    }
    // Lost a race with another creator; return the winner.
    let existing = existing_payout(pool, org, p.idempotency_key)
        .await?
        .ok_or(Error::Conflict)?;
    Ok((existing, false))
}

async fn existing_payout(pool: &PgPool, org: Uuid, key: &str) -> Result<Option<Uuid>> {
    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM finance.payout_orders WHERE org_id=$1 AND idempotency_key=$2",
    )
    .bind(org)
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

/// Manual approval step: PENDING_APPROVAL -> APPROVED.
pub async fn approve_payout_order(
    pool: &PgPool,
    org: Uuid,
    approver: Uuid,
    order_id: Uuid,
) -> Result<()> {
    let n = sqlx::query(
        "UPDATE finance.payout_orders SET status='APPROVED',approved_by=$3,approved_at=now()
         WHERE org_id=$1 AND id=$2 AND status='PENDING_APPROVAL'",
    )
    .bind(org)
    .bind(order_id)
    .bind(approver)
    .execute(pool)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("INVALID_PAYOUT_STATUS_TRANSITION"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankOutcome {
    /// Money moved and confirmed.
    Settled,
    /// Bank rejected the transfer.
    Failed,
    /// Money returned after submission.
    Returned,
    /// Bank response unclear: stays put, never auto-retried.
    Unknown,
}

impl BankOutcome {
    fn status(self) -> &'static str {
        match self {
            BankOutcome::Settled => "SETTLED",
            BankOutcome::Failed => "FAILED",
            BankOutcome::Returned => "RETURNED",
            BankOutcome::Unknown => "SUBMITTED_UNKNOWN",
        }
    }
}

/// Record the bank's result for a submitted payout. An unclear response is
/// stored as SUBMITTED_UNKNOWN; reconciliation happens out-of-band and the
/// order is never auto-resubmitted by this function.
pub async fn record_bank_result(
    pool: &PgPool,
    org: Uuid,
    executor: Uuid,
    order_id: Uuid,
    bank_transaction_id: Option<&str>,
    outcome: BankOutcome,
    bank_result: Value,
) -> Result<()> {
    let n = sqlx::query(
        "UPDATE finance.payout_orders
         SET status=$3,executed_by=$4,bank_transaction_id=COALESCE($5,bank_transaction_id),bank_result=$6
         WHERE org_id=$1 AND id=$2 AND status IN ('APPROVED','SUBMITTED','SUBMITTED_UNKNOWN')",
    )
    .bind(org)
    .bind(order_id)
    .bind(outcome.status())
    .bind(executor)
    .bind(bank_transaction_id)
    .bind(&bank_result)
    .execute(pool)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("INVALID_PAYOUT_STATUS_TRANSITION"));
    }
    Ok(())
}

/// Mark an approved order as handed to the bank (still awaiting result).
pub async fn mark_payout_submitted(
    pool: &PgPool,
    org: Uuid,
    executor: Uuid,
    order_id: Uuid,
    bank_transaction_id: &str,
) -> Result<()> {
    let n = sqlx::query(
        "UPDATE finance.payout_orders SET status='SUBMITTED',executed_by=$3,bank_transaction_id=$4
         WHERE org_id=$1 AND id=$2 AND status='APPROVED'",
    )
    .bind(org)
    .bind(order_id)
    .bind(executor)
    .bind(bank_transaction_id)
    .execute(pool)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("INVALID_PAYOUT_STATUS_TRANSITION"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldScope {
    Isrc,
    Release,
    Party,
}

impl HoldScope {
    fn as_str(self) -> &'static str {
        match self {
            HoldScope::Isrc => "ISRC",
            HoldScope::Release => "RELEASE",
            HoldScope::Party => "PARTY",
        }
    }
}

impl FromStr for HoldScope {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "ISRC" => Ok(HoldScope::Isrc),
            "RELEASE" => Ok(HoldScope::Release),
            "PARTY" => Ok(HoldScope::Party),
            _ => Err(Error::Invalid),
        }
    }
}

/// Place a finance hold on a narrowly defined scope. Holds are never
/// blanket-expanded: one hold row covers exactly one scope value.
pub async fn place_hold(
    pool: &PgPool,
    org: Uuid,
    actor: Option<Uuid>,
    scope: HoldScope,
    scope_value: &str,
    reason: &str,
    reason_class: &str,
) -> Result<Uuid> {
    const CLASSES: &[&str] = &[
        "RIGHTS_DISPUTE",
        "TAKEDOWN",
        "CONTRACT_WITHDRAWAL",
        "FRAUD_REVIEW",
        "OTHER",
    ];
    if scope_value.trim().is_empty() {
        return Err(Error::PolicyGate("HOLD_SCOPE_REQUIRED"));
    }
    if reason.trim().is_empty() {
        return Err(Error::PolicyGate("HOLD_REASON_REQUIRED"));
    }
    if !CLASSES.contains(&reason_class) {
        return Err(Error::PolicyGate("INVALID_HOLD_REASON_CLASS"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO finance.finance_holds(id,org_id,scope_type,scope_value,reason,reason_class,created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(org)
    .bind(scope.as_str())
    .bind(scope_value)
    .bind(reason)
    .bind(reason_class)
    .bind(actor)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Release a hold. History is kept (active=false), never deleted.
pub async fn release_hold(pool: &PgPool, org: Uuid, actor: Uuid, hold_id: Uuid) -> Result<()> {
    let n = sqlx::query(
        "UPDATE finance.finance_holds SET active=false,released_at=now(),released_by=$3
         WHERE org_id=$1 AND id=$2 AND active",
    )
    .bind(org)
    .bind(hold_id)
    .bind(actor)
    .execute(pool)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    Ok(())
}

/// True when no active hold covers the scope. Held scopes are excluded from
/// every payable list; unrelated releases are never affected.
pub async fn is_payable(
    pool: &PgPool,
    org: Uuid,
    scope_type: &str,
    scope_value: &str,
) -> Result<bool> {
    let held: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM finance.finance_holds WHERE org_id=$1 AND scope_type=$2 AND scope_value=$3 AND active)",
    )
    .bind(org)
    .bind(scope_type)
    .bind(scope_value)
    .fetch_one(pool)
    .await?;
    Ok(!held)
}

/// Sum of a transaction's sides, for balance assertions in tests and audits.
pub async fn transaction_totals(
    pool: &PgPool,
    org: Uuid,
    transaction_id: Uuid,
) -> Result<(Decimal, Decimal)> {
    let row: Option<(Decimal, Decimal)> = sqlx::query_as(
        "SELECT COALESCE(SUM(amount) FILTER (WHERE side='DEBIT'),0), COALESCE(SUM(amount) FILTER (WHERE side='CREDIT'),0)
         FROM finance.ledger_entries WHERE org_id=$1 AND transaction_id=$2",
    )
    .bind(org)
    .bind(transaction_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or((Decimal::ZERO, Decimal::ZERO)))
}

/// Fetch a row for inspection; used by tests and admin views.
pub async fn get_payout_status(pool: &PgPool, org: Uuid, order_id: Uuid) -> Result<String> {
    let s: Option<String> =
        sqlx::query_scalar("SELECT status FROM finance.payout_orders WHERE org_id=$1 AND id=$2")
            .bind(org)
            .bind(order_id)
            .fetch_optional(pool)
            .await?;
    s.ok_or(Error::NotFound)
}
