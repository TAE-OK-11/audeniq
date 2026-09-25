//! F3 Stage 2 review (BLUEPRINT §5): the durable `stage2.review` job.
//!
//! Five logical module areas run inside one job. Per-check checkpoints are
//! the immutable `operations.check_results` rows keyed by
//! `(revision_id, check_code, result_hash)`:
//!
//! - a retry that crashed *before* the verification package was pinned
//!   re-runs the modules (they are pure reads, so re-running is safe and
//!   picks up any rights/catalog changes since the crash) and the checkpoint
//!   rows dedupe identical results instead of recording them twice;
//! - a retry that crashed *after* the package was pinned returns the pinned
//!   package immediately without re-running anything (idempotent
//!   completion at the top of `run_stage2`).
//!
//! Checkpoints are therefore an idempotent audit trail and the source for
//! overrides ("the check must exist for this revision"), not a skip gate:
//! modules are never resumed from stale per-module state.
//! All mutations run in one transaction fenced by the job's lock token: an
//! expired worker cannot commit a decision.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, Row};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Rule version for Stage 2 review checks.
pub const REVIEW_RULE_VERSION: &str = "1";

pub struct Stage2Summary {
    pub revision_id: Uuid,
    pub decision: &'static str,
    pub verification_package_id: Option<Uuid>,
    pub release_status: String,
    pub needs_retry: bool,
}

struct ReviewCheck {
    check_code: &'static str,
    status: &'static str,
    detail: String,
}

impl ReviewCheck {
    fn result_hash(&self) -> String {
        sha256_hex(&format!(
            "{}:{}:{}",
            self.check_code, REVIEW_RULE_VERSION, self.detail
        ))
    }
}

/// Idempotent checkpoint record: an identical check for the same revision
/// returns the existing row instead of inserting a duplicate; a changed
/// detail (new result_hash) records a new row so retries never serve stale
/// results. Visible for tests.
async fn record_check_result(
    tx: &mut PgConnection,
    revision_id: Uuid,
    c: &ReviewCheck,
) -> Result<Uuid> {
    let rh = c.result_hash();
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM operations.check_results WHERE revision_id=$1 AND check_code=$2 AND result_hash=$3",
    )
    .bind(revision_id)
    .bind(c.check_code)
    .bind(&rh)
    .fetch_optional(&mut *tx)
    .await?;
    match existing {
        Some(id) => Ok(id),
        None => sqlx::query_scalar("INSERT INTO operations.check_results(id, revision_id, check_code, rule_version, status, result_hash, detail) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING id")
            .bind(Uuid::new_v4())
            .bind(revision_id)
            .bind(c.check_code)
            .bind(REVIEW_RULE_VERSION)
            .bind(c.status)
            .bind(&rh)
            .bind(&c.detail)
            .fetch_one(&mut *tx)
            .await
            .map_err(Error::from),
    }
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

struct Ctx {
    org: Uuid,
    release: Uuid,
    revision_id: Uuid,
    body: Value,
    body_hash: String,
    validation_package_id: Uuid,
    validation_body: Value,
    consent_path: String,
    applicant_party: Option<Uuid>,
}

async fn load_ctx(pool: &PgPool, revision_id: Uuid) -> Result<Ctx> {
    let rev = sqlx::query(
        "SELECT org_id, release_id, body, body_hash, consent_package_hash FROM catalog.application_revisions WHERE id=$1",
    )
    .bind(revision_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = rev.get("org_id");
    let release: Uuid = rev.get("release_id");
    let body: Value = rev.get("body");
    let body_hash: String = rev.get("body_hash");
    let consent_hash: String = rev.get("consent_package_hash");
    let cp = sqlx::query("SELECT body FROM catalog.consent_packages WHERE package_hash=$1")
        .bind(&consent_hash)
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)?;
    let cp_body: Value = cp.get("body");
    let consent_path = cp_body
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("self")
        .to_string();
    let applicant_party = cp_body
        .get("applicant_party_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .or_else(|| {
            cp_body
                .get("party_ids")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        });
    Ok(Ctx {
        org,
        release,
        revision_id,
        body,
        body_hash,
        validation_package_id: Uuid::nil(),
        validation_body: Value::Null,
        consent_path,
        applicant_party,
    })
}

/// 2-D durable entry point. Replaces the F2 `park()` for `stage2`.
/// Returns `None` when the job lost its lease: the caller must neither
/// succeed nor fail the job; the sweeper will reclaim it.
pub async fn run_stage2(pool: &PgPool, job: &operations::Job) -> Result<Option<Stage2Summary>> {
    let revision_id = job
        .payload
        .get("revision_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(Error::Internal)?;
    let mut ctx = load_ctx(pool, revision_id).await?;
    let vp_id = job
        .payload
        .get("validation_package_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(Error::Internal)?;
    ctx.validation_package_id = vp_id;
    let vp = sqlx::query("SELECT body FROM distribution.validation_packages WHERE id=$1")
        .bind(vp_id)
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)?;
    ctx.validation_body = vp.get("body");

    // Idempotent completion: a previous attempt already pinned the
    // verification package (worker crashed between commit and completion).
    if let Some(pkg_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM distribution.verification_packages WHERE revision_id=$1",
    )
    .bind(revision_id)
    .fetch_optional(pool)
    .await?
    {
        let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
            .bind(ctx.release)
            .fetch_one(pool)
            .await?;
        return Ok(Some(Stage2Summary {
            revision_id,
            decision: "PASS",
            verification_package_id: Some(pkg_id),
            release_status: status,
            needs_retry: false,
        }));
    }

    let mut tx = pool.begin().await?;
    // Fence every mutation on the job's live lease: an expired worker's
    // decision must never commit.
    let held: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM operations.jobs WHERE id=$1 AND lock_token=$2 AND status='RUNNING' AND lease_until>clock_timestamp() FOR UPDATE",
    )
    .bind(job.id)
    .bind(job.token)
    .fetch_optional(&mut *tx)
    .await?;
    if held.is_none() {
        return Ok(None);
    }
    // Only a release still waiting on this Stage 2 run may be decided. A
    // re-evaluation (queued after a reviewer override) can race a withdrawal
    // or resubmission; then there is nothing to decide.
    let status: String = sqlx::query_scalar(
        "SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(ctx.org)
    .bind(ctx.release)
    .fetch_one(&mut *tx)
    .await?;
    if !matches!(
        status.as_str(),
        "STAGE1_PASSED" | "STAGE2_RUNNING" | "STAGE2_REVIEW"
    ) {
        tx.commit().await?;
        return Ok(Some(Stage2Summary {
            revision_id,
            decision: "SKIPPED",
            verification_package_id: None,
            release_status: status,
            needs_retry: false,
        }));
    }
    // The worker owns Stage 2 now: STAGE1_PASSED -> STAGE2_RUNNING, or
    // STAGE2_REVIEW -> STAGE2_RUNNING when an override triggered a
    // re-evaluation (allowed transition since 0002).
    sqlx::query("UPDATE catalog.releases SET status='STAGE2_RUNNING', row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND status IN ('STAGE1_PASSED','STAGE2_REVIEW')")
        .bind(ctx.org)
        .bind(ctx.release)
        .execute(&mut *tx)
        .await?;

    let mut checks = Vec::new();
    checks.extend(module_applicant_rights(&mut tx, &ctx).await?);
    checks.extend(module_catalog_match(&mut tx, &ctx).await?);
    checks.extend(module_metadata_content(&ctx).await?);
    checks.extend(module_policy_integrity(&mut tx, &ctx).await?);

    let mut check_ids = Vec::new();
    for c in &checks {
        check_ids.push(record_check_result(&mut tx, revision_id, c).await?);
    }

    let summary = decide(&mut tx, &ctx, &checks, &check_ids).await?;
    tx.commit().await?;
    Ok(Some(summary))
}

/// Apply overrides, merge statuses, and commit the decision.
async fn decide(
    tx: &mut PgConnection,
    ctx: &Ctx,
    checks: &[ReviewCheck],
    check_ids: &[Uuid],
) -> Result<Stage2Summary> {
    // Overrides never mutate check_results; they replace the effective status.
    // Oldest first, so the latest override for a check wins.
    let overrides = sqlx::query(
        "SELECT check_code, proposed_status FROM rights.review_overrides WHERE org_id=$1 AND revision_id=$2 AND (expires_at IS NULL OR expires_at>now()) ORDER BY created_at, id",
    )
    .bind(ctx.org)
    .bind(ctx.revision_id)
    .fetch_all(&mut *tx)
    .await?;
    let ov: BTreeMap<String, String> = overrides
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("check_code"),
                r.get::<String, _>("proposed_status"),
            )
        })
        .collect();
    let effective =
        |c: &ReviewCheck| -> &str { ov.get(c.check_code).map(String::as_str).unwrap_or(c.status) };
    let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
    for c in checks {
        *counts.entry(effective(c)).or_default() += 1;
    }
    let n = |s: &str| counts.get(s).copied().unwrap_or(0);
    let decision: &'static str = if n("TECHNICAL_RETRY") > 0 {
        "TECHNICAL_RETRY"
    } else if n("REVIEW_REQUIRED") > 0 || n("BLOCKED") > 0 {
        "REVIEW_REQUIRED"
    } else if n("CORRECTION_REQUIRED") > 0 {
        "CORRECTION_REQUIRED"
    } else {
        "PASS"
    };
    let request = Uuid::new_v4();

    if decision == "TECHNICAL_RETRY" {
        // Stay STAGE2_RUNNING; checks are cached, the caller requeues.
        operations::audit(
            &mut *tx,
            None,
            Some(ctx.org),
            Some(ctx.revision_id),
            "stage2.technical_retry",
            "STAGE2_TECHNICAL_RETRY",
            request,
        )
        .await?;
        return Ok(Stage2Summary {
            revision_id: ctx.revision_id,
            decision,
            verification_package_id: None,
            release_status: "STAGE2_RUNNING".into(),
            needs_retry: true,
        });
    }

    if decision == "PASS" {
        // Pin the rights epoch: F3 never advances it mid-review; revocation
        // and dispute flows (F4+) bump it and force re-review.
        sqlx::query(
            "INSERT INTO rights.rights_epochs(org_id, release_id, epoch) VALUES($1,$2,0) ON CONFLICT DO NOTHING",
        )
        .bind(ctx.org)
        .bind(ctx.release)
        .execute(&mut *tx)
        .await?;
        let epoch: i64 = sqlx::query_scalar(
            "SELECT epoch FROM rights.rights_epochs WHERE org_id=$1 AND release_id=$2",
        )
        .bind(ctx.org)
        .bind(ctx.release)
        .fetch_one(&mut *tx)
        .await?;
        let approved_scope = approved_scope(checks);
        let split = commercial_split_snapshot(tx, ctx).await?;
        let pkg = json!({
            "schema_version": 1,
            "revision_id": ctx.revision_id,
            "revision_hash": ctx.body_hash,
            "stage1_validation_package_id": ctx.validation_package_id,
            "decision": "PASS",
            "check_summary": counts,
            "stage2_check_refs": check_ids,
            "approved_scope": approved_scope,
            "commercial_split_snapshot": split,
            "rights_epoch": epoch,
            "overrides_applied": ov.keys().collect::<Vec<_>>(),
            "special_flags": special_flags(ctx),
            "rule_version": REVIEW_RULE_VERSION,
        });
        let pkg_hash = sha256_hex(&serde_json::to_string(&pkg).expect("json serializes"));
        let pkg_id = Uuid::new_v4();
        sqlx::query("INSERT INTO distribution.verification_packages(id, org_id, revision_id, body, package_hash, rights_epoch) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(pkg_id).bind(ctx.org).bind(ctx.revision_id).bind(&pkg).bind(&pkg_hash).bind(epoch)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE catalog.releases SET status='STAGE2_PASSED', row_version=row_version+1 WHERE org_id=$1 AND id=$2")
            .bind(ctx.org).bind(ctx.release)
            .execute(&mut *tx)
            .await?;
        // Same-transaction handoff to Stage 3 prep (F4 implements the handler;
        // until then the worker parks it instead of dead-lettering).
        sqlx::query("INSERT INTO operations.jobs(id, queue, kind, payload, pinned_revision_id, idempotency_key) VALUES($1,'distribution','prepare_release',$2,$3,$4) ON CONFLICT(idempotency_key) DO NOTHING")
            .bind(Uuid::new_v4())
            .bind(json!({"revision_id": ctx.revision_id, "verification_package_id": pkg_id, "package_hash": pkg_hash}))
            .bind(ctx.revision_id)
            .bind(format!("prepare_release:{}", ctx.revision_id))
            .execute(&mut *tx)
            .await?;
        operations::audit(
            &mut *tx,
            None,
            Some(ctx.org),
            Some(ctx.revision_id),
            "stage2.pass",
            "STAGE2_PASS",
            request,
        )
        .await?;
        return Ok(Stage2Summary {
            revision_id: ctx.revision_id,
            decision,
            verification_package_id: Some(pkg_id),
            release_status: "STAGE2_PASSED".into(),
            needs_retry: false,
        });
    }

    // REVIEW_REQUIRED or CORRECTION_REQUIRED: record the return routing.
    let status = if decision == "REVIEW_REQUIRED" {
        "STAGE2_REVIEW"
    } else {
        "STAGE2_CORRECTION"
    };
    let reasons: Vec<&str> = checks
        .iter()
        .filter(|c| {
            let e = effective(c);
            e == "REVIEW_REQUIRED" || e == "BLOCKED" || e == "CORRECTION_REQUIRED"
        })
        .map(|c| c.check_code)
        .collect();
    sqlx::query("UPDATE catalog.releases SET status=$3, row_version=row_version+1 WHERE org_id=$1 AND id=$2")
        .bind(ctx.org)
        .bind(ctx.release)
        .bind(status)
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut *tx,
        None,
        Some(ctx.org),
        Some(ctx.revision_id),
        "stage2.decision",
        &format!("{}:{}", decision, reasons.join(",")),
        request,
    )
    .await?;
    Ok(Stage2Summary {
        revision_id: ctx.revision_id,
        decision,
        verification_package_id: None,
        release_status: status.into(),
        needs_retry: false,
    })
}

// ---------------------------------------------------------------------------
// Module 1: applicant_rights (2-0 router, 2-A/2-B)
// ---------------------------------------------------------------------------

fn special_flags(ctx: &Ctx) -> Vec<String> {
    ctx.validation_body
        .get("special_flags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

async fn module_applicant_rights(tx: &mut PgConnection, ctx: &Ctx) -> Result<Vec<ReviewCheck>> {
    let mut out = Vec::new();
    // 2-0 router: the path comes from the F2 consent package.
    let path = ctx.consent_path.clone();
    let minority = ctx
        .body
        .get("minority_declared")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if minority || path == "minor" {
        out.push(ReviewCheck {
            check_code: "S2_ROUTER_PATH",
            status: "REVIEW_REQUIRED",
            detail: "minority path is hard-gated to human review until legal review (§23.1)".into(),
        });
        return Ok(out);
    }
    out.push(ReviewCheck {
        check_code: "S2_ROUTER_PATH",
        status: "PASS",
        detail: format!(
            "router path={path} applicant={:?} from consent package",
            ctx.applicant_party
        ),
    });

    // 2-A/2-B: grant chain for every track/release target in this org.
    let flags = special_flags(ctx);
    let needs_extra_grant = flags.iter().any(|f| {
        matches!(
            f.as_str(),
            "COVER" | "REMIX" | "SAMPLE" | "AI" | "MIGRATION" | "EXPLICIT"
        )
    });
    let grants = sqlx::query(
        "SELECT id, target_kind, target_id, right_type, territory_set, use_set, start_at, end_exclusive, exclusive, sublicensable, parent_grant_id, revoked_at FROM rights.grant_atoms WHERE org_id=$1 AND revoked_at IS NULL",
    )
    .bind(ctx.org)
    .fetch_all(&mut *tx)
    .await?;
    // Chain depth guard: unclear or overly deep chains go to REVIEW, never
    // auto-expanded (BLUEPRINT §5.2).
    let mut max_depth = 0i32;
    for g in &grants {
        let mut depth = 0i32;
        let mut parent: Option<Uuid> = g.get("parent_grant_id");
        while let Some(pid) = parent {
            depth += 1;
            if depth > 8 {
                break;
            }
            parent = sqlx::query_scalar(
                "SELECT parent_grant_id FROM rights.grant_atoms WHERE org_id=$1 AND id=$2",
            )
            .bind(ctx.org)
            .bind(pid)
            .fetch_optional(&mut *tx)
            .await?
            .unwrap_or(None);
        }
        max_depth = max_depth.max(depth);
    }
    if max_depth > 8 {
        out.push(ReviewCheck {
            check_code: "S2_RIGHTS_SCOPE",
            status: "REVIEW_REQUIRED",
            detail: "grant chain exceeds automatic inspection depth; not auto-expanded".into(),
        });
        return Ok(out);
    }
    // Exclusive conflicts against another active grant for the same target.
    let mut exclusive_conflict = false;
    for g in &grants {
        let tk: String = g.get("target_kind");
        let tid: Uuid = g.get("target_id");
        let rt: String = g.get("right_type");
        if g.get::<bool, _>("exclusive") {
            let other: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM rights.grant_atoms WHERE org_id=$1 AND target_kind=$2 AND target_id=$3 AND right_type=$4 AND exclusive AND revoked_at IS NULL AND id<>$5",
            )
            .bind(ctx.org).bind(&tk).bind(tid).bind(&rt).bind(g.get::<Uuid,_>("id"))
            .fetch_one(&mut *tx)
            .await?;
            if other > 0 {
                exclusive_conflict = true;
            }
        }
    }
    if exclusive_conflict {
        out.push(ReviewCheck {
            check_code: "S2_RIGHTS_SCOPE",
            status: "REVIEW_REQUIRED",
            detail: "exclusive grant conflict requires human review".into(),
        });
        return Ok(out);
    }

    // Auto-pass allowlist (§5.9).
    if needs_extra_grant {
        // New rights conditions are never auto-passed.
        out.push(ReviewCheck {
            check_code: "S2_RIGHTS_SCOPE",
            status: "REVIEW_REQUIRED",
            detail: format!(
                "special flags require additional grants: {}",
                flags.join(",")
            ),
        });
    } else if path == "label" {
        // (b) verified label contract within scope.
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rights.contracts c JOIN rights.contract_revisions r ON r.org_id=c.org_id AND r.contract_id=c.id WHERE c.org_id=$1 AND r.policy_version<>'REVOKED'",
        )
        .bind(ctx.org)
        .fetch_one(&mut *tx)
        .await?;
        let covered: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rights.grant_atoms WHERE org_id=$1 AND contract_revision_id IS NOT NULL AND revoked_at IS NULL",
        )
        .bind(ctx.org)
        .fetch_one(&mut *tx)
        .await?;
        if active > 0 && covered > 0 {
            out.push(ReviewCheck {
                check_code: "S2_RIGHTS_SCOPE",
                status: "PASS",
                detail: "allowlist(b): verified label contract scope".into(),
            });
        } else {
            out.push(ReviewCheck {
                check_code: "S2_RIGHTS_SCOPE",
                status: "REVIEW_REQUIRED",
                detail: "label path without verifiable active contract scope".into(),
            });
        }
    } else {
        // (a) self rights-holder, general release, no conflicts.
        out.push(ReviewCheck {
            check_code: "S2_RIGHTS_SCOPE",
            status: "PASS",
            detail: "allowlist(a): self rights-holder, no conflicting grants".into(),
        });
    }

    // 2-B documents: AUDENIQ-generated consent hash was verified at submit.
    out.push(ReviewCheck {
        check_code: "S2_DOCS_ORIGIN",
        status: "PASS",
        detail: "allowlist(c): AUDENIQ-generated consent package hash verified at submit; external PDFs/scans never auto-passed".into(),
    });
    Ok(out)
}

// ---------------------------------------------------------------------------
// Module 2: catalog_match (2-C)
// ---------------------------------------------------------------------------

async fn module_catalog_match(tx: &mut PgConnection, ctx: &Ctx) -> Result<Vec<ReviewCheck>> {
    let mut out = Vec::new();
    let tracks = ctx
        .body
        .get("tracks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // 2-C.1: ISRC/UPC matching only; Stage 2 never issues identifiers.
    let mut dup_isrc: BTreeSet<String> = BTreeSet::new();
    for t in &tracks {
        if let Some(isrc) = t.get("isrc").and_then(Value::as_str) {
            let hits: Vec<(Uuid, Uuid)> = sqlx::query_as(
                "SELECT t.release_id, t.org_id FROM catalog.tracks t JOIN catalog.releases r ON r.org_id=t.org_id AND r.id=t.release_id WHERE t.isrc=$1 AND t.release_id<>$2 AND r.status NOT IN ('WITHDRAWN','SUPERSEDED')",
            )
            .bind(isrc)
            .bind(ctx.release)
            .fetch_all(&mut *tx)
            .await?;
            for (rel, org) in hits {
                if org != ctx.org {
                    dup_isrc.insert(format!("{isrc} org={org} release={rel}"));
                }
            }
        }
    }
    // 2-C.1b: UPC claimed by another org on a live release.
    if let Some(upc) = ctx.body.pointer("/release/upc").and_then(Value::as_str) {
        let hits: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT id, org_id FROM catalog.releases WHERE upc=$1 AND id<>$2 AND org_id<>$3 AND status NOT IN ('WITHDRAWN','SUPERSEDED')",
        )
        .bind(upc)
        .bind(ctx.release)
        .bind(ctx.org)
        .fetch_all(&mut *tx)
        .await?;
        for (rel, org) in hits {
            dup_isrc.insert(format!("UPC {upc} org={org} release={rel}"));
        }
    }
    // 2-C.2: SHA-256 against the active internal asset index.
    let mut dup_sha: BTreeSet<String> = BTreeSet::new();
    for t in &tracks {
        if let Some(sha) = t.get("asset_sha256").and_then(Value::as_str) {
            let hits: Vec<Uuid> = sqlx::query_scalar(
                "SELECT DISTINCT a.org_id FROM catalog.assets a JOIN catalog.tracks t ON t.org_id=a.org_id AND t.asset_id=a.id JOIN catalog.releases r ON r.org_id=t.org_id AND r.id=t.release_id WHERE a.sha256=$1 AND a.org_id<>$2 AND r.status NOT IN ('WITHDRAWN','SUPERSEDED')",
            )
            .bind(sha)
            .bind(ctx.org)
            .fetch_all(&mut *tx)
            .await?;
            for org in hits {
                dup_sha.insert(format!("sha {org}"));
            }
        }
    }
    if dup_isrc.is_empty() && dup_sha.is_empty() {
        out.push(ReviewCheck {
            check_code: "S2_CATALOG_IDENTIFIERS",
            status: "PASS",
            detail: "no conflicting ISRC/asset-SHA claims in other orgs".into(),
        });
    } else {
        let mut d = dup_isrc.into_iter().collect::<Vec<_>>();
        d.extend(dup_sha);
        out.push(ReviewCheck {
            check_code: "S2_CATALOG_IDENTIFIERS",
            status: "REVIEW_REQUIRED",
            detail: format!(
                "DUPLICATE_CLAIM candidates (not infringement findings): {}",
                d.join("; ")
            ),
        });
    }
    // 2-C.3: fingerprint is REVIEW_ONLY policy; not computed in F3.
    out.push(ReviewCheck {
        check_code: "S2_CATALOG_FINGERPRINT",
        status: "NOT_APPLICABLE",
        detail: "fingerprint policy recorded as REVIEW_ONLY; comparison engine is F4+".into(),
    });
    Ok(out)
}

// ---------------------------------------------------------------------------
// Module 3: metadata_content (2-D, 2-E, 2-F)
// ---------------------------------------------------------------------------

async fn module_metadata_content(ctx: &Ctx) -> Result<Vec<ReviewCheck>> {
    let mut out = Vec::new();
    // 2-D: credit cross-check — roles are taken as declared; unknown
    // participants are never invented.
    let tracks = ctx
        .body
        .get("tracks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut empty_credit_tracks = 0;
    for t in &tracks {
        let n = t
            .get("credits")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if n == 0 {
            empty_credit_tracks += 1;
        }
    }
    if empty_credit_tracks > 0 {
        out.push(ReviewCheck {
            check_code: "S2_META_CREDITS",
            status: "CORRECTION_REQUIRED",
            detail: format!("{empty_credit_tracks} track(s) declare no credits"),
        });
    } else {
        out.push(ReviewCheck {
            check_code: "S2_META_CREDITS",
            status: "PASS",
            detail: "credits declared per track; no participants invented".into(),
        });
    }
    // 2-E: reuse 1-C measurement hashes pinned in the validation package;
    // never re-run the full probe.
    let assets = ctx
        .validation_body
        .get("validated_assets")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if assets > 0 {
        out.push(ReviewCheck {
            check_code: "S2_CONTENT_SIGNALS",
            status: "PASS",
            detail: format!(
                "reused {assets} 1-C metric hashes from validation package; no re-probe"
            ),
        });
    } else {
        out.push(ReviewCheck {
            check_code: "S2_CONTENT_SIGNALS",
            status: "TECHNICAL_RETRY",
            detail: "no validated assets in Stage 1 package".into(),
        });
    }
    // 2-F: special content — 1-B confirmed documents were *submitted*; 2-B
    // judges authenticity. External evidence never auto-passes.
    let flags = special_flags(ctx);
    if flags.is_empty() {
        out.push(ReviewCheck {
            check_code: "S2_SPECIAL_FLAGS",
            status: "NOT_APPLICABLE",
            detail: "no special content flags".into(),
        });
    } else {
        out.push(ReviewCheck {
            check_code: "S2_SPECIAL_FLAGS",
            status: "REVIEW_REQUIRED",
            detail: format!(
                "special content requires rights judgement: {}",
                flags.join(",")
            ),
        });
    }
    // 2-F.2: undeclared-content tripwire. Titles that advertise special
    // content without the matching declaration route to human review.
    // Token-boundary matching only ("discovery" must not match "cover").
    let hits = undeclared_content_hits(ctx);
    if !hits.is_empty() {
        out.push(ReviewCheck {
            check_code: "S2_UNDECLARED_CONTENT",
            status: "REVIEW_REQUIRED",
            detail: format!(
                "title advertises special content without declaration: {}",
                hits.join("; ")
            ),
        });
    }
    // 2-F.3: release date more than a year out is almost always a typo
    // (2037 vs 2027). A human confirms before DSPs receive a far-future
    // street date.
    if let Some(rdate) = ctx
        .body
        .pointer("/release/draft/release_date")
        .and_then(Value::as_str)
        && let Ok(d) = chrono::NaiveDate::parse_from_str(rdate, "%Y-%m-%d")
        && d > chrono::Utc::now().date_naive() + chrono::Duration::days(365)
    {
        out.push(ReviewCheck {
            check_code: "S2_RELEASE_DATE_FAR_FUTURE",
            status: "REVIEW_REQUIRED",
            detail: format!("release_date {rdate} is more than a year out"),
        });
    }
    // 2-F.4: a street date before 1950 is a placeholder or typo (sandbox
    // round 2: 1900-01-01 was delivered unflagged). Genuine historic catalog
    // backfills are confirmed by a human.
    if let Some(rdate) = ctx
        .body
        .pointer("/release/draft/release_date")
        .and_then(Value::as_str)
        && let Ok(d) = chrono::NaiveDate::parse_from_str(rdate, "%Y-%m-%d")
        && d < chrono::NaiveDate::from_ymd_opt(1950, 1, 1).expect("valid date")
    {
        out.push(ReviewCheck {
            check_code: "S2_RELEASE_DATE_FAR_PAST",
            status: "REVIEW_REQUIRED",
            detail: format!(
                "release_date {rdate} is before 1950; confirm it is the real street date, not a placeholder"
            ),
        });
    }
    Ok(out)
}

/// Scan release + track titles for special-content indicators that lack the
/// matching submit-time declaration. Returns human-readable hit strings.
fn undeclared_content_hits(ctx: &Ctx) -> Vec<String> {
    let Some(decl) = ctx.body.get("declarations") else {
        return Vec::new();
    };
    let declared = |key: &str| decl.get(key).and_then(Value::as_bool).unwrap_or(false);
    // (indicator tokens, declaration key, label)
    const RULES: &[(&[&str], &str, &str)] = &[
        (&["cover", "커버"], "is_cover", "COVER"),
        (&["remix", "리믹스"], "is_remix", "REMIX"),
        (
            &["suno", "udio", "aicover", "aigenerated", "aivoice"],
            "ai_involved",
            "AI",
        ),
    ];
    let mut titles: Vec<String> = Vec::new();
    if let Some(t) = ctx.body.pointer("/release/title").and_then(Value::as_str) {
        titles.push(t.to_string());
    }
    if let Some(tracks) = ctx.body.get("tracks").and_then(Value::as_array) {
        for t in tracks {
            if let Some(title) = t.get("title").and_then(Value::as_str) {
                titles.push(title.to_string());
            }
        }
    }
    let mut hits = Vec::new();
    for title in &titles {
        let tokens: Vec<String> = title
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        // "ai cover" / "ai generated" arrive as separate tokens.
        let joined = tokens.join(" ");
        for (indicators, decl_key, label) in RULES {
            if declared(decl_key) {
                continue;
            }
            let hit = indicators.iter().any(|ind| {
                tokens.iter().any(|t| t == ind)
                    || joined.contains(&format!("ai {ind}"))
                    || joined.contains(&format!("ai-{ind}"))
            });
            if hit {
                hits.push(format!("{label} in title {title:?}"));
            }
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// Module 4: policy_integrity (2-G, 2-H)
// ---------------------------------------------------------------------------

fn approved_scope(checks: &[ReviewCheck]) -> Value {
    for c in checks {
        if c.check_code == "S2_DSP_ELIGIBILITY" {
            // detail is "eligible: dsp1,dsp2 | ..." or "eligible: (none) | ..."
            if let Some(list) = c.detail.strip_prefix("eligible: ") {
                let dsps: Vec<&str> = list
                    .split(" | ")
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != "(none)")
                    .collect();
                return json!({"dsp_ids": dsps});
            }
        }
    }
    json!({"dsp_ids": []})
}

async fn module_policy_integrity(tx: &mut PgConnection, ctx: &Ctx) -> Result<Vec<ReviewCheck>> {
    let mut out = Vec::new();
    // 2-G: eligibility from real route data only. F1 schema keeps every route
    // disabled and every endpoint INTEGRATION_PENDING, so the honest answer
    // today is INELIGIBLE_NO_CONTRACT for all candidates.
    let routes = sqlx::query(
        "SELECT r.dsp_id, r.enabled, e.integration_status, r.contract_id FROM distribution.route_plans r JOIN distribution.dsp_endpoints e ON e.org_id=r.org_id AND e.dsp_id=r.dsp_id AND e.id=r.endpoint_id WHERE r.org_id=$1",
    )
    .bind(ctx.org)
    .fetch_all(&mut *tx)
    .await?;
    let mut eligible = Vec::new();
    let mut ineligible = Vec::new();
    for r in &routes {
        let dsp: Uuid = r.get("dsp_id");
        let enabled: bool = r.get("enabled");
        let ist: String = r.get("integration_status");
        let active_contract: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rights.contract_revisions WHERE org_id=$1 AND contract_id=$2 AND policy_version<>'REVOKED'",
        )
        .bind(ctx.org)
        .bind(r.get::<Uuid, _>("contract_id"))
        .fetch_one(&mut *tx)
        .await?;
        if enabled && ist == "ACTIVE" && active_contract > 0 {
            eligible.push(dsp.to_string());
        } else {
            ineligible.push(format!("{dsp}=INELIGIBLE_NO_CONTRACT"));
        }
    }
    // F5: adapter profiles are the activation record for delivery partners
    // (MockDSP in tests, contracted partners from F6 on). A MOCK profile with
    // delivery_enabled marks a test partner the org has actually activated,
    // so its dsp_id joins the eligible set alongside contracted routes.
    // CONTRACTED profiles are deliberately excluded here: for a commercial
    // partner delivery_enabled is only an operator kill-switch, and
    // eligibility requires the contract route (route enabled + endpoint
    // ACTIVE + non-revoked contract revision) checked above. Without this, a
    // CONTRACTED profile with delivery_enabled=true would bypass the contract
    // path and reach the wire. The F1 route_plans table can never be enabled
    // (CHECK constraint), so without the MOCK branch the frozen route plan
    // would always be empty and E-0 could never enqueue a delivery job.
    // Review otherwise runs without RLS auth; set the org on this tx so the
    // execution-table policy sees the right tenant.
    sqlx::query("SELECT set_config('app.org_id', $1, true)")
        .bind(ctx.org.to_string())
        .execute(&mut *tx)
        .await?;
    let activated: Vec<String> = sqlx::query_scalar(
        "SELECT dsp_id::text FROM execution.adapter_profiles WHERE delivery_enabled AND dsp_id IS NOT NULL AND activation_kind='MOCK'",
    )
    .fetch_all(&mut *tx)
    .await?;
    for dsp in activated {
        if !eligible.contains(&dsp) {
            eligible.push(dsp);
        }
    }
    // CONTRACTED profiles with delivery_enabled but no contract route are
    // explicitly ineligible (not silently absent), so the audit trail shows
    // the contract bypass was refused.
    let contracted: Vec<String> = sqlx::query_scalar(
        "SELECT dsp_id::text FROM execution.adapter_profiles WHERE delivery_enabled AND dsp_id IS NOT NULL AND activation_kind='CONTRACTED'",
    )
    .fetch_all(&mut *tx)
    .await?;
    for dsp in contracted {
        if !eligible.contains(&dsp) {
            ineligible.push(format!("{dsp}=INELIGIBLE_NO_CONTRACT"));
        }
    }
    let el = if eligible.is_empty() {
        "(none)".into()
    } else {
        eligible.join(",")
    };
    out.push(ReviewCheck {
        check_code: "S2_DSP_ELIGIBILITY",
        status: "PASS",
        detail: format!("eligible: {el} | ineligible: {}", ineligible.join(",")),
    });
    // 2-H: duplicate applications of the same bytes under another release.
    let dups: Vec<Uuid> = sqlx::query_scalar(
        "SELECT release_id FROM catalog.application_revisions WHERE org_id=$1 AND body_hash=$2 AND release_id<>$3 LIMIT 5",
    )
    .bind(ctx.org)
    .bind(&ctx.body_hash)
    .bind(ctx.release)
    .fetch_all(&mut *tx)
    .await?;
    if dups.is_empty() {
        out.push(ReviewCheck {
            check_code: "S2_INTEGRITY_DUP",
            status: "PASS",
            detail: "no duplicate applications of the same body hash".into(),
        });
    } else {
        out.push(ReviewCheck {
            check_code: "S2_INTEGRITY_DUP",
            status: "REVIEW_REQUIRED",
            detail: format!("same body submitted under {} other release(s)", dups.len()),
        });
    }
    out.push(ReviewCheck {
        check_code: "S2_INTEGRITY_DISPUTES",
        status: "NOT_APPLICABLE",
        detail: "no dispute registry in F3; open disputes are F4+".into(),
    });
    Ok(out)
}

// ---------------------------------------------------------------------------
// commercial split snapshot (§5.8)
// ---------------------------------------------------------------------------

/// Pin the commercial split referenced by the verification package. F3 pins
/// the claimant set from the revision body: equal shares across credited
/// rights-holder parties unless the draft declares otherwise. Settlement must
/// use this pinned snapshot, never a fresh contract lookup.
async fn commercial_split_snapshot(tx: &mut PgConnection, ctx: &Ctx) -> Result<Value> {
    let mut parties: BTreeSet<String> = BTreeSet::new();
    if let Some(tracks) = ctx.body.get("tracks").and_then(Value::as_array) {
        for t in tracks {
            if let Some(credits) = t.get("credits").and_then(Value::as_array) {
                for cr in credits {
                    if let Some(p) = cr.get("party_id").and_then(Value::as_str) {
                        parties.insert(p.to_string());
                    }
                }
            }
        }
    }
    let contract_revision_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT contract_revision_id FROM rights.grant_atoms WHERE org_id=$1 AND contract_revision_id IS NOT NULL AND revoked_at IS NULL LIMIT 1",
    )
    .bind(ctx.org)
    .fetch_optional(&mut *tx)
    .await?;
    let n = parties.len().max(1) as i64;
    let share_bps = 10_000 / n;
    Ok(json!({
        "payee_party_ids": parties.into_iter().collect::<Vec<_>>(),
        "share_bps": share_bps,
        "contract_revision_id": contract_revision_id,
        "effective_model": "EQUAL_SPLIT_F3",
        "pinned_at_revision": ctx.revision_id,
    }))
}

// ---------------------------------------------------------------------------
// Overrides (2-I) with separation of duties (sandbox round 2.5)
//
//   POST /api/orgs/{org}/reviews/overrides                     request/apply
//   GET  /api/orgs/{org}/reviews/overrides                     pending requests
//   POST /api/orgs/{org}/reviews/overrides/{request}/approve   second person
//   POST /api/orgs/{org}/reviews/overrides/{request}/decline   withdraw/decline
//
// Policy (docs/REVIEW_OVERRIDES.md):
// - A stricter override (anything but PASS) by an OWNER/EDITOR with write
//   access applies immediately.
// - PASS on a low-risk code (LOW_RISK_SELF_APPROVABLE) may be applied by an
//   OWNER alone; it is audited as `override.self_approved`.
// - Every other PASS (rights/money classes, catalog identifiers, duplicates,
//   content signals, ...) becomes a PENDING request. A *different* member
//   approves it from their own session; the requester can never name the
//   approver. Rights/money-class PASS also needs an OWNER requester.
// - Approver eligibility: ACTIVE membership, role OWNER or EDITOR (never
//   VIEWER), accepted invitation at least MIN_APPROVER_TENURE_HOURS ago
//   (legacy memberships from before migration 0036 count as accepted), and
//   read access to the release.
// - An applied override re-evaluates a release parked in STAGE2_REVIEW.
// ---------------------------------------------------------------------------

/// PASS overrides a sole OWNER may apply without a second person.
pub const LOW_RISK_SELF_APPROVABLE: &[&str] = &[
    "S2_RELEASE_DATE_FAR_FUTURE",
    "S2_RELEASE_DATE_FAR_PAST",
    "S2_META_CREDITS",
];
/// Minimum time since a second approver accepted their membership.
pub const MIN_APPROVER_TENURE_HOURS: i32 = 72;
/// Upper bound on an override reason (the sandbox stored 300 x 60 KB).
pub const MAX_OVERRIDE_REASON_CHARS: usize = 2000;
const OVERRIDE_STATUSES: &[&str] = &["PASS", "CORRECTION_REQUIRED", "REVIEW_REQUIRED", "BLOCKED"];
const RIGHTS_MONEY_CLASSES: &[&str] = &["S2_RIGHTS_SCOPE", "S2_DOCS_ORIGIN", "S2_SPECIAL_FLAGS"];

/// True when forcing `proposed_status` on `check_code` needs a second person.
pub fn needs_second_approver(check_code: &str, proposed_status: &str) -> bool {
    proposed_status == "PASS" && !LOW_RISK_SELF_APPROVABLE.contains(&check_code)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideInput {
    pub revision_id: Uuid,
    pub check_code: String,
    pub proposed_status: String,
    pub reason: String,
    /// Accepted only to give a clear error: the second approver must approve
    /// from their own session, never be named by the requester.
    pub second_approver_user_id: Option<Uuid>,
}

fn validate_override(proposed_status: &str, reason: &str) -> Result<()> {
    if !OVERRIDE_STATUSES.contains(&proposed_status) {
        return Err(Error::PolicyGate("INVALID_OVERRIDE_STATUS"));
    }
    if reason.trim().is_empty() {
        return Err(Error::PolicyGate("OVERRIDE_REASON_REQUIRED"));
    }
    if reason.chars().count() > MAX_OVERRIDE_REASON_CHARS {
        return Err(Error::PolicyGate("OVERRIDE_REASON_TOO_LONG"));
    }
    crate::text_policy::check_multiline(reason)?;
    Ok(())
}

/// Latest recorded status of `check_code` on the revision (NotFound if the
/// check never ran for it).
async fn original_status(
    c: &mut PgConnection,
    revision_id: Uuid,
    check_code: &str,
) -> Result<String> {
    let original: Option<String> = sqlx::query_scalar(
        "SELECT status FROM operations.check_results WHERE revision_id=$1 AND check_code=$2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(revision_id)
    .bind(check_code)
    .fetch_optional(&mut *c)
    .await?;
    original.ok_or(Error::NotFound)
}

/// Second-approver eligibility (see module policy). Non-members get 403 so
/// membership is not disclosed; members that fail a rule get a 422 code.
pub async fn check_approver_eligible(
    c: &mut PgConnection,
    org: Uuid,
    approver: Uuid,
) -> Result<()> {
    let row = sqlx::query(
        "SELECT m.role, m.accepted_at IS NULL AS legacy,
                COALESCE(m.accepted_at <= now() - make_interval(hours => $3), false) AS tenured
           FROM identity.memberships m JOIN identity.users u ON u.id=m.user_id
          WHERE m.org_id=$1 AND m.user_id=$2 AND m.status='ACTIVE' AND u.status='ACTIVE'
          FOR SHARE OF m",
    )
    .bind(org)
    .bind(approver)
    .bind(MIN_APPROVER_TENURE_HOURS)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::Forbidden)?;
    let role: String = row.get("role");
    if !matches!(role.as_str(), "OWNER" | "EDITOR") {
        return Err(Error::PolicyGate("APPROVER_ROLE_NOT_ELIGIBLE"));
    }
    let legacy: bool = row.get("legacy");
    let tenured: bool = row.get("tenured");
    if !legacy && !tenured {
        return Err(Error::PolicyGate("APPROVER_TENURE_TOO_SHORT"));
    }
    Ok(())
}

async fn release_of(c: &mut PgConnection, org: Uuid, revision_id: Uuid) -> Result<Uuid> {
    sqlx::query_scalar(
        "SELECT release_id FROM catalog.application_revisions WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(revision_id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

async fn active_role(c: &mut PgConnection, org: Uuid, user: Uuid) -> Result<String> {
    sqlx::query_scalar(
        "SELECT m.role FROM identity.memberships m WHERE m.org_id=$1 AND m.user_id=$2 AND m.status='ACTIVE'",
    )
    .bind(org)
    .bind(user)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::Forbidden)
}

/// Write one override row (append-only) in the caller's transaction.
#[allow(clippy::too_many_arguments)]
async fn insert_override(
    c: &mut PgConnection,
    org: Uuid,
    revision_id: Uuid,
    check_code: &str,
    original: &str,
    proposed_status: &str,
    reason: &str,
    actor: Uuid,
    second_approver: Option<Uuid>,
) -> Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO rights.review_overrides(id, org_id, revision_id, check_code, original_status, proposed_status, reason, actor_user_id, second_approver_user_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(id).bind(org).bind(revision_id).bind(check_code).bind(original).bind(proposed_status).bind(reason).bind(actor).bind(second_approver)
        .execute(&mut *c)
        .await?;
    Ok(id)
}

/// Overrides only change the outcome when Stage 2 decides again. If the
/// release is parked in STAGE2_REVIEW on this revision, queue a re-run of
/// Stage 2 (same validation package as the original run). Returns whether a
/// job was queued. At most one queued re-run per revision at a time.
pub async fn enqueue_reevaluation(
    c: &mut PgConnection,
    org: Uuid,
    revision_id: Uuid,
    cause: Uuid,
) -> Result<bool> {
    let status: Option<String> = sqlx::query_scalar(
        "SELECT rel.status FROM catalog.application_revisions r
           JOIN catalog.releases rel ON rel.org_id=r.org_id AND rel.id=r.release_id AND rel.current_revision_id=r.id
          WHERE r.org_id=$1 AND r.id=$2",
    )
    .bind(org)
    .bind(revision_id)
    .fetch_optional(&mut *c)
    .await?;
    if status.as_deref() != Some("STAGE2_REVIEW") {
        return Ok(false);
    }
    let queued: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.jobs WHERE kind='stage2' AND pinned_revision_id=$1 AND status='QUEUED')",
    )
    .bind(revision_id)
    .fetch_one(&mut *c)
    .await?;
    if queued {
        return Ok(true);
    }
    let payload: Option<Value> = sqlx::query_scalar(
        "SELECT payload FROM operations.jobs WHERE kind='stage2' AND pinned_revision_id=$1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(revision_id)
    .fetch_optional(&mut *c)
    .await?;
    let Some(payload) = payload else {
        return Ok(false);
    };
    let n = sqlx::query("INSERT INTO operations.jobs(id, queue, kind, payload, pinned_revision_id, idempotency_key) VALUES($1,'rights','stage2',$2,$3,$4) ON CONFLICT(idempotency_key) DO NOTHING")
        .bind(Uuid::new_v4())
        .bind(payload)
        .bind(revision_id)
        .bind(format!("stage2:{revision_id}:override:{cause}"))
        .execute(&mut *c)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// POST /reviews/overrides. Applies a stricter or low-risk override, or
/// files a PENDING request that needs a second person.
pub async fn create_override_api(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    input: OverrideInput,
) -> Result<Value> {
    if input.second_approver_user_id.is_some() {
        return Err(Error::PolicyGate(
            "SECOND_APPROVER_MUST_APPROVE_IN_OWN_SESSION",
        ));
    }
    validate_override(&input.proposed_status, &input.reason)?;
    let mut tx = s.pool.begin().await?;
    let release = release_of(&mut tx, org, input.revision_id).await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let role = active_role(&mut tx, org, a.user).await?;
    let original = original_status(&mut tx, input.revision_id, &input.check_code).await?;

    if needs_second_approver(&input.check_code, &input.proposed_status) {
        if RIGHTS_MONEY_CLASSES.contains(&input.check_code.as_str()) && role != "OWNER" {
            return Err(Error::PolicyGate("SENIOR_REVIEWER_REQUIRED"));
        }
        // One open request per (revision, check, status): repeats return it.
        let existing = sqlx::query(
            "SELECT id, expires_at FROM rights.override_requests WHERE org_id=$1 AND revision_id=$2 AND check_code=$3 AND proposed_status=$4 AND status='PENDING' AND expires_at>now() ORDER BY created_at DESC LIMIT 1",
        )
        .bind(org)
        .bind(input.revision_id)
        .bind(&input.check_code)
        .bind(&input.proposed_status)
        .fetch_optional(&mut *tx)
        .await?;
        let (id, expires_at): (Uuid, chrono::DateTime<chrono::Utc>) = match existing {
            Some(r) => (r.get("id"), r.get("expires_at")),
            None => {
                let id = Uuid::new_v4();
                let expires_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
                    "INSERT INTO rights.override_requests(id, org_id, revision_id, check_code, original_status, proposed_status, reason, requested_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING expires_at",
                )
                .bind(id)
                .bind(org)
                .bind(input.revision_id)
                .bind(&input.check_code)
                .bind(&original)
                .bind(&input.proposed_status)
                .bind(&input.reason)
                .bind(a.user)
                .fetch_one(&mut *tx)
                .await?;
                operations::audit(
                    &mut tx,
                    Some(a.user),
                    Some(org),
                    Some(id),
                    "override.requested",
                    "REVIEWER_REQUEST",
                    a.request,
                )
                .await?;
                (id, expires_at)
            }
        };
        tx.commit().await?;
        return Ok(json!({
            "status": "PENDING_SECOND_APPROVAL",
            "override_request_id": id,
            "expires_at": expires_at,
        }));
    }

    if input.proposed_status == "PASS" && role != "OWNER" {
        // Low-risk self-approval is an OWNER privilege.
        return Err(Error::PolicyGate("SENIOR_REVIEWER_REQUIRED"));
    }
    // Repeating the override that is already in effect changes nothing (and
    // must not bump the rights epoch or queue work again).
    let current: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, proposed_status FROM rights.review_overrides WHERE org_id=$1 AND revision_id=$2 AND check_code=$3 AND (expires_at IS NULL OR expires_at>now()) ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(org)
    .bind(input.revision_id)
    .bind(&input.check_code)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((id, status)) = current
        && status == input.proposed_status
    {
        tx.commit().await?;
        return Ok(
            json!({"override_id": id, "status": "ALREADY_APPLIED", "reevaluation_queued": false}),
        );
    }
    let id = insert_override(
        &mut tx,
        org,
        input.revision_id,
        &input.check_code,
        &original,
        &input.proposed_status,
        &input.reason,
        a.user,
        None,
    )
    .await?;
    let action = if input.proposed_status == "PASS" {
        "override.self_approved"
    } else {
        "override.recorded"
    };
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        action,
        "REVIEWER_OVERRIDE",
        a.request,
    )
    .await?;
    let queued = enqueue_reevaluation(&mut tx, org, input.revision_id, id).await?;
    tx.commit().await?;
    Ok(json!({"override_id": id, "status": "APPLIED", "reevaluation_queued": queued}))
}

/// GET /reviews/overrides: open requests in the org (any active member).
pub async fn list_override_requests(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let rows = sqlx::query(
        "SELECT id, revision_id, check_code, original_status, proposed_status, reason, requested_by, created_at, expires_at
           FROM rights.override_requests WHERE org_id=$1 AND status='PENDING' AND expires_at>now()
          ORDER BY created_at LIMIT 200",
    )
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "revision_id": r.get::<Uuid, _>("revision_id"),
                "check_code": r.get::<String, _>("check_code"),
                "original_status": r.get::<String, _>("original_status"),
                "proposed_status": r.get::<String, _>("proposed_status"),
                "reason": r.get::<String, _>("reason"),
                "requested_by": r.get::<Uuid, _>("requested_by"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                "expires_at": r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at"),
            })
        })
        .collect();
    Ok(json!({"items": items}))
}

struct PendingRequest {
    revision_id: Uuid,
    check_code: String,
    original_status: String,
    proposed_status: String,
    reason: String,
    requested_by: Uuid,
}

async fn lock_pending(c: &mut PgConnection, org: Uuid, request_id: Uuid) -> Result<PendingRequest> {
    let r = sqlx::query(
        "SELECT revision_id, check_code, original_status, proposed_status, reason, requested_by, status, expires_at>now() AS live
           FROM rights.override_requests WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(request_id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    if r.get::<String, _>("status") != "PENDING" {
        return Err(Error::Conflict);
    }
    if !r.get::<bool, _>("live") {
        return Err(Error::PolicyGate("OVERRIDE_REQUEST_EXPIRED"));
    }
    Ok(PendingRequest {
        revision_id: r.get("revision_id"),
        check_code: r.get("check_code"),
        original_status: r.get("original_status"),
        proposed_status: r.get("proposed_status"),
        reason: r.get("reason"),
        requested_by: r.get("requested_by"),
    })
}

/// POST /reviews/overrides/{request}/approve: the second person, acting in
/// their own authenticated session.
pub async fn approve_override_request(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    request_id: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    // Membership first so non-members learn nothing about request ids.
    auth::membership(&mut tx, a, org, false).await?;
    let req = lock_pending(&mut tx, org, request_id).await?;
    if req.requested_by == a.user {
        return Err(Error::PolicyGate("SECOND_APPROVER_MUST_DIFFER"));
    }
    check_approver_eligible(&mut tx, org, a.user).await?;
    let release = release_of(&mut tx, org, req.revision_id).await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    // The requester must still be entitled to ask (not revoked meanwhile).
    let requester_role = active_role(&mut tx, org, req.requested_by)
        .await
        .map_err(|_| Error::PolicyGate("REQUESTER_NO_LONGER_MEMBER"))?;
    if RIGHTS_MONEY_CLASSES.contains(&req.check_code.as_str()) && requester_role != "OWNER" {
        return Err(Error::PolicyGate("SENIOR_REVIEWER_REQUIRED"));
    }
    let id = insert_override(
        &mut tx,
        org,
        req.revision_id,
        &req.check_code,
        &req.original_status,
        &req.proposed_status,
        &req.reason,
        req.requested_by,
        Some(a.user),
    )
    .await?;
    sqlx::query("UPDATE rights.override_requests SET status='APPROVED', decided_by=$3, decided_at=now(), override_id=$4 WHERE org_id=$1 AND id=$2")
        .bind(org)
        .bind(request_id)
        .bind(a.user)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "override.approved",
        "SECOND_APPROVER",
        a.request,
    )
    .await?;
    let queued = enqueue_reevaluation(&mut tx, org, req.revision_id, id).await?;
    tx.commit().await?;
    Ok(json!({"override_id": id, "status": "APPLIED", "reevaluation_queued": queued}))
}

/// POST /reviews/overrides/{request}/decline: the requester withdraws, or
/// another OWNER/EDITOR declines.
pub async fn decline_override_request(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    request_id: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let role = auth::membership(&mut tx, a, org, false).await?;
    let req = lock_pending(&mut tx, org, request_id).await?;
    if req.requested_by != a.user && !matches!(role.as_str(), "OWNER" | "EDITOR") {
        return Err(Error::Forbidden);
    }
    sqlx::query("UPDATE rights.override_requests SET status='DECLINED', decided_by=$3, decided_at=now() WHERE org_id=$1 AND id=$2")
        .bind(org)
        .bind(request_id)
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(request_id),
        "override.declined",
        "REVIEWER_DECISION",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"status": "DECLINED"}))
}

/// Inputs for [`record_override`].
pub struct OverrideRequest<'a> {
    pub org: Uuid,
    pub actor: Uuid,
    pub revision_id: Uuid,
    pub check_code: &'a str,
    pub proposed_status: &'a str,
    pub reason: &'a str,
    pub second_approver: Option<Uuid>,
    pub senior: bool,
}

/// Library-level override writer (operator tooling and tests; the API uses
/// the request/approve flow above). Enforces the same rules: a PASS that
/// needs a second person requires a *different*, eligible approver (ACTIVE,
/// OWNER/EDITOR, accepted and tenured), and rights/money-class PASS also a
/// senior actor. The caller vouches that `second_approver` really approved.
pub async fn record_override(pool: &PgPool, r: OverrideRequest<'_>) -> Result<Uuid> {
    validate_override(r.proposed_status, r.reason)?;
    let mut tx = pool.begin().await?;
    let original = original_status(&mut tx, r.revision_id, r.check_code).await?;
    if needs_second_approver(r.check_code, r.proposed_status) {
        if RIGHTS_MONEY_CLASSES.contains(&r.check_code) && !r.senior {
            return Err(Error::PolicyGate("SENIOR_REVIEWER_REQUIRED"));
        }
        match r.second_approver {
            Some(sa) if sa != r.actor => match check_approver_eligible(&mut tx, r.org, sa).await {
                Ok(()) => {}
                Err(Error::Forbidden) => {
                    return Err(Error::PolicyGate("SECOND_APPROVER_NOT_MEMBER"));
                }
                Err(e) => return Err(e),
            },
            _ => return Err(Error::PolicyGate("SECOND_APPROVER_REQUIRED")),
        }
    }
    let id = insert_override(
        &mut tx,
        r.org,
        r.revision_id,
        r.check_code,
        &original,
        r.proposed_status,
        r.reason,
        r.actor,
        r.second_approver,
    )
    .await?;
    enqueue_reevaluation(&mut tx, r.org, r.revision_id, id).await?;
    tx.commit().await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;

    fn check(code: &'static str, status: &'static str, detail: &str) -> ReviewCheck {
        ReviewCheck {
            check_code: code,
            status,
            detail: detail.into(),
        }
    }

    #[sqlx::test]
    async fn checkpoint_dedupes_identical_results(pool: sqlx::PgPool) {
        database::MIGRATOR.run(&pool).await.unwrap();
        let org = Uuid::new_v4();
        let release = Uuid::new_v4();
        let user = Uuid::new_v4();
        let revision_id = Uuid::new_v4();
        let hash = "c".repeat(64);
        sqlx::query("INSERT INTO identity.orgs(id, name, kind) VALUES($1,'t','LABEL')")
            .bind(org)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO identity.parties(id, org_id, kind, display_name) VALUES($1,$2,'PERSON','t')")
            .bind(user)
            .bind(org)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO identity.users(id, email, password_hash, party_id) VALUES($1,'t@t','x',$1)")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO identity.resources(org_id, id, kind) VALUES($1,$2,'release')")
            .bind(org)
            .bind(release)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO catalog.releases(id, org_id, title, release_type) VALUES($1,$2,'t','SINGLE')")
            .bind(release)
            .bind(org)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO catalog.application_revisions(id, org_id, release_id, revision, body, body_hash, consent_package_hash, created_by) VALUES($1,$2,$3,1,'{}',$4,$4,$5)")
            .bind(revision_id)
            .bind(org)
            .bind(release)
            .bind(&hash)
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let c = check("S2_RIGHTS_SCOPE", "PASS", "ok");
        let first = record_check_result(&mut tx, revision_id, &c).await.unwrap();
        // Same check again (simulated retry): no duplicate row.
        let second = record_check_result(&mut tx, revision_id, &c).await.unwrap();
        assert_eq!(first, second);
        // Changed detail = new result: recorded as a new row, never stale.
        let changed = check("S2_RIGHTS_SCOPE", "PASS", "ok-changed");
        let third = record_check_result(&mut tx, revision_id, &changed)
            .await
            .unwrap();
        assert_ne!(first, third);
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM operations.check_results WHERE revision_id=$1",
        )
        .bind(revision_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(n, 2);
        tx.commit().await.unwrap();
    }
}
