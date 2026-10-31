//! Bounded owner-only maintenance. Never age-delete contracts, finance or assets.
use crate::{
    error::{Error, Result},
    operations,
    payout_keys::{Field, KeyRing},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub(crate) async fn owner(pool: &PgPool, operator: &str) -> Result<()> {
    if operator.trim().is_empty() || operator.chars().count() > 120 {
        return Err(Error::Invalid);
    }
    crate::text_policy::check(operator)?;
    let owner: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_roles r ON r.oid=c.relowner WHERE n.nspname='identity' AND c.relname='sessions' AND r.rolname=current_user) OR (SELECT rolsuper FROM pg_roles WHERE rolname=current_user)")
        .fetch_one(pool).await?;
    if !owner {
        return Err(Error::Forbidden);
    }
    Ok(())
}

/// A 24-hour recovery grace is an operational setting, not a statutory period.
pub async fn purge_transient(pool: &PgPool, operator: &str) -> Result<Value> {
    owner(pool, operator).await?;
    let mut tx = pool.begin().await?;
    let sessions=sqlx::query("WITH old AS (SELECT token_hash FROM identity.sessions WHERE expires_at<now()-interval '1 day' OR revoked_at<now()-interval '1 day' ORDER BY expires_at LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM identity.sessions s USING old WHERE s.token_hash=old.token_hash")
        .execute(&mut *tx).await?.rows_affected();
    let limits=sqlx::query("WITH old AS (SELECT bucket_hash FROM identity.auth_limits WHERE window_start<now()-interval '1 day' ORDER BY window_start LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM identity.auth_limits a USING old WHERE a.bucket_hash=old.bucket_hash")
        .execute(&mut *tx).await?.rows_affected();
    // AI connections: spent codes, expired tokens and device codes, old daily counts.
    let mut mcp = 0;
    for q in [
        "WITH old AS (SELECT code_hash FROM identity.mcp_codes WHERE expires_at<now()-interval '1 day' ORDER BY expires_at LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM identity.mcp_codes c USING old WHERE c.code_hash=old.code_hash",
        "WITH old AS (SELECT token_hash FROM identity.mcp_tokens WHERE expires_at<now()-interval '1 day' ORDER BY expires_at LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM identity.mcp_tokens t USING old WHERE t.token_hash=old.token_hash",
        "WITH old AS (SELECT device_code_hash FROM identity.mcp_device_codes WHERE expires_at<now()-interval '1 day' ORDER BY expires_at LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM identity.mcp_device_codes d USING old WHERE d.device_code_hash=old.device_code_hash",
        "DELETE FROM identity.mcp_daily_usage WHERE day < (now() AT TIME ZONE 'Asia/Seoul')::date - 1",
        "DELETE FROM identity.mcp_idempotency WHERE created_at < now()-interval '7 days'",
        // Dynamically registered AI clients that never got a connection.
        "WITH old AS (SELECT c.id FROM identity.mcp_clients c WHERE c.kind='DCR' AND c.created_at < now()-interval '30 days'
            AND NOT EXISTS (SELECT 1 FROM identity.mcp_grants g WHERE g.client_pk=c.id) ORDER BY c.created_at LIMIT 10000 FOR UPDATE OF c SKIP LOCKED)
         DELETE FROM identity.mcp_clients c USING old WHERE c.id=old.id",
    ] {
        mcp += sqlx::query(q).execute(&mut *tx).await?.rows_affected();
    }
    let logs=sqlx::query("WITH old AS (SELECT id FROM privacy.staff_access_logs WHERE retain_until<now() ORDER BY occurred_at LIMIT 10000 FOR UPDATE SKIP LOCKED) DELETE FROM privacy.staff_access_logs l USING old WHERE l.id=old.id")
        .execute(&mut *tx).await?.rows_affected();
    operations::audit(
        &mut tx,
        None,
        None,
        None,
        "privacy.transient.purged",
        &format!(
            "OPERATOR:{} sessions={sessions} limits={limits} expired_logs={logs} mcp={mcp}",
            operator.trim()
        ),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"sessions":sessions,"auth_limits":limits,"expired_access_logs":logs,"mcp":mcp,"batch_limit":10000}),
    )
}

/// Idempotent 100-row batch. Historical keys must stay available until rows and
/// retained backups have been migrated or expired. No clear numbers leave here.
pub async fn reencrypt_accounts(pool: &PgPool, operator: &str) -> Result<Value> {
    owner(pool, operator).await?;
    let keys = KeyRing::from_env(crate::config::kms_enabled()?)?;
    let mut tx = pool.begin().await?;
    let rows=sqlx::query("SELECT org_id,account_cipher FROM portal.payout_accounts WHERE key_version<>$1 OR substring(account_cipher FROM 1 FOR 6)<>$2 ORDER BY org_id LIMIT 100 FOR UPDATE SKIP LOCKED")
        .bind(keys.active_version()).bind(crate::payout_keys::TAG).fetch_all(&mut *tx).await?;
    let mut orgs = Vec::with_capacity(rows.len());
    let mut ciphers = Vec::with_capacity(rows.len());
    let mut event_ids = Vec::with_capacity(rows.len());
    let mut request_ids = Vec::with_capacity(rows.len());
    for row in &rows {
        let org: Uuid = row.get("org_id");
        let cipher: &[u8] = row.get("account_cipher");
        let number = keys.open(org, cipher)?;
        orgs.push(org);
        ciphers.push(keys.seal(org, &number)?);
        event_ids.push(Uuid::new_v4());
        request_ids.push(Uuid::new_v4());
    }
    if !rows.is_empty() {
        // The selected rows remain locked. Update and one audit event per
        // account share one statement/transaction; an audit failure rolls all
        // changes back. Only ciphertext and IDs enter these bounded arrays.
        let changed = sqlx::query(
            "WITH changed AS (
                UPDATE portal.payout_accounts AS account
                SET account_cipher=source.cipher,key_version=$3,row_version=account.row_version+1
                FROM unnest($1::uuid[],$2::bytea[]) AS source(org_id,cipher)
                WHERE account.org_id=source.org_id RETURNING account.org_id
             )
             INSERT INTO operations.audit_events(id,actor_user_id,actor_service,org_id,resource_id,action,reason_code,request_id)
             SELECT source.event_id,NULL,'audeniq-system',changed.org_id,changed.org_id,
                    'privacy.account.reencrypted',$6,source.request_id
             FROM changed JOIN unnest($1::uuid[],$4::uuid[],$5::uuid[]) AS source(org_id,event_id,request_id)
                  ON changed.org_id=source.org_id"
        )
        .bind(&orgs).bind(&ciphers).bind(keys.active_version()).bind(&event_ids).bind(&request_ids)
        .bind(format!("OPERATOR:{} key_version={}", operator.trim(), keys.active_version()))
        .execute(&mut *tx).await?.rows_affected();
        if changed != rows.len() as u64 {
            return Err(Error::Internal);
        }
    }
    tx.commit().await?;
    Ok(json!({"reencrypted":rows.len(),"active_version":keys.active_version(),"batch_limit":100}))
}

/// How one sealed column is opened and sealed.
#[derive(Clone, Copy)]
enum Seal {
    Field(Field),
    /// Account-number copies (`KeyRing::seal`, AAD = org; header-less legacy allowed).
    Account,
    /// TINs sealed before 0102 used the account-number AAD with the user id.
    Tin,
}

struct Column {
    seal: Seal,
    /// Plaintext expression (None: ciphertext only).
    plain: Option<&'static str>,
    cipher: &'static str,
    /// SET clause that drops the plaintext once sealed.
    clear: Option<&'static str>,
    /// Key-version column to set, if the table has one.
    version: Option<&'static str>,
    /// Blind-index column to set (login email).
    index: Option<&'static str>,
    /// Extra condition that marks a row as needing work.
    stale: Option<&'static str>,
}

struct Target {
    name: &'static str,
    table: &'static str,
    id: &'static str,
    subject: &'static str,
    columns: &'static [Column],
}

const fn col(seal: Seal, plain: &'static str, cipher: &'static str, clear: &'static str) -> Column {
    Column {
        seal,
        plain: Some(plain),
        cipher,
        clear: Some(clear),
        version: None,
        index: None,
        stale: None,
    }
}

const SNAPSHOT_HOLDER: Column = col(
    Seal::Field(Field::PayoutHolderName),
    "account_snapshot->>'holder_name'",
    "holder_cipher",
    "account_snapshot=account_snapshot - 'holder_name'",
);
const ACCOUNT_COPY: Column = Column {
    seal: Seal::Account,
    plain: None,
    cipher: "account_cipher",
    clear: None,
    version: Some("account_key_version"),
    index: None,
    stale: None,
};

/// A value that is only ever stored sealed (no plaintext column).
const fn sealed_only(f: Field, cipher: &'static str) -> Column {
    Column {
        seal: Seal::Field(f),
        plain: None,
        cipher,
        clear: None,
        version: None,
        index: None,
        stale: None,
    }
}

const TARGETS: &[Target] = &[
    // Infringement claimants (0114): contact details are sealed from the start;
    // listed so key rotation re-seals them.
    Target {
        name: "portal.infringement_claims",
        table: "portal.infringement_claims",
        id: "id",
        subject: "id",
        columns: &[
            sealed_only(Field::ClaimantName, "claimant_name_cipher"),
            sealed_only(Field::ClaimantEmail, "claimant_email_cipher"),
            sealed_only(Field::ClaimantPhone, "claimant_phone_cipher"),
        ],
    },
    // Release alerts (0118): fan addresses, sealed until sent then erased.
    Target {
        name: "portal.release_alerts",
        table: "portal.release_alerts",
        id: "id",
        subject: "id",
        columns: &[sealed_only(Field::ReleaseAlertEmail, "email_cipher")],
    },
    // Pre-saves (0152): the fan's platform token, sealed until the album is saved.
    Target {
        name: "portal.presaves",
        table: "portal.presaves",
        id: "id",
        subject: "id",
        columns: &[sealed_only(Field::PresaveToken, "token_cipher")],
    },
    // Split collaborators (0119): invite address, sealed until answered.
    Target {
        name: "portal.split_shares",
        table: "portal.split_shares",
        id: "id",
        subject: "id",
        columns: &[sealed_only(Field::SplitEmail, "email_cipher")],
    },
    // Co-writer split invites (0171): address and link, sealed until mailed / answered.
    Target {
        name: "portal.pub_split_invites",
        table: "portal.pub_split_invites",
        id: "id",
        subject: "id",
        columns: &[
            sealed_only(Field::PubInviteEmail, "email_cipher"),
            sealed_only(Field::PubInviteToken, "token_cipher"),
        ],
    },
    Target {
        name: "identity.users",
        table: "identity.users",
        id: "id",
        subject: "id",
        columns: &[Column {
            index: Some("email_bidx"),
            ..col(
                Seal::Field(Field::UserEmail),
                "email",
                "email_cipher",
                "email=NULL",
            )
        }],
    },
    Target {
        name: "portal.account_identities",
        table: "portal.account_identities",
        id: "user_id",
        subject: "user_id",
        columns: &[
            col(
                Seal::Field(Field::IdentityRealName),
                "real_name",
                "real_name_cipher",
                "real_name=NULL",
            ),
            col(
                Seal::Field(Field::IdentityBirthDate),
                "birth_date::text",
                "birth_date_cipher",
                "birth_date=NULL",
            ),
        ],
    },
    Target {
        name: "portal.payout_accounts",
        table: "portal.payout_accounts",
        id: "org_id",
        subject: "org_id",
        columns: &[col(
            Seal::Field(Field::PayoutHolderName),
            "holder_name",
            "holder_cipher",
            "holder_name=NULL",
        )],
    },
    Target {
        name: "portal.payout_requests",
        table: "portal.payout_requests",
        id: "id",
        subject: "org_id",
        columns: &[SNAPSHOT_HOLDER, ACCOUNT_COPY],
    },
    Target {
        name: "finance.payout_orders",
        table: "finance.payout_orders",
        id: "id",
        subject: "org_id",
        columns: &[SNAPSHOT_HOLDER, ACCOUNT_COPY],
    },
    Target {
        name: "portal.artist_profiles",
        table: "portal.artist_profiles",
        id: "user_id",
        subject: "user_id",
        columns: &[col(
            Seal::Field(Field::ProfileContactEmail),
            "contact_email",
            "contact_email_cipher",
            "contact_email=''",
        )],
    },
    Target {
        name: "work.relationship_contacts",
        table: "work.relationship_contacts",
        id: "id",
        subject: "id",
        columns: &[
            col(
                Seal::Field(Field::ContactName),
                "name",
                "name_cipher",
                "name=NULL",
            ),
            col(
                Seal::Field(Field::ContactEmail),
                "email",
                "email_cipher",
                "email=NULL",
            ),
            col(
                Seal::Field(Field::ContactPhone),
                "phone",
                "phone_cipher",
                "phone=NULL",
            ),
        ],
    },
    Target {
        name: "portal.tax_residencies",
        table: "portal.tax_residencies",
        id: "user_id",
        subject: "user_id",
        columns: &[Column {
            seal: Seal::Tin,
            plain: None,
            cipher: "tin_cipher",
            clear: Some("tin_field_aad=true"),
            version: Some("key_version"),
            index: None,
            stale: Some("(tin_cipher IS NOT NULL AND NOT tin_field_aad)"),
        }],
    },
];

fn open_value(
    keys: &KeyRing,
    seal: Seal,
    subject: Uuid,
    cipher: &[u8],
) -> Result<zeroize::Zeroizing<String>> {
    match seal {
        Seal::Field(f) => keys.open_field(f, subject, cipher),
        Seal::Account => keys.open(subject, cipher),
        Seal::Tin => keys
            .open_field(Field::TaxTin, subject, cipher)
            .or_else(|_| keys.open(subject, cipher)),
    }
}

fn seal_value(keys: &KeyRing, seal: Seal, subject: Uuid, value: &str) -> Result<Vec<u8>> {
    match seal {
        Seal::Field(f) => keys.seal_field(f, subject, value),
        Seal::Account => keys.seal(subject, value),
        Seal::Tin => keys.seal_field(Field::TaxTin, subject, value),
    }
}

/// One table: seal up to 100 plaintext rows and re-seal rows under an old key
/// version. Every value of a row is re-sealed together, in one UPDATE.
async fn seal_batch(tx: &mut sqlx::PgConnection, keys: &KeyRing, t: &Target) -> Result<u64> {
    let active = keys.active_version().to_be_bytes().to_vec();
    let mut select = format!("SELECT {} AS row_id, {} AS subject", t.id, t.subject);
    let mut stale = Vec::new();
    for (i, c) in t.columns.iter().enumerate() {
        select.push_str(&format!(
            ", ({})::text AS p{i}, {} AS c{i}",
            c.plain.unwrap_or("NULL"),
            c.cipher
        ));
        if let Some(p) = c.plain {
            stale.push(format!("NULLIF(({p})::text,'') IS NOT NULL"));
        }
        stale.push(format!(
            "({0} IS NOT NULL AND substring({0} FROM 7 FOR 2)<>$1)",
            c.cipher
        ));
        if let Some(x) = c.stale {
            stale.push(x.to_owned());
        }
    }
    let sql = format!(
        "{select} FROM {} WHERE {} ORDER BY {} LIMIT 100 FOR UPDATE SKIP LOCKED",
        t.table,
        stale.join(" OR "),
        t.id
    );
    // Table and column names come only from TARGETS (no input).
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(&active)
        .fetch_all(&mut *tx)
        .await?;
    for row in &rows {
        let id: Uuid = row.get("row_id");
        let subject: Uuid = row.get("subject");
        let mut sets = Vec::new();
        // Bound after $1 (the row id), in this order.
        let mut ciphers: Vec<Vec<u8>> = Vec::new();
        let mut indexes: Vec<String> = Vec::new();
        let mut index_sets = Vec::new();
        for (i, c) in t.columns.iter().enumerate() {
            let plain: Option<String> = row
                .get::<Option<String>, _>(format!("p{i}").as_str())
                .filter(|p| !p.is_empty());
            let cipher: Option<Vec<u8>> = row.get(format!("c{i}").as_str());
            let value = match (&cipher, plain) {
                (Some(sealed), _) => open_value(keys, c.seal, subject, sealed)?,
                (None, Some(p)) => zeroize::Zeroizing::new(p),
                (None, None) => continue,
            };
            ciphers.push(seal_value(keys, c.seal, subject, &value)?);
            sets.push(format!("{}=${}", c.cipher, ciphers.len() + 1));
            sets.extend(c.clear.map(str::to_owned));
            if let Some(v) = c.version {
                sets.push(format!("{v}={}", keys.active_version()));
            }
            if let (Some(col), Seal::Field(f)) = (c.index, c.seal) {
                indexes.push(keys.blind_index(f, &value)?);
                index_sets.push((col, indexes.len()));
            }
        }
        for (col, k) in index_sets {
            sets.push(format!("{col}=${}", ciphers.len() + 1 + k));
        }
        let update = format!(
            "UPDATE {} SET {} WHERE {}=$1",
            t.table,
            sets.join(","),
            t.id
        );
        let mut q = sqlx::query(sqlx::AssertSqlSafe(update)).bind(id);
        for c in &ciphers {
            q = q.bind(c);
        }
        for i in &indexes {
            q = q.bind(i);
        }
        if q.execute(&mut *tx).await?.rows_affected() != 1 {
            return Err(Error::Internal);
        }
    }
    Ok(rows.len() as u64)
}

/// Seals personal data still stored as plaintext (rows written before 0102)
/// and re-seals values under an old key version, 100 rows per table and call.
/// Run until every count is 0. Values never leave this process.
pub async fn encrypt_pii(pool: &PgPool, operator: &str) -> Result<Value> {
    owner(pool, operator).await?;
    let keys = KeyRing::from_env(crate::config::kms_enabled()?)?;
    if !keys.ready() {
        return Err(Error::PolicyGate("PAYOUT_ACCOUNT_KEY_MISSING"));
    }
    let mut counts = serde_json::Map::new();
    let mut total = 0;
    for t in TARGETS {
        let mut tx = pool.begin().await?;
        let n = seal_batch(&mut tx, &keys, t).await?;
        if n > 0 {
            operations::audit(
                &mut tx,
                None,
                None,
                None,
                "privacy.pii.sealed",
                &format!(
                    "OPERATOR:{} table={} rows={n} key_version={}",
                    operator.trim(),
                    t.name,
                    keys.active_version()
                ),
                Uuid::new_v4(),
            )
            .await?;
        }
        tx.commit().await?;
        counts.insert(t.name.to_owned(), json!(n));
        total += n;
    }
    Ok(
        json!({"sealed": counts, "total": total, "active_version": keys.active_version(), "batch_limit": 100}),
    )
}
