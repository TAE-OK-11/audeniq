use worker::*;
pub mod content;
/// Rust/WASM edge BFF. The generated JS loader is toolchain glue, not application logic.
#[event(fetch)]
pub async fn main(mut request: Request, env: Env, ctx: Context) -> Result<Response> {
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
        return content_cached(request, &env, &ctx, r, &origin, &path).await;
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
    // Partner webhooks (DSP ingestion ACKs) are server-to-server: no browser
    // Origin. The API authenticates them by the partner's HMAC signature.
    let partner_hook = is_partner_hook(&request.method(), &path);
    if !matches!(request.method(), Method::Get | Method::Head)
        && !partner_hook
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
    // accept-encoding: the API compresses JSON (zstd > br > gzip) for what the
    // browser accepts, so the tunnel hop carries compressed bytes and the
    // response passes through to the browser unchanged.
    for name in [
        "cookie",
        "origin",
        "content-type",
        "x-csrf-token",
        "sec-fetch-site",
        "accept-encoding",
    ] {
        if let Some(value) = request.headers().get(name)? {
            forwarded.headers_mut()?.set(name, &value)?;
        }
    }
    if partner_hook {
        // Signature/timestamp headers are partner-specific (x-signature,
        // x-hub-signature-256, ...): forward x-* except our own namespace.
        for (name, value) in request.headers().entries() {
            if forward_partner_header(&name) {
                forwarded.headers_mut()?.set(&name, &value)?;
            }
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

/// Public notice/event reads are answered from the colo's Cache API (the
/// response's own `max-age`), so repeat reads skip D1 and JSON encoding.
/// Admin writes purge this colo's copies; other colos expire within max-age.
async fn content_cached(
    request: Request,
    env: &Env,
    ctx: &Context,
    r: content::Route<'_>,
    origin: &str,
    path: &str,
) -> Result<Response> {
    let public_get = request.method() == Method::Get
        && matches!(r, content::Route::List(_) | content::Route::Get(..));
    let purge = match &r {
        content::Route::Create(t) => Some(vec![format!("{origin}/api/{t}")]),
        content::Route::Update(t, id) | content::Route::Delete(t, id) => Some(vec![
            format!("{origin}/api/{t}"),
            format!("{origin}/api/{t}/{id}"),
        ]),
        _ => None,
    };
    let key = format!("{origin}{path}");
    if public_get && let Ok(Some(hit)) = Cache::default().get(key.as_str(), false).await {
        return Ok(hit);
    }
    let mut response = content::handle(request, env, r).await?;
    if public_get && response.status_code() == 200 {
        let copy = response.cloned()?;
        ctx.wait_until(async move {
            let _ = Cache::default().put(key, copy).await;
        });
    }
    if let Some(keys) = purge
        && response.status_code() < 300
    {
        ctx.wait_until(async move {
            let cache = Cache::default();
            for k in keys {
                let _ = cache.delete(k, false).await;
            }
        });
    }
    Ok(response)
}

fn is_partner_hook(method: &Method, path: &str) -> bool {
    *method == Method::Post
        && path.starts_with("/api/partner-hooks/")
        && path.len() > "/api/partner-hooks/".len()
        && !path["/api/partner-hooks/".len()..].contains('/')
}

fn forward_partner_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("x-")
        && !n.starts_with("x-audeniq")
        && !n.starts_with("x-forwarded")
        && n != "x-real-ip"
        && n != "x-csrf-token"
}

/// React Studio build (`bun run build:edge` → web/studio/edge-dist), served
/// at the site root. Only the SPA shell, its hashed assets and static files
/// are exposed. Screens use real paths (/login, /releases/r1), so every
/// extensionless path gets the shell and the client router picks the screen.
const STUDIO_BASE: &str = "/";

enum StaticRoute {
    Asset(&'static str),
    /// Client-side route: answer with the index shell.
    Shell,
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
        // Old entry points from before the app moved to the site root.
        _ if is_under(path, "/studio") || is_under(path, "/connected") => StaticRoute::Redirect,
        // Unknown files are not exposed; everything else is an app screen.
        _ if path.contains('.') => StaticRoute::NotFound,
        _ => StaticRoute::Shell,
    }
}

fn is_under(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

async fn serve_studio(request: Request, env: &Env, origin: &str) -> Result<Response> {
    let route = static_route(&request.path());
    let (cache, shell) = match route {
        StaticRoute::Asset(cache) => (cache, false),
        StaticRoute::Shell => ("no-cache", true),
        StaticRoute::Redirect => {
            let mut to = Url::parse(origin)?;
            to.set_path(STUDIO_BASE);
            return Response::redirect_with_status(to, 308);
        }
        StaticRoute::NotFound => return Response::error("Not found", 404),
    };
    let assets = env.assets("ASSETS")?;
    let mut response = if shell {
        let mut index = Url::parse(origin)?;
        index.set_path(STUDIO_BASE);
        let mut init = RequestInit::new();
        init.with_method(request.method());
        assets.fetch(index.to_string(), Some(init)).await?
    } else {
        assets.fetch_request(request).await?
    };
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
        // app screens are real paths served by the shell
        for p in [
            "/login",
            "/signup",
            "/releases/r1",
            "/releases/r1/",
            "/settlement",
        ] {
            assert!(matches!(static_route(p), StaticRoute::Shell), "{p}");
        }
        // old entry points go to the site root
        assert!(matches!(static_route("/connected/"), StaticRoute::Redirect));
        assert!(matches!(static_route("/studio"), StaticRoute::Redirect));
        assert!(matches!(
            static_route("/studio/login"),
            StaticRoute::Redirect
        ));
        assert!(matches!(static_route("/studios"), StaticRoute::Shell));
        // anything else with a file extension is not exposed
        assert!(matches!(static_route("/index.html"), StaticRoute::NotFound));
        assert!(matches!(
            static_route("/connected/index.html"),
            StaticRoute::Redirect
        ));
        assert!(matches!(static_route("/404.html"), StaticRoute::NotFound));
    }

    #[test]
    fn partner_hooks_skip_origin_but_not_our_headers() {
        assert!(is_partner_hook(&Method::Post, "/api/partner-hooks/D-5"));
        assert!(!is_partner_hook(&Method::Get, "/api/partner-hooks/D-5"));
        assert!(!is_partner_hook(&Method::Post, "/api/partner-hooks/"));
        assert!(!is_partner_hook(&Method::Post, "/api/partner-hooks/D-5/x"));
        assert!(!is_partner_hook(&Method::Post, "/api/orgs"));
        assert!(forward_partner_header("X-Signature"));
        assert!(forward_partner_header("x-hub-signature-256"));
        assert!(!forward_partner_header("x-audeniq-service"));
        assert!(!forward_partner_header("X-Audeniq-Client-Ip"));
        assert!(!forward_partner_header("cookie"));
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
