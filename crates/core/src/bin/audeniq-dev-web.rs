//! Loopback-only development gateway. Never included in the production image.
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Response, StatusCode},
    routing::any,
};
use std::{path::PathBuf, sync::Arc};
struct Dev {
    client: reqwest::Client,
    secret: String,
    root: PathBuf,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::var("APP_ENV").unwrap_or_default() == "development",
        "development only"
    );
    let state = Arc::new(Dev {
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()?,
        secret: std::env::var("EDGE_SERVICE_SECRET")?,
        root: std::env::current_dir()?.join("web/studio/dist"),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5173").await?;
    axum::serve(
        listener,
        Router::new().fallback(any(serve)).with_state(state),
    )
    .await?;
    Ok(())
}
async fn serve(
    State(s): State<Arc<Dev>>,
    req: Request,
) -> std::result::Result<Response<Body>, StatusCode> {
    if req.headers().get("host").and_then(|v| v.to_str().ok()) != Some("localhost:5173") {
        return Err(StatusCode::FORBIDDEN);
    }
    let path = req.uri().path().to_owned();
    if path.starts_with("/api/admin") {
        return Err(StatusCode::NOT_FOUND);
    }
    if path.starts_with("/api/") {
        let mut r = s
            .client
            .request(
                req.method().clone(),
                format!("http://127.0.0.1:8080{}", req.uri()),
            )
            .header("x-audeniq-service", &s.secret);
        for key in [
            "cookie",
            "origin",
            "content-type",
            "x-csrf-token",
            "sec-fetch-site",
        ] {
            if let Some(v) = req.headers().get(key) {
                r = r.header(key, v);
            }
        }
        let body = to_bytes(req.into_body(), 65536)
            .await
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        let result = r
            .body(body)
            .send()
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let mut out = Response::builder().status(result.status());
        for key in ["set-cookie", "content-type", "x-request-id"] {
            if let Some(v) = result.headers().get(key) {
                out = out.header(key, v);
            }
        }
        out = out.header("cache-control", "no-store");
        return out
            .body(Body::from(
                result.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?,
            ))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }
    if !matches!(req.method().as_str(), "GET" | "HEAD") {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }
    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if relative.split('/').any(|v| v == ".." || v.starts_with('.'))
        || relative.contains('%')
        || relative.contains('\\')
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let data = tokio::fs::read(s.root.join(relative))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mime = if relative.ends_with(".wasm") {
        "application/wasm"
    } else if relative.ends_with(".js") {
        "text/javascript"
    } else if relative.ends_with(".css") {
        "text/css"
    } else if relative.ends_with(".svg") {
        "image/svg+xml"
    } else if relative.ends_with(".html") {
        "text/html; charset=utf-8"
    } else {
        "text/plain"
    };
    Response::builder()
        .header("content-type", mime)
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-store")
        .body(if req.method() == "HEAD" {
            Body::empty()
        } else {
            Body::from(data)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
