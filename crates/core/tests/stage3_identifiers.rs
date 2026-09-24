use audeniq_core::{
    database,
    identifiers::{ExistingAssignment, IdentifierKind, record_existing},
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

struct Seed {
    org: Uuid,
    release: Uuid,
    revision: Uuid,
    track: Uuid,
}

async fn seed(pool: &PgPool) -> Seed {
    let s = Seed {
        org: Uuid::new_v4(),
        release: Uuid::new_v4(),
        revision: Uuid::new_v4(),
        track: Uuid::new_v4(),
    };
    let party = Uuid::new_v4();
    let user = Uuid::new_v4();
    let artist = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.orgs(id,name,kind) VALUES($1,'Synthetic','PERSONAL')")
        .bind(s.org)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'PERSON','Synthetic')").bind(party).bind(s.org).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO identity.users(id,email,password_hash,party_id) VALUES($1,$2,'test',$3)",
    )
    .bind(user)
    .bind(format!("{user}@example.test"))
    .bind(party)
    .execute(pool)
    .await
    .unwrap();
    for (id, kind) in [(s.release, "release"), (artist, "artist")] {
        sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,$3)")
            .bind(s.org)
            .bind(id)
            .bind(kind)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO catalog.releases(id,org_id,title,release_type) VALUES($1,$2,'Synthetic','SINGLE')").bind(s.release).bind(s.org).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO catalog.artists(id,org_id,name) VALUES($1,$2,'Synthetic')")
        .bind(artist)
        .bind(s.org)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.tracks(id,org_id,release_id,title,disc_number,track_number,artist_id) VALUES($1,$2,$3,'Synthetic',1,1,$4)").bind(s.track).bind(s.org).bind(s.release).bind(artist).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO catalog.application_revisions(id,org_id,release_id,revision,body,body_hash,consent_package_hash,created_by) VALUES($1,$2,$3,1,'{}',$4,$4,$5)")
        .bind(s.revision).bind(s.org).bind(s.release).bind("a".repeat(64)).bind(user).execute(pool).await.unwrap();
    s
}

fn assignment(s: &Seed) -> ExistingAssignment<'static> {
    ExistingAssignment {
        org_id: s.org,
        release_id: s.release,
        track_id: Some(s.track),
        revision_id: s.revision,
        kind: IdentifierKind::Isrc,
        value: "USAAA2600001",
    }
}

async fn set_org(c: &mut PgConnection, org: Uuid) {
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(c)
        .await
        .unwrap();
}

#[sqlx::test]
async fn ledger_reuse_concurrency_and_conflict(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let s = seed(&pool).await;
    let other = seed(&pool).await;
    let a = assignment(&s);
    let mut c1 = pool.acquire().await.unwrap();
    let mut c2 = pool.acquire().await.unwrap();
    let (one, two) = tokio::join!(record_existing(&mut c1, &a), record_existing(&mut c2, &a));
    let id = one.unwrap();
    assert_eq!(id, two.unwrap());
    assert!(record_existing(&mut c1, &assignment(&other)).await.is_err());
    let mut changed = assignment(&s);
    changed.value = "USAAA2600002";
    assert!(record_existing(&mut c1, &changed).await.is_err());
    changed = assignment(&s);
    changed.revision_id = other.revision;
    assert!(record_existing(&mut c1, &changed).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM distribution.identifier_assignments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn ledger_sql_constraints_immutability_and_org_isolation(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let s = seed(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    set_org(&mut tx, s.org).await;
    let a = ExistingAssignment {
        org_id: s.org,
        release_id: s.release,
        track_id: None,
        revision_id: s.revision,
        kind: IdentifierKind::Upc,
        value: "012345678905",
    };
    let id = record_existing(&mut tx, &a).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        sqlx::query(
            "UPDATE distribution.identifier_assignments SET identifier='036000291452' WHERE id=$1"
        )
        .bind(id)
        .execute(&pool)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM distribution.identifier_assignments WHERE id=$1")
            .bind(id)
            .execute(&pool)
            .await
            .is_err()
    );
    for identifier in ["012345678904", "000000000000", "0036000291452"] {
        assert!(sqlx::query("INSERT INTO distribution.identifier_assignments(id,org_id,release_id,revision_id,kind,identifier) VALUES($1,$2,$3,$4,'UPC',$5)")
            .bind(Uuid::new_v4()).bind(s.org).bind(s.release).bind(s.revision).bind(identifier).execute(&pool).await.is_err());
    }
    // Role is created and dropped transactionally; it has SELECT but cannot bypass RLS.
    let mut tx = pool.begin().await.unwrap();
    let role = format!("f4_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!(
        "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOBYPASSRLS"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(&format!("GRANT USAGE ON SCHEMA distribution TO {role}"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(&format!(
        "GRANT SELECT ON distribution.identifier_assignments TO {role}"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(&format!("SET LOCAL ROLE {role}"))
        .execute(&mut *tx)
        .await
        .unwrap();
    set_org(&mut tx, Uuid::new_v4()).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM distribution.identifier_assignments")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 0);
    set_org(&mut tx, s.org).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM distribution.identifier_assignments")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 1);
    tx.rollback().await.unwrap();
}
