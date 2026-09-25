//! Per-package DSP route decisions: direct / aggregator / upstream.
//!
//! The engine answers one question per approved DSP: "is there a sendable
//! adapter profile for this DSP right now, and on which route?" Preference
//! order is direct first, then aggregator (Merlin), then upstream (LIMBO).
//! A profile is sendable when it is delivery_enabled, its capabilities declare
//! send_or_publish=true, and it is MOCK — or CONTRACTED with a live contract
//! route (enabled route plan + ACTIVE endpoint + non-revoked contract
//! revision). Without contracts every real DSP resolves to NO_ROUTE and the
//! package waits for F6 instead of failing.
//!
//! The engine never transmits anything; it only records the decision so
//! delivery enqueue and F6 have an auditable, deterministic basis.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteKind {
    Direct,
    Aggregator,
    Upstream,
}

impl RouteKind {
    fn from_db(s: &str) -> Option<Self> {
        match s {
            "direct" => Some(Self::Direct),
            "aggregator" => Some(Self::Aggregator),
            "upstream" => Some(Self::Upstream),
            _ => None,
        }
    }

    /// Preference rank: direct first, then aggregator, then upstream.
    fn rank(&self) -> u8 {
        match self {
            Self::Direct => 0,
            Self::Aggregator => 1,
            Self::Upstream => 2,
        }
    }

    pub fn as_db(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Aggregator => "aggregator",
            Self::Upstream => "upstream",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteDecision {
    pub dsp_id: Uuid,
    pub route_kind: Option<RouteKind>,
    pub partner_id: Option<String>,
    pub routable: bool,
    pub reason: &'static str,
}

struct Profile {
    partner_id: String,
    activation_kind: String,
    route_kind: RouteKind,
    delivery_enabled: bool,
    /// capabilities.send_or_publish: the adapter itself must declare it can
    /// put bytes on the wire. Undeclared or false = never sendable, even if
    /// someone flips delivery_enabled on a placeholder.
    cap_sendable: bool,
}

/// A CONTRACTED profile is sendable only through a live contract route:
/// an enabled route plan whose endpoint is ACTIVE and whose contract
/// revision is not revoked. MOCK profiles are always sendable (test only).
async fn contract_route_live(
    c: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: Uuid,
    dsp_id: Uuid,
) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM distribution.route_plans r
         JOIN distribution.dsp_endpoints e ON e.org_id=r.org_id AND e.dsp_id=r.dsp_id AND e.id=r.endpoint_id
         JOIN rights.contract_revisions cr ON cr.org_id=r.org_id AND cr.contract_id=r.contract_id
         WHERE r.org_id=$1 AND r.dsp_id=$2 AND r.enabled
           AND e.integration_status='ACTIVE' AND cr.policy_version<>'REVOKED'",
    )
    .bind(org_id)
    .bind(dsp_id)
    .fetch_one(&mut **c)
    .await?;
    Ok(n > 0)
}

/// Decide the route for every DSP in `dsp_ids`, in preference order:
/// direct profile, then covering aggregator, then covering upstream.
/// Deterministic: ties break on partner_id. Duplicate DSP ids are decided
/// once.
///
/// Fail-closed rules, all of which must hold for a profile to be sendable:
/// - delivery_enabled = true
/// - capabilities.send_or_publish = true (explicit; placeholders declare false)
/// - activation MOCK, or CONTRACTED with a live contract route (enabled route
///   plan + ACTIVE endpoint + non-revoked contract revision)
/// - aggregator/upstream profiles additionally need an explicit
///   route_coverage row for the DSP — coverage is never assumed.
pub async fn decide_routes(
    pool: &PgPool,
    org_id: Uuid,
    dsp_ids: &[Uuid],
) -> Result<Vec<RouteDecision>> {
    /// One candidate query per route class. Aggregator and upstream both
    /// require an explicit route_coverage row: the engine never assumes an
    /// aggregator's member set or an upstream's downstream footprint.
    async fn profiles(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        dsp_id: Uuid,
        kind: RouteKind,
    ) -> Result<Vec<Profile>> {
        let rows = if kind == RouteKind::Direct {
            sqlx::query(
                "SELECT partner_id, activation_kind, route_kind, delivery_enabled,
                        COALESCE((capabilities->>'send_or_publish')::boolean, false) AS cap_sendable
                 FROM execution.adapter_profiles
                 WHERE dsp_id=$1 AND route_kind='direct'",
            )
            .bind(dsp_id)
            .fetch_all(&mut **tx)
            .await?
        } else {
            sqlx::query(
                "SELECT p.partner_id, p.activation_kind, p.route_kind, p.delivery_enabled,
                        COALESCE((p.capabilities->>'send_or_publish')::boolean, false) AS cap_sendable
                 FROM execution.adapter_profiles p
                 JOIN execution.route_coverage c ON c.partner_id=p.partner_id
                 WHERE c.dsp_id=$1 AND p.route_kind=$2",
            )
            .bind(dsp_id)
            .bind(kind.as_db())
            .fetch_all(&mut **tx)
            .await?
        };
        Ok(rows
            .iter()
            .filter_map(|r| {
                Some(Profile {
                    partner_id: r.get("partner_id"),
                    activation_kind: r.get("activation_kind"),
                    route_kind: RouteKind::from_db(r.get("route_kind"))?,
                    delivery_enabled: r.get("delivery_enabled"),
                    cap_sendable: r.get("cap_sendable"),
                })
            })
            .collect())
    }

    let mut tx = pool.begin().await?;
    let mut out = Vec::with_capacity(dsp_ids.len());
    let mut seen = std::collections::HashSet::new();
    for dsp_id in dsp_ids {
        if !seen.insert(*dsp_id) {
            continue;
        }
        let mut candidates = Vec::new();
        candidates.extend(profiles(&mut tx, *dsp_id, RouteKind::Direct).await?);
        candidates.extend(profiles(&mut tx, *dsp_id, RouteKind::Aggregator).await?);
        candidates.extend(profiles(&mut tx, *dsp_id, RouteKind::Upstream).await?);
        // Preference: direct, then aggregator, then upstream; partner_id
        // breaks ties deterministically.
        candidates.sort_by(|a, b| {
            (a.route_kind.rank(), &a.partner_id).cmp(&(b.route_kind.rank(), &b.partner_id))
        });

        let mut decision = RouteDecision {
            dsp_id: *dsp_id,
            route_kind: None,
            partner_id: None,
            routable: false,
            reason: "NO_PROFILE",
        };
        if candidates.is_empty() {
            out.push(decision);
            continue;
        }
        let mut saw_disabled = false;
        let mut saw_uncontracted = false;
        let mut saw_unsendable_cap = false;
        for p in candidates {
            if !p.delivery_enabled {
                saw_disabled = true;
                continue;
            }
            if !p.cap_sendable {
                // The adapter itself declares it cannot send (or never
                // declared it): enabling the profile must not open the wire.
                saw_unsendable_cap = true;
                continue;
            }
            let contract_ok = if p.activation_kind == "CONTRACTED" {
                contract_route_live(&mut tx, org_id, *dsp_id).await?
            } else {
                true
            };
            if !contract_ok {
                saw_uncontracted = true;
                continue;
            }
            decision.route_kind = Some(p.route_kind);
            decision.partner_id = Some(p.partner_id);
            decision.routable = true;
            decision.reason = "SENDABLE_PROFILE";
            break;
        }
        if !decision.routable {
            decision.reason = if saw_uncontracted {
                "NO_CONTRACT_ROUTE"
            } else if saw_disabled {
                "PROFILE_DISABLED"
            } else if saw_unsendable_cap {
                "ADAPTER_CANNOT_SEND"
            } else {
                "NO_PROFILE"
            };
        }
        out.push(decision);
    }
    tx.commit().await?;
    Ok(out)
}

/// Persist one package's route decisions. Idempotent per (org, package, dsp):
/// re-running the decision never duplicates rows.
pub async fn record_route_decisions(
    pool: &PgPool,
    org_id: Uuid,
    package_id: Uuid,
    decisions: &[RouteDecision],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    for d in decisions {
        let status = if d.routable { "ROUTABLE" } else { "NO_ROUTE" };
        sqlx::query(
            "INSERT INTO execution.route_decisions(id,org_id,package_id,dsp_id,route_kind,partner_id,status,reason)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT(org_id,package_id,dsp_id) DO UPDATE SET
               route_kind=EXCLUDED.route_kind, partner_id=EXCLUDED.partner_id,
               status=EXCLUDED.status, reason=EXCLUDED.reason, decided_at=now()",
        )
        .bind(Uuid::new_v4())
        .bind(org_id)
        .bind(package_id)
        .bind(d.dsp_id)
        .bind(d.route_kind.map(|k| k.as_db()))
        .bind(d.partner_id.clone())
        .bind(status)
        .bind(d.reason)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Read back the recorded decision for one (package, dsp), if any.
pub async fn get_route_decision(
    pool: &PgPool,
    org_id: Uuid,
    package_id: Uuid,
    dsp_id: Uuid,
) -> Result<Option<RouteDecision>> {
    let r = sqlx::query(
        "SELECT dsp_id, route_kind, partner_id, status, reason FROM execution.route_decisions
         WHERE org_id=$1 AND package_id=$2 AND dsp_id=$3",
    )
    .bind(org_id)
    .bind(package_id)
    .bind(dsp_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::NotFound);
    let r = match r {
        Ok(r) => r,
        Err(Error::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(Some(RouteDecision {
        dsp_id: r.get("dsp_id"),
        route_kind: r
            .get::<Option<String>, _>("route_kind")
            .as_deref()
            .and_then(RouteKind::from_db),
        partner_id: r.get("partner_id"),
        routable: r.get::<String, _>("status") == "ROUTABLE",
        reason: match r.get::<String, _>("reason").as_str() {
            "SENDABLE_PROFILE" => "SENDABLE_PROFILE",
            "NO_CONTRACT_ROUTE" => "NO_CONTRACT_ROUTE",
            "PROFILE_DISABLED" => "PROFILE_DISABLED",
            _ => "NO_PROFILE",
        },
    }))
}
