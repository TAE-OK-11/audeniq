//! 출금 정책 — 정산 약관 제8조(최소 출금금액)·제9조(출금 수수료)·제10조(소액 출금 제한)와
//! 부칙 제2조(최소 출금금액 면제 이벤트)를 서버 한곳에서 계산한다. 견적·신청·화면이 같은 값을 쓴다.
//!
//! - 최소 출금금액 10,000원은 수수료 공제 전 신청금액 기준. 2027-12-31 23:59:59 KST까지는 면제.
//! - 수수료: 신청금액 30,000원 이하 건당 500원(부가가치세 포함, 별도 가산 없음), 초과 무료.
//! - 수수료를 뺀 실수령액이 0원 이하이면 신청을 받지 않는다.
use crate::error::{Error, Result};
use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use serde_json::{Value, json};

/// 신청·주문·원장에 함께 기록하는 정책 버전.
pub const POLICY_VERSION: &str = "settlement-terms-ko/payout-2026-10";
pub const MIN_PAYOUT: i64 = 10_000;
pub const SMALL_PAYOUT_LIMIT: i64 = 30_000;
pub const SMALL_PAYOUT_FEE: i64 = 500;

/// 면제가 끝나는 시각: 2028-01-01 00:00:00 KST = 2027-12-31 15:00:00 UTC.
pub fn minimum_waiver_ends() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2027, 12, 31, 15, 0, 0)
        .single()
        .expect("valid instant")
}

/// 지금 적용되는 최소 출금금액 (면제 기간에는 0).
pub fn minimum_at(now: DateTime<Utc>) -> Decimal {
    if now < minimum_waiver_ends() {
        Decimal::ZERO
    } else {
        Decimal::from(MIN_PAYOUT)
    }
}

pub fn fee_for(amount: Decimal) -> Decimal {
    if amount <= Decimal::from(SMALL_PAYOUT_LIMIT) {
        Decimal::from(SMALL_PAYOUT_FEE)
    } else {
        Decimal::ZERO
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub amount: Decimal,
    pub fee: Decimal,
    pub net: Decimal,
    pub minimum: Decimal,
}

impl Quote {
    pub fn json(&self) -> Value {
        json!({
            "amount": self.amount.to_string(),
            "fee": self.fee.to_string(),
            "net": self.net.to_string(),
            "minimum_payout": self.minimum.to_string(),
            "policy_version": POLICY_VERSION,
        })
    }
}

/// 원 단위 양의 정수 금액만. 최소 금액 미달·실수령액 0원 이하는 거절.
pub fn quote(amount: Decimal, now: DateTime<Utc>) -> Result<Quote> {
    if amount <= Decimal::ZERO || amount.fract() != Decimal::ZERO {
        return Err(Error::Invalid);
    }
    let minimum = minimum_at(now);
    if amount < minimum {
        return Err(Error::PolicyGate("PAYOUT_BELOW_MINIMUM"));
    }
    let fee = fee_for(amount);
    let net = amount - fee;
    if net <= Decimal::ZERO {
        return Err(Error::PolicyGate("PAYOUT_NET_NOT_POSITIVE"));
    }
    Ok(Quote {
        amount,
        fee,
        net,
        minimum,
    })
}

/// 화면에 보여 줄 현재 정책.
pub fn policy_json(now: DateTime<Utc>) -> Value {
    let waived = now < minimum_waiver_ends();
    json!({
        "version": POLICY_VERSION,
        "minimum_payout": minimum_at(now).to_string(),
        "standard_minimum_payout": MIN_PAYOUT.to_string(),
        "minimum_waived_until": waived.then_some("2027-12-31T23:59:59+09:00"),
        "small_payout_limit": SMALL_PAYOUT_LIMIT.to_string(),
        "small_payout_fee": SMALL_PAYOUT_FEE.to_string(),
        "fee_includes_vat": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(n: i64) -> Decimal {
        Decimal::from(n)
    }
    fn kst(y: i32, mo: u32, da: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        chrono::FixedOffset::east_opt(9 * 3600)
            .unwrap()
            .with_ymd_and_hms(y, mo, da, h, mi, s)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn waiver_boundary() {
        let last = kst(2027, 12, 31, 23, 59, 59);
        let first = kst(2028, 1, 1, 0, 0, 0);
        assert_eq!(minimum_at(last), d(0));
        assert_eq!(minimum_at(first), d(10_000));
        assert!(
            quote(d(5_000), last).is_ok(),
            "small payouts allowed during the waiver"
        );
        assert!(matches!(
            quote(d(5_000), first),
            Err(Error::PolicyGate("PAYOUT_BELOW_MINIMUM"))
        ));
        assert!(
            quote(d(10_000), first).is_ok(),
            "minimum judged before the fee"
        );
    }

    #[test]
    fn fee_boundary_and_net() {
        let now = kst(2026, 10, 10, 12, 0, 0);
        let q = quote(d(30_000), now).unwrap();
        assert_eq!((q.fee, q.net), (d(500), d(29_500)));
        let q = quote(d(30_001), now).unwrap();
        assert_eq!((q.fee, q.net), (d(0), d(30_001)));
        assert!(matches!(
            quote(d(500), now),
            Err(Error::PolicyGate("PAYOUT_NET_NOT_POSITIVE"))
        ));
        assert_eq!(quote(d(501), now).unwrap().net, d(1));
        assert!(
            quote(Decimal::new(10_0005, 1), now).is_err(),
            "whole won only"
        );
        assert!(quote(d(0), now).is_err());
        assert!(quote(d(-1), now).is_err());
    }
}
