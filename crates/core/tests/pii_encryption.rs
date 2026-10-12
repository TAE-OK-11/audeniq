//! Personal data written before migration 0102 (plaintext) is sealed by
//! `privacy encrypt-pii`; afterwards no value is left readable in the database.
use audeniq_core::{
    payout_keys::{Field, KeyRing},
    pii, privacy_maintenance,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const EMAIL: &str = "legacy-person@example.test";
const NAME: &str = "홍길동";
const HOLDER: &str = "서린";
const CONTACT_EMAIL: &str = "booking@example.test";
const CONTACT_PHONE: &str = "010-1234-5678";
const TIN: &str = "123456789";

fn keys() -> KeyRing {
    KeyRing::from_env(false).unwrap()
}

/// Rows in the pre-0102 form: plaintext personal data, TIN under the
/// account-number AAD.
async fn legacy_rows(pool: &PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    audeniq_core::database::MIGRATOR.run(pool).await.unwrap();
    let keys = keys();
    let (org, party, user, contact) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let rel = Uuid::new_v4();
    for sql in [
        "INSERT INTO identity.orgs(id,name,kind) VALUES($1,'t','PERSONAL')",
        "INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($2,$1,'PERSON','t')",
        "INSERT INTO identity.users(id,email,password_hash,party_id) VALUES($3,'legacy-person@example.test','x',$2)",
        "INSERT INTO portal.account_identities(user_id,real_name,birth_date,provider,method,transaction_id,ci_hash)
           VALUES($3,'홍길동','1990-05-17','test','PASS','tx-legacy',repeat('a',64))",
        "INSERT INTO portal.artist_profiles(user_id,display_name,contact_email) VALUES($3,'t','booking@example.test')",
        "INSERT INTO work.relationships(id,kind,name,created_by) VALUES($4,'ARTIST','t',$3)",
    ] {
        sqlx::query(sql)
            .bind(org)
            .bind(party)
            .bind(user)
            .bind(rel)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO work.relationship_contacts(id,relationship_id,name,email,phone,created_by) VALUES($1,$2,'김담당',$3,$4,$5)")
        .bind(contact).bind(rel).bind(CONTACT_EMAIL).bind(CONTACT_PHONE).bind(user)
        .execute(pool).await.unwrap();
    let account = keys.seal(org, "11012345678901").unwrap();
    sqlx::query("INSERT INTO portal.payout_accounts(org_id,payee_type,holder_name,bank_name,account_last4,account_cipher,registered_by)
                 VALUES($1,'INDIVIDUAL',$2,'국민은행','8901',$3,$4)")
        .bind(org).bind(HOLDER).bind(&account).bind(user)
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO portal.payout_requests(id,org_id,requested_by,payee_party_id,amount,currency,idempotency_key,account_snapshot,account_cipher,account_key_version)
                 VALUES($1,$2,$3,$4,10000,'KRW','legacy-request',$5,$6,1)")
        .bind(Uuid::new_v4()).bind(org).bind(user).bind(party)
        .bind(serde_json::json!({"payee_type":"INDIVIDUAL","holder_name":HOLDER,"bank_name":"국민은행","account_last4":"8901"}))
        .bind(&account)
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO portal.tax_residencies(user_id,foreign_tax,country,tin_cipher,tin_last4,key_version) VALUES($1,true,'US',$2,'6789',1)")
        .bind(user).bind(keys.seal(user, TIN).unwrap())
        .execute(pool).await.unwrap();
    (org, user, contact, rel)
}

#[sqlx::test]
async fn encrypt_pii_seals_legacy_plaintext_and_is_idempotent(pool: PgPool) {
    // Safety: set before any keyring is read in this test binary.
    unsafe { std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32)) };
    let (org, user, contact, _) = legacy_rows(&pool).await;
    let keys = keys();
    // Readable in the legacy form before the backfill.
    assert_eq!(pii::user_email(&keys, &pool, user).await.unwrap(), EMAIL);
    assert_eq!(
        pii::user_by_email(&keys, &pool, EMAIL).await.unwrap(),
        Some(user)
    );

    let mut rounds = 0;
    loop {
        let r = privacy_maintenance::encrypt_pii(&pool, "test-operator")
            .await
            .unwrap();
        rounds += 1;
        if r["total"] == 0 {
            break;
        }
        assert!(rounds < 5, "{r}");
    }
    assert_eq!(
        rounds, 2,
        "one round seals everything, the next finds nothing"
    );

    // Nothing readable is left in any of these tables.
    let dump: String = sqlx::query_scalar(
        "SELECT string_agg(t::text, ' ') FROM (
           SELECT row_to_json(u)::text t FROM identity.users u
           UNION ALL SELECT row_to_json(i)::text FROM portal.account_identities i
           UNION ALL SELECT row_to_json(a)::text FROM portal.payout_accounts a
           UNION ALL SELECT row_to_json(r)::text FROM portal.payout_requests r
           UNION ALL SELECT row_to_json(p)::text FROM portal.artist_profiles p
           UNION ALL SELECT row_to_json(c)::text FROM work.relationship_contacts c
           UNION ALL SELECT row_to_json(x)::text FROM portal.tax_residencies x) s",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    for secret in [
        EMAIL,
        NAME,
        "1990-05-17",
        HOLDER,
        CONTACT_EMAIL,
        CONTACT_PHONE,
        "김담당",
    ] {
        assert!(!dump.contains(secret), "{secret} left in plaintext");
    }

    // Every value reads back, and the email is still found for login.
    assert_eq!(pii::user_email(&keys, &pool, user).await.unwrap(), EMAIL);
    assert_eq!(
        pii::user_by_email(&keys, &pool, " Legacy-Person@Example.TEST ")
            .await
            .unwrap(),
        Some(user)
    );
    assert_eq!(
        pii::user_by_email(&keys, &pool, "other@example.test")
            .await
            .unwrap(),
        None
    );
    let id = sqlx::query(
        "SELECT real_name_cipher,birth_date_cipher FROM portal.account_identities WHERE user_id=$1",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        &*keys
            .open_field(Field::IdentityRealName, user, id.get::<&[u8], _>(0))
            .unwrap(),
        NAME
    );
    assert_eq!(
        &*keys
            .open_field(Field::IdentityBirthDate, user, id.get::<&[u8], _>(1))
            .unwrap(),
        "1990-05-17"
    );
    let holder: Vec<u8> =
        sqlx::query_scalar("SELECT holder_cipher FROM portal.payout_accounts WHERE org_id=$1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        &*keys
            .open_field(Field::PayoutHolderName, org, &holder)
            .unwrap(),
        HOLDER
    );
    let req = sqlx::query("SELECT holder_cipher,account_cipher,account_snapshot FROM portal.payout_requests WHERE org_id=$1")
        .bind(org).fetch_one(&pool).await.unwrap();
    assert_eq!(
        &*keys
            .open_field(Field::PayoutHolderName, org, req.get::<&[u8], _>(0))
            .unwrap(),
        HOLDER
    );
    assert_eq!(
        &*keys.open(org, req.get::<&[u8], _>(1)).unwrap(),
        "11012345678901"
    );
    assert_eq!(req.get::<serde_json::Value, _>(2)["account_last4"], "8901");
    let c = sqlx::query("SELECT name,email,phone,name_cipher,email_cipher,phone_cipher FROM work.relationship_contacts WHERE id=$1")
        .bind(contact).fetch_one(&pool).await.unwrap();
    assert!(
        c.get::<Option<String>, _>(0).is_none()
            && c.get::<Option<String>, _>(1).is_none()
            && c.get::<Option<String>, _>(2).is_none()
    );
    assert_eq!(
        &*keys
            .open_field(Field::ContactName, contact, c.get::<&[u8], _>(3))
            .unwrap(),
        "김담당"
    );
    assert_eq!(
        &*keys
            .open_field(Field::ContactEmail, contact, c.get::<&[u8], _>(4))
            .unwrap(),
        CONTACT_EMAIL
    );
    assert_eq!(
        &*keys
            .open_field(Field::ContactPhone, contact, c.get::<&[u8], _>(5))
            .unwrap(),
        CONTACT_PHONE
    );
    let tin =
        sqlx::query("SELECT tin_cipher,tin_field_aad FROM portal.tax_residencies WHERE user_id=$1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(tin.get::<bool, _>(1));
    assert_eq!(
        &*keys
            .open_field(Field::TaxTin, user, tin.get::<&[u8], _>(0))
            .unwrap(),
        TIN
    );
    assert!(
        keys.open(user, tin.get::<&[u8], _>(0)).is_err(),
        "TIN moved off the account-number AAD"
    );

    // A second sign-up with the same address is refused across forms.
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identity.users WHERE email_bidx = ANY($1)")
            .bind(pii::email_indexes(&keys, EMAIL))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1);
    // The backfill is audited without values.
    let reasons: Vec<String> = sqlx::query_scalar(
        "SELECT reason_code FROM operations.audit_events WHERE action='privacy.pii.sealed'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(!reasons.is_empty());
    assert!(reasons.iter().all(|r| !r.contains('@')));
}

#[sqlx::test]
async fn database_constraints_refuse_both_forms_at_once(pool: PgPool) {
    unsafe { std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32)) };
    let (_, user, contact, _) = legacy_rows(&pool).await;
    for sql in [
        "UPDATE identity.users SET email_cipher='\\x00' WHERE id=$1",
        "UPDATE identity.users SET email=NULL WHERE id=$1",
        "UPDATE portal.account_identities SET real_name_cipher=decode(repeat('00',40),'hex') WHERE user_id=$1",
        "UPDATE portal.artist_profiles SET contact_email_cipher=decode(repeat('00',40),'hex') WHERE user_id=$1",
        "UPDATE work.relationship_contacts SET phone_cipher=decode(repeat('00',40),'hex') WHERE id=$2",
    ] {
        let r = sqlx::query(sql)
            .bind(user)
            .bind(contact)
            .execute(&pool)
            .await;
        assert!(r.is_err(), "{sql}");
    }
}
