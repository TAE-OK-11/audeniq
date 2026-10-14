//! Post a royalty report to the books (migration 0119; owner CLI
//! `audeniq-admin royalty post`).
//!
//! Only a report whose every line matched automatically and whose payment
//! was received is posted, once. The money actually received is spread over
//! the lines in proportion to their gross amounts and converted to KRW at the
//! rate the operator gives (1 for a KRW report). Per release, the AUDENIQ fee
//! from its signed distribution agreement is taken (`fee_bps`, of the net
//! receipts), and per track the collaborators' shares
//! ([`crate::splits`]) come out of what is left; the applicant keeps the
//! rest. Collaborators who accepted receive their share in their own books at
//! once; shares not yet answered are held (`SPLIT_HELD`) until they are.
//!
//! Amounts are whole won, rounded down at each step in the artist's favour:
//! the received total (in won, rounded down once per report) is spread over
//! the tracks by largest remainders so no won is lost per track; the fee is
//! rounded down, each collaborator's share is rounded down and the applicant
//! gets the remainder (settlement terms art. 36 ⑤, 37).
//!
//! Books per release (owner org): DEBIT `ROYALTY_RECEIVED` = gross;
//! CREDIT `AUDENIQ_FEE`, `ROYALTY_PAYABLE` (applicant), `SPLIT_HELD`
//! (collaborators). Accepted shares then move through
//! `finance.settle_split_share`.
use crate::{
    error::{Error, Result},
    finance::{self, EntrySide, LedgerEntryInput, PostTransaction},
};
use rust_decimal::{Decimal, RoundingStrategy};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use uuid::Uuid;

/// One collaborator's cut of a track.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub share_id: Uuid,
    pub amount: Decimal,
}

/// Split one track's gross (whole won): fee first, then collaborators, the
/// applicant keeps the rest. Returns (fee, applicant, collaborators).
pub fn split_track(
    gross: Decimal,
    fee_bps: i64,
    shares: &[(Uuid, i64)],
) -> (Decimal, Decimal, Vec<Cut>) {
    let won = |d: Decimal| d.round_dp_with_strategy(0, RoundingStrategy::ToZero);
    let fee = won(gross * Decimal::from(fee_bps) / Decimal::from(10_000));
    let net = gross - fee;
    let cuts: Vec<Cut> = shares
        .iter()
        .map(|(id, bps)| Cut {
            share_id: *id,
            amount: won(net * Decimal::from(*bps) / Decimal::from(10_000)),
        })
        .filter(|c| c.amount > Decimal::ZERO)
        .collect();
    let given: Decimal = cuts.iter().map(|c| c.amount).sum();
    (fee, net - given, cuts)
}

/// Split `total` whole won over `weights` in proportion, largest remainders
/// first, so the parts add up to `total` exactly.
pub fn apportion(total: Decimal, weights: &[Decimal]) -> Vec<Decimal> {
    let sum: Decimal = weights.iter().copied().sum();
    if sum <= Decimal::ZERO {
        return vec![Decimal::ZERO; weights.len()];
    }
    let exact: Vec<Decimal> = weights.iter().map(|w| total * *w / sum).collect();
    let mut parts: Vec<Decimal> = exact
        .iter()
        .map(|e| e.round_dp_with_strategy(0, RoundingStrategy::ToZero))
        .collect();
    let mut left = total - parts.iter().copied().sum::<Decimal>();
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|a, b| {
        (exact[*b] - parts[*b])
            .cmp(&(exact[*a] - parts[*a]))
            .then(a.cmp(b))
    });
    for i in order {
        if left <= Decimal::ZERO {
            break;
        }
        parts[i] += Decimal::ONE;
        left -= Decimal::ONE;
    }
    parts
}

/// Post `report` of `org`. `fx_rate` is KRW per unit of the report currency.
pub async fn post_report(
    pool: &PgPool,
    org: Uuid,
    report: Uuid,
    actor: Uuid,
    fx_rate: Decimal,
) -> Result<Value> {
    let mut tx = pool.begin().await?;
    let rep = sqlx::query(
        "SELECT status, currency::text AS currency, dsp_id, period_start::text AS period_start
           FROM finance.royalty_reports WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(report)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    match rep.get::<String, _>("status").as_str() {
        "POSTED" => return Err(Error::PolicyGate("REPORT_ALREADY_POSTED")),
        "MATCHED" => {}
        _ => return Err(Error::PolicyGate("REPORT_NOT_FULLY_MATCHED")),
    }
    let currency: String = rep.get("currency");
    if fx_rate <= Decimal::ZERO || (currency == "KRW" && fx_rate != Decimal::ONE) {
        return Err(Error::PolicyGate("INVALID_FX_RATE"));
    }
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM finance.report_lines WHERE org_id=$1 AND report_id=$2 AND match_status <> 'AUTO'",
    )
    .bind(org)
    .bind(report)
    .fetch_one(&mut *tx)
    .await?;
    if open > 0 {
        return Err(Error::PolicyGate("REPORT_NOT_FULLY_MATCHED"));
    }
    let received: Decimal = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount),0) FROM finance.royalty_receipts WHERE org_id=$1 AND report_id=$2",
    )
    .bind(org)
    .bind(report)
    .fetch_one(&mut *tx)
    .await?;
    if received <= Decimal::ZERO {
        return Err(Error::PolicyGate("REPORT_NOT_PAID"));
    }
    // Gross per (release, ISRC), in the report currency.
    let lines = sqlx::query(
        "SELECT matched_release_id AS release_id, isrc, SUM(COALESCE(gross_amount,0)) AS gross
           FROM finance.report_lines WHERE org_id=$1 AND report_id=$2
          GROUP BY matched_release_id, isrc ORDER BY matched_release_id, isrc",
    )
    .bind(org)
    .bind(report)
    .fetch_all(&mut *tx)
    .await?;
    let total: Decimal = lines.iter().map(|l| l.get::<Decimal, _>("gross")).sum();
    if total <= Decimal::ZERO {
        return Err(Error::PolicyGate("REPORT_NOTHING_TO_POST"));
    }
    let mut keyed: Vec<(Uuid, Option<String>, Decimal)> = Vec::new();
    for l in &lines {
        let gross: Decimal = l.get("gross");
        if gross < Decimal::ZERO {
            return Err(Error::PolicyGate("REPORT_NEGATIVE_TRACK_TOTAL"));
        }
        let release: Option<Uuid> = l.get("release_id");
        let release = release.ok_or(Error::PolicyGate("REPORT_NOT_FULLY_MATCHED"))?;
        keyed.push((release, l.get("isrc"), gross));
    }
    // The received money in whole won, spread over the tracks in proportion
    // to their gross without losing a won (settlement terms art. 36 ⑤).
    let krw_total = (received * fx_rate).round_dp_with_strategy(0, RoundingStrategy::ToZero);
    let weights: Vec<Decimal> = keyed.iter().map(|k| k.2).collect();
    let amounts = apportion(krw_total, &weights);
    let mut by_release: BTreeMap<Uuid, Vec<(Option<String>, Decimal)>> = BTreeMap::new();
    for ((release, isrc, _), krw) in keyed.into_iter().zip(amounts) {
        by_release.entry(release).or_default().push((isrc, krw));
    }
    let dsp: String = rep.get("dsp_id");
    let period: String = rep.get("period_start");
    let mut gross_krw_total = Decimal::ZERO;
    let mut summary = Vec::new();
    let mut accepted_shares: Vec<Uuid> = Vec::new();
    for (release, tracks) in by_release {
        let r = sqlx::query(
            "SELECT r.title,
                    (SELECT (d.agreement_terms->>'fee_bps')::bigint FROM portal.documents d
                      WHERE d.org_id=r.org_id AND d.release_id=r.id AND d.kind='AGREEMENT' AND d.status='SIGNED'
                        AND d.agreement_terms ? 'fee_bps') AS fee_bps
               FROM catalog.releases r WHERE r.org_id=$1 AND r.id=$2",
        )
        .bind(org)
        .bind(release)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let title: String = r.get("title");
        let fee_bps: i64 = r
            .get::<Option<i64>, _>("fee_bps")
            .filter(|b| (0..=10_000).contains(b))
            .ok_or(Error::PolicyGate("RELEASE_FEE_UNKNOWN"))?;
        let mut entries: Vec<(String, EntrySide, Decimal, Option<String>)> = Vec::new();
        let mut allocations: Vec<(Uuid, Option<String>, Decimal, String)> = Vec::new();
        let (mut fee_sum, mut owner_sum, mut held_sum, mut gross_sum) =
            (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
        for (isrc, gross) in tracks {
            if gross == Decimal::ZERO {
                continue;
            }
            // The track's live collaborators (declined or removed ones are the applicant's).
            let shares = sqlx::query(
                "SELECT sh.id, sh.share_bps::bigint AS bps, sh.status FROM portal.split_shares sh
                   JOIN catalog.tracks t ON t.id=sh.track_id
                  WHERE sh.org_id=$1 AND sh.release_id=$2 AND t.isrc=$3 AND sh.status IN ('INVITED','ACCEPTED')
                  ORDER BY sh.created_at FOR SHARE OF sh",
            )
            .bind(org)
            .bind(release)
            .bind(&isrc)
            .fetch_all(&mut *tx)
            .await?;
            let list: Vec<(Uuid, i64)> =
                shares.iter().map(|s| (s.get("id"), s.get("bps"))).collect();
            if list.iter().map(|(_, b)| b).sum::<i64>() > 10_000 {
                return Err(Error::PolicyGate("SPLIT_TOTAL_OVER_100"));
            }
            let (fee, owner, cuts) = split_track(gross, fee_bps, &list);
            entries.push((
                "ROYALTY_RECEIVED".into(),
                EntrySide::Debit,
                gross,
                isrc.clone(),
            ));
            if fee > Decimal::ZERO {
                entries.push(("AUDENIQ_FEE".into(), EntrySide::Credit, fee, isrc.clone()));
            }
            if owner > Decimal::ZERO {
                entries.push((
                    "ROYALTY_PAYABLE".into(),
                    EntrySide::Credit,
                    owner,
                    isrc.clone(),
                ));
            }
            for c in &cuts {
                entries.push((
                    "SPLIT_HELD".into(),
                    EntrySide::Credit,
                    c.amount,
                    isrc.clone(),
                ));
                let status: String = shares
                    .iter()
                    .find(|s| s.get::<Uuid, _>("id") == c.share_id)
                    .map(|s| s.get("status"))
                    .unwrap_or_default();
                allocations.push((c.share_id, isrc.clone(), c.amount, status));
                held_sum += c.amount;
            }
            fee_sum += fee;
            owner_sum += owner;
            gross_sum += gross;
        }
        if gross_sum == Decimal::ZERO {
            continue;
        }
        let description = format!("{title} · {dsp} {} 정산", &period[..7.min(period.len())]);
        let entry_inputs: Vec<LedgerEntryInput> = entries
            .iter()
            .map(|(account, side, amount, isrc)| LedgerEntryInput {
                account,
                side: *side,
                amount: *amount,
                currency: None,
                party_id: None,
                isrc: isrc.as_deref(),
                split_snapshot_id: None,
            })
            .collect();
        let fee_version = format!("agreement:fee_bps={fee_bps}");
        let tx_id = finance::post_transaction_in(
            &mut tx,
            org,
            PostTransaction {
                transaction_code: "ROYALTY",
                currency: "KRW",
                entries: entry_inputs,
                match_status: Some("AUTO"),
                fx_rate: Some(fx_rate),
                fee_policy_version: Some(&fee_version),
                contract_version: None,
                tax_rule_version: None,
                source_ref: json!({"report": report, "release": release, "dsp": dsp, "period_start": period}),
                description: &description,
                created_by: Some(actor),
            },
        )
        .await?;
        for (share, isrc, amount, status) in allocations {
            sqlx::query(
                "INSERT INTO finance.split_allocations(id, org_id, share_id, report_id, release_id, isrc, amount, status, posted_transaction_id)
                 VALUES($1,$2,$3,$4,$5,$6,$7,'HELD',$8)",
            )
            .bind(Uuid::new_v4())
            .bind(org)
            .bind(share)
            .bind(report)
            .bind(release)
            .bind(isrc)
            .bind(amount)
            .bind(tx_id)
            .execute(&mut *tx)
            .await?;
            if status == "ACCEPTED" && !accepted_shares.contains(&share) {
                accepted_shares.push(share);
            }
        }
        gross_krw_total += gross_sum;
        summary.push(json!({"release_id": release, "title": title, "gross": gross_sum.to_string(),
            "fee": fee_sum.to_string(), "owner": owner_sum.to_string(), "collaborators": held_sum.to_string()}));
    }
    // Accepted collaborators get theirs now.
    for share in accepted_shares {
        sqlx::query("SELECT finance.settle_split_share($1)")
            .bind(share)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO finance.royalty_postings(org_id, report_id, fx_rate, received_amount, gross_krw, posted_by)
         VALUES($1,$2,$3,$4,$5,$6)",
    )
    .bind(org)
    .bind(report)
    .bind(fx_rate)
    .bind(received)
    .bind(gross_krw_total)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE finance.royalty_reports SET status='POSTED' WHERE org_id=$1 AND id=$2")
        .bind(org)
        .bind(report)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(json!({"report": report, "gross_krw": gross_krw_total.to_string(), "releases": summary}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_then_shares_rounded_down_applicant_keeps_rest() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        // 10,001 won, 8% fee → fee 800, net 9,201; 30% → 2,760, 12.5% → 1,150.
        let (fee, owner, cuts) = split_track(Decimal::from(10_001), 800, &[(a, 3000), (b, 1250)]);
        assert_eq!(fee, Decimal::from(800));
        assert_eq!(
            cuts,
            vec![
                Cut {
                    share_id: a,
                    amount: Decimal::from(2760)
                },
                Cut {
                    share_id: b,
                    amount: Decimal::from(1150)
                }
            ]
        );
        assert_eq!(owner, Decimal::from(9201 - 2760 - 1150));
        assert_eq!(
            fee + owner + cuts.iter().map(|c| c.amount).sum::<Decimal>(),
            Decimal::from(10_001)
        );
    }

    #[test]
    fn apportion_keeps_every_won() {
        let parts = apportion(
            Decimal::from(100),
            &[Decimal::ONE, Decimal::ONE, Decimal::ONE],
        );
        assert_eq!(
            parts,
            vec![Decimal::from(34), Decimal::from(33), Decimal::from(33)]
        );
        let parts = apportion(
            Decimal::from(11_700),
            &[Decimal::new(600, 2), Decimal::new(400, 2)],
        );
        assert_eq!(parts, vec![Decimal::from(7020), Decimal::from(4680)]);
        let parts = apportion(
            Decimal::from(7),
            &[Decimal::new(1, 3), Decimal::new(2, 3), Decimal::new(4, 3)],
        );
        assert_eq!(parts.iter().copied().sum::<Decimal>(), Decimal::from(7));
        assert_eq!(
            apportion(Decimal::from(5), &[Decimal::ZERO]),
            vec![Decimal::ZERO]
        );
    }

    #[test]
    fn tiny_amounts_and_whole_shares() {
        let a = Uuid::new_v4();
        let (fee, owner, cuts) = split_track(Decimal::from(3), 800, &[(a, 5000)]);
        assert_eq!((fee, owner), (Decimal::ZERO, Decimal::from(2)));
        assert_eq!(cuts[0].amount, Decimal::ONE);
        let (_, owner, cuts) = split_track(Decimal::from(1000), 0, &[(a, 10_000)]);
        assert_eq!(
            (owner, cuts[0].amount),
            (Decimal::ZERO, Decimal::from(1000))
        );
        let (_, owner, cuts) = split_track(Decimal::ONE, 0, &[(a, 3000)]);
        assert!(cuts.is_empty(), "a zero cut is not booked");
        assert_eq!(owner, Decimal::ONE);
    }
}
