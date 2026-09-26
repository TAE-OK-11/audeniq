//! Delivery staging: everything up to the wire, per requested DSP.
//!
//! After Stage 3 freezes a package (READY_FOR_DELIVERY) the `delivery.stage`
//! job evaluates the package against every DSP the artist asked for
//! (`release.draft.platforms`, frozen in the submitted revision) and records
//! one `distribution.delivery_staging` row per (package, DSP code):
//!
//! - the DSP's delivery spec checks (`dsp_registry::DspSpec`): artwork size
//!   and shape, lossless master, sample rate / bit depth, composer and
//!   lyricist credits, genre, release lead time, KR youth-harmful marking;
//! - the DDEX ERN 3.8.2 message this DSP would receive (XSD + business-rule
//!   validated). With real sender/recipient DPIDs it is also persisted as
//!   the wire artifact (`distribution.ddex_messages`); without them it is a
//!   preview built on placeholder party ids and never leaves the database;
//! - the partner side: Stage 2 approved scope, the routing engine verdict,
//!   DPIDs, and whether the codes are still virtual test codes.
//!
//! Findings are CONTENT (the release must change) or PARTNER (onboarding
//! must finish). `readiness` is CONTENT_BLOCKED, AWAITING_PARTNER or READY.
//! A staff member approves a non-content-blocked row; E-0 then enqueues the
//! send once the row is APPROVED *and* the route is live. A re-stage whose
//! ERN bytes changed resets the approval: staff approve exactly what goes out.
use crate::{
    ddex_ern,
    ddex_preset::DspMessagePreset,
    ddex_validate, ddex_xsd,
    distribution::CanonicalRelease,
    dsp_registry::{DeliveryFormat, Dsp, DspSpec, Region},
    error::{Error, Result},
    identifiers::{IdentifierKind, is_virtual},
    preparation_model::PreparedRelease,
};
use chrono::NaiveDate;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Class {
    /// The release itself must change (artist correction).
    Content,
    /// Partner onboarding / operations must finish (contract, DPID, codes).
    Partner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    Blocker,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct DspCheck {
    pub code: &'static str,
    pub class: Class,
    pub severity: Severity,
    pub detail: String,
}

impl DspCheck {
    fn new(code: &'static str, class: Class, severity: Severity, detail: String) -> Self {
        Self {
            code,
            class,
            severity,
            detail,
        }
    }
}

/// Per-track facts the spec checks need beyond `PreparedRelease`.
#[derive(Debug, Default, Clone)]
pub struct TrackFacts {
    pub has_composer: bool,
    pub has_lyricist: bool,
    pub instrumental: bool,
}

/// Everything `evaluate` reads. Pure data: no DB, no clock.
pub struct StagingInput<'a> {
    pub prepared: &'a PreparedRelease,
    pub genre: Option<&'a str>,
    /// Cover (width, height) measured by Stage 1; None when unknown.
    pub artwork_px: Option<(u32, u32)>,
    pub tracks: &'a HashMap<Uuid, TrackFacts>,
    pub today: NaiveDate,
    /// Stage 1 advisory audio findings (code, detail): loudness outside the
    /// delivery target, short clip events. Never blocking; staff must see
    /// and acknowledge them before approving a delivery.
    pub audio_advisories: &'a [(String, String)],
}

/// Codes of advisory findings that need an explicit staff acknowledgement.
pub const ACK_REQUIRED: &[&str] = &["DSP_LOUDNESS_ADVISORY", "DSP_CLIPPING_ADVISORY"];

/// `integrated_lufs=-7.5 ...` -> -7.5
fn integrated_lufs(detail: &str) -> Option<f32> {
    let rest = detail.split("integrated_lufs=").nth(1)?;
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
        .collect();
    num.parse().ok()
}

fn lossless(content_type: &str) -> bool {
    matches!(
        content_type,
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/flac" | "audio/x-flac"
    )
}

/// Spec checks of one DSP against one frozen release. Only findings are
/// returned; an empty list means the content meets the DSP's spec.
pub fn evaluate(spec: &DspSpec, i: &StagingInput<'_>) -> Vec<DspCheck> {
    use Class::*;
    use Severity::*;
    let p = i.prepared;
    let mut out = Vec::new();
    match i.artwork_px {
        Some((w, h)) => {
            if w != h {
                out.push(DspCheck::new(
                    "DSP_ARTWORK_NOT_SQUARE",
                    Content,
                    Blocker,
                    format!("{w}x{h}"),
                ));
            }
            if w.min(h) < spec.artwork_min_px {
                out.push(DspCheck::new(
                    "DSP_ARTWORK_TOO_SMALL",
                    Content,
                    Blocker,
                    format!("{w}x{h} < {}px", spec.artwork_min_px),
                ));
            }
            if let Some(max) = spec.artwork_max_px
                && w.max(h) > max
            {
                out.push(DspCheck::new(
                    "DSP_ARTWORK_TOO_LARGE",
                    Content,
                    Blocker,
                    format!("{w}x{h} > {max}px"),
                ));
            }
        }
        None => out.push(DspCheck::new(
            "DSP_ARTWORK_UNMEASURED",
            Content,
            Warning,
            "cover dimensions were not measured by Stage 1".into(),
        )),
    }
    for t in &p.tracks {
        let a = &t.audio;
        if spec.lossless_only && !lossless(&a.content_type) {
            out.push(DspCheck::new(
                "DSP_AUDIO_NOT_LOSSLESS",
                Content,
                Blocker,
                format!("track={} {}", t.id, a.content_type),
            ));
        }
        match a.sample_rate {
            Some(sr) if (sr as u32) < spec.audio_min_sample_rate => out.push(DspCheck::new(
                "DSP_AUDIO_SAMPLE_RATE_LOW",
                Content,
                Blocker,
                format!("track={} {sr}Hz", t.id),
            )),
            None => out.push(DspCheck::new(
                "DSP_AUDIO_UNMEASURED",
                Content,
                Warning,
                format!("track={}", t.id),
            )),
            _ => {}
        }
        if let Some(bits) = a.bits_per_sample
            && (bits as u32) < spec.audio_min_bits
        {
            out.push(DspCheck::new(
                "DSP_AUDIO_BIT_DEPTH_LOW",
                Content,
                Blocker,
                format!("track={} {bits}bit", t.id),
            ));
        }
        let facts = i.tracks.get(&t.id).cloned().unwrap_or_default();
        if spec.requires_composer && !facts.has_composer {
            out.push(DspCheck::new(
                "DSP_CREDIT_COMPOSER_MISSING",
                Content,
                Blocker,
                format!("track={}", t.id),
            ));
        }
        if spec.requires_lyricist && !facts.instrumental && !facts.has_lyricist {
            out.push(DspCheck::new(
                "DSP_CREDIT_LYRICIST_MISSING",
                Content,
                Blocker,
                format!("track={}", t.id),
            ));
        }
    }
    if i.genre.is_none_or(|g| g.trim().is_empty()) {
        out.push(DspCheck::new(
            "DSP_GENRE_MISSING",
            Content,
            Blocker,
            "genre is required by every DSP".into(),
        ));
    }
    let lead = (p.release_date - i.today).num_days();
    if lead < spec.lead_days {
        out.push(DspCheck::new(
            "DSP_LEAD_TIME_SHORT",
            Content,
            Warning,
            format!(
                "{lead} day(s) before release, {} needed: go-live may slip",
                spec.lead_days
            ),
        ));
    }
    for (code, detail) in i.audio_advisories {
        match code.as_str() {
            "AUDIO_LOUDNESS_OUT_OF_RANGE" => {
                let what = match integrated_lufs(detail) {
                    Some(l) => format!(
                        "integrated {l:.1} LUFS vs {} target {:.0} LUFS: the platform will turn it {} by {:.1} dB",
                        spec.code,
                        spec.loudness_target_lufs,
                        if l > spec.loudness_target_lufs {
                            "down"
                        } else {
                            "up"
                        },
                        (l - spec.loudness_target_lufs).abs()
                    ),
                    None => detail.clone(),
                };
                out.push(DspCheck::new(
                    "DSP_LOUDNESS_ADVISORY",
                    Content,
                    Warning,
                    what,
                ));
            }
            "AUDIO_CLIPPING" => out.push(DspCheck::new(
                "DSP_CLIPPING_ADVISORY",
                Content,
                Warning,
                detail.clone(),
            )),
            _ => {}
        }
    }
    if spec.region == Region::Kr && p.explicit {
        out.push(DspCheck::new(
            "DSP_KR_YOUTH_HARMFUL_MARKING",
            Content,
            Info,
            "delivered with the '19세 미만 이용 불가' marking (청소년보호법)".into(),
        ));
    }
    if is_virtual(IdentifierKind::Upc, &p.upc)
        || p.tracks
            .iter()
            .any(|t| is_virtual(IdentifierKind::Isrc, &t.isrc))
    {
        out.push(DspCheck::new(
            "DSP_IDENTIFIER_VIRTUAL",
            Partner,
            Blocker,
            "UPC/ISRC from the virtual test range: register a real issuer and re-stage".into(),
        ));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Readiness {
    ContentBlocked,
    AwaitingPartner,
    Ready,
}

impl Readiness {
    pub fn of(checks: &[DspCheck]) -> Self {
        let blocking = |c: Class| {
            checks
                .iter()
                .any(|x| x.severity == Severity::Blocker && x.class == c)
        };
        if blocking(Class::Content) {
            Readiness::ContentBlocked
        } else if blocking(Class::Partner) {
            Readiness::AwaitingPartner
        } else {
            Readiness::Ready
        }
    }

    pub fn as_db(self) -> &'static str {
        match self {
            Readiness::ContentBlocked => "CONTENT_BLOCKED",
            Readiness::AwaitingPartner => "AWAITING_PARTNER",
            Readiness::Ready => "READY",
        }
    }
}

/// Placeholder party id for preview messages. Clearly not a DDEX DPID
/// (those are `PADPIDA` + 13 chars), so it can never be mistaken for one.
fn preview_dpid(code: &str) -> String {
    format!("PREVIEW-{code}")
}

struct Ern {
    xml: String,
    sha256: String,
    message_id: String,
}

/// Build and validate the ERN this DSP would receive. Errors are returned
/// as findings so one DSP's problem never hides the others'.
#[allow(clippy::too_many_arguments)]
fn build_ern(
    spec: &DspSpec,
    prepared: &PreparedRelease,
    package_id: Uuid,
    preset: &DspMessagePreset,
    sender_name: &str,
    sender_dpid: &str,
    recipient_dpid: &str,
    created_at: &str,
) -> std::result::Result<Ern, DspCheck> {
    let fail = |code: &'static str, detail: String| {
        DspCheck::new(code, Class::Content, Severity::Blocker, detail)
    };
    let deal_start = preset
        .deal_start_date(prepared.release_date)
        .map_err(|_| fail("DSP_ERN_PRESET_INVALID", "deal start out of range".into()))?;
    let message_id = preset
        .render_message_id(
            &package_id.to_string(),
            spec.code,
            &deal_start.format("%Y%m%d").to_string(),
        )
        .map_err(|_| fail("DSP_ERN_PRESET_INVALID", "message id template".into()))?;
    let config = ddex_ern::DdexErnConfig {
        message_id: message_id.clone(),
        message_thread_id: None,
        message_sub_type: ddex_ern::MessageSubType::Initial,
        created_at: created_at.to_owned(),
        sender_name: sender_name.to_owned(),
        sender_party_id: Some(sender_dpid.to_owned()),
        sent_on_behalf_of: None,
        recipient_name: spec.name.to_owned(),
        recipient_party_id: Some(recipient_dpid.to_owned()),
        deal_start_date: deal_start.format("%Y-%m-%d").to_string(),
        takedown_date: None,
    };
    let pf = ddex_validate::preflight_release(prepared, &config);
    let errors: Vec<String> = pf
        .all()
        .filter(|f| f.is_error() || preset.escalates(&f.rule_id))
        .map(|f| format!("{}: {}", f.rule_id, f.message))
        .collect();
    if !errors.is_empty() {
        return Err(fail("DSP_ERN_PREFLIGHT", errors.join("; ")));
    }
    let xml = ddex_ern::generate_ddex_ern_382(prepared, &config)
        .map_err(|e| fail("DSP_ERN_BUILD", format!("{e}")))?;
    ddex_xsd::validate_ern_382_xml(&xml).map_err(|e| fail("DSP_ERN_XSD", format!("{e}")))?;
    let expected = if prepared.tracks.len() > 1 {
        ddex_validate::ErnProfile::AudioAlbum
    } else {
        ddex_validate::ErnProfile::AudioSingle
    };
    let report = ddex_validate::validate_ern_message(&xml, Some(expected));
    let errors: Vec<String> = report
        .errors()
        .iter()
        .map(|f| format!("{}: {}", f.rule_id, f.message))
        .collect();
    if !errors.is_empty() {
        return Err(fail("DSP_ERN_BUSINESS_RULE", errors.join("; ")));
    }
    let sha256 = hex::encode(Sha256::digest(xml.as_bytes()));
    Ok(Ern {
        xml,
        sha256,
        message_id,
    })
}

fn credit_facts(canonical: &CanonicalRelease, draft: &Value) -> HashMap<Uuid, TrackFacts> {
    // Studio keeps the instrumental flag on its draft track, linked to the
    // server track by `serverId`.
    let instrumental: HashSet<Uuid> = draft
        .get("draftTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|t| t.get("instrumental").and_then(Value::as_bool) == Some(true))
        .filter_map(|t| t.get("serverId")?.as_str()?.parse().ok())
        .collect();
    canonical
        .tracks
        .iter()
        .map(|t| {
            let has = |needles: &[&str]| {
                t.credits.iter().any(|c| {
                    let r = c.role.to_lowercase();
                    needles.iter().any(|n| r.contains(n))
                })
            };
            (
                t.track_id,
                TrackFacts {
                    has_composer: has(&["compos", "songwriter", "작곡"]),
                    has_lyricist: has(&["lyric", "songwriter", "작사"]),
                    instrumental: instrumental.contains(&t.track_id),
                },
            )
        })
        .collect()
}

/// Parse the Stage 1 cover measurement ("3000x3000").
fn parse_px(detail: &str) -> Option<(u32, u32)> {
    let detail = detail
        .trim_start_matches("cache_hit")
        .trim_start_matches(": ");
    let (w, h) = detail.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

pub struct StageSummary {
    pub staged: usize,
    pub ready: usize,
    pub awaiting_partner: usize,
    pub content_blocked: usize,
}

/// `delivery.stage` job body: (re)evaluate a frozen package for every
/// requested DSP. Idempotent; re-running after onboarding progress updates
/// the partner findings and produces the wire ERN once real DPIDs exist.
pub async fn stage_package(pool: &PgPool, package_id: Uuid) -> Result<StageSummary> {
    let row = sqlx::query(
        "SELECT dp.org_id, cr.id AS canonical_id, cr.body AS canonical, cr.release_id, cr.revision_id,
                COALESCE(NULLIF(ar.body -> 'release' -> 'draft', 'null'::jsonb), rel.draft) AS draft,
                o.name AS org_name, o.ddex_sender_dpid
         FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
         JOIN catalog.application_revisions ar ON ar.org_id = cr.org_id AND ar.id = cr.revision_id
         JOIN catalog.releases rel ON rel.org_id = cr.org_id AND rel.id = cr.release_id
         JOIN identity.orgs o ON o.id = dp.org_id
         WHERE dp.id = $1",
    )
    .bind(package_id)
    .fetch_optional(pool)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = row.get("org_id");
    let canonical_id: Uuid = row.get("canonical_id");
    let release_id: Uuid = row.get("release_id");
    let revision_id: Uuid = row.get("revision_id");
    let draft: Value = row.get::<Option<Value>, _>("draft").unwrap_or(Value::Null);
    let org_name: String = row.get("org_name");
    let sender_dpid: Option<String> = row.get("ddex_sender_dpid");
    let canonical: CanonicalRelease =
        serde_json::from_value(row.get("canonical")).map_err(|_| Error::Internal)?;
    let prepared =
        std::sync::Arc::new(PreparedRelease::from_canonical(pool, canonical_id, &canonical).await?);

    let requested = crate::dsp_registry::requested(&draft).unwrap_or_else(|| Dsp::ALL.to_vec());
    let genre = draft
        .get("genreCustom")
        .and_then(Value::as_str)
        .filter(|g| {
            !g.trim().is_empty() && draft.get("genre").and_then(Value::as_str) == Some("__other__")
        })
        .or_else(|| draft.get("genre").and_then(Value::as_str))
        .filter(|g| *g != "__other__")
        .map(str::to_owned);
    let artwork_px: Option<(u32, u32)> = sqlx::query_scalar::<_, String>(
        "SELECT detail FROM operations.check_results WHERE revision_id=$1 AND check_code='IMAGE_TOO_SMALL' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(revision_id)
    .fetch_optional(pool)
    .await?
    .as_deref()
    .and_then(parse_px);
    let audio_advisories: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT check_code, COALESCE(detail,'') FROM operations.check_results
         WHERE revision_id=$1 AND status='REVIEW_REQUIRED'
           AND check_code IN ('AUDIO_LOUDNESS_OUT_OF_RANGE','AUDIO_CLIPPING')",
    )
    .bind(revision_id)
    .fetch_all(pool)
    .await?;
    let facts = credit_facts(&canonical, &draft);
    let today = chrono::Utc::now().date_naive();
    let approved: HashSet<Uuid> = canonical.approved_dsp_ids.iter().copied().collect();

    let dsp_ids: Vec<Uuid> = requested.iter().map(|d| d.uuid()).collect();
    let routes: HashMap<Uuid, crate::routing::RouteDecision> =
        crate::routing::decide_routes(pool, org, &dsp_ids)
            .await?
            .into_iter()
            .map(|d| (d.dsp_id, d))
            .collect();
    let profiles: HashMap<String, (Option<String>, Value)> = sqlx::query(
        "SELECT partner_id, ddex_recipient_dpid, capabilities FROM execution.adapter_profiles WHERE partner_id = ANY($1)",
    )
    .bind(requested.iter().map(|d| d.code()).collect::<Vec<_>>())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| (r.get("partner_id"), (r.get("ddex_recipient_dpid"), r.get("capabilities"))))
    .collect();

    let created_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let input = StagingInput {
        prepared: &prepared,
        genre: genre.as_deref(),
        artwork_px,
        tracks: &facts,
        today,
        audio_advisories: &audio_advisories,
    };
    struct Staged {
        dsp: Dsp,
        checks: Vec<DspCheck>,
        ern: Option<Ern>,
        wire: bool,
        route_status: &'static str,
        route_reason: &'static str,
    }
    let mut staged = Vec::with_capacity(requested.len());
    for dsp in requested {
        let spec = dsp.spec();
        let mut checks = evaluate(spec, &input);
        if !approved.contains(&dsp.uuid()) {
            checks.push(DspCheck::new(
                "DSP_NOT_IN_APPROVED_SCOPE",
                Class::Partner,
                Severity::Blocker,
                "Stage 2 had no contract route for this DSP; re-review after onboarding".into(),
            ));
        }
        let route = routes.get(&dsp.uuid());
        let (route_status, route_reason) = match route {
            Some(r) if r.routable => ("ROUTABLE", r.reason),
            Some(r) => ("NO_ROUTE", r.reason),
            None => ("NO_ROUTE", "NO_PROFILE"),
        };
        if route_status != "ROUTABLE" {
            checks.push(DspCheck::new(
                "DSP_ROUTE_NOT_LIVE",
                Class::Partner,
                Severity::Blocker,
                route_reason.into(),
            ));
        }
        let (recipient_dpid, caps) = profiles
            .get(dsp.code())
            .cloned()
            .unwrap_or((None, Value::Null));
        let mut ern = None;
        let mut wire = false;
        if spec.format == DeliveryFormat::Ddex {
            if sender_dpid.is_none() {
                checks.push(DspCheck::new(
                    "DSP_SENDER_DPID_MISSING",
                    Class::Partner,
                    Severity::Blocker,
                    "register the org's DDEX sender DPID".into(),
                ));
            }
            if recipient_dpid.is_none() {
                checks.push(DspCheck::new(
                    "DSP_RECIPIENT_DPID_MISSING",
                    Class::Partner,
                    Severity::Blocker,
                    "register the DSP's recipient DPID during onboarding".into(),
                ));
            }
            wire = sender_dpid.is_some() && recipient_dpid.is_some();
            match DspMessagePreset::resolve(spec.code, &caps) {
                Err(_) => checks.push(DspCheck::new(
                    "DSP_ERN_PRESET_INVALID",
                    Class::Partner,
                    Severity::Blocker,
                    "adapter profile ddex_preset is malformed".into(),
                )),
                Ok(preset) => {
                    let (s, r) = (
                        sender_dpid
                            .clone()
                            .unwrap_or_else(|| preview_dpid("AUDENIQ")),
                        recipient_dpid.unwrap_or_else(|| preview_dpid(spec.code)),
                    );
                    let (prepared, org_name, created_at) =
                        (prepared.clone(), org_name.clone(), created_at.clone());
                    // xmllint is a blocking child process per message.
                    let built = tokio::task::spawn_blocking(move || {
                        build_ern(
                            spec,
                            &prepared,
                            package_id,
                            &preset,
                            &org_name,
                            &s,
                            &r,
                            &created_at,
                        )
                    })
                    .await
                    .map_err(|_| Error::Internal)?;
                    match built {
                        Ok(e) => ern = Some(e),
                        Err(c) => checks.push(c),
                    }
                }
            }
        } else {
            checks.push(DspCheck::new(
                "DSP_PARTNER_SPEC_PENDING",
                Class::Partner,
                Severity::Blocker,
                "no public delivery spec: the feed format arrives with the contract".into(),
            ));
        }
        staged.push(Staged {
            dsp,
            checks,
            ern,
            wire,
            route_status,
            route_reason,
        });
    }

    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx)
        .await?;
    let mut summary = StageSummary {
        staged: 0,
        ready: 0,
        awaiting_partner: 0,
        content_blocked: 0,
    };
    for s in &staged {
        let readiness = Readiness::of(&s.checks);
        match readiness {
            Readiness::Ready => summary.ready += 1,
            Readiness::AwaitingPartner => summary.awaiting_partner += 1,
            Readiness::ContentBlocked => summary.content_blocked += 1,
        }
        if let (Some(e), true) = (&s.ern, s.wire) {
            sqlx::query(
                "INSERT INTO distribution.ddex_messages(package_id,org_id,dsp_id,sender_name,sender_dpid,recipient_name,recipient_dpid,ern_xml,ern_sha256)
                 SELECT $1,$2,$3,$4,o.ddex_sender_dpid,$5,p.ddex_recipient_dpid,$6,$7
                 FROM identity.orgs o, execution.adapter_profiles p
                 WHERE o.id=$2 AND p.partner_id=$8
                 ON CONFLICT(package_id,dsp_id) DO NOTHING",
            )
            .bind(package_id)
            .bind(org)
            .bind(s.dsp.uuid())
            .bind(&org_name)
            .bind(s.dsp.spec().name)
            .bind(&e.xml)
            .bind(&e.sha256)
            .bind(s.dsp.code())
            .execute(&mut *tx)
            .await?;
        }
        let checks = serde_json::to_value(&s.checks).map_err(|_| Error::Internal)?;
        // Approval survives a re-stage only when the message bytes did not
        // change: staff approve exactly what will be sent.
        sqlx::query(
            "INSERT INTO distribution.delivery_staging
               (package_id, dsp_code, org_id, release_id, revision_id, dsp_id, readiness, checks,
                route_status, route_reason, ern_message_id, ern_sha256, ern_xml, ern_is_preview)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
             ON CONFLICT(package_id, dsp_code) DO UPDATE SET
               readiness=EXCLUDED.readiness, checks=EXCLUDED.checks,
               route_status=EXCLUDED.route_status, route_reason=EXCLUDED.route_reason,
               ern_message_id=EXCLUDED.ern_message_id, ern_sha256=EXCLUDED.ern_sha256,
               ern_xml=EXCLUDED.ern_xml, ern_is_preview=EXCLUDED.ern_is_preview,
               approval = CASE WHEN delivery_staging.ern_sha256 IS NOT DISTINCT FROM EXCLUDED.ern_sha256
                               AND EXCLUDED.readiness <> 'CONTENT_BLOCKED'
                               THEN delivery_staging.approval ELSE 'PENDING' END,
               staged_at=now()",
        )
        .bind(package_id)
        .bind(s.dsp.code())
        .bind(org)
        .bind(release_id)
        .bind(revision_id)
        .bind(s.dsp.uuid())
        .bind(readiness.as_db())
        .bind(checks)
        .bind(s.route_status)
        .bind(s.route_reason)
        .bind(s.ern.as_ref().map(|e| e.message_id.as_str()))
        .bind(s.ern.as_ref().map(|e| e.sha256.as_str()))
        .bind(s.ern.as_ref().map(|e| e.xml.as_str()))
        .bind(!s.wire)
        .execute(&mut *tx)
        .await?;
        summary.staged += 1;
    }
    crate::operations::audit(
        &mut tx,
        None,
        Some(org),
        Some(release_id),
        "delivery.staged",
        &format!(
            "package={package_id} staged={} ready={} awaiting_partner={} content_blocked={}",
            summary.staged, summary.ready, summary.awaiting_partner, summary.content_blocked
        ),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(summary)
}

/// Artist-facing view of the latest staged package of a release. Internal
/// details (ERN bytes, route reasons) are left out; each DSP carries its
/// readiness, approval and the content findings the artist can act on.
pub async fn release_delivery_view(pool: &PgPool, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx)
        .await?;
    let rows = sqlx::query(
        "SELECT s.dsp_code, s.readiness, s.approval, s.checks, s.staged_at, s.package_id
         FROM distribution.delivery_staging s
         WHERE s.org_id=$1 AND s.release_id=$2
           AND s.package_id = (SELECT package_id FROM distribution.delivery_staging
                               WHERE org_id=$1 AND release_id=$2 ORDER BY staged_at DESC LIMIT 1)",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let jobs: HashMap<String, (String, Option<String>)> = sqlx::query(
        "SELECT j.partner_id, j.status, b.live_status FROM execution.delivery_jobs j
         JOIN distribution.delivery_staging s ON s.package_id=j.package_id AND s.dsp_code=j.partner_id
         LEFT JOIN execution.live_bindings b ON b.org_id=j.org_id AND b.package_id=j.package_id AND b.partner_id=j.partner_id
         WHERE s.org_id=$1 AND s.release_id=$2",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|r| {
        (
            r.get("partner_id"),
            (r.get("status"), r.get("live_status")),
        )
    })
    .collect();
    tx.commit().await?;
    let mut items: Vec<(Dsp, Value)> = rows
        .iter()
        .filter_map(|r| {
            let code: String = r.get("dsp_code");
            let dsp = Dsp::from_code(&code)?;
            let checks: Value = r.get("checks");
            let issues: Vec<Value> = checks
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| c["class"] == "CONTENT" && c["severity"] != "INFO")
                .map(|c| json!({"code": c["code"], "severity": c["severity"], "detail": c["detail"]}))
                .collect();
            let readiness: String = r.get("readiness");
            let approval: String = r.get("approval");
            let (delivery, live) = jobs.get(&code).cloned().unzip();
            let live = live.flatten();
            let stage = match (delivery.as_deref(), readiness.as_str(), approval.as_str()) {
                _ if live.as_deref() == Some("LIVE") => "LIVE",
                _ if live.as_deref() == Some("TAKEN_DOWN") => "TAKEN_DOWN",
                (Some("DELIVERED"), _, _) => "DELIVERED",
                (Some(_), _, _) => "SENDING",
                (None, "CONTENT_BLOCKED", _) => "NEEDS_CORRECTION",
                (None, _, "HELD") => "ON_HOLD",
                (None, _, "APPROVED") => "SCHEDULED",
                (None, "AWAITING_PARTNER", _) => "PREPARING",
                (None, _, _) => "IN_REVIEW",
            };
            Some((
                dsp,
                json!({
                    "dsp": code, "slug": dsp.spec().slug, "name": dsp.spec().name,
                    "stage": stage, "readiness": readiness, "approval": approval,
                    "delivery_status": delivery, "live_status": live, "issues": issues,
                    "staged_at": r.get::<chrono::DateTime<chrono::Utc>, _>("staged_at"),
                }),
            ))
        })
        .collect();
    items.sort_by_key(|(d, _)| *d);
    Ok(
        json!({"release_id": release, "items": items.into_iter().map(|(_, v)| v).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preparation_model::{AssetRef, PreparedTrack};

    fn asset(ct: &str, sr: Option<i32>, bits: Option<i32>) -> AssetRef {
        AssetRef {
            id: Uuid::new_v4(),
            object_key: "k".into(),
            sha256: "0".repeat(64),
            size_bytes: 1,
            content_type: ct.into(),
            duration_secs: Some(180.0),
            sample_rate: sr,
            channels: Some(2),
            bits_per_sample: bits,
        }
    }

    fn release(tracks: Vec<PreparedTrack>, explicit: bool) -> PreparedRelease {
        let canonical: CanonicalRelease = serde_json::from_value(json!({
            "schema_version": 2, "rule_version": "1", "org_id": Uuid::nil(), "release_id": Uuid::nil(),
            "revision_id": Uuid::nil(), "revision_hash": "", "verification_package_id": Uuid::nil(),
            "verification_package_hash": "", "rights_epoch": 0, "approved_dsp_ids": [],
            "release_title": "t", "release_type": "SINGLE", "upc": null, "artwork": null, "tracks": []
        }))
        .unwrap();
        PreparedRelease {
            canonical,
            org_id: Uuid::nil(),
            release_id: Uuid::nil(),
            revision_id: Uuid::nil(),
            revision_hash: String::new(),
            snapshot_id: Uuid::nil(),
            verification_package_id: Uuid::nil(),
            verification_package_hash: String::new(),
            rights_epoch: 0,
            approved_scope: vec![],
            title: "Song".into(),
            artist: "Artist".into(),
            release_type: "SINGLE".into(),
            release_date: NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(),
            language: "ko".into(),
            p_line: "2026 A".into(),
            c_line: "2026 A".into(),
            upc: "880000000001".into(),
            tracks,
            artwork: asset("image/jpeg", None, None),
            explicit,
        }
    }

    fn track(a: AssetRef) -> PreparedTrack {
        PreparedTrack {
            id: Uuid::new_v4(),
            title: "Song".into(),
            version: String::new(),
            artist: "Artist".into(),
            isrc: "KRA012600001".into(),
            disc_number: 1,
            track_number: 1,
            audio: a,
        }
    }

    fn codes(v: &[DspCheck]) -> Vec<&'static str> {
        v.iter().map(|c| c.code).collect()
    }

    #[test]
    fn clean_release_meets_every_spec() {
        let t = track(asset("audio/flac", Some(48_000), Some(24)));
        let mut facts = HashMap::new();
        facts.insert(
            t.id,
            TrackFacts {
                has_composer: true,
                has_lyricist: true,
                instrumental: false,
            },
        );
        let p = release(vec![t], false);
        let i = StagingInput {
            prepared: &p,
            genre: Some("K-Pop"),
            artwork_px: Some((3000, 3000)),
            tracks: &facts,
            today: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            audio_advisories: &[],
        };
        for d in Dsp::ALL {
            let c = evaluate(d.spec(), &i);
            assert!(c.is_empty(), "{}: {:?}", d.code(), codes(&c));
            assert_eq!(Readiness::of(&c), Readiness::Ready);
        }
    }

    #[test]
    fn per_dsp_differences_are_enforced() {
        let t = track(asset("audio/wav", Some(44_100), Some(16)));
        let mut facts = HashMap::new();
        facts.insert(
            t.id,
            TrackFacts {
                has_composer: true,
                has_lyricist: false,
                instrumental: false,
            },
        );
        let p = release(vec![t], true);
        let i = StagingInput {
            prepared: &p,
            genre: Some("Pop"),
            artwork_px: Some((5000, 5000)),
            tracks: &facts,
            today: NaiveDate::from_ymd_opt(2026, 11, 25).unwrap(),
            audio_advisories: &[],
        };
        // Deezer: 4096px cap and lyricist required.
        let d = codes(&evaluate(Dsp::D10.spec(), &i));
        assert!(d.contains(&"DSP_ARTWORK_TOO_LARGE"));
        assert!(d.contains(&"DSP_CREDIT_LYRICIST_MISSING"));
        // Spotify: no cap, no lyricist rule; 6 days < 7 lead days is a warning.
        let s = evaluate(Dsp::D5.spec(), &i);
        assert_eq!(codes(&s), vec!["DSP_LEAD_TIME_SHORT"]);
        assert_eq!(Readiness::of(&s), Readiness::Ready);
        // Melon: lyricist required + youth-harmful marking note.
        let m = codes(&evaluate(Dsp::D1.spec(), &i));
        assert!(m.contains(&"DSP_CREDIT_LYRICIST_MISSING"));
        assert!(m.contains(&"DSP_KR_YOUTH_HARMFUL_MARKING"));
    }

    #[test]
    fn instrumental_tracks_need_no_lyricist_and_lossy_masters_block() {
        let t = track(asset("audio/mpeg", Some(44_100), None));
        let mut facts = HashMap::new();
        facts.insert(
            t.id,
            TrackFacts {
                has_composer: true,
                has_lyricist: false,
                instrumental: true,
            },
        );
        let p = release(vec![t], false);
        let i = StagingInput {
            prepared: &p,
            genre: None,
            artwork_px: None,
            tracks: &facts,
            today: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            audio_advisories: &[],
        };
        let c = evaluate(Dsp::D1.spec(), &i);
        let k = codes(&c);
        assert!(!k.contains(&"DSP_CREDIT_LYRICIST_MISSING"));
        assert!(k.contains(&"DSP_AUDIO_NOT_LOSSLESS"));
        assert!(k.contains(&"DSP_GENRE_MISSING"));
        assert_eq!(Readiness::of(&c), Readiness::ContentBlocked);
    }

    #[test]
    fn loud_masters_are_advised_per_dsp_target() {
        let t = track(asset("audio/wav", Some(44_100), Some(16)));
        let mut facts = HashMap::new();
        facts.insert(
            t.id,
            TrackFacts {
                has_composer: true,
                has_lyricist: true,
                instrumental: false,
            },
        );
        let p = release(vec![t], false);
        let adv = vec![(
            "AUDIO_LOUDNESS_OUT_OF_RANGE".to_string(),
            "integrated_lufs=-7.5 target=-14±1".to_string(),
        )];
        let i = StagingInput {
            prepared: &p,
            genre: Some("Pop"),
            artwork_px: Some((3000, 3000)),
            tracks: &facts,
            today: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            audio_advisories: &adv,
        };
        let apple = evaluate(Dsp::D6.spec(), &i);
        let c = apple
            .iter()
            .find(|c| c.code == "DSP_LOUDNESS_ADVISORY")
            .unwrap();
        assert!(
            c.detail.contains("-16") && c.detail.contains("8.5"),
            "{}",
            c.detail
        );
        // Advisory only: the release stays deliverable.
        assert_eq!(Readiness::of(&apple), Readiness::Ready);
    }

    #[test]
    fn stage1_cover_detail_parses() {
        assert_eq!(parse_px("3000x3000"), Some((3000, 3000)));
        assert_eq!(parse_px("cache_hit: 3000x3000"), Some((3000, 3000)));
        assert_eq!(parse_px("probe failed"), None);
    }
}
