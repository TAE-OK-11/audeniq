//! F3 follow-up: rights epoch auto-bump.
//!
//! The E-1 rights-drift guard (preparation + execution) compares the epoch
//! pinned in the verification package against rights.rights_epochs. These
//! tests prove the 0020 triggers advance the epoch automatically whenever a
//! rights fact row is appended, so the guard can actually fire without a
//! manual UPDATE.
use audeniq_core::database;
use sqlx::PgPool;
use uuid::Uuid;

struct Fx {
    org: Uuid,
    release: Uuid,
    revision: Uuid,
    track: Uuid,
    party: Uuid,
    user: Uuid,
}

async fn fixtures(pool: &PgPool) -> Fx {
    database::MIGRATOR.run(pool).await.unwrap();
    let org = Uuid::new_v4();
    let release = Uuid::new_v4();
    let artist = Uuid::new_v4();
    let track = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let party = Uuid::new_v4();
    let user = Uuid::new_v4();
    let hash = "a".repeat(64);

    sqlx::query("INSERT INTO identity.orgs(id, name, kind) VALUES($1,'t','LABEL')")
        .bind(org)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO identity.resources(org_id, id, kind) VALUES($1,$2,'release'),($1,$3,'artist')",
    )
    .bind(org)
    .bind(release)
    .bind(artist)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO catalog.releases(id, org_id, title, release_type) VALUES($1,$2,'t','SINGLE')",
    )
    .bind(release)
    .bind(org)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO identity.parties(id, org_id, kind, display_name) VALUES($1,$2,'PERSON','t')",
    )
    .bind(party)
    .bind(org)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO identity.users(id, email, password_hash, party_id) VALUES($1,'t@x.y','x',$2)",
    )
    .bind(user)
    .bind(party)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO catalog.application_revisions(id, org_id, release_id, revision, body, body_hash, consent_package_hash, created_by) VALUES($1,$2,$3,1,'{}',$4,$4,$5)")
        .bind(revision).bind(org).bind(release).bind(&hash).bind(user)
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO catalog.artists(id, org_id, name) VALUES($1,$2,'t')")
        .bind(artist)
        .bind(org)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.tracks(id, org_id, release_id, title, disc_number, track_number, artist_id) VALUES($1,$2,$3,'t',1,1,$4)")
        .bind(track).bind(org).bind(release).bind(artist)
        .execute(pool).await.unwrap();

    Fx {
        org,
        release,
        revision,
        track,
        party,
        user,
    }
}

async fn epoch(pool: &PgPool, fx: &Fx) -> Option<i64> {
    sqlx::query_scalar("SELECT epoch FROM rights.rights_epochs WHERE org_id=$1 AND release_id=$2")
        .bind(fx.org)
        .bind(fx.release)
        .fetch_optional(pool)
        .await
        .unwrap()
}

async fn insert_override(pool: &PgPool, fx: &Fx) {
    sqlx::query("INSERT INTO rights.review_overrides(id, org_id, revision_id, check_code, original_status, proposed_status, reason, actor_user_id) VALUES($1,$2,$3,'CHK','REVIEW_REQUIRED','PASS','r',$4)")
        .bind(Uuid::new_v4()).bind(fx.org).bind(fx.revision).bind(fx.user)
        .execute(pool).await.unwrap();
}

async fn insert_grant(pool: &PgPool, fx: &Fx, kind: &str, target: Uuid) {
    sqlx::query("INSERT INTO rights.grant_atoms(id, org_id, party_id, target_kind, target_id, right_type) VALUES($1,$2,$3,$4,$5,'REPRODUCE')")
        .bind(Uuid::new_v4()).bind(fx.org).bind(fx.party).bind(kind).bind(target)
        .execute(pool).await.unwrap();
}

#[sqlx::test]
async fn override_insert_bumps_epoch(pool: PgPool) {
    let fx = fixtures(&pool).await;
    assert_eq!(epoch(&pool, &fx).await, None);

    insert_override(&pool, &fx).await;
    assert_eq!(epoch(&pool, &fx).await, Some(1));

    insert_override(&pool, &fx).await;
    assert_eq!(epoch(&pool, &fx).await, Some(2));
}

#[sqlx::test]
async fn grant_insert_release_target_bumps_epoch(pool: PgPool) {
    let fx = fixtures(&pool).await;
    insert_grant(&pool, &fx, "RELEASE", fx.release).await;
    assert_eq!(epoch(&pool, &fx).await, Some(1));
}

#[sqlx::test]
async fn grant_insert_track_target_bumps_epoch(pool: PgPool) {
    let fx = fixtures(&pool).await;
    // The trigger resolves the track to its release via catalog.tracks.
    insert_grant(&pool, &fx, "TRACK", fx.track).await;
    assert_eq!(epoch(&pool, &fx).await, Some(1));
}

#[sqlx::test]
async fn grant_insert_unknown_track_fails_closed(pool: PgPool) {
    let fx = fixtures(&pool).await;
    let err = sqlx::query("INSERT INTO rights.grant_atoms(id, org_id, party_id, target_kind, target_id, right_type) VALUES($1,$2,$3,'TRACK',$4,'REPRODUCE')")
        .bind(Uuid::new_v4()).bind(fx.org).bind(fx.party).bind(Uuid::new_v4())
        .execute(&pool).await.unwrap_err();
    assert!(format!("{err:?}").contains("unknown track"), "{err:?}");
    assert_eq!(epoch(&pool, &fx).await, None);
}

#[sqlx::test]
async fn bump_after_stage2_pin_moves_epoch(pool: PgPool) {
    let fx = fixtures(&pool).await;
    // Stage 2 pins the epoch with INSERT ... ON CONFLICT DO NOTHING.
    sqlx::query(
        "INSERT INTO rights.rights_epochs(org_id, release_id, epoch) VALUES($1,$2,0) ON CONFLICT DO NOTHING",
    )
    .bind(fx.org)
    .bind(fx.release)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(epoch(&pool, &fx).await, Some(0));

    // A reviewer override recorded after the pin must move the epoch so the
    // E-1 drift guard (DSP-08) fires at preparation/execution.
    insert_override(&pool, &fx).await;
    assert_eq!(epoch(&pool, &fx).await, Some(1));
}

#[sqlx::test]
async fn rights_tables_stay_append_only(pool: PgPool) {
    let fx = fixtures(&pool).await;
    insert_override(&pool, &fx).await;
    // UPDATE/DELETE on the fact tables are still rejected (F3 immutable
    // triggers); the epoch only moves via the auto-bump on INSERT.
    assert!(
        sqlx::query("UPDATE rights.review_overrides SET reason='x'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM rights.review_overrides")
            .execute(&pool)
            .await
            .is_err()
    );
    insert_grant(&pool, &fx, "RELEASE", fx.release).await;
    assert!(
        sqlx::query("UPDATE rights.grant_atoms SET right_type='x'")
            .execute(&pool)
            .await
            .is_err()
    );
}
