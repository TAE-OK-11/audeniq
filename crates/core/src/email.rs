//! 알림 메일: 스튜디오 알림(`portal.notifications`)을 받는 사람의 로그인 이메일로도 보낸다.
//!
//! - 알림이 생기면 트리거(0112)가 받는 사람마다 `portal.notification_emails`에 한 줄씩
//!   넣는다 (알림 대상 사용자, 또는 조직 전체 알림이면 활성 멤버 전원. 메일을 끈 사람 제외).
//! - API 프로세스가 대기열을 꺼내 SMTP(STARTTLS, 587)로 보낸다. 로그인 이메일은 봉인돼
//!   있어(0102) 키를 가진 API만 주소를 알 수 있다. 주소·본문은 보낼 때 읽는다.
//! - 발송 서비스는 `EMAIL_PROVIDERS` 순서대로 쓴다 (`oci,ses`: 더 저렴한 OCI로 먼저 보내고, 실패하면 SES로).
//!   클라우드 IAM 자격 증명이 아니라 발송 전용 SMTP 자격 증명만 쓴다.
//!   - `ses`: Amazon SES `email-smtp.<EMAIL_SES_REGION>.amazonaws.com`
//!   - `oci`: OCI Email Delivery `smtp.email.<EMAIL_OCI_REGION>.oci.oraclecloud.com`
//!   - `smtp`: 그 밖의 SMTP 서버 (`EMAIL_SMTP_HOST`)
//!   - `test`, `test-fail`: 개발·테스트용 (운영 `APP_ENV=production`에서는 거절)
//! - 실패하면 1분·5분·30분·2시간·6시간 뒤 다시 보내고, 6번 실패하면 `FAILED`.
//!   3일이 지난 알림은 보내지 않는다 (`SKIPPED`/`EMAIL_EXPIRED`). 처리 끝난 줄은 90일 뒤 지운다.
//! - 대기열·로그에는 주소·본문·발송 서비스 응답을 남기지 않는다 (고정 코드만).
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    payout_keys::KeyRing,
};
use axum::{Json, Router, extract::State, http::HeaderMap, routing::get};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{sync::Mutex, time::Duration};
use uuid::Uuid;

/// 한 번에 꺼내는 메일 수.
const BATCH: i64 = 20;
/// 이만큼 실패하면 더 보내지 않는다.
pub const MAX_ATTEMPTS: i32 = 6;
/// 이보다 오래된 알림은 메일로 보내지 않는다.
const EXPIRE_DAYS: i32 = 3;
/// 보내는 중인 줄을 다른 API 인스턴스가 다시 꺼내지 않게 미뤄 두는 시간.
const LEASE_MINUTES: i32 = 10;
/// 대기열이 비었을 때 다시 볼 간격.
const IDLE: Duration = Duration::from_secs(10);

/// n번째 실패 뒤 다시 보낼 때까지 기다리는 시간(초).
fn backoff_seconds(attempts: i32) -> i64 {
    match attempts {
        ..=1 => 60,
        2 => 300,
        3 => 1800,
        4 => 7200,
        _ => 21600,
    }
}

/// 보낸 메일 (`test` 발송 서비스). 테스트에서만 읽는다.
#[derive(Debug, Clone)]
pub struct SentEmail {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}
static TEST_OUTBOX: Mutex<Vec<SentEmail>> = Mutex::new(Vec::new());

/// `test` 발송 서비스가 이 주소로 보낸 메일.
pub fn test_sent_to(to: &str) -> Vec<SentEmail> {
    TEST_OUTBOX
        .lock()
        .map(|o| o.iter().filter(|m| m.to == to).cloned().collect())
        .unwrap_or_default()
}

enum Transport {
    Smtp(AsyncSmtpTransport<Tokio1Executor>),
    Test,
    TestFail,
}

struct Provider {
    name: &'static str,
    transport: Transport,
}

pub struct Mailer {
    providers: Vec<Provider>,
    from: Mailbox,
    /// 메일 속 바로 가기 링크의 기준 (스튜디오 주소).
    pub(crate) origin: String,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn required(key: &str) -> anyhow::Result<String> {
    env(key).ok_or_else(|| anyhow::anyhow!("EMAIL_CONFIG_INVALID: {key} is required"))
}

fn region(key: &str) -> anyhow::Result<String> {
    let r = required(key)?;
    anyhow::ensure!(
        r.len() <= 40
            && r.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "EMAIL_CONFIG_INVALID: {key} must be a region name like ap-seoul-1"
    );
    Ok(r)
}

fn smtp(host: &str, port: u16, user: String, password: String) -> anyhow::Result<Transport> {
    // STARTTLS is required (Tls::Required): SES and OCI both refuse plaintext
    // AUTH, and a downgraded connection must never carry the credential.
    let t = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
        .map_err(|_| anyhow::anyhow!("EMAIL_CONFIG_INVALID: bad SMTP host"))?
        .port(port)
        .credentials(Credentials::new(user, password))
        .timeout(Some(Duration::from_secs(30)))
        .build();
    Ok(Transport::Smtp(t))
}

impl Mailer {
    /// `EMAIL_PROVIDERS`가 비어 있으면 `None` (메일을 보내지 않고 대기열만 정리한다).
    pub fn from_env(origin: &str) -> anyhow::Result<Option<Mailer>> {
        let Some(list) = env("EMAIL_PROVIDERS") else {
            return Ok(None);
        };
        let production = std::env::var("APP_ENV").as_deref() == Ok("production");
        let mut providers = Vec::new();
        for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (name, transport) = match name {
                "ses" => {
                    let host = env("EMAIL_SES_HOST").map_or_else(
                        || {
                            region("EMAIL_SES_REGION")
                                .map(|r| format!("email-smtp.{r}.amazonaws.com"))
                        },
                        Ok,
                    )?;
                    (
                        "ses",
                        smtp(
                            &host,
                            587,
                            required("EMAIL_SES_USERNAME")?,
                            required("EMAIL_SES_PASSWORD")?,
                        )?,
                    )
                }
                "oci" => {
                    let host = env("EMAIL_OCI_HOST").map_or_else(
                        || {
                            region("EMAIL_OCI_REGION")
                                .map(|r| format!("smtp.email.{r}.oci.oraclecloud.com"))
                        },
                        Ok,
                    )?;
                    (
                        "oci",
                        smtp(
                            &host,
                            587,
                            required("EMAIL_OCI_USERNAME")?,
                            required("EMAIL_OCI_PASSWORD")?,
                        )?,
                    )
                }
                "smtp" => {
                    let port = env("EMAIL_SMTP_PORT")
                        .map_or(Ok(587), |p| p.parse::<u16>())
                        .map_err(|_| anyhow::anyhow!("EMAIL_CONFIG_INVALID: EMAIL_SMTP_PORT"))?;
                    (
                        "smtp",
                        smtp(
                            &required("EMAIL_SMTP_HOST")?,
                            port,
                            required("EMAIL_SMTP_USERNAME")?,
                            required("EMAIL_SMTP_PASSWORD")?,
                        )?,
                    )
                }
                "test" | "test-fail" => {
                    anyhow::ensure!(
                        !production,
                        "EMAIL_CONFIG_INVALID: test providers are not allowed in production"
                    );
                    if name == "test" {
                        ("test", Transport::Test)
                    } else {
                        ("test-fail", Transport::TestFail)
                    }
                }
                other => anyhow::bail!(
                    "EMAIL_CONFIG_INVALID: unknown provider {other:?} (ses, oci, smtp)"
                ),
            };
            anyhow::ensure!(
                !providers.iter().any(|p: &Provider| p.name == name),
                "EMAIL_CONFIG_INVALID: provider {name} listed twice"
            );
            providers.push(Provider { name, transport });
        }
        anyhow::ensure!(
            !providers.is_empty(),
            "EMAIL_CONFIG_INVALID: EMAIL_PROVIDERS lists no provider"
        );
        let from: Mailbox = required("EMAIL_FROM")?.parse().map_err(|_| {
            anyhow::anyhow!(
                "EMAIL_CONFIG_INVALID: EMAIL_FROM must look like AUDENIQ <no-reply@example.com>"
            )
        })?;
        Ok(Some(Mailer {
            providers,
            from,
            origin: origin.trim_end_matches('/').to_string(),
        }))
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers.iter().map(|p| p.name).collect()
    }

    /// 앞의 발송 서비스부터 차례로 보내 보고, 처음 성공한 서비스 이름을 돌려준다.
    pub(crate) async fn send(&self, id: Uuid, to: &str, mail: &Rendered) -> Result<&'static str> {
        let to: Mailbox = to
            .parse()
            .map_err(|_| Error::PolicyGate("EMAIL_ADDRESS_INVALID"))?;
        let domain = self.from.email.domain().to_string();
        for p in &self.providers {
            let message = Message::builder()
                .from(self.from.clone())
                .to(to.clone())
                .subject(mail.subject.clone())
                .message_id(Some(format!("<{id}@{domain}>")))
                .multipart(MultiPart::alternative_plain_html(
                    mail.text.clone(),
                    mail.html.clone(),
                ))
                .map_err(|_| Error::Internal)?;
            let sent = match &p.transport {
                Transport::Smtp(t) => t.send(message).await.map(|_| ()).map_err(|e| {
                    if e.is_permanent() {
                        "EMAIL_REJECTED"
                    } else {
                        "EMAIL_SEND_FAILED"
                    }
                }),
                Transport::Test => {
                    TEST_OUTBOX
                        .lock()
                        .map_err(|_| Error::Internal)?
                        .push(SentEmail {
                            to: to.email.to_string(),
                            subject: mail.subject.clone(),
                            text: mail.text.clone(),
                            html: mail.html.clone(),
                        });
                    Ok(())
                }
                Transport::TestFail => Err("EMAIL_SEND_FAILED"),
            };
            match sent {
                Ok(()) => return Ok(p.name),
                Err(code) => tracing::warn!(provider = p.name, code, "notification email not sent"),
            }
        }
        Err(Error::PolicyGate("EMAIL_SEND_FAILED"))
    }
}

/// 메일 한 통의 제목과 본문 (일반 텍스트 + HTML).
pub struct Rendered {
    pub subject: String,
    pub text: String,
    pub html: String,
}

pub(crate) fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// 알림 한 건을 메일로. 링크는 스튜디오 안 경로만(0039의 CHECK) 붙는다.
pub fn render(origin: &str, title: &str, detail: &str, link: &str) -> Rendered {
    let title = one_line(title);
    let detail = detail.trim();
    let path = if link.starts_with('/') {
        link
    } else {
        "/notifications"
    };
    let url = format!("{origin}{path}");
    let settings = format!("{origin}/notifications");
    let mut text = format!("{title}\n\n");
    if !detail.is_empty() {
        text.push_str(detail);
        text.push_str("\n\n");
    }
    text.push_str(&format!(
        "바로 가기: {url}\n\n--\nAUDENIQ 스튜디오 알림이에요. 이 주소로는 답장을 받지 않아요.\n알림 메일은 {settings} 에서 끌 수 있어요.\n"
    ));
    let detail_html = if detail.is_empty() {
        String::new()
    } else {
        format!(
            "<p style=\"margin:0 0 20px;line-height:1.6;color:#374151\">{}</p>",
            escape_html(detail).replace('\n', "<br>")
        )
    };
    let html = format!(
        "<!doctype html><html lang=\"ko\"><body style=\"margin:0;padding:24px;background:#f5f6f8;font-family:'Pretendard Variable',Pretendard,-apple-system,'Apple SD Gothic Neo','Malgun Gothic',sans-serif\">\
<div style=\"max-width:520px;margin:0 auto;background:#ffffff;border-radius:16px;padding:28px\">\
<p style=\"margin:0 0 6px;font-size:12px;font-weight:700;letter-spacing:.08em;color:#3b63f6\">AUDENIQ STUDIO</p>\
<h1 style=\"margin:0 0 14px;font-size:20px;line-height:1.4;color:#111827\">{title}</h1>{detail_html}\
<a href=\"{url}\" style=\"display:inline-block;padding:12px 20px;border-radius:10px;background:#3b63f6;color:#ffffff;text-decoration:none;font-weight:700\">스튜디오에서 보기</a>\
<p style=\"margin:24px 0 0;font-size:12px;line-height:1.6;color:#6b7280\">이 주소로는 답장을 받지 않아요. 알림 메일은 <a href=\"{settings}\" style=\"color:#6b7280\">알림 화면</a>에서 끌 수 있어요.</p>\
</div></body></html>",
        title = escape_html(&title),
        url = escape_html(&url),
        settings = escape_html(&settings),
    );
    Rendered {
        subject: format!("[AUDENIQ] {title}"),
        text,
        html,
    }
}

async fn finish(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    provider: Option<&str>,
    code: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE portal.notification_emails SET status=$2, provider=$3, last_error=$4,
            sent_at=CASE WHEN $2='SENT' THEN now() ELSE sent_at END
         WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .bind(provider)
    .bind(code)
    .execute(pool)
    .await?;
    Ok(())
}

/// 보낼 차례가 된 메일을 한 묶음 보낸다. 처리한 줄 수를 돌려준다.
pub async fn dispatch_once(pool: &PgPool, keys: &KeyRing, mailer: &Mailer) -> Result<usize> {
    let rows = sqlx::query(
        "UPDATE portal.notification_emails e
            SET attempts=e.attempts+1, next_attempt_at=now() + make_interval(mins => $2)
          WHERE e.id IN (SELECT id FROM portal.notification_emails
                          WHERE status='PENDING' AND next_attempt_at <= now()
                          ORDER BY next_attempt_at LIMIT $1 FOR UPDATE SKIP LOCKED)
          RETURNING e.id, e.user_id, e.attempts,
            (SELECT jsonb_build_object('title',n.title,'detail',n.detail,'link',n.link,'org_id',n.org_id,
                     'expired', n.created_at < now() - make_interval(days => $3))
               FROM portal.notifications n WHERE n.id=e.notification_id) AS n",
    )
    .bind(BATCH)
    .bind(LEASE_MINUTES)
    .bind(EXPIRE_DAYS)
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let id: Uuid = row.get("id");
        let user: Uuid = row.get("user_id");
        let attempts: i32 = row.get("attempts");
        let n: Option<Value> = row.get("n");
        let Some(n) = n else {
            finish(pool, id, "SKIPPED", None, Some("EMAIL_NOTICE_GONE")).await?;
            continue;
        };
        if n["expired"] == true {
            finish(pool, id, "SKIPPED", None, Some("EMAIL_EXPIRED")).await?;
            continue;
        }
        // 넣은 뒤 메일을 끄거나 멤버에서 빠졌으면 보내지 않는다.
        let org: Option<Uuid> = n["org_id"].as_str().and_then(|v| v.parse().ok());
        let still: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM identity.memberships m JOIN identity.users u ON u.id=m.user_id
                            LEFT JOIN portal.notification_settings s ON s.user_id=m.user_id
                            WHERE m.org_id=$1 AND m.user_id=$2 AND m.status='ACTIVE' AND u.status='ACTIVE'
                              AND COALESCE(s.email_enabled, true))",
        )
        .bind(org)
        .bind(user)
        .fetch_one(pool)
        .await?;
        if !still {
            finish(pool, id, "SKIPPED", None, Some("EMAIL_OPTED_OUT")).await?;
            continue;
        }
        let to = match crate::pii::user_email(keys, pool, user).await {
            Ok(to) => to,
            Err(_) => {
                finish(pool, id, "FAILED", None, Some("EMAIL_ADDRESS_UNAVAILABLE")).await?;
                continue;
            }
        };
        let mail = render(
            &mailer.origin,
            n["title"].as_str().unwrap_or(""),
            n["detail"].as_str().unwrap_or(""),
            n["link"].as_str().unwrap_or(""),
        );
        match mailer.send(id, &to, &mail).await {
            Ok(provider) => finish(pool, id, "SENT", Some(provider), None).await?,
            Err(Error::PolicyGate("EMAIL_ADDRESS_INVALID")) => {
                finish(pool, id, "FAILED", None, Some("EMAIL_ADDRESS_INVALID")).await?
            }
            Err(_) if attempts >= MAX_ATTEMPTS => {
                finish(pool, id, "FAILED", None, Some("EMAIL_SEND_FAILED")).await?
            }
            Err(_) => {
                sqlx::query(
                    "UPDATE portal.notification_emails SET last_error='EMAIL_SEND_FAILED',
                        next_attempt_at=now() + make_interval(secs => $2) WHERE id=$1",
                )
                .bind(id)
                .bind(backoff_seconds(attempts) as f64)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(rows.len())
}

/// 오래된 대기열 정리: 보낼 수 없게 된(3일 지난) 줄은 건너뛰고, 끝난 줄은 90일 뒤 지운다.
/// 발송 서비스가 없을 때도 돈다 (대기열이 끝없이 쌓이지 않게).
pub async fn sweep(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "UPDATE portal.notification_emails SET status='SKIPPED', last_error='EMAIL_EXPIRED'
          WHERE status='PENDING' AND created_at < now() - make_interval(days => $1)",
    )
    .bind(EXPIRE_DAYS)
    .execute(pool)
    .await?;
    sqlx::query(
        "DELETE FROM portal.notification_emails WHERE status<>'PENDING' AND created_at < now() - interval '90 days'",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// API 프로세스 안에서 계속 도는 발송기.
pub fn spawn(state: AppState, mailer: Option<Mailer>) {
    tokio::spawn(async move {
        let mut last_sweep: Option<tokio::time::Instant> = None;
        loop {
            if last_sweep.is_none_or(|t| t.elapsed() >= Duration::from_secs(3600)) {
                if let Err(error) = sweep(&state.pool).await {
                    tracing::warn!(%error, "notification email sweep failed");
                }
                if let Err(error) = crate::release_alerts::sweep(&state.pool).await {
                    tracing::warn!(%error, "release alert sweep failed");
                }
                if let Err(error) = crate::presave::sweep(&state.pool).await {
                    tracing::warn!(%error, "presave sweep failed");
                }
                last_sweep = Some(tokio::time::Instant::now());
            }
            // Pre-saves need no mail, only the platforms' settings.
            let mut busy = false;
            if !crate::presave::offered().is_empty() {
                match crate::presave::dispatch_once(&state.pool, &state.payout_keys).await {
                    Ok(n) => busy = n as i64 >= crate::presave::BATCH,
                    Err(error) => tracing::warn!(%error, "presave dispatch failed"),
                }
            }
            if let Some(m) = &mailer {
                match dispatch_once(&state.pool, &state.payout_keys, m).await {
                    Ok(n) => busy |= n as i64 >= BATCH,
                    Err(error) => tracing::warn!(%error, "notification email dispatch failed"),
                }
                match crate::release_alerts::dispatch_once(&state.pool, &state.payout_keys, m).await
                {
                    Ok(n) => busy |= n as i64 >= crate::release_alerts::BATCH,
                    Err(error) => tracing::warn!(%error, "release alert dispatch failed"),
                }
                match crate::splits::dispatch_once(&state.pool, &state.payout_keys, m).await {
                    Ok(n) => busy |= n as i64 >= crate::splits::BATCH,
                    Err(error) => tracing::warn!(%error, "split invite dispatch failed"),
                }
            }
            if !busy {
                tokio::time::sleep(if mailer.is_some() {
                    IDLE
                } else {
                    Duration::from_secs(3600)
                })
                .await;
            }
        }
    });
}

// ---------------------------------------------------------------------------
// 수신 설정 (알림 화면)
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsInput {
    pub email_enabled: bool,
}

pub async fn get_settings(s: &AppState, user: Uuid) -> Result<Value> {
    let enabled: Option<bool> = sqlx::query_scalar(
        "SELECT email_enabled FROM portal.notification_settings WHERE user_id=$1",
    )
    .bind(user)
    .fetch_optional(&s.pool)
    .await?;
    Ok(json!({"email_enabled": enabled.unwrap_or(true)}))
}

/// 끄면 아직 보내지 않은 메일도 보내지 않는다.
pub async fn put_settings(s: &AppState, user: Uuid, i: SettingsInput) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO portal.notification_settings(user_id,email_enabled) VALUES($1,$2)
         ON CONFLICT(user_id) DO UPDATE SET email_enabled=EXCLUDED.email_enabled, updated_at=now()",
    )
    .bind(user)
    .bind(i.email_enabled)
    .execute(&mut *tx)
    .await?;
    if !i.email_enabled {
        sqlx::query(
            "UPDATE portal.notification_emails SET status='SKIPPED', last_error='EMAIL_OPTED_OUT'
              WHERE user_id=$1 AND status='PENDING'",
        )
        .bind(user)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(json!({"email_enabled": i.email_enabled}))
}

async fn h_get(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(get_settings(&s, a.user).await?))
}
async fn h_put(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<SettingsInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(put_settings(&s, a.user, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/me/notification-settings", get(h_get).put(h_put))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_mail_escapes_and_links_into_the_studio() {
        let m = render(
            "https://studio.example.com",
            "발매 <승인>\r\n완료",
            "검토가 끝났어요 & 곧 송출돼요.",
            "/releases/1",
        );
        assert_eq!(m.subject, "[AUDENIQ] 발매 <승인>  완료");
        assert!(m.text.contains("https://studio.example.com/releases/1"));
        assert!(m.html.contains("발매 &lt;승인&gt;  완료"));
        assert!(m.html.contains("곧 송출돼요") && m.html.contains("&amp;"));
        assert!(!m.html.contains("<승인>"));
        // A notice without a link points at the notification list.
        let m = render("https://studio.example.com", "공지", "", "");
        assert!(m.text.contains("https://studio.example.com/notifications"));
    }

    #[test]
    fn backoff_grows_and_caps() {
        let s: Vec<i64> = (1..=7).map(backoff_seconds).collect();
        assert!(s.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(*s.last().unwrap(), 21600);
    }
}
