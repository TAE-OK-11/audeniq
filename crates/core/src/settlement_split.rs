//! 서명 계약 → 정산 배분율 (감사 5번, migration 0095).
//!
//! When the artist signs the distribution agreement, the signed terms become
//! the release's commercial split snapshots (finance.commercial_split_snapshots,
//! append-only, effective from the signature):
//!   * AUDENIQ keeps the signed fee (`fee_bps`; `promo_fee_bps` until
//!     `promo_until`, then a second snapshot with the normal fee);
//!   * royalty holders (rights grants, class ROYALTY) receive their share of
//!     the artist's portion — release-wide grants in `lines`, track-specific
//!     grants as per-track lines in `source_ref.track_lines`;
//!   * the release owner receives the rest (rounding remainders included).
//!
//! Settlement takes the snapshot effective at the revenue date ([`split_for`]).
use crate::{
    error::{Error, Result},
    finance::SplitLine,
};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::collections::BTreeMap;
use uuid::Uuid;

/// The company's line (not a party of the artist's organisation).
pub const AUDENIQ_PARTY: Uuid = Uuid::nil();

struct Holder {
    party: Uuid,
    name: String,
    share_bps: i64,
}

/// Lines for one scope: fee to AUDENIQ, holders' shares of the artist
/// portion, the owner the remainder. Always sums to exactly 10000.
fn lines(fee_bps: i64, owner: (Uuid, &str), holders: &[Holder]) -> Result<Vec<SplitLine>> {
    let artist = 10_000 - fee_bps;
    let mut out = vec![SplitLine {
        party_id: AUDENIQ_PARTY,
        party_name: "AUDENIQ".into(),
        share_bps: fee_bps,
    }];
    let mut given = 0;
    for h in holders {
        // Floor: the owner absorbs rounding, never a holder's promised share.
        let bps = artist * h.share_bps / 10_000;
        given += bps;
        out.push(SplitLine {
            party_id: h.party,
            party_name: h.name.clone(),
            share_bps: bps,
        });
    }
    if given > artist {
        return Err(Error::PolicyGate("SPLIT_SHARES_EXCEED_ARTIST_PORTION"));
    }
    out.push(SplitLine {
        party_id: owner.0,
        party_name: owner.1.to_owned(),
        share_bps: artist - given,
    });
    out.retain(|l| l.share_bps > 0 || l.party_id == owner.0);
    // Several grants for the same party merge into one line.
    let mut merged: Vec<SplitLine> = Vec::new();
    for l in out {
        match merged.iter_mut().find(|m| m.party_id == l.party_id) {
            Some(m) => m.share_bps += l.share_bps,
            None => merged.push(l),
        }
    }
    debug_assert_eq!(merged.iter().map(|l| l.share_bps).sum::<i64>(), 10_000);
    Ok(merged)
}

fn midnight(d: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
}

/// Write the split snapshots for a just-signed agreement (inside the signing
/// transaction). Returns the snapshot ids (1, or 2 with a promotional rate).
pub async fn snapshot_signed_agreement(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    document: Uuid,
    terms: &Value,
    version: &str,
    signed_at: DateTime<Utc>,
) -> Result<Vec<Uuid>> {
    let fee = terms["fee_bps"]
        .as_i64()
        .filter(|f| (0..=10_000).contains(f))
        .ok_or(Error::Internal)?;
    let owner = crate::rights_grants::owner_party(c, org, release).await?;
    let owner_name: String =
        sqlx::query_scalar("SELECT display_name FROM identity.parties WHERE id=$1")
            .bind(owner)
            .fetch_one(&mut *c)
            .await?;
    // Active royalty grants: release-wide and per track.
    let grants: Vec<(String, Uuid, Uuid, String, i32)> = sqlx::query_as(
        "SELECT g.target_kind, g.target_id, g.party_id, p.display_name, g.share_bps
           FROM rights.grant_atoms g JOIN identity.parties p ON p.id=g.party_id
          WHERE g.org_id=$1 AND g.grant_class='ROYALTY' AND g.revoked_at IS NULL
            AND (g.end_exclusive IS NULL OR g.end_exclusive > $3)
            AND ((g.target_kind='RELEASE' AND g.target_id=$2)
                 OR (g.target_kind='TRACK' AND g.target_id IN (SELECT id FROM catalog.tracks WHERE org_id=$1 AND release_id=$2)))
          ORDER BY g.created_at",
    )
    .bind(org)
    .bind(release)
    .bind(signed_at)
    .fetch_all(&mut *c)
    .await?;
    let mut release_holders = Vec::new();
    let mut track_holders: BTreeMap<Uuid, Vec<Holder>> = BTreeMap::new();
    for (kind, target, party, name, share) in grants {
        let h = Holder {
            party,
            name,
            share_bps: i64::from(share),
        };
        if kind == "RELEASE" {
            release_holders.push(h);
        } else {
            track_holders.entry(target).or_default().push(h);
        }
    }
    let windows: Vec<(DateTime<Utc>, i64, &str)> = match (
        terms["promo_fee_bps"].as_i64(),
        terms["promo_until"].as_str(),
    ) {
        (Some(promo), Some(until)) => {
            let until =
                NaiveDate::parse_from_str(until, "%Y-%m-%d").map_err(|_| Error::Internal)?;
            let after = midnight(until.succ_opt().unwrap_or(until));
            if after > signed_at {
                vec![(signed_at, promo, "PROMO"), (after, fee, "STANDARD")]
            } else {
                vec![(signed_at, fee, "STANDARD")]
            }
        }
        _ => vec![(signed_at, fee, "STANDARD")],
    };
    let contract_version = format!(
        "{}:{document}:v{version}",
        terms["form"].as_str().unwrap_or("AGREEMENT")
    );
    let mut ids = Vec::new();
    for (from, fee_bps, rate) in windows {
        let base = lines(fee_bps, (owner, &owner_name), &release_holders)?;
        let mut track_lines = serde_json::Map::new();
        for (track, holders) in &track_holders {
            // A track's own grants come on top of the release-wide ones.
            let mut all: Vec<Holder> = release_holders
                .iter()
                .map(|h| Holder {
                    party: h.party,
                    name: h.name.clone(),
                    share_bps: h.share_bps,
                })
                .collect();
            all.extend(holders.iter().map(|h| Holder {
                party: h.party,
                name: h.name.clone(),
                share_bps: h.share_bps,
            }));
            track_lines.insert(
                track.to_string(),
                serde_json::to_value(lines(fee_bps, (owner, &owner_name), &all)?)
                    .map_err(|_| Error::Internal)?,
            );
        }
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO finance.commercial_split_snapshots(id,org_id,release_id,valid_from,lines,contract_version,source_ref)
             VALUES($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(id)
        .bind(org)
        .bind(release)
        .bind(from)
        .bind(serde_json::to_value(&base).map_err(|_| Error::Internal)?)
        .bind(&contract_version)
        .bind(json!({
            "source": "SIGNED_AGREEMENT", "document_id": document, "document_version": version,
            "rate": rate, "fee_bps": fee_bps, "terms_version": terms["terms_version"],
            "track_lines": track_lines,
        }))
        .execute(&mut *c)
        .await?;
        ids.push(id);
    }
    Ok(ids)
}

/// The split that applies to revenue earned at `at` on one track: that
/// track's lines when it has its own royalty grants, else the release's.
pub async fn split_for(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    track: Option<Uuid>,
    at: DateTime<Utc>,
) -> Result<Option<(Uuid, Vec<SplitLine>)>> {
    let row: Option<(Uuid, Value, Value)> = sqlx::query_as(
        "SELECT id, lines, source_ref FROM finance.commercial_split_snapshots
          WHERE org_id=$1 AND release_id=$2 AND valid_from <= $3
          ORDER BY valid_from DESC LIMIT 1",
    )
    .bind(org)
    .bind(release)
    .bind(at)
    .fetch_optional(&mut *c)
    .await?;
    let Some((id, lines, source)) = row else {
        return Ok(None);
    };
    let chosen = track
        .and_then(|t| source["track_lines"].get(t.to_string()).cloned())
        .unwrap_or(lines);
    let parsed: Vec<SplitLine> = serde_json::from_value(chosen).map_err(|_| Error::Internal)?;
    Ok(Some((id, parsed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_then_holders_then_owner_sum_to_10000() {
        let owner = Uuid::new_v4();
        let holder = Holder {
            party: Uuid::new_v4(),
            name: "작곡가".into(),
            share_bps: 3000,
        };
        let l = lines(800, (owner, "아티스트"), &[holder]).unwrap();
        // 92% artist portion: holder 30% of it = 27.6%, owner 64.4%.
        assert_eq!(l[0].share_bps, 800);
        assert_eq!(l[1].share_bps, 2760);
        assert_eq!(l[2].party_id, owner);
        assert_eq!(l[2].share_bps, 6440);
        assert_eq!(l.iter().map(|x| x.share_bps).sum::<i64>(), 10_000);
        // Rounding goes to the owner.
        let odd = Holder {
            party: Uuid::new_v4(),
            name: "x".into(),
            share_bps: 3333,
        };
        let l = lines(777, (owner, "o"), &[odd]).unwrap();
        assert_eq!(l.iter().map(|x| x.share_bps).sum::<i64>(), 10_000);
        assert!(
            lines(
                0,
                (owner, "o"),
                &[
                    Holder {
                        party: Uuid::new_v4(),
                        name: "a".into(),
                        share_bps: 7000
                    },
                    Holder {
                        party: Uuid::new_v4(),
                        name: "b".into(),
                        share_bps: 7000
                    },
                ]
            )
            .is_err()
        );
    }
}
