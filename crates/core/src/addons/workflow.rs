use super::{jobs, model::*};
use crate::{
    auth::{self, Actor},
    domain,
    error::{Error, Result},
    operations,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

pub async fn scope(c: &mut PgConnection, org: Uuid) -> Result<()> {
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(c)
        .await?;
    Ok(())
}
pub fn text(s: &str, max: usize, required: bool) -> Result<()> {
    if s.chars().count() > max || (required && s.trim().is_empty()) {
        return Err(Error::Invalid);
    }
    crate::text_policy::check_multiline(s)
}
pub fn url(s: &str) -> Result<()> {
    let u = url::Url::parse(s).map_err(|_| Error::Invalid)?;
    if s.len() > 2000
        || u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub async fn audit(
    c: &mut PgConnection,
    actor: Option<&Actor>,
    org: Uuid,
    id: Uuid,
    action: &str,
    reason: &str,
    before: Value,
    after: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO operations.audit_events(id,actor_user_id,actor_service,org_id,resource_id,action,reason_code,request_id,before_value,after_value) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(Uuid::new_v4()).bind(actor.map(|a|a.user)).bind(if actor.is_none(){Some("audeniq-system")}else{None})
        .bind(org).bind(id).bind(action).bind(reason).bind(actor.map(|a|a.request).unwrap_or_else(Uuid::new_v4)).bind(before).bind(after).execute(c).await?;
    Ok(())
}
pub async fn load(c: &mut PgConnection, org: Uuid, id: Uuid) -> Result<Order> {
    scope(c, org).await?;
    sqlx::query_as("SELECT * FROM catalog.addon_orders WHERE org_id=$1 AND id=$2 FOR UPDATE")
        .bind(org)
        .bind(id)
        .fetch_optional(c)
        .await?
        .ok_or(Error::NotFound)
}
pub async fn view(c: &mut PgConnection, org: Uuid, id: Uuid) -> Result<Value> {
    sqlx::query_scalar("SELECT to_jsonb(o) || jsonb_build_object(
        'organization_id',o.org_id,'details',jsonb_build_object(
        'artist_profile',(SELECT to_jsonb(d)-'org_id'-'addon_order_id' FROM catalog.artist_profile_requests d WHERE d.addon_order_id=o.id),
        'migration',(SELECT to_jsonb(d)-'org_id'-'addon_order_id' FROM catalog.migration_requests d WHERE d.addon_order_id=o.id),
        'lyrics',(SELECT to_jsonb(d)-'org_id'-'addon_order_id' FROM catalog.lyrics_requests d WHERE d.addon_order_id=o.id),
        'lyric_video',(SELECT to_jsonb(d)-'org_id'-'addon_order_id' FROM catalog.lyric_video_requests d WHERE d.addon_order_id=o.id),
        'mv',(SELECT to_jsonb(d)-'org_id'-'addon_order_id' FROM catalog.mv_requests d WHERE d.addon_order_id=o.id),
        'promo',(SELECT to_jsonb(d)-'org_id'-'addon_order_id' FROM catalog.promo_requests d WHERE d.addon_order_id=o.id)),
        'provider_tasks',coalesce((SELECT jsonb_agg(to_jsonb(t)-'org_id') FROM catalog.addon_provider_tasks t WHERE t.addon_order_id=o.id),'[]'::jsonb),
        'dsp_links',coalesce((SELECT jsonb_agg(to_jsonb(l)-'org_id'-'addon_order_id') FROM catalog.addon_dsp_links l WHERE l.addon_order_id=o.id),'[]'::jsonb))
        FROM catalog.addon_orders o WHERE org_id=$1 AND id=$2")
        .bind(org).bind(id).fetch_optional(c).await?.ok_or(Error::NotFound)
}
pub async fn authorize_order(
    c: &mut PgConnection,
    a: &Actor,
    o: &Order,
    write: bool,
) -> Result<()> {
    auth::authorize(c, a, o.org_id, o.id, "addon_order", write).await?;
    authorize_target(c, a, o.org_id, &o.target_type, o.target_id, write).await?;
    Ok(())
}
pub async fn authorize_target(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    ty: &str,
    id: Uuid,
    write: bool,
) -> Result<(Option<Uuid>, Option<Uuid>)> {
    let mut artist = None;
    let mut release = None;
    match ty {
        "artist" => {
            auth::authorize(c, a, org, id, "artist", write).await?;
            sqlx::query_scalar::<_,Uuid>("SELECT id FROM catalog.artists WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR SHARE").bind(org).bind(id).fetch_optional(&mut *c).await?.ok_or(Error::NotFound)?;
            artist = Some(id);
        }
        "release" => {
            auth::authorize(c, a, org, id, "release", write).await?;
            sqlx::query_scalar::<_,Uuid>("SELECT id FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR SHARE").bind(org).bind(id).fetch_optional(&mut *c).await?.ok_or(Error::NotFound)?;
            release = Some(id);
        }
        "track" => {
            let t=sqlx::query("SELECT t.release_id,t.artist_id FROM catalog.tracks t JOIN catalog.releases r ON r.org_id=t.org_id AND r.id=t.release_id WHERE t.org_id=$1 AND t.id=$2 AND t.archived_at IS NULL AND r.archived_at IS NULL FOR SHARE OF t,r").bind(org).bind(id).fetch_optional(&mut *c).await?.ok_or(Error::Forbidden)?;
            let r = t.get("release_id");
            auth::authorize(c, a, org, r, "release", write).await?;
            artist = Some(t.get("artist_id"));
            release = Some(r);
        }
        "music_video" => {
            auth::authorize(c, a, org, id, "asset", write).await?;
            asset_ready(c, org, id, "VIDEO").await?;
        }
        _ => return Err(Error::Invalid),
    }
    Ok((artist, release))
}
pub async fn asset_ready(c: &mut PgConnection, org: Uuid, id: Uuid, kind: &str) -> Result<()> {
    let valid:Option<Uuid>=sqlx::query_scalar("SELECT a.id FROM catalog.assets a JOIN catalog.upload_sessions u ON u.org_id=a.org_id AND u.asset_id=a.id
        WHERE a.org_id=$1 AND a.id=$2 AND a.kind=$3 AND a.state='REGISTERED' AND a.sha256 IS NOT NULL AND a.etag IS NOT NULL
        AND u.status='COMPLETED' AND (a.kind<>'AUDIO' OR a.qc_status='PASS') FOR SHARE OF a").bind(org).bind(id).bind(kind).fetch_optional(c).await?;
    if valid.is_none() {
        return Err(Error::PolicyGate("ADDON_ASSET_NOT_VERIFIED"));
    }
    Ok(())
}
async fn checked_asset(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    kind: &str,
) -> Result<()> {
    auth::authorize(c, a, org, id, "asset", false).await?;
    asset_ready(c, org, id, kind).await
}
pub fn validate_details<'a>(
    c: &'a mut PgConnection,
    a: &'a Actor,
    org: Uuid,
    code: &'a str,
    ty: &'a str,
    target: Uuid,
    d: &'a Details,
) -> BoxFuture<'a, ()> {
    Box::pin(validate_details_inner(c, a, org, code, ty, target, d))
}
async fn validate_details_inner(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    code: &str,
    ty: &str,
    target: Uuid,
    d: &Details,
) -> Result<()> {
    let expected = match code {
        "PROFILE_BASIC" | "PROFILE_PLUS" => "artist",
        "MIGRATION" | "PRIORITY_DELIVERY" | "PROMO_BASIC" | "MUSIC_DATA_BASIC" => "release",
        "LYRICS_BASIC" | "AI_SYNC_LYRICS" | "LYRIC_VIDEO_PLUS" => "track",
        "MV_REVIEW_AND_GLOBAL" | "MV_GLOBAL_ONLY" => "music_video",
        _ => return Err(Error::InvalidCode("ADDON_SERVICE_UNSUPPORTED")),
    };
    if ty != expected {
        return Err(Error::Invalid);
    }
    match (code, d) {
        (
            "PROFILE_BASIC" | "PROFILE_PLUS",
            Details::ArtistProfile {
                request_type,
                platforms,
                notes,
            },
        ) => {
            let basic = matches!(request_type.as_str(), "LINK" | "OAC");
            if !(basic
                || matches!(
                    request_type.as_str(),
                    "MISMATCH" | "SEPARATE" | "MERGE" | "RENAME" | "RETRY"
                ))
                || (code == "PROFILE_BASIC" && !basic)
                || platforms.is_empty()
                || platforms.len() > 30
            {
                return Err(Error::Invalid);
            }
            for p in platforms {
                text(p, 80, true)?;
            }
            text(notes, 4000, false)?;
        }
        (
            "MIGRATION",
            Details::Migration {
                previous_distributor,
                previous_release_urls,
                ..
            },
        ) => {
            text(previous_distributor, 200, true)?;
            if previous_release_urls.is_empty() || previous_release_urls.len() > 30 {
                return Err(Error::Invalid);
            }
            for s in previous_release_urls {
                url(s)?;
            }
        }
        ("PRIORITY_DELIVERY", Details::Priority {}) => {
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL AND status NOT IN ('WITHDRAWN','SUPERSEDED','COMPLETED','TAKEN_DOWN') AND NOT EXISTS(SELECT 1 FROM execution.live_bindings b JOIN distribution.distribution_packages p ON p.id=b.package_id JOIN distribution.canonical_releases cr ON cr.id=p.canonical_release_id WHERE cr.org_id=$1 AND cr.release_id=$2 AND b.live_status IN ('LIVE','TAKEN_DOWN')))").bind(org).bind(target).fetch_one(&mut *c).await?;
            if !valid {
                return Err(Error::Conflict);
            }
        }
        (
            "LYRICS_BASIC" | "AI_SYNC_LYRICS",
            Details::Lyrics {
                lyrics_text,
                lrc_asset_id,
                basic_video_requested,
            },
        ) => {
            text(lyrics_text, 30000, false)?;
            if lyrics_text.trim().is_empty() && lrc_asset_id.is_none() {
                return Err(Error::Invalid);
            }
            if code != "LYRICS_BASIC" && *basic_video_requested {
                return Err(Error::Invalid);
            }
            if let Some(id) = lrc_asset_id {
                checked_asset(c, a, org, *id, "LRC").await?;
            }
        }
        (
            "LYRIC_VIDEO_PLUS",
            Details::LyricVideo {
                lyrics_text,
                lrc_asset_id,
                template_id,
                source_asset_id,
            },
        ) => {
            text(lyrics_text, 30000, true)?;
            text(template_id, 100, true)?;
            if let Some(id) = lrc_asset_id {
                checked_asset(c, a, org, *id, "LRC").await?;
            }
            if let Some(id) = source_asset_id {
                checked_asset(c, a, org, *id, "VIDEO").await?;
            }
            let r=sqlx::query("SELECT t.asset_id,r.artwork_asset_id FROM catalog.tracks t JOIN catalog.releases r ON r.org_id=t.org_id AND r.id=t.release_id WHERE t.org_id=$1 AND t.id=$2").bind(org).bind(target).fetch_one(&mut *c).await?;
            checked_asset(
                c,
                a,
                org,
                r.get::<Option<Uuid>, _>("asset_id").ok_or(Error::Invalid)?,
                "AUDIO",
            )
            .await?;
            checked_asset(
                c,
                a,
                org,
                r.get::<Option<Uuid>, _>("artwork_asset_id")
                    .ok_or(Error::Invalid)?,
                "IMAGE",
            )
            .await?;
        }
        (
            "MV_REVIEW_AND_GLOBAL" | "MV_GLOBAL_ONLY",
            Details::Mv {
                review_evidence_asset_id,
                release_id,
            },
        ) => {
            if code == "MV_GLOBAL_ONLY" && review_evidence_asset_id.is_none() {
                return Err(Error::InvalidCode("MV_REVIEW_EVIDENCE_REQUIRED"));
            }
            if let Some(id) = review_evidence_asset_id {
                checked_asset(c, a, org, *id, "DOCUMENT").await?;
            }
            if let Some(id) = release_id {
                authorize_target(c, a, org, "release", *id, true).await?;
            }
        }
        ("PROMO_BASIC", Details::Promo { .. }) | ("MUSIC_DATA_BASIC", Details::MusicData {}) => {}
        _ => return Err(Error::Invalid),
    }
    Ok(())
}
pub fn store_details<'a>(
    c: &'a mut PgConnection,
    org: Uuid,
    id: Uuid,
    d: &'a Details,
) -> BoxFuture<'a, ()> {
    Box::pin(store_details_inner(c, org, id, d))
}
async fn store_details_inner(c: &mut PgConnection, org: Uuid, id: Uuid, d: &Details) -> Result<()> {
    match d {
        Details::ArtistProfile {
            request_type,
            platforms,
            notes,
        } => {
            sqlx::query("INSERT INTO catalog.artist_profile_requests(org_id,addon_order_id,request_type,platforms,notes) VALUES($1,$2,$3,$4,$5) ON CONFLICT(addon_order_id) DO UPDATE SET request_type=$3,platforms=$4,notes=$5").bind(org).bind(id).bind(request_type).bind(platforms).bind(notes).execute(&mut *c).await?;
        }
        Details::Migration {
            previous_distributor,
            preserve_upc,
            original_release_date,
            previous_release_urls,
        } => {
            sqlx::query("INSERT INTO catalog.migration_requests(org_id,addon_order_id,previous_distributor,preserve_upc,original_release_date,previous_release_urls) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(addon_order_id) DO UPDATE SET previous_distributor=$3,preserve_upc=$4,original_release_date=$5,previous_release_urls=$6").bind(org).bind(id).bind(previous_distributor).bind(preserve_upc).bind(original_release_date).bind(previous_release_urls).execute(&mut *c).await?;
        }
        Details::Lyrics {
            lyrics_text,
            lrc_asset_id,
            basic_video_requested,
        } => {
            store_lyrics(
                c,
                org,
                id,
                lyrics_text,
                *lrc_asset_id,
                *basic_video_requested,
            )
            .await?;
            if *basic_video_requested {
                sqlx::query("INSERT INTO catalog.lyric_video_requests(org_id,addon_order_id,template_id) VALUES($1,$2,'basic') ON CONFLICT(addon_order_id) DO UPDATE SET render_status='PENDING',output_asset_id=NULL").bind(org).bind(id).execute(&mut *c).await?;
            }
        }
        Details::LyricVideo {
            lyrics_text,
            lrc_asset_id,
            template_id,
            source_asset_id,
        } => {
            store_lyrics(c, org, id, lyrics_text, *lrc_asset_id, false).await?;
            sqlx::query("INSERT INTO catalog.lyric_video_requests(org_id,addon_order_id,template_id,source_asset_id) VALUES($1,$2,$3,$4) ON CONFLICT(addon_order_id) DO UPDATE SET template_id=$3,source_asset_id=$4,render_status='PENDING',output_asset_id=NULL").bind(org).bind(id).bind(template_id).bind(source_asset_id).execute(&mut *c).await?;
        }
        Details::Mv {
            review_evidence_asset_id,
            ..
        } => {
            sqlx::query("UPDATE catalog.mv_requests SET review_evidence_asset_id=$3,review_status='PENDING',review_decided_by=NULL,review_decided_at=NULL,evidence_valid_until=NULL,global_distribution_status='PENDING' WHERE org_id=$1 AND addon_order_id=$2").bind(org).bind(id).bind(review_evidence_asset_id).execute(&mut *c).await?;
        }
        Details::Promo {
            presave_enabled,
            preorder_enabled,
        } => {
            sqlx::query("INSERT INTO catalog.promo_requests(org_id,addon_order_id,smartlink_slug,presave_enabled,preorder_enabled) VALUES($1,$2,$3,$4,$5) ON CONFLICT(addon_order_id) DO UPDATE SET presave_enabled=$4,preorder_enabled=$5").bind(org).bind(id).bind(id.simple().to_string()).bind(presave_enabled).bind(preorder_enabled).execute(&mut *c).await?;
        }
        Details::Priority {} | Details::MusicData {} => {}
    }
    Ok(())
}
async fn store_lyrics(
    c: &mut PgConnection,
    org: Uuid,
    id: Uuid,
    lyrics: &str,
    lrc: Option<Uuid>,
    basic: bool,
) -> Result<()> {
    sqlx::query("INSERT INTO catalog.lyrics_requests(org_id,addon_order_id,lyrics_text,lrc_asset_id,basic_video_requested) VALUES($1,$2,$3,$4,$5) ON CONFLICT(addon_order_id) DO UPDATE SET lyrics_text=$3,lrc_asset_id=$4,basic_video_requested=$5,sync_status='PENDING'").bind(org).bind(id).bind(lyrics).bind(lrc).bind(basic).execute(c).await?;
    Ok(())
}
pub fn create<'a>(
    pool: &'a PgPool,
    a: &'a Actor,
    org: Uuid,
    key: &'a str,
    i: CreateOrder,
) -> BoxFuture<'a, Value> {
    Box::pin(create_inner(pool, a, org, key, i))
}
async fn create_inner(
    pool: &PgPool,
    a: &Actor,
    org: Uuid,
    key: &str,
    i: CreateOrder,
) -> Result<Value> {
    if !(8..=128).contains(&key.len()) || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::InvalidCode("IDEMPOTENCY_KEY_REQUIRED"));
    }
    let hash = domain::sha256_json(&i);
    let mut tx = pool.begin().await?;
    scope(&mut tx, org).await?;
    auth::membership(&mut tx, a, org, true).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("addon:idempotency:{org}:{}:{key}", a.user))
        .execute(&mut *tx)
        .await?;
    if let Some(r)=sqlx::query("SELECT request_hash,order_id FROM catalog.addon_idempotency WHERE org_id=$1 AND user_id=$2 AND key=$3").bind(org).bind(a.user).bind(key).fetch_optional(&mut *tx).await? {
        if r.get::<String,_>("request_hash")!=hash {return Err(Error::Conflict);}
        let o=load(&mut tx,org,r.get("order_id")).await?;authorize_order(&mut tx,a,&o,true).await?;
        let v=view(&mut tx,org,o.id).await?;tx.commit().await?;return Ok(v);
    }
    let (artist, mut release) =
        authorize_target(&mut tx, a, org, &i.target_type, i.target_id, true).await?;
    validate_details(
        &mut tx,
        a,
        org,
        &i.service_code,
        &i.target_type,
        i.target_id,
        &i.details,
    )
    .await?;
    if let Details::Mv { release_id, .. } = &i.details {
        release = *release_id;
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "addon:target:{org}:{}:{}",
            i.service_code, i.target_id
        ))
        .execute(&mut *tx)
        .await?;
    let duplicate:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.addon_orders WHERE org_id=$1 AND service_code=$2 AND target_id=$3 AND (status NOT IN ('COMPLETED','REJECTED','CANCELLED') OR (service_code='PROFILE_PLUS' AND status='COMPLETED' AND valid_until>now())))").bind(org).bind(&i.service_code).bind(i.target_id).fetch_one(&mut *tx).await?;
    if duplicate {
        return Err(Error::PolicyGate("ADDON_ACTIVE_ORDER_EXISTS"));
    }
    let cat = sqlx::query(
        "SELECT * FROM catalog.addon_service_catalog WHERE code=$1 AND active FOR SHARE",
    )
    .bind(&i.service_code)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if cat.get::<String, _>("billing_unit") != i.target_type {
        return Err(Error::Invalid);
    }
    let id = Uuid::new_v4();
    let price: i64 = cat.get("price_krw");
    let version: i32 = cat.get("version");
    auth::create_resource(&mut tx, a, org, id, "addon_order").await?;
    sqlx::query("INSERT INTO catalog.addon_orders(id,org_id,requester_user_id,service_code,catalog_version,price_snapshot_krw,amount,currency,validity_days_snapshot,max_revisions_snapshot,validity_months_snapshot,payment_status,status,target_type,target_id,artist_id,release_id,track_id,video_asset_id) VALUES($1,$2,$3,$4,$5,$6,$6,'KRW',$7,$8,(SELECT validity_months FROM catalog.addon_service_catalog WHERE code=$4 AND version=$5),$9,'DRAFT',$10,$11,$12,$13,$14,$15)")
        .bind(id).bind(org).bind(a.user).bind(&i.service_code).bind(version).bind(price).bind(cat.get::<Option<i32>,_>("validity_days")).bind(cat.get::<Option<i32>,_>("max_revisions"))
        .bind(if price==0{"NOT_REQUIRED"}else{"PENDING"}).bind(&i.target_type).bind(i.target_id).bind(artist).bind(release)
        .bind((i.target_type=="track").then_some(i.target_id)).bind((i.target_type=="music_video").then_some(i.target_id)).execute(&mut *tx).await?;
    if let Details::Mv {
        review_evidence_asset_id,
        ..
    } = &i.details
    {
        sqlx::query("INSERT INTO catalog.mv_requests(org_id,addon_order_id,review_mode,review_evidence_asset_id) VALUES($1,$2,$3,$4)").bind(org).bind(id).bind(if i.service_code=="MV_GLOBAL_ONLY"{"ARTIST_EVIDENCE"}else{"AUDENIQ"}).bind(review_evidence_asset_id).execute(&mut *tx).await?;
    }
    store_details(&mut tx, org, id, &i.details).await?;
    audit(&mut tx,Some(a),org,id,"addon.created","CATALOG_SNAPSHOT",Value::Null,json!({"service_code":i.service_code,"catalog_version":version,"amount":price,"currency":"KRW","target_type":i.target_type,"target_id":i.target_id,"status":"DRAFT"})).await?;
    transition(
        &mut tx,
        Some(a),
        org,
        id,
        Status::SUBMITTED,
        "USER_SUBMITTED",
    )
    .await?;
    transition(
        &mut tx,
        Some(a),
        org,
        id,
        if price == 0 {
            Status::QUEUED
        } else {
            Status::PAYMENT_REQUIRED
        },
        "PRICE_SNAPSHOT",
    )
    .await?;
    sqlx::query("INSERT INTO catalog.addon_idempotency(org_id,user_id,key,request_hash,order_id) VALUES($1,$2,$3,$4,$5)").bind(org).bind(a.user).bind(key).bind(hash).bind(id).execute(&mut *tx).await?;
    let v = view(&mut tx, org, id).await?;
    tx.commit().await?;
    Ok(v)
}

/// All order state writes pass here, including workers and payment overrides.
pub type BoxFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>;
pub fn transition<'a>(
    c: &'a mut PgConnection,
    a: Option<&'a Actor>,
    org: Uuid,
    id: Uuid,
    next: Status,
    reason: &'a str,
) -> BoxFuture<'a, ()> {
    Box::pin(transition_inner(c, a, org, id, next, reason))
}
async fn transition_inner(
    c: &mut PgConnection,
    a: Option<&Actor>,
    org: Uuid,
    id: Uuid,
    next: Status,
    reason: &str,
) -> Result<()> {
    use Status::*;
    text(reason, 1000, true)?;
    let o = load(c, org, id).await?;
    o.state()?.transition(next)?;
    if matches!(
        next,
        PAID | QUEUED | UNDER_REVIEW | APPROVED | IN_PROGRESS | EXTERNAL_PENDING | COMPLETED
    ) && !matches!(o.payment_status.as_str(), "NOT_REQUIRED" | "PAID")
    {
        return Err(Error::PolicyGate("ADDON_PAYMENT_REQUIRED"));
    }
    if next == PAID {
        return Err(Error::Conflict);
    } // payment confirmation owns this edge.
    if matches!(next, APPROVED | IN_PROGRESS | EXTERNAL_PENDING | COMPLETED)
        && o.service_code.starts_with("MV_")
    {
        // AUDENIQ review orders can start preparation while waiting for review;
        // distribution itself always has the evidence gate in jobs::dispatch.
        if o.service_code == "MV_GLOBAL_ONLY" || next == COMPLETED {
            evidence_gate(c, &o).await?;
        }
    }
    if next == COMPLETED {
        completion_gate(c, &o).await?;
    }
    if o.state()? == COMPLETED && next == UNDER_REVIEW && o.service_code != "LYRIC_VIDEO_PLUS" {
        return Err(Error::Conflict);
    }
    let next_s = serde_json::to_value(next).map_err(|_| Error::Internal)?;
    sqlx::query("UPDATE catalog.addon_orders SET status=$3,row_version=row_version+1,
        submitted_at=CASE WHEN $3='SUBMITTED' THEN coalesce(submitted_at,now()) ELSE submitted_at END,
        accepted_at=CASE WHEN $3='APPROVED' THEN coalesce(accepted_at,now()) ELSE accepted_at END,
        first_reviewed_at=CASE WHEN $3 IN ('UNDER_REVIEW','NEEDS_INFO','APPROVED','REJECTED') THEN coalesce(first_reviewed_at,now()) ELSE first_reviewed_at END,
        processing_at=CASE WHEN $3='IN_PROGRESS' THEN coalesce(processing_at,now()) ELSE processing_at END,
        completed_at=CASE WHEN $3='COMPLETED' THEN now() ELSE completed_at END,
        rejected_at=CASE WHEN $3='REJECTED' THEN now() ELSE rejected_at END,
        cancelled_at=CASE WHEN $3='CANCELLED' THEN now() ELSE cancelled_at END,
        refund_status=CASE WHEN $3 IN ('CANCELLED','REJECTED') AND payment_status='PAID' THEN 'REQUESTED' ELSE refund_status END,
        dispatch_generation=dispatch_generation+CASE WHEN $3='IN_PROGRESS' OR ($3='QUEUED' AND service_code='PRIORITY_DELIVERY') THEN 1 ELSE 0 END
        WHERE org_id=$1 AND id=$2").bind(org).bind(id).bind(next_s.as_str().ok_or(Error::Internal)?).execute(&mut *c).await?;
    audit(c,a,org,id,"addon.status",reason,json!({"status":o.status,"payment_status":o.payment_status}),json!({"status":next_s,"refund_requested":matches!(next,CANCELLED|REJECTED)&&o.payment_status=="PAID"})).await?;
    if matches!(next, CANCELLED | REJECTED | FAILED) {
        jobs::remove_priority(c, a, &o, reason).await?;
    }
    let now = load(c, org, id).await?;
    operations::event(
        c,
        org,
        id,
        "addon.status.changed",
        &format!("addon.status:{id}:{}", now.row_version),
    )
    .await?;
    if next == IN_PROGRESS || (next == QUEUED && o.service_code == "PRIORITY_DELIVERY") {
        jobs::dispatch(c, &now).await?;
    }
    Ok(())
}
pub async fn evidence_gate(c: &mut PgConnection, o: &Order) -> Result<()> {
    let r=sqlx::query("SELECT review_status,review_evidence_asset_id,evidence_valid_until>clock_timestamp() AS valid FROM catalog.mv_requests WHERE org_id=$1 AND addon_order_id=$2 FOR SHARE").bind(o.org_id).bind(o.id).fetch_one(&mut *c).await?;
    if r.get::<String, _>("review_status") != "APPROVED"
        || !r.get::<Option<bool>, _>("valid").unwrap_or(false)
    {
        return Err(Error::PolicyGate("MV_EVIDENCE_NOT_APPROVED"));
    }
    asset_ready(
        c,
        o.org_id,
        r.get::<Option<Uuid>, _>("review_evidence_asset_id")
            .ok_or(Error::Invalid)?,
        "DOCUMENT",
    )
    .await
}
async fn completion_gate(c: &mut PgConnection, o: &Order) -> Result<()> {
    let valid:bool=match o.service_code.as_str() {
        "LYRICS_BASIC"=>sqlx::query_scalar("SELECT NOT basic_video_requested OR EXISTS(SELECT 1 FROM catalog.lyric_video_requests v WHERE v.addon_order_id=l.addon_order_id AND render_status='READY' AND output_asset_id IS NOT NULL) FROM catalog.lyrics_requests l WHERE addon_order_id=$1").bind(o.id).fetch_one(&mut *c).await?,
        "MIGRATION"=>sqlx::query_scalar("SELECT takedown_status='COMPLETED' AND (NOT preserve_upc OR upc_preservation_status<>'PENDING') FROM catalog.migration_requests WHERE addon_order_id=$1").bind(o.id).fetch_one(&mut *c).await?,
        "AI_SYNC_LYRICS"=>sqlx::query_scalar("SELECT sync_status='READY' AND lrc_asset_id IS NOT NULL FROM catalog.lyrics_requests WHERE addon_order_id=$1").bind(o.id).fetch_one(&mut *c).await?,
        "LYRIC_VIDEO_PLUS"=>sqlx::query_scalar("SELECT v.render_status='READY' AND v.output_asset_id IS NOT NULL AND l.sync_status='READY' AND l.lrc_asset_id IS NOT NULL FROM catalog.lyric_video_requests v JOIN catalog.lyrics_requests l USING(addon_order_id) WHERE v.addon_order_id=$1").bind(o.id).fetch_one(&mut *c).await?,
        "MV_GLOBAL_ONLY"|"MV_REVIEW_AND_GLOBAL"=>sqlx::query_scalar("SELECT global_distribution_status='DELIVERED' FROM catalog.mv_requests WHERE addon_order_id=$1").bind(o.id).fetch_one(&mut *c).await?,
        "PROMO_BASIC"=>sqlx::query_scalar("SELECT qr_asset_id IS NOT NULL AND promo_card_asset_id IS NOT NULL FROM catalog.promo_requests WHERE addon_order_id=$1").bind(o.id).fetch_one(&mut *c).await?,
        _=>true,
    };
    let tasks:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.addon_provider_tasks WHERE addon_order_id=$1 AND generation=$2) AND NOT EXISTS(SELECT 1 FROM catalog.addon_provider_tasks WHERE addon_order_id=$1 AND generation=$2 AND status<>'COMPLETED')").bind(o.id).bind(o.dispatch_generation).fetch_one(&mut *c).await?;
    let dispatched:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations.outbox WHERE aggregate_id=$1 AND event_type='addon.dispatch' AND (payload->>'generation')::integer=$2 AND published_at IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM operations.jobs WHERE kind LIKE 'addon.%' AND payload->>'order_id'=$3 AND (payload->>'generation')::integer=$2 AND status<>'SUCCEEDED')").bind(o.id).bind(o.dispatch_generation).bind(o.id.to_string()).fetch_one(&mut *c).await?;
    if !valid || !tasks || !dispatched {
        return Err(Error::PolicyGate("ADDON_RESULTS_REQUIRED"));
    }
    Ok(())
}
pub fn version(o: &Order, expected: i64) -> Result<()> {
    if o.row_version != expected {
        Err(Error::Conflict)
    } else {
        Ok(())
    }
}
pub async fn pay(
    c: &mut PgConnection,
    a: &Actor,
    o: &Order,
    reference: &str,
    reason: &str,
) -> Result<()> {
    text(reference, 200, true)?;
    text(reason, 1000, true)?;
    if o.amount == 0 || o.status != "PAYMENT_REQUIRED" || o.payment_status != "PENDING" {
        return Err(Error::Conflict);
    }
    o.state()?.transition(Status::PAID)?;
    sqlx::query("UPDATE catalog.addon_orders SET status='PAID',payment_status='PAID',payment_reference=$3,paid_at=now(),valid_until=CASE WHEN validity_months_snapshot IS NOT NULL THEN now()+make_interval(months=>validity_months_snapshot) WHEN validity_days_snapshot IS NOT NULL THEN now()+make_interval(days=>validity_days_snapshot) END,row_version=row_version+1 WHERE org_id=$1 AND id=$2")
        .bind(o.org_id).bind(o.id).bind(reference).execute(&mut *c).await?;
    audit(c,Some(a),o.org_id,o.id,"addon.payment.override",reason,json!({"status":o.status,"payment_status":o.payment_status}),json!({"status":"PAID","payment_status":"PAID","payment_reference":reference,"amount":o.amount})).await?;
    operations::event(
        c,
        o.org_id,
        o.id,
        "addon.payment.changed",
        &format!("addon.payment:{}", o.id),
    )
    .await?;
    Ok(())
}
pub async fn decide_evidence(
    c: &mut PgConnection,
    a: &Actor,
    o: &Order,
    i: &EvidenceDecision,
) -> Result<()> {
    version(o, i.row_version)?;
    text(&i.reason, 1000, true)?;
    if o.terminal() {
        return Err(Error::Conflict);
    }
    let old:Value=sqlx::query_scalar("SELECT to_jsonb(m) FROM catalog.mv_requests m WHERE org_id=$1 AND addon_order_id=$2 FOR UPDATE").bind(o.org_id).bind(o.id).fetch_optional(&mut *c).await?.ok_or(Error::Invalid)?;
    let asset: Uuid = serde_json::from_value(old["review_evidence_asset_id"].clone())
        .map_err(|_| Error::PolicyGate("MV_REVIEW_EVIDENCE_REQUIRED"))?;
    asset_ready(c, o.org_id, asset, "DOCUMENT").await?;
    if i.approved && !i.valid_until.is_some_and(|v| v > Utc::now()) {
        return Err(Error::Invalid);
    }
    if !i.approved && old["global_distribution_status"] != "PENDING" {
        return Err(Error::Conflict);
    }
    sqlx::query("UPDATE catalog.mv_requests SET review_status=$3,evidence_valid_until=$4,review_decided_by=$5,review_decided_at=now() WHERE org_id=$1 AND addon_order_id=$2")
        .bind(o.org_id).bind(o.id).bind(if i.approved{"APPROVED"}else{"REJECTED"}).bind(i.valid_until).bind(a.user).execute(&mut *c).await?;
    sqlx::query(
        "UPDATE catalog.addon_orders SET row_version=row_version+1 WHERE org_id=$1 AND id=$2",
    )
    .bind(o.org_id)
    .bind(o.id)
    .execute(&mut *c)
    .await?;
    audit(c,Some(a),o.org_id,o.id,"addon.evidence.decided",&i.reason,old,json!({"review_status":if i.approved{"APPROVED"}else{"REJECTED"},"asset_id":asset,"valid_until":i.valid_until})).await?;
    operations::event(
        c,
        o.org_id,
        o.id,
        "addon.evidence.changed",
        &format!("addon.evidence:{}:{}", o.id, o.row_version + 1),
    )
    .await?;
    Ok(())
}

pub async fn revise(c: &mut PgConnection, a: &Actor, o: &Order, i: &Revise) -> Result<()> {
    version(o, i.row_version)?;
    authorize_order(c, a, o, true).await?;
    text(&i.reason, 1000, true)?;
    let video = o.service_code == "LYRIC_VIDEO_PLUS"
        && matches!(
            o.status.as_str(),
            "IN_PROGRESS" | "EXTERNAL_PENDING" | "COMPLETED"
        );
    if o.status != "NEEDS_INFO" && !video {
        return Err(Error::Conflict);
    }
    if let Some(d) = &i.details {
        validate_details(
            c,
            a,
            o.org_id,
            &o.service_code,
            &o.target_type,
            o.target_id,
            d,
        )
        .await?;
        if let Details::Mv { release_id, .. } = d
            && *release_id != o.release_id
        {
            return Err(Error::Invalid);
        }
        store_details(c, o.org_id, o.id, d).await?;
        audit(
            c,
            Some(a),
            o.org_id,
            o.id,
            "addon.details.updated",
            &i.reason,
            json!({"row_version":o.row_version}),
            serde_json::to_value(d).map_err(|_| Error::Internal)?,
        )
        .await?;
    }
    if video {
        let count = o.revision_count + 1;
        sqlx::query("UPDATE catalog.addon_orders SET revision_count=$3,row_version=row_version+1 WHERE org_id=$1 AND id=$2").bind(o.org_id).bind(o.id).bind(count).execute(&mut *c).await?;
        sqlx::query("UPDATE catalog.lyric_video_requests SET render_status='PENDING',output_asset_id=NULL WHERE org_id=$1 AND addon_order_id=$2").bind(o.org_id).bind(o.id).execute(&mut *c).await?;
        audit(c,Some(a),o.org_id,o.id,"addon.revision",&i.reason,json!({"revision_count":o.revision_count}),json!({"revision_count":count,"manual_review":count>o.max_revisions_snapshot.unwrap_or(0)})).await?;
        transition(c, Some(a), o.org_id, o.id, Status::UNDER_REVIEW, &i.reason).await?;
        if count <= o.max_revisions_snapshot.unwrap_or(0) {
            transition(
                c,
                Some(a),
                o.org_id,
                o.id,
                Status::APPROVED,
                "INCLUDED_REVISION",
            )
            .await?;
            transition(
                c,
                Some(a),
                o.org_id,
                o.id,
                Status::IN_PROGRESS,
                "INCLUDED_REVISION",
            )
            .await?;
        }
    } else {
        transition(c, Some(a), o.org_id, o.id, Status::QUEUED, &i.reason).await?;
    }
    Ok(())
}

pub fn record_results<'a>(
    c: &'a mut PgConnection,
    a: &'a Actor,
    o: &'a Order,
    i: &'a Results,
) -> BoxFuture<'a, ()> {
    Box::pin(record_results_inner(c, a, o, i))
}
async fn record_results_inner(
    c: &mut PgConnection,
    a: &Actor,
    o: &Order,
    i: &Results,
) -> Result<()> {
    version(o, i.row_version)?;
    text(&i.reason, 1000, true)?;
    text(&i.external_reference, 200, true)?;
    if !matches!(
        o.status.as_str(),
        "IN_PROGRESS" | "EXTERNAL_PENDING" | "UNDER_REVIEW"
    ) {
        return Err(Error::Conflict);
    }
    let before = view(c, o.org_id, o.id).await?;
    let lyrics = matches!(
        o.service_code.as_str(),
        "LYRICS_BASIC" | "AI_SYNC_LYRICS" | "LYRIC_VIDEO_PLUS"
    );
    if let Some(id) = i.lrc_asset_id {
        if !lyrics {
            return Err(Error::Invalid);
        }
        asset_ready(c, o.org_id, id, "LRC").await?;
        sqlx::query("UPDATE catalog.lyrics_requests SET lrc_asset_id=$3,sync_status='READY' WHERE org_id=$1 AND addon_order_id=$2").bind(o.org_id).bind(o.id).bind(id).execute(&mut *c).await?;
    }
    if let Some(id) = i.output_asset_id {
        if !matches!(o.service_code.as_str(), "LYRIC_VIDEO_PLUS" | "LYRICS_BASIC") {
            return Err(Error::Invalid);
        }
        asset_ready(c, o.org_id, id, "VIDEO").await?;
        sqlx::query("UPDATE catalog.lyric_video_requests SET output_asset_id=$3,render_status='READY' WHERE org_id=$1 AND addon_order_id=$2").bind(o.org_id).bind(o.id).bind(id).execute(&mut *c).await?;
    }
    if i.qr_asset_id.is_some() || i.promo_card_asset_id.is_some() || i.dsp_links.is_some() {
        if o.service_code != "PROMO_BASIC" {
            return Err(Error::Invalid);
        }
        for id in [i.qr_asset_id, i.promo_card_asset_id].into_iter().flatten() {
            asset_ready(c, o.org_id, id, "IMAGE").await?;
        }
        sqlx::query("UPDATE catalog.promo_requests SET qr_asset_id=coalesce($3,qr_asset_id),promo_card_asset_id=coalesce($4,promo_card_asset_id) WHERE org_id=$1 AND addon_order_id=$2").bind(o.org_id).bind(o.id).bind(i.qr_asset_id).bind(i.promo_card_asset_id).execute(&mut *c).await?;
        if let Some(links) = &i.dsp_links {
            if links.len() > 50 {
                return Err(Error::Invalid);
            }
            for l in links {
                text(&l.platform, 80, true)?;
                url(&l.url)?;
                sqlx::query("INSERT INTO catalog.addon_dsp_links(org_id,addon_order_id,platform,url) VALUES($1,$2,$3,$4) ON CONFLICT(addon_order_id,platform) DO UPDATE SET url=$4").bind(o.org_id).bind(o.id).bind(&l.platform).bind(&l.url).execute(&mut *c).await?;
            }
        }
    }
    if let Some(id) = i.evidence_asset_id {
        if !o.service_code.starts_with("MV_") {
            return Err(Error::Invalid);
        }
        asset_ready(c, o.org_id, id, "DOCUMENT").await?;
        let started:bool=sqlx::query_scalar("SELECT global_distribution_status<>'PENDING' FROM catalog.mv_requests WHERE addon_order_id=$1").bind(o.id).fetch_one(&mut *c).await?;
        if started {
            return Err(Error::Conflict);
        }
        sqlx::query("UPDATE catalog.mv_requests SET review_evidence_asset_id=$3,review_status='PENDING',review_decided_by=NULL,review_decided_at=NULL,evidence_valid_until=NULL WHERE org_id=$1 AND addon_order_id=$2").bind(o.org_id).bind(o.id).bind(id).execute(&mut *c).await?;
    }
    if let Some(step) = &i.migration_step {
        if o.service_code != "MIGRATION" {
            return Err(Error::Invalid);
        }
        let query = match step.as_str() {
            "UPC_APPROVED" => {
                "UPDATE catalog.migration_requests SET upc_preservation_status='APPROVED' WHERE org_id=$1 AND addon_order_id=$2 AND preserve_upc AND upc_preservation_status='PENDING'"
            }
            "UPC_NOT_AVAILABLE" => {
                "UPDATE catalog.migration_requests SET upc_preservation_status='NOT_AVAILABLE' WHERE org_id=$1 AND addon_order_id=$2 AND preserve_upc AND upc_preservation_status='PENDING'"
            }
            "DELIVERED" => {
                "UPDATE catalog.migration_requests SET delivery_status='DELIVERED' WHERE org_id=$1 AND addon_order_id=$2 AND delivery_status='PENDING' AND (NOT preserve_upc OR upc_preservation_status<>'PENDING')"
            }
            "MATCH_CONFIRMED" => {
                "UPDATE catalog.migration_requests SET matching_status='CONFIRMED' WHERE org_id=$1 AND addon_order_id=$2 AND delivery_status='DELIVERED' AND matching_status='PENDING'"
            }
            "TAKEDOWN_REQUESTED" => {
                "UPDATE catalog.migration_requests SET takedown_status='REQUESTED' WHERE org_id=$1 AND addon_order_id=$2 AND matching_status='CONFIRMED' AND takedown_status='PENDING'"
            }
            "TAKEDOWN_COMPLETED" => {
                "UPDATE catalog.migration_requests SET takedown_status='COMPLETED' WHERE org_id=$1 AND addon_order_id=$2 AND takedown_status='REQUESTED'"
            }
            _ => return Err(Error::Invalid),
        };
        if sqlx::query(query)
            .bind(o.org_id)
            .bind(o.id)
            .execute(&mut *c)
            .await?
            .rows_affected()
            != 1
        {
            return Err(Error::Conflict);
        }
    }
    if let Some(status) = &i.mv_distribution_status {
        if !o.service_code.starts_with("MV_") {
            return Err(Error::Invalid);
        }
        evidence_gate(c, o).await?;
        if !matches!(status.as_str(), "PREPARED" | "DELIVERED") {
            return Err(Error::Invalid);
        }
        let previous = if status == "PREPARED" {
            "PENDING"
        } else {
            "PREPARED"
        };
        if sqlx::query("UPDATE catalog.mv_requests SET global_distribution_status=$3 WHERE org_id=$1 AND addon_order_id=$2 AND global_distribution_status=$4").bind(o.org_id).bind(o.id).bind(status).bind(previous).execute(&mut *c).await?.rows_affected()!=1 {return Err(Error::Conflict);}
    }
    sqlx::query("UPDATE catalog.addon_provider_tasks SET status='COMPLETED',external_reference=$3,completed_at=now() WHERE org_id=$1 AND addon_order_id=$2 AND status='EXTERNAL_PENDING' AND generation=$4").bind(o.org_id).bind(o.id).bind(&i.external_reference).bind(o.dispatch_generation).execute(&mut *c).await?;
    sqlx::query(
        "UPDATE catalog.addon_orders SET row_version=row_version+1 WHERE org_id=$1 AND id=$2",
    )
    .bind(o.org_id)
    .bind(o.id)
    .execute(&mut *c)
    .await?;
    let after = view(c, o.org_id, o.id).await?;
    audit(
        c,
        Some(a),
        o.org_id,
        o.id,
        "addon.external.results",
        &i.reason,
        before,
        after,
    )
    .await?;
    operations::event(
        c,
        o.org_id,
        o.id,
        "addon.external.results",
        &format!("addon.results:{}:{}", o.id, o.row_version + 1),
    )
    .await?;
    Ok(())
}

pub async fn list(
    c: &mut PgConnection,
    a: Option<&Actor>,
    org: Option<Uuid>,
    f: &Filters,
) -> Result<Value> {
    if f.before_created_at.is_some() != f.before_id.is_some()
        || f.unprocessed_hours
            .is_some_and(|n| !(0..=87600).contains(&n))
    {
        return Err(Error::Invalid);
    }
    if let (Some(a), Some(org)) = (a, org) {
        scope(c, org).await?;
        auth::membership(c, a, org, false).await?;
    }
    // Bound pages and keyset cursor; ACLs on both the order and its existing target.
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(o)||jsonb_build_object('organization_id',o.org_id) FROM catalog.addon_orders o
        WHERE ($1::uuid IS NULL OR o.org_id=$1) AND ($2::uuid IS NULL OR (
          EXISTS(SELECT 1 FROM identity.resource_acl x WHERE x.org_id=o.org_id AND x.resource_id=o.id AND x.principal_party_id=$2 AND x.action='read' AND x.revoked_at IS NULL AND x.starts_at<=now() AND (x.ends_at IS NULL OR x.ends_at>now()))
          AND EXISTS(SELECT 1 FROM identity.resource_acl x WHERE x.org_id=o.org_id AND x.resource_id=CASE WHEN o.target_type='track' THEN o.release_id ELSE o.target_id END AND x.principal_party_id=$2 AND x.action='read' AND x.revoked_at IS NULL AND x.starts_at<=now() AND (x.ends_at IS NULL OR x.ends_at>now()))))
        AND ($3::text IS NULL OR o.service_code=$3) AND ($4::boolean IS NULL OR (o.amount>0)=$4)
        AND ($5::text IS NULL OR o.status=$5) AND ($6::uuid IS NULL OR o.assigned_admin_user_id=$6)
        AND ($7::timestamptz IS NULL OR o.submitted_at>=$7) AND ($8::timestamptz IS NULL OR o.submitted_at<=$8)
        AND ($9::uuid IS NULL OR o.artist_id=$9 OR EXISTS(SELECT 1 FROM catalog.tracks t WHERE t.org_id=o.org_id AND t.release_id=o.release_id AND t.artist_id=$9 AND t.archived_at IS NULL))
        AND ($10::uuid IS NULL OR o.release_id=$10) AND ($11::boolean IS NULL OR (o.priority>0)=$11)
        AND ($12::boolean IS NULL OR (o.status='NEEDS_INFO')=$12) AND ($13::boolean IS NULL OR (o.status='EXTERNAL_PENDING')=$13)
        AND ($14::boolean IS NULL OR (o.status='FAILED')=$14)
        AND ($15::integer IS NULL OR (o.first_reviewed_at IS NULL AND o.status NOT IN ('CANCELLED','REJECTED','COMPLETED') AND o.submitted_at<=now()-make_interval(hours=>$15)))
        AND ($16::timestamptz IS NULL OR (o.created_at,o.id)<($16,$17)) ORDER BY o.created_at DESC,o.id DESC LIMIT $18")
        .bind(org).bind(a.map(|a|a.party)).bind(&f.service_code).bind(f.paid).bind(f.status.map(|s|serde_json::to_value(s).unwrap().as_str().unwrap().to_owned()))
        .bind(f.assigned_admin_user_id).bind(f.submitted_from).bind(f.submitted_to).bind(f.artist_id).bind(f.release_id).bind(f.priority).bind(f.needs_info).bind(f.external_pending).bind(f.failed).bind(f.unprocessed_hours).bind(f.before_created_at).bind(f.before_id).bind(f.limit.unwrap_or(50).clamp(1,100)).fetch_all(c).await?;
    let cursor = rows
        .last()
        .map(|v| json!({"before_created_at":v["created_at"],"before_id":v["id"]}));
    Ok(json!({"items":rows,"next_cursor":cursor}))
}

pub async fn catalog(c: &mut PgConnection) -> Result<Value> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(c) FROM catalog.addon_service_catalog c WHERE active ORDER BY category,code").fetch_all(c).await?;
    Ok(json!({"items":rows}))
}
pub async fn update_catalog(
    c: &mut PgConnection,
    a: &Actor,
    code: &str,
    i: &CatalogUpdate,
) -> Result<Value> {
    text(&i.reason, 1000, true)?;
    text(&i.display_name, 200, true)?;
    text(&i.description, 4000, true)?;
    if i.price_krw < 0
        || i.validity_days.is_some_and(|n| n <= 0)
        || i.max_revisions.is_some_and(|n| n < 0)
    {
        return Err(Error::Invalid);
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("addon:catalog:{code}"))
        .execute(&mut *c)
        .await?;
    let old:Value=sqlx::query_scalar("SELECT to_jsonb(c) FROM catalog.addon_service_catalog c WHERE code=$1 ORDER BY version DESC LIMIT 1 FOR UPDATE").bind(code).fetch_optional(&mut *c).await?.ok_or(Error::NotFound)?;
    if old["version"].as_i64() != Some(i.expected_version as i64) {
        return Err(Error::Conflict);
    }
    sqlx::query("UPDATE catalog.addon_service_catalog SET active=false,updated_at=now() WHERE code=$1 AND active").bind(code).execute(&mut *c).await?;
    let id = Uuid::new_v4();
    let after:Value=sqlx::query_scalar("INSERT INTO catalog.addon_service_catalog(id,code,category,display_name,description,price_krw,billing_unit,active,requires_payment,validity_days,max_revisions,version,validity_months) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,(SELECT validity_months FROM catalog.addon_service_catalog WHERE code=$2 AND version=$12-1)) RETURNING to_jsonb(addon_service_catalog)")
        .bind(id).bind(code).bind(old["category"].as_str().ok_or(Error::Internal)?).bind(&i.display_name).bind(&i.description).bind(i.price_krw).bind(old["billing_unit"].as_str().ok_or(Error::Internal)?).bind(i.active).bind(i.price_krw>0).bind(i.validity_days).bind(i.max_revisions).bind(i.expected_version+1).fetch_one(&mut *c).await?;
    // Catalog is global: the existing append-only audit accepts org_id=NULL.
    sqlx::query("INSERT INTO operations.audit_events(id,actor_user_id,resource_id,action,reason_code,request_id,before_value,after_value) VALUES($1,$2,$3,'addon.catalog.version',$4,$5,$6,$7)").bind(Uuid::new_v4()).bind(a.user).bind(id).bind(&i.reason).bind(a.request).bind(old).bind(&after).execute(c).await?;
    Ok(after)
}

pub async fn refund(
    c: &mut PgConnection,
    a: &Actor,
    o: &Order,
    reference: &str,
    reason: &str,
) -> Result<()> {
    text(reference, 200, true)?;
    text(reason, 1000, true)?;
    if !matches!(o.status.as_str(), "CANCELLED" | "REJECTED") || o.payment_status != "PAID" {
        return Err(Error::Conflict);
    }
    sqlx::query("UPDATE catalog.addon_orders SET payment_status='REFUNDED',refund_status='REFUNDED',refund_reference=$3,refunded_at=now(),row_version=row_version+1 WHERE org_id=$1 AND id=$2").bind(o.org_id).bind(o.id).bind(reference).execute(&mut *c).await?;
    audit(
        c,
        Some(a),
        o.org_id,
        o.id,
        "addon.refund.override",
        reason,
        json!({"payment_status":o.payment_status}),
        json!({"payment_status":"REFUNDED","refund_reference":reference}),
    )
    .await?;
    operations::event(
        c,
        o.org_id,
        o.id,
        "addon.refund.changed",
        &format!("addon.refund:{}", o.id),
    )
    .await?;
    Ok(())
}
