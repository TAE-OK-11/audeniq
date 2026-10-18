//! Ways to connect besides the browser redirect (docs/MCP.md "Connecting").
//!
//! - Device authorization (RFC 8628): the client gets a link and a short
//!   code, the assistant shows both, the artist signs in to the Studio in a
//!   browser and approves on the usual consent screen.
//! - Password grant: only for agents an operator registered with
//!   `password_grant` (their credential form sends the e-mail and password
//!   straight to AUDENIQ, never through the model). They authenticate with a
//!   client secret, go through the login rate limits, and never get
//!   `payouts:request`.
//! - Connection tokens: every other agent uses a fixed bearer token the
//!   artist makes in the Studio (shown once, 1-90 days or no end date,
//!   revocable; password change and log out everywhere end it too).
use super::oauth::{
    self, BAD_GRANT, CLIENT_COLS, Client, ConsentTarget, TokenError, authenticate_client,
    create_grant, escape, issue_tokens, live_grant, oauth_error, page, page_consent, page_error,
    page_sign_in, requested_scopes, resource_ok, row_client, scopes_from_form, single_params,
    user_orgs,
};
use super::{SCOPE_PAYOUT, SCOPE_READ, SCOPES};
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    operations,
};
use axum::{
    Json,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// A device authorization must be approved within this window.
const DEVICE_SECONDS: i64 = 600;
/// Minimum seconds between polls of one device code.
const DEVICE_INTERVAL: i64 = 5;
/// User codes: 8 letters without vowels (no words, no 0/O or 1/I mix-ups).
const USER_CODE_ALPHABET: &[u8] = b"BCDFGHJKLMNPQRSTVWXZ";
/// Connection tokens live 30 days unless the artist picks up to 90.
const TOKEN_DEFAULT_DAYS: i64 = 30;
const TOKEN_MAX_DAYS: i64 = 90;
/// The built-in client that owns connection tokens (migration 0132).
pub const TOKEN_CLIENT: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_c0de);

fn no_store(mut r: Response) -> Response {
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

// ---------------------------------------------------------------------------
// Device authorization (RFC 8628)
// ---------------------------------------------------------------------------

fn new_user_code() -> String {
    let mut out = String::with_capacity(8);
    while out.len() < 8 {
        let bytes = hex::decode(auth::random_token()).expect("hex token");
        // Rejection sampling keeps every letter equally likely.
        for b in bytes {
            if out.len() < 8 && b < 240 {
                out.push(USER_CODE_ALPHABET[(b % 20) as usize] as char);
            }
        }
    }
    out
}

/// "bcdf-ghjk" / "BCDF GHJK" → "BCDFGHJK"; anything else is not a code.
pub fn normalize_user_code(raw: &str) -> Option<String> {
    let code: String = raw
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (code.len() == 8 && code.bytes().all(|b| USER_CODE_ALPHABET.contains(&b))).then_some(code)
}

fn display_code(code: &str) -> String {
    format!("{}-{}", &code[..4], &code[4..])
}

pub async fn device_authorization(
    State(s): State<AppState>,
    h: HeaderMap,
    body: Bytes,
) -> Response {
    if !s.mcp.enabled {
        return Error::NotFound.into_response();
    }
    let Some(form) = std::str::from_utf8(&body).ok().and_then(single_params) else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Malformed form body.",
        );
    };
    let source = auth::client_ip_key(&h).unwrap_or_else(|| "unknown".into());
    if let Err(e) = auth::rate(&s.pool, &format!("mcp-device:src:{source}"), 60).await {
        return e.into_response();
    }
    let c = match authenticate_client(&s, &h, &form).await {
        Ok(c) => c,
        Err(r) => return *r,
    };
    if !resource_ok(&s, form.get("resource").map(String::as_str)) {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_target",
            "Unknown resource.",
        );
    }
    let Some(mut scopes) = requested_scopes(form.get("scope").map(String::as_str)) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_scope", "Unknown scope.");
    };
    if s.mcp.payout_limit_krw == 0 {
        scopes.retain(|v| *v != SCOPE_PAYOUT);
    }
    let device_code = auth::random_token();
    let user_code = new_user_code();
    let stored = async {
        let mut tx = s.pool.begin().await?;
        sqlx::query("DELETE FROM identity.mcp_device_codes WHERE expires_at < now() - interval '1 day'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO identity.mcp_device_codes(device_code_hash,user_code_hash,client_pk,scopes,expires_at) VALUES($1,$2,$3,$4,now()+make_interval(secs=>$5))")
            .bind(auth::hash_token(&device_code)).bind(auth::hash_token(&user_code)).bind(c.pk)
            .bind(&scopes).bind(DEVICE_SECONDS as f64)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok::<_, Error>(())
    }
    .await;
    if let Err(e) = stored {
        return e.into_response();
    }
    let uri = format!("{}/api/oauth/device", super::issuer(&s));
    let shown = display_code(&user_code);
    no_store(
        Json(json!({
            "device_code": device_code,
            "user_code": shown,
            "verification_uri": uri,
            "verification_uri_complete": format!("{uri}?user_code={shown}"),
            "expires_in": DEVICE_SECONDS,
            "interval": DEVICE_INTERVAL,
        }))
        .into_response(),
    )
}

fn page_code_entry(message: Option<&str>) -> Response {
    let note = message
        .map(|m| format!("<div class=\"card warn\">{}</div>", escape(m)))
        .unwrap_or_default();
    page(
        "어데닉에 AI 앱 연결",
        &format!(
            "<h1>AI 앱에 보이는 코드를 입력해 주세요</h1>{note}\
             <form method=\"get\" action=\"/api/oauth/device\"><div class=\"card\">\
             <input name=\"user_code\" autocomplete=\"off\" autocapitalize=\"characters\" placeholder=\"BCDF-GHJK\" required style=\"width:100%;box-sizing:border-box;padding:12px;font:inherit;font-size:20px;letter-spacing:2px;border-radius:10px;border:1px solid #ccc\">\
             </div><div class=\"row\"><button class=\"primary\">다음</button></div></form>\
             <p class=\"muted\">코드는 AI 앱이 대화에서 알려 줘요. 내가 시작하지 않은 연결이라면 입력하지 마세요.</p>"
        ),
        None,
    )
}

fn page_done(title: &str, message: &str) -> Response {
    page(
        title,
        &format!(
            "<h1>{}</h1><div class=\"card\">{}</div>",
            escape(title),
            escape(message)
        ),
        None,
    )
}

async fn client_by_pk(s: &AppState, pk: Uuid) -> Result<Option<Client>> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {CLIENT_COLS} FROM identity.mcp_clients WHERE id=$1 AND disabled_at IS NULL AND kind<>'TOKEN'"
    )))
    .bind(pk)
    .fetch_optional(&s.pool)
    .await?;
    Ok(row.as_ref().map(row_client))
}

/// The page the assistant's link opens: sign in, check the code, approve.
pub async fn device_page(
    State(s): State<AppState>,
    h: HeaderMap,
    RawQuery(q): RawQuery,
) -> Response {
    if !s.mcp.enabled {
        return Error::NotFound.into_response();
    }
    let Some(p) = single_params(q.as_deref().unwrap_or("")) else {
        return page_code_entry(None);
    };
    let a = match auth::actor(&s.pool, &h, &s.config, false).await {
        Ok(a) => a,
        Err(Error::Unauthorized) => return page_sign_in(&s),
        Err(e) => return e.into_response(),
    };
    let Some(raw) = p.get("user_code") else {
        return page_code_entry(None);
    };
    if auth::rate(&s.pool, &format!("mcp-device-lookup:{}", a.user), 20)
        .await
        .is_err()
    {
        return page_error("코드를 너무 많이 입력했어요. 15분 뒤에 다시 해 주세요.");
    }
    let Some(code) = normalize_user_code(raw) else {
        return page_code_entry(Some("코드 형식이 맞지 않아요. 영문 8자리를 확인해 주세요."));
    };
    let row = match sqlx::query(
        "SELECT device_code_hash, client_pk, scopes FROM identity.mcp_device_codes
          WHERE user_code_hash=$1 AND expires_at > now() AND grant_id IS NULL AND denied_at IS NULL AND used_at IS NULL",
    )
    .bind(auth::hash_token(&code))
    .fetch_optional(&s.pool)
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => {
            return page_code_entry(Some(
                "코드가 맞지 않거나 만료됐어요. AI 앱에 보이는 코드를 다시 확인해 주세요.",
            ));
        }
        Err(e) => return Error::from(e).into_response(),
    };
    let client = match client_by_pk(&s, row.get("client_pk")).await {
        Ok(Some(c)) => c,
        Ok(None) => return page_error("등록되지 않았거나 허용되지 않은 AI 앱이에요."),
        Err(e) => return e.into_response(),
    };
    let stored: Vec<String> = row.get("scopes");
    let scopes: Vec<&str> = SCOPES
        .iter()
        .copied()
        .filter(|k| stored.iter().any(|v| v == k))
        .collect();
    let orgs = match user_orgs(&s, a.user).await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let request = super::seal(
        &s,
        "device-consent",
        &json!({
            "d": hex::encode(row.get::<Vec<u8>, _>("device_code_hash")),
            "c": client.pk, "sc": scopes, "sh": hex::encode(&a.session_hash),
            "exp": chrono::Utc::now().timestamp() + DEVICE_SECONDS,
        }),
    );
    page_consent(
        &client,
        &ConsentTarget {
            action: "/api/oauth/device",
            note: format!(
                "연결 코드: <code>{}</code><br>AI 앱에 보이는 코드와 같을 때만 승인해 주세요.",
                display_code(&code)
            ),
            form_origin: None,
        },
        &scopes,
        &orgs,
        &request,
        s.mcp.payout_limit_krw,
    )
}

pub async fn device_decide(State(s): State<AppState>, h: HeaderMap, body: Bytes) -> Response {
    if !s.mcp.enabled {
        return Error::NotFound.into_response();
    }
    match device_decide_inner(&s, &h, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn device_decide_inner(s: &AppState, h: &HeaderMap, body: &[u8]) -> Result<Response> {
    auth::origin(h, &s.config)?;
    let a = auth::actor(&s.pool, h, &s.config, false).await?;
    let form = single_params(std::str::from_utf8(body).map_err(|_| Error::Invalid)?)
        .ok_or(Error::Invalid)?;
    let req = super::open(
        s,
        "device-consent",
        form.get("request").map(String::as_str).unwrap_or(""),
    )
    .ok_or(Error::InvalidCode("CONSENT_EXPIRED"))?;
    if req["sh"].as_str() != Some(hex::encode(&a.session_hash).as_str()) {
        return Err(Error::Forbidden);
    }
    let device = req["d"]
        .as_str()
        .and_then(|v| hex::decode(v).ok())
        .ok_or(Error::Invalid)?;
    let client_pk: Uuid = req["c"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Invalid)?;
    let mut tx = s.pool.begin().await?;
    let pending: Option<bool> = sqlx::query_scalar(
        "SELECT true FROM identity.mcp_device_codes WHERE device_code_hash=$1 AND client_pk=$2
           AND expires_at > now() AND grant_id IS NULL AND denied_at IS NULL AND used_at IS NULL FOR UPDATE",
    )
    .bind(&device)
    .bind(client_pk)
    .fetch_optional(&mut *tx)
    .await?;
    if pending.is_none() {
        return Ok(page_error(
            "연결 코드가 만료됐거나 이미 처리됐어요. AI 앱에서 연결을 다시 시작해 주세요.",
        ));
    }
    if form.get("decision").map(String::as_str) != Some("approve") {
        sqlx::query(
            "UPDATE identity.mcp_device_codes SET denied_at=now() WHERE device_code_hash=$1",
        )
        .bind(&device)
        .execute(&mut *tx)
        .await?;
        operations::audit(
            &mut tx,
            Some(a.user),
            None,
            Some(client_pk),
            "mcp.consent_denied",
            "USER_DENIED",
            a.request,
        )
        .await?;
        tx.commit().await?;
        return Ok(page_done(
            "연결을 거부했어요",
            "AI 앱은 이 계정에 접근할 수 없어요. 이 창을 닫아도 돼요.",
        ));
    }
    let org: Uuid = form
        .get("org")
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Invalid)?;
    let offered: Vec<String> =
        serde_json::from_value(req["sc"].clone()).map_err(|_| Error::Invalid)?;
    let scopes = scopes_from_form(&form, &offered);
    auth::membership(&mut tx, &a, org, false).await?;
    let grant = create_grant(
        &mut tx, s, client_pk, a.user, org, &scopes, "DEVICE", None, true, a.request,
    )
    .await?;
    sqlx::query("UPDATE identity.mcp_device_codes SET grant_id=$2 WHERE device_code_hash=$1")
        .bind(&device)
        .bind(grant)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(page_done(
        "연결했어요",
        "AI 앱이 어데닉 계정에 연결됐어요. AI 앱으로 돌아가 대화를 이어 가 주세요.",
    ))
}

/// The client's poll (RFC 8628 §3.4/3.5).
pub(super) async fn exchange_device_code(
    s: &AppState,
    client: &Client,
    form: &BTreeMap<String, String>,
) -> std::result::Result<Value, TokenError> {
    let code = form.get("device_code").map(String::as_str).unwrap_or("");
    if code.len() != 64 {
        return Err(TokenError::Oauth(
            "invalid_request",
            "device_code is required.",
        ));
    }
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query(
        "SELECT client_pk, grant_id, expires_at > now() AS live, denied_at IS NOT NULL AS denied, used_at IS NOT NULL AS used,
                COALESCE(polled_at > now() - make_interval(secs => $2), false) AS fast
           FROM identity.mcp_device_codes WHERE device_code_hash=$1 FOR UPDATE",
    )
    .bind(auth::hash_token(code))
    .bind((DEVICE_INTERVAL - 1) as f64)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(BAD_GRANT)?;
    if row.get::<Uuid, _>("client_pk") != client.pk || row.get::<bool, _>("used") {
        return Err(BAD_GRANT);
    }
    if !row.get::<bool, _>("live") {
        return Err(TokenError::Oauth(
            "expired_token",
            "The device code expired.",
        ));
    }
    if row.get::<bool, _>("denied") {
        return Err(TokenError::Oauth(
            "access_denied",
            "The user denied access.",
        ));
    }
    let Some(grant) = row.get::<Option<Uuid>, _>("grant_id") else {
        sqlx::query(
            "UPDATE identity.mcp_device_codes SET polled_at=now() WHERE device_code_hash=$1",
        )
        .bind(auth::hash_token(code))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Err(if row.get::<bool, _>("fast") {
            TokenError::Oauth("slow_down", "Poll less often.")
        } else {
            TokenError::Oauth("authorization_pending", "The user has not approved yet.")
        });
    };
    sqlx::query("UPDATE identity.mcp_device_codes SET used_at=now() WHERE device_code_hash=$1")
        .bind(auth::hash_token(code))
        .execute(&mut *tx)
        .await?;
    let scopes = live_grant(&mut tx, grant).await?;
    let v = issue_tokens(&mut tx, grant, &scopes).await?;
    tx.commit().await?;
    Ok(v)
}

// ---------------------------------------------------------------------------
// Password grant (operator-registered agents only)
// ---------------------------------------------------------------------------

pub(super) async fn password_grant(
    s: &AppState,
    h: &HeaderMap,
    client: &Client,
    form: &BTreeMap<String, String>,
) -> std::result::Result<Value, TokenError> {
    if !client.password_grant {
        return Err(TokenError::Client(
            "unauthorized_client",
            "This client may not use the password grant.",
        ));
    }
    let Some(scopes) = requested_scopes(form.get("scope").map(String::as_str)) else {
        return Err(TokenError::Oauth("invalid_scope", "Unknown scope."));
    };
    if scopes.contains(&SCOPE_PAYOUT) {
        return Err(TokenError::Oauth(
            "invalid_scope",
            "payouts:request is approved in the browser or with a Studio connection token.",
        ));
    }
    let org_wanted: Option<Uuid> = match form.get("org_id") {
        Some(v) => Some(
            v.parse()
                .map_err(|_| TokenError::Oauth("invalid_request", "org_id must be a UUID."))?,
        ),
        None => None,
    };
    let creds = auth::Credentials {
        email: form.get("username").cloned().unwrap_or_default(),
        password: form.get("password").cloned().unwrap_or_default(),
        terms_version: None,
        privacy_notice_version: None,
    };
    let bad_login = TokenError::Oauth("invalid_grant", "The e-mail or password is incorrect.");
    let (user, hash) = match auth::check_password(s, h, creds, "mcp-password").await {
        Ok(v) => v,
        Err(Error::Unauthorized | Error::Invalid) => return Err(bad_login),
        Err(e) => return Err(TokenError::Api(e)),
    };
    let mut tx = s.pool.begin().await?;
    if auth::current_password(&mut tx, user, &hash).await.is_err() {
        return Err(bad_login);
    }
    let org: Uuid = sqlx::query_scalar(
        "SELECT m.org_id FROM identity.memberships m JOIN identity.orgs o ON o.id=m.org_id
          WHERE m.user_id=$1 AND m.status='ACTIVE' AND ($2::uuid IS NULL OR m.org_id=$2)
          ORDER BY (m.role='VIEWER'), o.created_at, o.id LIMIT 1 FOR SHARE OF m",
    )
    .bind(user)
    .bind(org_wanted)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(TokenError::Oauth(
        "invalid_request",
        "The account has no such workspace.",
    ))?;
    let scopes: Vec<String> = scopes.iter().map(|v| v.to_string()).collect();
    let grant = create_grant(
        &mut tx,
        s,
        client.pk,
        user,
        org,
        &scopes,
        "PASSWORD",
        None,
        true,
        auth::request_id(h),
    )
    .await?;
    let v = issue_tokens(&mut tx, grant, &scopes).await?;
    tx.commit().await?;
    Ok(v)
}

// ---------------------------------------------------------------------------
// Connection tokens (made in the Studio)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenInput {
    pub label: String,
    pub org_id: Uuid,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub days: Option<i64>,
    /// No end date: the token works until the artist revokes it (or
    /// changes the password / logs out everywhere). `days` must be absent.
    #[serde(default)]
    pub no_expiry: bool,
}

/// A fixed bearer token for an agent that cannot use OAuth. The value is
/// returned once; only its digest is stored.
pub async fn create_connection_token(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<TokenInput>,
) -> Result<Json<Value>> {
    if !s.mcp.enabled {
        return Err(Error::NotFound);
    }
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("mcp-connection-token:{}", a.user), 20).await?;
    let label = i.label.trim();
    if label.is_empty() || label.chars().count() > 60 || label.chars().any(char::is_control) {
        return Err(Error::InvalidCode("LABEL_INVALID"));
    }
    let days = match (i.no_expiry, i.days) {
        (true, Some(_)) => return Err(Error::InvalidCode("DAYS_INVALID")),
        (true, None) => None,
        (false, d) => Some(d.unwrap_or(TOKEN_DEFAULT_DAYS)),
    };
    if days.is_some_and(|d| !(1..=TOKEN_MAX_DAYS).contains(&d)) {
        return Err(Error::InvalidCode("DAYS_INVALID"));
    }
    let mut scopes = vec![SCOPE_READ.to_string()];
    for v in &i.scopes {
        if !SCOPES.contains(&v.as_str()) {
            return Err(Error::InvalidCode("SCOPE_INVALID"));
        }
        if v == SCOPE_PAYOUT && s.mcp.payout_limit_krw == 0 {
            return Err(Error::InvalidCode("SCOPE_INVALID"));
        }
        if !scopes.contains(v) {
            scopes.push(v.clone());
        }
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, &a, i.org_id, false).await?;
    let grant = create_grant(
        &mut tx,
        &s,
        TOKEN_CLIENT,
        a.user,
        i.org_id,
        &scopes,
        "TOKEN",
        Some(label),
        false,
        a.request,
    )
    .await?;
    let token = auth::random_token();
    let expires_at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "INSERT INTO identity.mcp_tokens(token_hash,grant_id,kind,expires_at) VALUES($1,$2,'ACCESS',now()+make_interval(days=>$3)) RETURNING expires_at",
    )
    .bind(auth::hash_token(&token))
    .bind(grant)
    .bind(days.map(|d| d as i32))
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({
        "id": grant,
        "token": token,
        "expires_at": expires_at,
        "no_expiry": expires_at.is_none(),
        "scopes": scopes,
        "mcp_url": super::resource(&s),
    })))
}

// ---------------------------------------------------------------------------
// Operator commands (audeniq-admin mcp-client)
// ---------------------------------------------------------------------------

/// Registers a confidential agent. Returns `(client_id, client_secret)`;
/// the secret is shown once.
pub async fn admin_add_client(
    pool: &PgPool,
    operator: &str,
    name: &str,
    password_grant: bool,
    secret_post: bool,
    redirect_uris: &[String],
) -> anyhow::Result<(String, String)> {
    let operator = operator.trim();
    let name = name.trim();
    anyhow::ensure!(!operator.is_empty(), "--operator NAME is required");
    anyhow::ensure!(
        (1..=200).contains(&name.chars().count()),
        "name must be 1-200 characters"
    );
    for r in redirect_uris {
        anyhow::ensure!(oauth::valid_redirect_uri(r), "invalid redirect URI {r}");
    }
    let redirects = if redirect_uris.is_empty() {
        vec![oauth::NO_REDIRECT.to_string()]
    } else {
        redirect_uris.to_vec()
    };
    let client_id = format!("audeniq-agent-{}", &auth::random_token()[..24]);
    let secret = auth::random_token();
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO identity.mcp_clients(id,client_id,kind,client_name,redirect_uris,token_endpoint_auth_method,secret_hash,password_grant) VALUES($1,$2,'ADMIN',$3,$4,$5,$6,$7)")
        .bind(id).bind(&client_id).bind(name).bind(&redirects)
        .bind(if secret_post { "client_secret_post" } else { "client_secret_basic" })
        .bind(auth::hash_token(&secret)).bind(password_grant)
        .execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        None,
        None,
        Some(id),
        "mcp.client_registered",
        &format!(
            "OPERATOR:{operator}{}",
            if password_grant {
                " PASSWORD_GRANT"
            } else {
                ""
            }
        ),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok((client_id, secret))
}

pub async fn admin_list_clients(pool: &PgPool) -> anyhow::Result<Value> {
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('client_id',client_id,'name',client_name,'password_grant',password_grant,
                'auth',token_endpoint_auth_method,'redirect_uris',redirect_uris,'created_at',created_at,'disabled_at',disabled_at)
           FROM identity.mcp_clients WHERE kind='ADMIN' ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(json!({"items": items}))
}

/// Disables an operator-registered agent and ends all of its connections.
pub async fn admin_disable_client(
    pool: &PgPool,
    operator: &str,
    client_id: &str,
) -> anyhow::Result<()> {
    let operator = operator.trim();
    anyhow::ensure!(!operator.is_empty(), "--operator NAME is required");
    let mut tx = pool.begin().await?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE identity.mcp_clients SET disabled_at=COALESCE(disabled_at,now()) WHERE client_id=$1 AND kind='ADMIN' RETURNING id",
    )
    .bind(client_id)
    .fetch_optional(&mut *tx)
    .await?;
    let id = id.ok_or_else(|| anyhow::anyhow!("no such agent"))?;
    sqlx::query("DELETE FROM identity.mcp_tokens WHERE grant_id IN (SELECT id FROM identity.mcp_grants WHERE client_pk=$1 AND revoked_at IS NULL)")
        .bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE identity.mcp_grants SET revoked_at=now(), revoke_reason='CLIENT_DISABLED' WHERE client_pk=$1 AND revoked_at IS NULL")
        .bind(id).execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        None,
        None,
        Some(id),
        "mcp.client_disabled",
        &format!("OPERATOR:{operator}"),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_codes_are_eight_unambiguous_letters() {
        for _ in 0..200 {
            let c = new_user_code();
            assert_eq!(normalize_user_code(&c).as_deref(), Some(c.as_str()));
        }
        assert_eq!(
            normalize_user_code("bcdf-ghjk").as_deref(),
            Some("BCDFGHJK")
        );
        assert_eq!(
            normalize_user_code("BCDF GHJK").as_deref(),
            Some("BCDFGHJK")
        );
        assert!(normalize_user_code("BCDF-GHJ").is_none());
        assert!(normalize_user_code("ABCD-EFGH").is_none());
        assert!(normalize_user_code("BCDF-GHJ0").is_none());
        assert_eq!(display_code("BCDFGHJK"), "BCDF-GHJK");
    }
    #[test]
    fn token_client_matches_migration() {
        assert_eq!(
            TOKEN_CLIENT.to_string(),
            "00000000-0000-4000-8000-00000000c0de"
        );
    }
}
