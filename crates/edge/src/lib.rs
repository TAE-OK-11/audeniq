use worker::*;
pub mod content;
/// Rust/WASM edge BFF. The generated JS loader is toolchain glue, not application logic.
#[event(fetch)]
pub async fn main(mut request: Request, env: Env, _ctx: Context) -> Result<Response> {
    let origin = env.var("APP_ORIGIN")?.to_string();
    if request.url()?.origin().ascii_serialization() != origin {
        return Response::error("Forbidden host", 403);
    }
    if request.path().starts_with("/api/admin") {
        return Response::error("Not found", 404);
    }
    // Notices and events are served from D1 at the edge (no private API hop).
    let path = request.path();
    if let Some(r) = content::route(&request.method(), &path) {
        return content::handle(request, &env, r).await;
    }
    if path.starts_with("/api/content/") {
        return Response::error("Not found", 404);
    }
    if !request.path().starts_with("/api/") {
        if !matches!(request.method(), Method::Get | Method::Head) {
            return Response::error("Method not allowed", 405);
        }
        return serve_studio(request, &env, &origin).await;
    }
    if !matches!(request.method(), Method::Get | Method::Head)
        && request.headers().get("origin")?.as_deref() != Some(&origin)
    {
        return Response::error("Forbidden origin", 403);
    }
    let mut forwarded = Request::new_with_init(
        &format!(
            "http://audeniq-api.internal{}{}",
            request.path(),
            request
                .url()?
                .query()
                .map(|q| format!("?{q}"))
                .unwrap_or_default()
        ),
        RequestInit::new().with_method(request.method()),
    )?;
    // Allowlist, never propagate browser-provided service identity or user-id headers.
    for name in [
        "cookie",
        "origin",
        "content-type",
        "x-csrf-token",
        "sec-fetch-site",
    ] {
        if let Some(value) = request.headers().get(name)? {
            forwarded.headers_mut()?.set(name, &value)?;
        }
    }
    // End-user IP for per-source auth rate limits. CF-Connecting-IP is set by
    // Cloudflare itself (client copies are overwritten); X-Forwarded-For is
    // client-controlled and never used. A browser-sent x-audeniq-client-ip is
    // not in the allowlist above, so it cannot reach the API.
    if let Some(ip) = request.headers().get("cf-connecting-ip")? {
        forwarded.headers_mut()?.set("x-audeniq-client-ip", &ip)?;
    }
    forwarded.headers_mut()?.set(
        "x-audeniq-service",
        &env.secret("EDGE_SERVICE_SECRET")?.to_string(),
    )?;
    // JSON control-plane only. Never proxy original audio bytes.
    if !matches!(request.method(), Method::Get | Method::Head) {
        let length = request
            .headers()
            .get("content-length")?
            .and_then(|s| s.parse::<usize>().ok());
        if length.is_none_or(|n| n > 65536) {
            return Response::error("Length required or payload too large", 413);
        }
        let body = request.bytes().await?;
        if body.len() > 65536 {
            return Response::error("Payload too large", 413);
        }
        let headers = forwarded.headers().clone();
        forwarded = Request::new_with_init(
            forwarded.url()?.as_ref(),
            RequestInit::new()
                .with_method(request.method())
                .with_headers(headers)
                .with_body(Some(worker::wasm_bindgen::JsValue::from(
                    worker::js_sys::Uint8Array::from(body.as_slice()),
                ))),
        )?;
    }
    match env.service("PRIVATE_API")?.fetch_request(forwarded).await {
        Ok(mut response) => {
            response.headers_mut().set("Cache-Control", "no-store")?;
            Ok(response)
        }
        Err(_) => Response::error("Private API unavailable", 503),
    }
}

/// React Studio build (`bun run build:edge` → web/studio/edge-dist), served
/// at the site root. Only the SPA shell, its hashed assets and static files
/// are exposed.
const STUDIO_BASE: &str = "/";

enum StaticRoute {
    Asset(&'static str),
    Redirect,
    NotFound,
}

fn static_route(path: &str) -> StaticRoute {
    match path {
        "/" => StaticRoute::Asset("no-cache"),
        "/favicon.ico" | "/robots.txt" => StaticRoute::Asset("public, max-age=86400"),
        _ if path.starts_with("/assets/") => {
            StaticRoute::Asset("public, max-age=31536000, immutable")
        }
        _ if path.starts_with("/static/") => {
            StaticRoute::Asset("public, max-age=86400, stale-while-revalidate=604800")
        }
        // Hash routing keeps every screen at /, so any other path is an old
        // bookmark (e.g. /connected/, /studio) or a typo: send it to the shell.
        _ if path.contains('.') => StaticRoute::NotFound,
        _ => StaticRoute::Redirect,
    }
}

async fn serve_studio(request: Request, env: &Env, origin: &str) -> Result<Response> {
    let cache = match static_route(&request.path()) {
        StaticRoute::Asset(cache) => cache,
        StaticRoute::Redirect => {
            let mut to = Url::parse(origin)?;
            to.set_path(STUDIO_BASE);
            return Response::redirect_with_status(to, 308);
        }
        StaticRoute::NotFound => return Response::error("Not found", 404),
    };
    let mut response = env.assets("ASSETS")?.fetch_request(request).await?;
    let ok = response.status_code() == 200 || response.status_code() == 304;
    let headers = response.headers_mut();
    headers.set("X-Content-Type-Options", "nosniff")?;
    headers.set("Referrer-Policy", "no-referrer")?;
    headers.set("X-Frame-Options", "DENY")?;
    headers.set("X-Robots-Tag", "noindex, nofollow, noarchive")?;
    headers.set("Cross-Origin-Opener-Policy", "same-origin")?;
    headers.set(
        "Permissions-Policy",
        "camera=(), microphone=(), geolocation=(), payment=()",
    )?;
    headers.set(
        "Strict-Transport-Security",
        "max-age=31536000; includeSubDomains",
    )?;
    headers.set("Cache-Control", if ok { cache } else { "no-store" })?;
    headers.set(
        "Content-Security-Policy",
        include_str!("../../../config/studio-react-csp.txt").trim(),
    )?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_of(path: &str) -> Option<&'static str> {
        match static_route(path) {
            StaticRoute::Asset(c) => Some(c),
            _ => None,
        }
    }

    #[test]
    fn routes_only_the_react_build() {
        assert_eq!(cache_of("/"), Some("no-cache"));
        assert!(
            cache_of("/assets/index-abc.js")
                .unwrap()
                .contains("immutable")
        );
        assert!(cache_of("/static/AUDENIQ_Logo_Light.svg").is_some());
        assert!(cache_of("/favicon.ico").is_some());
        // old entry points go to the app shell
        assert!(matches!(static_route("/connected/"), StaticRoute::Redirect));
        assert!(matches!(static_route("/studio"), StaticRoute::Redirect));
        // anything else with a file extension is not exposed
        assert!(matches!(static_route("/index.html"), StaticRoute::NotFound));
        assert!(matches!(
            static_route("/connected/index.html"),
            StaticRoute::NotFound
        ));
        assert!(matches!(static_route("/404.html"), StaticRoute::NotFound));
    }

    #[test]
    fn react_csp_allows_the_app_and_direct_uploads_only() {
        let csp = include_str!("../../../config/studio-react-csp.txt");
        assert!(csp.contains("script-src 'self';"));
        assert!(!csp.contains("unsafe-eval"));
        assert!(csp.contains("https://*.r2.cloudflarestorage.com"));
        assert!(csp.contains("frame-ancestors 'none'"));
    }
}
