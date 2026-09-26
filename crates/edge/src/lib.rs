use worker::*;
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
    if !request.path().starts_with("/api/") {
        if !matches!(request.method(), Method::Get | Method::Head) {
            return Response::error("Method not allowed", 405);
        }
        let mut response = env.assets("ASSETS")?.fetch_request(request).await?;
        response
            .headers_mut()
            .set("X-Content-Type-Options", "nosniff")?;
        response
            .headers_mut()
            .set("Referrer-Policy", "no-referrer")?;
        response.headers_mut().set("X-Frame-Options", "DENY")?;
        response.headers_mut().set("Cache-Control", "no-cache")?;
        response.headers_mut().set(
            "Content-Security-Policy",
            include_str!("../../../config/studio-csp.txt").trim(),
        )?;
        return Ok(response);
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
