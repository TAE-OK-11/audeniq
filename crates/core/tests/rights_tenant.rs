//! F3 follow-up: tenant boundary on grant foreign keys (migration 0021).
//!
//! rights.grant_atoms.parent_grant_id and .contract_revision_id used to
//! reference the bare global id, so a grant in org A could name a parent
//! grant or contract revision owned by org B. Both are now composite
//! (org_id, id) foreign keys; these tests pin that boundary.
use audeniq_core::database;
use sqlx::PgPool;
use uuid::Uuid;

struct Org {
    id: Uuid,
    party: Uuid,
    release: Uuid,
}

async fn org(pool: &PgPool) -> Org {
    database::MIGRATOR.run(pool).await.unwrap();
    let id = Uuid::new_v4();
    let party = Uuid::new_v4();
    let release = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.orgs(id, name, kind) VALUES($1,'t','LABEL')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.resources(org_id, id, kind) VALUES($1,$2,'release')")
        .bind(id)
        .bind(release)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO catalog.releases(id, org_id, title, release_type) VALUES($1,$2,'t','SINGLE')",
    )
    .bind(release)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO identity.parties(id, org_id, kind, display_name) VALUES($1,$2,'PERSON','t')",
    )
    .bind(party)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    Org { id, party, release }
}

async fn grant(
    pool: &PgPool,
    o: &Org,
    parent: Option<Uuid>,
    contract_rev: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO rights.grant_atoms(id, org_id, party_id, target_kind, target_id, right_type, parent_grant_id, contract_revision_id) VALUES($1,$2,$3,'RELEASE',$4,'REPRODUCE',$5,$6)")
        .bind(id).bind(o.id).bind(o.party).bind(o.release).bind(parent).bind(contract_rev)
        .execute(pool).await?;
    Ok(id)
}

async fn contract_revision(pool: &PgPool, o: &Org) -> Uuid {
    let contract = Uuid::new_v4();
    let rev = Uuid::new_v4();
    let asset = Uuid::new_v4();
    let hash = "b".repeat(64);
    sqlx::query("INSERT INTO identity.resources(org_id, id, kind) VALUES($1,$2,'asset')")
        .bind(o.id)
        .bind(asset)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO catalog.assets(id, org_id, kind, object_key, size_bytes, content_type) VALUES($1,$2,'AUDIO','k',100,'audio/wav')",
    )
    .bind(asset)
    .bind(o.id)
    .execute(pool)
    .await
    .unwrap();
    // contracts require two distinct parties.
    let p2 = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO identity.parties(id, org_id, kind, display_name) VALUES($1,$2,'PERSON','u')",
    )
    .bind(p2)
    .bind(o.id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO rights.contracts(id, org_id, grantor_party_id, grantee_party_id) VALUES($1,$2,$3,$4)",
    )
    .bind(contract)
    .bind(o.id)
    .bind(o.party)
    .bind(p2)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO rights.contract_revisions(id, org_id, contract_id, revision, document_asset_id, document_hash, policy_version) VALUES($1,$2,$3,1,$4,$5,'v1')")
        .bind(rev).bind(o.id).bind(contract).bind(asset).bind(&hash)
        .execute(pool).await.unwrap();
    rev
}

#[sqlx::test]
async fn parent_grant_same_org_accepted(pool: PgPool) {
    let a = org(&pool).await;
    let parent = grant(&pool, &a, None, None).await.unwrap();
    grant(&pool, &a, Some(parent), None).await.unwrap();
}

#[sqlx::test]
async fn parent_grant_cross_org_rejected(pool: PgPool) {
    let a = org(&pool).await;
    let b = org(&pool).await;
    let parent = grant(&pool, &a, None, None).await.unwrap();
    let err = grant(&pool, &b, Some(parent), None).await.unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("grant_atoms_parent_org_fkey") || msg.contains("violates foreign key"),
        "{msg}"
    );
}

#[sqlx::test]
async fn contract_revision_same_org_accepted(pool: PgPool) {
    let a = org(&pool).await;
    let rev = contract_revision(&pool, &a).await;
    grant(&pool, &a, None, Some(rev)).await.unwrap();
}

#[sqlx::test]
async fn contract_revision_cross_org_rejected(pool: PgPool) {
    let a = org(&pool).await;
    let b = org(&pool).await;
    let rev = contract_revision(&pool, &a).await;
    let err = grant(&pool, &b, None, Some(rev)).await.unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("grant_atoms_contract_rev_org_fkey") || msg.contains("violates foreign key"),
        "{msg}"
    );
}
