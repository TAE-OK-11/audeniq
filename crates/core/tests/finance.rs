//! F7 finance ledger core tests (partner-independent): double-entry balance,
//! immutability-by-reversal, split snapshot append-only semantics, payout
//! idempotency, unclear-bank-response handling, and scoped holds.
//! Synthetic fixtures only; real Postgres via #[sqlx::test].
use audeniq_core::{
    database,
    error::Error,
    finance::{
        self, BankOutcome, EntrySide, HoldScope, LedgerEntryInput, PayoutOrderInput,
        PostTransaction, SplitLine,
    },
};
use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use std::str::FromStr;
use uuid::Uuid;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

async fn org(pool: &PgPool) -> Uuid {
    audeniq_core::config::set_dual_approval(true);
    database::MIGRATOR.run(pool).await.unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.orgs(id,name,kind) VALUES($1,'fin-test','LABEL')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}

fn entry<'a>(account: &'a str, side: EntrySide, amount: &str) -> LedgerEntryInput<'a> {
    LedgerEntryInput {
        account,
        side,
        amount: dec(amount),
        currency: None,
        party_id: None,
        isrc: None,
        split_snapshot_id: None,
    }
}

fn balanced<'a>() -> PostTransaction<'a> {
    PostTransaction {
        transaction_code: "ROYALTY_ACCRUAL",
        currency: "KRW",
        entries: vec![
            entry("ROYALTY_RECEIVABLE", EntrySide::Debit, "100000"),
            entry("ROYALTY_PAYABLE", EntrySide::Credit, "100000"),
        ],
        match_status: Some("AUTO"),
        fx_rate: None,
        fee_policy_version: Some("fee-2026-01"),
        contract_version: Some("ctr-7"),
        tax_rule_version: Some("tax-2026-kr"),
        source_ref: json!({"report_line": "synthetic"}),
        description: "synthetic accrual",
        created_by: None,
    }
}

#[sqlx::test]
async fn ledger_happy_path_balanced(pool: PgPool) {
    let org = org(&pool).await;
    let id = finance::post_transaction(&pool, org, balanced())
        .await
        .unwrap();
    let (d, c) = finance::transaction_totals(&pool, org, id).await.unwrap();
    assert_eq!(d, dec("100000"));
    assert_eq!(c, dec("100000"));
    // Policy versions are pinned on the transaction.
    let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT fee_policy_version,contract_version,tax_rule_version FROM finance.ledger_transactions WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0.as_deref(), Some("fee-2026-01"));
    assert_eq!(row.1.as_deref(), Some("ctr-7"));
    assert_eq!(row.2.as_deref(), Some("tax-2026-kr"));
}

#[sqlx::test]
async fn ledger_rejects_unbalanced(pool: PgPool) {
    let org = org(&pool).await;
    let mut t = balanced();
    t.entries[1].amount = dec("99999");
    let e = finance::post_transaction(&pool, org, t).await.unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("UNBALANCED_TRANSACTION")),
        "{e:?}"
    );
    // Nothing was written.
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM finance.ledger_transactions WHERE org_id=$1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn ledger_rejects_negative_amount(pool: PgPool) {
    let org = org(&pool).await;
    let mut t = balanced();
    t.entries[0].amount = dec("-5");
    let e = finance::post_transaction(&pool, org, t).await.unwrap_err();
    assert!(matches!(e, Error::PolicyGate("NEGATIVE_AMOUNT")), "{e:?}");
}

#[sqlx::test]
async fn ledger_rejects_mixed_currency(pool: PgPool) {
    let org = org(&pool).await;
    let mut t = balanced();
    t.entries[1].currency = Some("USD");
    let e = finance::post_transaction(&pool, org, t).await.unwrap_err();
    assert!(matches!(e, Error::PolicyGate("MIXED_CURRENCY")), "{e:?}");
}

#[sqlx::test]
async fn ledger_rejects_ambiguous_match(pool: PgPool) {
    let org = org(&pool).await;
    for status in ["MANUAL", "UNMATCHED"] {
        let mut t = balanced();
        t.match_status = Some(status);
        let e = finance::post_transaction(&pool, org, t).await.unwrap_err();
        assert!(
            matches!(e, Error::PolicyGate("AMBIGUOUS_MATCH_NO_POST")),
            "{status}: {e:?}"
        );
    }
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM finance.ledger_transactions WHERE org_id=$1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 0, "ambiguous matches never reach the ledger (INV-09)");
}

#[sqlx::test]
async fn ledger_reversal_is_offsetting_only(pool: PgPool) {
    let org = org(&pool).await;
    let actor = Uuid::new_v4();
    let id = finance::post_transaction(&pool, org, balanced())
        .await
        .unwrap();
    let rev = finance::reverse_transaction(&pool, org, Some(actor), id, "test correction")
        .await
        .unwrap();
    assert_ne!(id, rev);
    // Original is untouched (still POSTED; immutable trigger forbids edits).
    let status: String =
        sqlx::query_scalar("SELECT status FROM finance.ledger_transactions WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "POSTED");
    // Reversal flips every side and references the original.
    let (d, c) = finance::transaction_totals(&pool, org, rev).await.unwrap();
    assert_eq!((d, c), (dec("100000"), dec("100000")));
    let sides: Vec<String> = sqlx::query_scalar(
        "SELECT side FROM finance.ledger_entries WHERE transaction_id=$1 ORDER BY account",
    )
    .bind(rev)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(sides, vec!["DEBIT".to_string(), "CREDIT".to_string()]);
    let link: Uuid =
        sqlx::query_scalar("SELECT reversal_of FROM finance.ledger_transactions WHERE id=$1")
            .bind(rev)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(link, id);
    // Double reversal is rejected.
    let e = finance::reverse_transaction(&pool, org, Some(actor), id, "again")
        .await
        .unwrap_err();
    assert!(matches!(e, Error::PolicyGate("ALREADY_REVERSED")), "{e:?}");
}

async fn release(pool: &PgPool, org: Uuid) -> Uuid {
    let rel = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'release')")
        .bind(org)
        .bind(rel)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO catalog.releases(id,org_id,title,release_type,status,draft,row_version) VALUES($1,$2,'R','SINGLE','DRAFT','{}',1)",
    )
    .bind(rel)
    .bind(org)
    .execute(pool)
    .await
    .unwrap();
    rel
}

fn lines() -> Vec<SplitLine> {
    vec![
        SplitLine {
            party_id: Uuid::new_v4(),
            party_name: "A".into(),
            share_bps: 7000,
        },
        SplitLine {
            party_id: Uuid::new_v4(),
            party_name: "B".into(),
            share_bps: 3000,
        },
    ]
}

#[sqlx::test]
async fn split_snapshot_is_append_only(pool: PgPool) {
    let org = org(&pool).await;
    let rel = release(&pool, org).await;
    let t1 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let s1 = finance::apply_split_snapshot(&pool, org, rel, t1, &lines(), "ctr-7", json!({}))
        .await
        .unwrap();
    // A contract change creates a new row; the old window is not rewritten.
    let mut v2 = lines();
    v2[0].share_bps = 5000;
    v2[1].share_bps = 5000;
    let s2 = finance::apply_split_snapshot(&pool, org, rel, t2, &v2, "ctr-8", json!({}))
        .await
        .unwrap();
    assert_ne!(s1, s2);
    // Effective at t1 -> v1 (70/30); at t2 -> v2 (50/50). Past is not recomputed.
    let (_, got1, cv1) = finance::effective_split(&pool, org, rel, t1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cv1, "ctr-7");
    assert_eq!(got1[0].share_bps, 7000);
    let mid = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
    let (_, got_mid, _) = finance::effective_split(&pool, org, rel, mid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        got_mid[0].share_bps, 7000,
        "March still uses the January contract"
    );
    let (_, got2, cv2) = finance::effective_split(&pool, org, rel, t2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cv2, "ctr-8");
    assert_eq!(got2[0].share_bps, 5000);
    // Shares must total exactly 10000 bps.
    let mut bad = lines();
    bad[0].share_bps = 6000;
    let e = finance::apply_split_snapshot(&pool, org, rel, t2, &bad, "ctr-9", json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("SPLIT_SHARES_MUST_SUM_10000")),
        "{e:?}"
    );
    // Same valid_from twice is rejected (no silent overwrite).
    let e = finance::apply_split_snapshot(&pool, org, rel, t2, &lines(), "ctr-9", json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("OVERLAPPING_SPLIT_WINDOW")),
        "{e:?}"
    );
}

#[sqlx::test]
async fn payout_order_idempotency(pool: PgPool) {
    let org = org(&pool).await;
    finance::post_transaction(&pool, org, balanced())
        .await
        .unwrap();
    let payee = Uuid::new_v4();
    let p = PayoutOrderInput {
        idempotency_key: "pay-001",
        payee_party_id: payee,
        amount: dec("50000"),
        currency: "KRW",
        created_by: None,
    };
    let (id1, created1) = finance::create_payout_order(&pool, org, p.clone())
        .await
        .unwrap();
    assert!(created1);
    assert_eq!(
        finance::get_payout_status(&pool, org, id1).await.unwrap(),
        "PENDING_APPROVAL"
    );
    // Retry with the same key returns the same order; no duplicate payout.
    let (id2, created2) = finance::create_payout_order(&pool, org, p).await.unwrap();
    assert!(!created2);
    assert_eq!(id1, id2);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM finance.payout_orders WHERE org_id=$1")
        .bind(org)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn payout_unknown_bank_response_is_not_retried(pool: PgPool) {
    let org = org(&pool).await;
    finance::post_transaction(&pool, org, balanced())
        .await
        .unwrap();
    let (id, _) = finance::create_payout_order(
        &pool,
        org,
        PayoutOrderInput {
            idempotency_key: "pay-002",
            payee_party_id: Uuid::new_v4(),
            amount: dec("10000"),
            currency: "KRW",
            created_by: None,
        },
    )
    .await
    .unwrap();
    let approver = Uuid::new_v4();
    finance::approve_payout_order(&pool, org, approver, id)
        .await
        .unwrap();
    finance::mark_payout_submitted(&pool, org, approver, id, "bank-tx-1")
        .await
        .unwrap();
    // Bank response unclear -> stays SUBMITTED_UNKNOWN.
    finance::record_bank_result(
        &pool,
        org,
        approver,
        id,
        Some("bank-tx-1"),
        BankOutcome::Unknown,
        json!({"note": "no confirmation received"}),
    )
    .await
    .unwrap();
    assert_eq!(
        finance::get_payout_status(&pool, org, id).await.unwrap(),
        "SUBMITTED_UNKNOWN"
    );
    // No automatic resubmission path: re-marking as submitted is rejected.
    let e = finance::mark_payout_submitted(&pool, org, approver, id, "bank-tx-2")
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("INVALID_PAYOUT_STATUS_TRANSITION")),
        "{e:?}"
    );
    // Later reconciliation can still settle it once the bank confirms.
    finance::record_bank_result(
        &pool,
        org,
        approver,
        id,
        Some("bank-tx-1"),
        BankOutcome::Settled,
        json!({"confirmed": true}),
    )
    .await
    .unwrap();
    assert_eq!(
        finance::get_payout_status(&pool, org, id).await.unwrap(),
        "SETTLED"
    );
}

#[sqlx::test]
async fn finance_hold_scopes_payout_exclusion(pool: PgPool) {
    let org = org(&pool).await;
    let actor = Uuid::new_v4();
    let isrc = "KR-AAA-26-00001";
    assert!(finance::is_payable(&pool, org, "ISRC", isrc).await.unwrap());
    let hold = finance::place_hold(
        &pool,
        org,
        Some(actor),
        HoldScope::Isrc,
        isrc,
        "rights dispute filed",
        "RIGHTS_DISPUTE",
    )
    .await
    .unwrap();
    assert!(!finance::is_payable(&pool, org, "ISRC", isrc).await.unwrap());
    // Unrelated ISRCs are unaffected: no blanket expansion.
    assert!(
        finance::is_payable(&pool, org, "ISRC", "KR-AAA-26-00002")
            .await
            .unwrap()
    );
    // A payout to a held party is rejected at creation.
    let party = Uuid::new_v4();
    finance::place_hold(
        &pool,
        org,
        Some(actor),
        HoldScope::Party,
        &party.to_string(),
        "fraud review",
        "FRAUD_REVIEW",
    )
    .await
    .unwrap();
    let e = finance::create_payout_order(
        &pool,
        org,
        PayoutOrderInput {
            idempotency_key: "pay-003",
            payee_party_id: party,
            amount: dec("1000"),
            currency: "KRW",
            created_by: None,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(e, Error::PolicyGate("PAYEE_ON_HOLD")), "{e:?}");
    // Releasing the hold restores payability; history is kept.
    finance::release_hold(&pool, org, actor, hold)
        .await
        .unwrap();
    assert!(finance::is_payable(&pool, org, "ISRC", isrc).await.unwrap());
    let active: bool = sqlx::query_scalar("SELECT active FROM finance.finance_holds WHERE id=$1")
        .bind(hold)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!active);
}

#[sqlx::test]
async fn payout_partial_return_is_idempotent_and_restores_only_received_money(pool: PgPool) {
    let org = org(&pool).await;
    let creator = Uuid::new_v4();
    let approver = Uuid::new_v4();
    // The order pays out money actually earned (finance.withdrawable).
    let mut earned = balanced();
    earned.entries = vec![
        entry("ROYALTY_RECEIVABLE", EntrySide::Debit, "2000"),
        entry("ROYALTY_PAYABLE", EntrySide::Credit, "2000"),
    ];
    finance::post_transaction(&pool, org, earned).await.unwrap();
    let (order, _) = finance::create_payout_order(
        &pool,
        org,
        PayoutOrderInput {
            idempotency_key: "return-test",
            payee_party_id: Uuid::new_v4(),
            amount: dec("2000"),
            currency: "KRW",
            created_by: Some(creator),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE finance.payout_orders SET fee_amount=500,net_amount=1500 WHERE id=$1")
        .bind(order)
        .execute(&pool)
        .await
        .unwrap();
    let make =
        |reference: &'static str, amount: &str, refund_fee: bool| finance::PayoutReturnInput {
            bank_reference: reference,
            returned_amount: dec(amount),
            refund_fee,
            evidence: json!({"statement":"synthetic-bank-return"}),
        };
    assert!(
        finance::record_payout_return(&pool, org, approver, order, make("early", "100", false))
            .await
            .is_err()
    );
    finance::approve_payout_order(&pool, org, approver, order)
        .await
        .unwrap();
    finance::mark_payout_submitted(&pool, org, approver, order, "original-bank-transfer")
        .await
        .unwrap();
    assert!(
        finance::record_bank_result(
            &pool,
            org,
            approver,
            order,
            Some("wrong-transfer"),
            BankOutcome::Settled,
            json!({})
        )
        .await
        .is_err()
    );
    finance::record_bank_result(
        &pool,
        org,
        approver,
        order,
        Some("original-bank-transfer"),
        BankOutcome::Settled,
        json!({"confirmed":true}),
    )
    .await
    .unwrap();
    assert!(
        finance::record_bank_result(
            &pool,
            org,
            approver,
            order,
            None,
            BankOutcome::Returned,
            json!({})
        )
        .await
        .is_err()
    );
    let first =
        finance::record_payout_return(&pool, org, approver, order, make("partial", "500", false))
            .await
            .unwrap();
    assert_eq!(
        first,
        finance::record_payout_return(&pool, org, approver, order, make("partial", "500", false))
            .await
            .unwrap()
    );
    assert!(
        finance::record_payout_return(&pool, org, approver, order, make("partial", "501", false))
            .await
            .is_err()
    );
    assert!(
        finance::record_payout_return(&pool, org, approver, order, make("too-much", "1001", false))
            .await
            .is_err()
    );
    assert!(
        finance::record_payout_return(&pool, org, approver, order, make("early-fee", "1", true))
            .await
            .is_err()
    );
    assert_eq!(
        finance::get_payout_status(&pool, org, order).await.unwrap(),
        "SETTLED"
    );
    let net:rust_decimal::Decimal=sqlx::query_scalar("SELECT SUM(CASE WHEN side='CREDIT' THEN amount ELSE -amount END) FROM finance.ledger_entries WHERE org_id=$1 AND account='ROYALTY_PAYABLE'")
        .bind(org).fetch_one(&pool).await.unwrap();
    assert_eq!(net, dec("500"));
    finance::record_payout_return(&pool, org, approver, order, make("final", "1000", true))
        .await
        .unwrap();
    assert_eq!(
        finance::get_payout_status(&pool, org, order).await.unwrap(),
        "RETURNED"
    );
    // Even after reaching RETURNED, duplicate evidence remains a no-op.
    finance::record_payout_return(&pool, org, approver, order, make("final", "1000", true))
        .await
        .unwrap();
    assert!(
        finance::record_payout_return(&pool, org, approver, order, make("another", "1", false))
            .await
            .is_err()
    );
    let net:rust_decimal::Decimal=sqlx::query_scalar("SELECT SUM(CASE WHEN side='CREDIT' THEN amount ELSE -amount END) FROM finance.ledger_entries WHERE org_id=$1 AND account='ROYALTY_PAYABLE'")
        .bind(org).fetch_one(&pool).await.unwrap();
    assert_eq!(net, dec("2000"));
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM finance.payout_returns WHERE payout_order_id=$1")
            .bind(order)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 2);
    assert!(
        sqlx::query("DELETE FROM finance.payout_returns WHERE payout_order_id=$1")
            .bind(order)
            .execute(&pool)
            .await
            .is_err()
    );
}

/// One withdrawable balance (payable − held − other open payouts)
/// gates the order, the approval and the bank submission alike.
#[sqlx::test]
async fn withdrawable_balance_gates_every_payout_step(pool: PgPool) {
    let org = org(&pool).await;
    let order = |key: &'static str, amount: &str| PayoutOrderInput {
        idempotency_key: key,
        payee_party_id: Uuid::nil(),
        amount: dec(amount),
        currency: "KRW",
        created_by: None,
    };
    // Nothing earned: nothing can be ordered.
    let e = finance::create_payout_order(&pool, org, order("w-0", "1000"))
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("PAYOUT_EXCEEDS_UNHELD_BALANCE")),
        "{e:?}"
    );
    finance::post_transaction(&pool, org, balanced())
        .await
        .unwrap(); // 100000 payable
    let (a, _) = finance::create_payout_order(&pool, org, order("w-1", "60000"))
        .await
        .unwrap();
    // The open order reserves its amount: a second one cannot overdraw.
    let e = finance::create_payout_order(&pool, org, order("w-2", "50000"))
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("PAYOUT_EXCEEDS_UNHELD_BALANCE")),
        "{e:?}"
    );
    // A dispute hold after the order stops its approval.
    let hold = finance::place_hold(
        &pool,
        org,
        None,
        HoldScope::Isrc,
        "KRA402600009",
        "dispute",
        "RIGHTS_DISPUTE",
    )
    .await
    .unwrap();
    // Re-label 50000 of the payable as the held track's: reverse-free way is a
    // new balanced pair that debits unlabelled payable and credits labelled.
    let mut move_to_track = balanced();
    move_to_track.transaction_code = "ROYALTY_RECLASS";
    move_to_track.entries = vec![
        entry("ROYALTY_PAYABLE", EntrySide::Debit, "50000"),
        LedgerEntryInput {
            isrc: Some("KRA402600009"),
            ..entry("ROYALTY_PAYABLE", EntrySide::Credit, "50000")
        },
    ];
    finance::post_transaction(&pool, org, move_to_track)
        .await
        .unwrap();
    let approver = Uuid::new_v4();
    let e = finance::approve_payout_order(&pool, org, approver, a)
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("PAYOUT_EXCEEDS_UNHELD_BALANCE")),
        "{e:?}"
    );
    // Released: it approves, and a hold placed after approval stops the submission.
    finance::release_hold(&pool, org, approver, hold)
        .await
        .unwrap();
    finance::approve_payout_order(&pool, org, approver, a)
        .await
        .unwrap();
    finance::place_hold(
        &pool,
        org,
        None,
        HoldScope::Isrc,
        "KRA402600009",
        "dispute again",
        "RIGHTS_DISPUTE",
    )
    .await
    .unwrap();
    let e = finance::mark_payout_submitted(&pool, org, approver, a, "bank-w-1")
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("PAYOUT_EXCEEDS_UNHELD_BALANCE")),
        "{e:?}"
    );
}

async fn withdrawable_now(pool: &PgPool, org: Uuid) -> rust_decimal::Decimal {
    let mut c = pool.acquire().await.unwrap();
    finance::withdrawable(&mut c, org, None).await.unwrap()
}

/// A manual transfer settles only with transfer evidence and by someone
/// other than the approver; a returned order is reissued through approval
/// again, once, to the account registered now.
#[sqlx::test]
async fn manual_payout_needs_evidence_and_reissue_goes_through_approval(pool: PgPool) {
    let org = org(&pool).await;
    finance::post_transaction(&pool, org, balanced())
        .await
        .unwrap(); // 100000 payable
    let (id, _) = finance::create_payout_order(
        &pool,
        org,
        PayoutOrderInput {
            idempotency_key: "man-0001",
            payee_party_id: Uuid::nil(),
            amount: dec("30000"),
            currency: "KRW",
            created_by: None,
        },
    )
    .await
    .unwrap();
    let approver = Uuid::new_v4();
    let executor = Uuid::new_v4();
    finance::approve_payout_order(&pool, org, approver, id)
        .await
        .unwrap();
    let e = finance::record_manual_payout(&pool, org, executor, id, "IBK-20261011-1", json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("PAYOUT_EVIDENCE_REQUIRED")),
        "{e:?}"
    );
    let receipt = json!({"transfer_receipt":"IBK 이체확인증 2026-10-11"});
    let e =
        finance::record_manual_payout(&pool, org, approver, id, "IBK-20261011-1", receipt.clone())
            .await
            .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("PAYOUT_EXECUTOR_MUST_DIFFER")),
        "{e:?}"
    );
    finance::record_manual_payout(&pool, org, executor, id, "IBK-20261011-1", receipt.clone())
        .await
        .unwrap();
    let (status, evidence): (String, Option<serde_json::Value>) =
        sqlx::query_as("SELECT status, manual_evidence FROM finance.payout_orders WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), evidence), ("SETTLED", Some(receipt)));
    assert_eq!(withdrawable_now(&pool, org).await, dec("70000"));

    // Only a returned or failed order is reissued, and it needs an account.
    let e = finance::reissue_payout(&pool, org, executor, id)
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("INVALID_PAYOUT_STATUS_TRANSITION")),
        "{e:?}"
    );
    finance::record_payout_return(
        &pool,
        org,
        executor,
        id,
        finance::PayoutReturnInput {
            bank_reference: "IBK-RET-1",
            returned_amount: dec("30000"),
            refund_fee: false,
            evidence: json!({"statement":"bank returned the transfer"}),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        finance::get_payout_status(&pool, org, id).await.unwrap(),
        "RETURNED"
    );
    assert_eq!(withdrawable_now(&pool, org).await, dec("100000"));
    let e = finance::reissue_payout(&pool, org, executor, id)
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("PAYOUT_ACCOUNT_REQUIRED")),
        "{e:?}"
    );
    let (party, user) = (Uuid::new_v4(), Uuid::new_v4());
    sqlx::query(
        "INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'PERSON','p')",
    )
    .bind(party)
    .bind(org)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO identity.users(id,email,password_hash,party_id) VALUES($1,'pay@x.y','x',$2)",
    )
    .bind(user)
    .bind(party)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO portal.payout_accounts(org_id,payee_type,holder_name,bank_name,account_last4,account_cipher,registered_by) VALUES($1,'INDIVIDUAL','홍길동','하나은행','1234',$2,$3)")
        .bind(org)
        .bind(vec![0u8; 40])
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let new = finance::reissue_payout(&pool, org, executor, id)
        .await
        .unwrap();
    assert_eq!(
        finance::get_payout_status(&pool, org, new).await.unwrap(),
        "PENDING_APPROVAL"
    );
    // The reissued order reserves the amount again, and only once.
    assert_eq!(withdrawable_now(&pool, org).await, dec("70000"));
    let e = finance::reissue_payout(&pool, org, executor, id)
        .await
        .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("PAYOUT_ALREADY_REISSUED")),
        "{e:?}"
    );
}
