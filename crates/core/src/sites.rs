//! 어느 화면(사이트)에서 온 요청인지에 따른 규칙 — 예전 web/admin/worker.js의 몫
//!
//! 화면 앞단(Cloudflare Worker, 나중에는 AWS CloudFront)이 `/api/*`를 이 API로 넘기면서
//! `x-audeniq-site`(studio|admin)와 `x-audeniq-service`를 붙인다. 브라우저가 같은 이름을 보내도
//! 앞단이 덮어쓰고, 서비스 비밀이 없으면 boundary가 거절한다.
//! 헤더가 없으면(개발·테스트·예전 경로) studio로 본다.
//!
//! - admin: 관리자 콘솔이 쓰는 경로만 연다. 쓰기는 관리자 주소(ADMIN_ORIGIN)에서 온 것만 받고,
//!   Origin을 APP_ORIGIN으로 바꿔 넘긴다(세션·CSRF 검사는 그대로). 점검 중에도 막지 않는다.
use crate::{api::AppState, site_content::fail};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::OnceLock;

pub const SITE_HEADER: &str = "x-audeniq-site";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Site {
    Studio,
    Admin,
}

pub fn site(h: &HeaderMap) -> Site {
    match h.get(SITE_HEADER).and_then(|v| v.to_str().ok()) {
        Some("admin") => Site::Admin,
        _ => Site::Studio,
    }
}

/// 환경변수의 정확한 HTTPS origin (예: https://admin.audeniq.com). 잘못된 값은 없는 것으로.
fn env_origin(name: &str) -> Option<String> {
    let raw = std::env::var(name).ok()?;
    let url = url::Url::parse(raw.trim()).ok()?;
    let origin = url.origin().ascii_serialization();
    (origin == raw.trim()
        && (url.scheme() == "https" || std::env::var("APP_ENV").as_deref() != Ok("production")))
    .then_some(origin)
}
pub fn admin_origin() -> Option<&'static str> {
    static V: OnceLock<Option<String>> = OnceLock::new();
    V.get_or_init(|| env_origin("ADMIN_ORIGIN")).as_deref()
}

/// 관리자 콘솔이 쓰는 경로 (공지·이벤트·점검 API 포함)
pub fn admin_path(method: &Method, path: &str) -> Result<(), StatusCode> {
    let content = path == "/api/status"
        || path.starts_with("/api/content/")
        || path == "/api/notices"
        || path.starts_with("/api/notices/")
        || path == "/api/events"
        || path.starts_with("/api/events/");
    if content {
        return Ok(());
    }
    let allowed = matches!(
        path,
        "/api/auth/login" | "/api/auth/logout" | "/api/auth/csrf" | "/api/me" | "/api/staff"
    ) || path.starts_with("/api/staff/");
    if !allowed {
        return Err(StatusCode::NOT_FOUND);
    }
    if matches!(*method, Method::GET | Method::HEAD | Method::POST) {
        Ok(())
    } else {
        Err(StatusCode::METHOD_NOT_ALLOWED)
    }
}

fn read(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// 쓰기 요청의 Origin이 그 사이트 주소면 APP_ORIGIN으로 바꾼다. 아니면 403.
fn adopt_origin(req: &mut Request, site_origin: Option<&str>, app_origin: &str) -> Result<(), ()> {
    if read(req.method()) {
        return Ok(());
    }
    let given = req.headers().get("origin").and_then(|v| v.to_str().ok());
    match (given, site_origin) {
        (Some(g), Some(o)) if g == o => {
            let v = HeaderValue::from_str(app_origin).map_err(|_| ())?;
            req.headers_mut().insert("origin", v);
            Ok(())
        }
        _ => Err(()),
    }
}

/// boundary(서비스 비밀·Origin 검사) 바깥에서 돈다: 사이트별로 경로를 고르고 Origin을 맞춘다.
pub async fn policy(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    match site(req.headers()) {
        Site::Studio => {}
        Site::Admin => {
            if let Err(status) = admin_path(req.method(), req.uri().path()) {
                let code = if status == StatusCode::NOT_FOUND {
                    "NOT_FOUND"
                } else {
                    "METHOD_NOT_ALLOWED"
                };
                return fail(status, code);
            }
            if adopt_origin(&mut req, admin_origin(), &s.config.origin).is_err() {
                return fail(StatusCode::FORBIDDEN, "FORBIDDEN");
            }
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_header_defaults_to_studio() {
        let mut h = HeaderMap::new();
        assert_eq!(site(&h), Site::Studio);
        h.insert(SITE_HEADER, HeaderValue::from_static("admin"));
        assert_eq!(site(&h), Site::Admin);
        h.insert(SITE_HEADER, HeaderValue::from_static("other"));
        assert_eq!(site(&h), Site::Studio);
    }

    #[test]
    fn admin_console_paths_match_the_old_worker() {
        for (m, p) in [
            (Method::POST, "/api/auth/login"),
            (Method::GET, "/api/me"),
            (Method::GET, "/api/staff/releases"),
            (Method::POST, "/api/staff/releases/x/decision"),
            (Method::PUT, "/api/content/notices/n-1"),
            (Method::DELETE, "/api/content/events/e-1"),
            (Method::GET, "/api/status"),
        ] {
            assert_eq!(admin_path(&m, p), Ok(()), "{m} {p}");
        }
        assert_eq!(
            admin_path(&Method::GET, "/api/orgs"),
            Err(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            admin_path(&Method::POST, "/api/auth/register"),
            Err(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            admin_path(&Method::GET, "/api/staffx"),
            Err(StatusCode::NOT_FOUND)
        );
        assert_eq!(
            admin_path(&Method::PUT, "/api/staff/releases/x/agreement-terms"),
            Err(StatusCode::METHOD_NOT_ALLOWED)
        );
    }

    #[test]
    fn writes_must_come_from_the_site_origin() {
        let app = "https://studio.audeniq.com";
        let admin = Some("https://admin.audeniq.com");
        let req = |method: Method, origin: Option<&str>| {
            let mut b = axum::http::Request::builder()
                .method(method)
                .uri("/api/staff/x");
            if let Some(o) = origin {
                b = b.header("origin", o);
            }
            b.body(axum::body::Body::empty()).unwrap()
        };
        let mut r = req(Method::POST, Some("https://admin.audeniq.com"));
        assert!(adopt_origin(&mut r, admin, app).is_ok());
        assert_eq!(r.headers()["origin"], app);
        // 같은 사이트(audeniq.com)의 스튜디오 화면이 관리자 주소로 쓰기를 보내도 막는다
        assert!(adopt_origin(&mut req(Method::POST, Some(app)), admin, app).is_err());
        assert!(adopt_origin(&mut req(Method::POST, None), admin, app).is_err());
        assert!(
            adopt_origin(
                &mut req(Method::POST, Some("https://admin.audeniq.com")),
                None,
                app
            )
            .is_err()
        );
        assert!(adopt_origin(&mut req(Method::GET, None), admin, app).is_ok());
    }
}
