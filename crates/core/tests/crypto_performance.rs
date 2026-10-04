//! Batched re-encryption must retain per-account audits, rollback and fencing.
mod support;
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Generate, Payload},
};
use audeniq_core::{payout_keys::KeyRing, privacy_maintenance::reencrypt_accounts};
use sqlx::{PgPool, Row};
use uuid::Uuid;

async fn seed(pool: &PgPool, user: Uuid, legacy: bool) -> (Uuid, Vec<u8>) {
    let org = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO identity.orgs(id,name,kind) VALUES($1,'Synthetic crypto fixture','PERSONAL')",
    )
    .bind(org)
    .execute(pool)
    .await
    .unwrap();
    let cipher = if legacy {
        let nonce = Nonce::generate();
        let encrypted = Aes256Gcm::new_from_slice(&[0x3a; 32])
            .unwrap()
            .encrypt(
                &nonce,
                Payload {
                    msg: b"11012345678901",
                    aad: org.as_bytes(),
                },
            )
            .unwrap();
        let mut cipher = nonce.to_vec();
        cipher.extend_from_slice(&encrypted);
        cipher
    } else {
        KeyRing::from_env(false)
            .unwrap()
            .seal(org, "11012345678901")
            .unwrap()
    };
    sqlx::query("INSERT INTO portal.payout_accounts(org_id,payee_type,holder_name,bank_name,account_last4,account_cipher,registered_by) VALUES($1,'INDIVIDUAL','Synthetic holder','Synthetic bank','8901',$2,$3)")
        .bind(org).bind(&cipher).bind(user).execute(pool).await.unwrap();
    (org, cipher)
}

async fn assert_original(pool: &PgPool, org: Uuid, original: &[u8]) {
    let row = sqlx::query(
        "SELECT account_cipher,row_version FROM portal.payout_accounts WHERE org_id=$1",
    )
    .bind(org)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(row.get::<Vec<u8>, _>("account_cipher"), original);
    assert_eq!(row.get::<i64, _>("row_version"), 0);
}

#[sqlx::test]
async fn bounded_batches_keep_audits_and_rollback_on_authentication_or_audit_failure(pool: PgPool) {
    // This integration binary owns one test and uses only synthetic key material.
    unsafe {
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
        std::env::set_var("PAYOUT_KMS_ENABLED", "false");
        std::env::remove_var("PAYOUT_KEYRING_FILE");
    }
    let (api, _) = support::app(pool.clone()).await;
    let user = support::user(&api).await;
    for _ in 0..105 {
        seed(&pool, user.user, true).await;
    }
    let (current, original) = seed(&pool, user.user, false).await;
    for expected in [100, 5, 0] {
        let result = reencrypt_accounts(&pool, "test-crypto-batch")
            .await
            .unwrap();
        assert_eq!(result["reencrypted"], expected);
        assert_eq!(result["batch_limit"], 100);
    }
    assert_original(&pool, current, &original).await;
    let rows = sqlx::query("SELECT org_id,account_cipher,row_version,account_last4 FROM portal.payout_accounts WHERE org_id<>$1")
        .bind(current).fetch_all(&pool).await.unwrap();
    let keys = KeyRing::from_env(false).unwrap();
    for row in rows {
        let org = row.get("org_id");
        assert_eq!(
            &*keys.open(org, row.get("account_cipher")).unwrap(),
            "11012345678901"
        );
        assert_eq!(row.get::<i64, _>("row_version"), 1);
        assert_eq!(row.get::<String, _>("account_last4"), "8901");
    }
    let audit = sqlx::query("SELECT count(*) AS n,count(DISTINCT request_id) AS requests,bool_and(org_id=resource_id AND actor_user_id IS NULL AND actor_service='audeniq-system' AND reason_code='OPERATOR:test-crypto-batch key_version=1') AS intact FROM operations.audit_events WHERE action='privacy.account.reencrypted'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(audit.get::<i64, _>("n"), 105);
    assert_eq!(audit.get::<i64, _>("requests"), 105);
    assert!(audit.get::<bool, _>("intact"));

    let (audit_failure, original) = seed(&pool, user.user, true).await;
    sqlx::query("ALTER TABLE operations.audit_events ADD CONSTRAINT reject_reencryption_fixture CHECK(action<>'privacy.account.reencrypted') NOT VALID")
        .execute(&pool).await.unwrap();
    assert!(
        reencrypt_accounts(&pool, "test-crypto-batch")
            .await
            .is_err()
    );
    assert_original(&pool, audit_failure, &original).await;
    sqlx::query("ALTER TABLE operations.audit_events DROP CONSTRAINT reject_reencryption_fixture")
        .execute(&pool)
        .await
        .unwrap();

    let (corrupt, mut bad) = seed(&pool, user.user, true).await;
    *bad.last_mut().unwrap() ^= 1;
    sqlx::query("UPDATE portal.payout_accounts SET account_cipher=$2 WHERE org_id=$1")
        .bind(corrupt)
        .bind(&bad)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        reencrypt_accounts(&pool, "test-crypto-batch")
            .await
            .is_err()
    );
    assert_original(&pool, audit_failure, &original).await;
    assert_original(&pool, corrupt, &bad).await;
    // Remove the corrupt fixture from the pending set using a valid current packet.
    sqlx::query("UPDATE portal.payout_accounts SET account_cipher=$2 WHERE org_id=$1")
        .bind(corrupt)
        .bind(keys.seal(corrupt, "11012345678901").unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let (unlocked, _) = seed(&pool, user.user, true).await;
    let mut held = pool.begin().await.unwrap();
    sqlx::query("SELECT org_id FROM portal.payout_accounts WHERE org_id=$1 FOR UPDATE")
        .bind(audit_failure)
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let result = reencrypt_accounts(&pool, "test-crypto-batch")
        .await
        .unwrap();
    assert_eq!(result["reencrypted"], 1);
    assert_original(&pool, audit_failure, &original).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT row_version FROM portal.payout_accounts WHERE org_id=$1"
        )
        .bind(unlocked)
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    held.commit().await.unwrap();
    assert_eq!(
        reencrypt_accounts(&pool, "test-crypto-batch")
            .await
            .unwrap()["reencrypted"],
        1
    );
}
