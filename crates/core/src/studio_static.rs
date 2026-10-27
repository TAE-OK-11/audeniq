//! Studio 화면(정적 파일) 서빙 — 예전 Cloudflare Workers Static Assets의 몫
//!
//! 앞단(Cloudflare Worker, 나중에는 AWS CloudFront)은 헤더만 다듬어 모든 요청을 이 서버로 넘긴다.
//! `/api/*`·`/ready`·`/health`·OAuth 발견 문서는 지금처럼 API 라우터(boundary 안)로 가고,
//! 그 밖의 경로는 여기서 답한다. 화면 파일은 누구에게나 공개라 boundary(서비스 비밀) **바깥**에서 돈다.
//!
//! - 파일: `STUDIO_DIR`(배포: `/srv/studio/current` → `releases/<커밋>` 심링크, deploy/studio-release.sh).
//!   폴더를 통째로 메모리에 읽고, 심링크가 다른 릴리스를 가리키면 다음 요청에서 새로 읽는다(api 재시작 없음).
//! - 규칙: 빌드 결과의 `_headers`(보안 헤더·캐시)와 `_redirects`를 그대로 따른다. Workers·S3 배포와 같은 원본.
//!   `_headers`에 캐시 규칙이 없으면 index.html은 no-cache, `/assets/*`는 1년 immutable.
//! - SPA 폴백: 확장자 없는 경로는 `/index.html`(no-cache). 확장자 있는 없는 파일은 404
//!   (예전 배포의 스크립트가 HTML로 "로드"되지 않게), `/.well-known/*`의 없는 문서는 JSON 404.
//! - 압축: 빌드가 만든 `.br` 사이드카(web/shared/compress-assets.ts)를 Accept-Encoding에 맞춰 보낸다.
//!
//! 앞단 함수는 헤더만 다룬다(CloudFront Function으로 옮길 수 있게): 폴백·리다이렉트·캐시 규칙은 모두 여기.
use crate::{api::AppState, auth::CLIENT_IP_HEADER, site_content::fail, sites};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tracing::Instrument;

/// API 라우터(boundary 안)로 가는 경로. 나머지는 화면 파일.
pub fn api_path(path: &str) -> bool {
    path == "/api"
        || path.starts_with("/api/")
        || path == "/ready"
        || path == "/health"
        // AI 어시스턴트 OAuth 발견 문서 (crate::mcp, RFC 8414·9728)
        || path.starts_with("/.well-known/oauth-")
}

/// API 라우터에 화면 서빙과 요청 로그의 사용자 IP를 더한다.
/// 둘 다 API 라우터의 모든 층(boundary 포함) 바깥에 둔다.
pub fn app(s: AppState, studio: Option<Arc<Studio>>) -> Router {
    let api = crate::api::router(s.clone());
    let api = match studio {
        Some(studio) => api.layer(middleware::from_fn_with_state(
            (s.clone(), studio),
            static_first,
        )),
        None => api,
    };
    api.layer(middleware::from_fn_with_state(s, edge_span))
}

/// 앞단이 넣은 사용자 IP. 서비스 비밀이 맞을 때만 믿는다(브라우저가 직접 보낸 값은 버림).
fn client_ip(s: &AppState, h: &HeaderMap) -> Option<String> {
    if !s.edge_authenticated(h) {
        return None;
    }
    let ip = h.get(CLIENT_IP_HEADER)?.to_str().ok()?.trim();
    ip.parse::<std::net::IpAddr>().ok().map(|ip| ip.to_string())
}

/// 이 요청의 모든 로그 줄(API의 http_request 포함)에 `client_ip`를 남긴다.
async fn edge_span(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let span = tracing::info_span!("edge", client_ip = tracing::field::Empty);
    if let Some(ip) = client_ip(&s, req.headers()) {
        span.record("client_ip", tracing::field::display(ip));
    }
    next.run(req).instrument(span).await
}

async fn static_first(
    State((s, studio)): State<(AppState, Arc<Studio>)>,
    req: Request,
    next: Next,
) -> Response {
    // 관리자 화면은 자기 앞단이 서빙한다: 여기로 오는 건 /api뿐
    if api_path(req.uri().path()) || sites::site(req.headers()) != sites::Site::Studio {
        return next.run(req).await;
    }
    let path = req.uri().path().to_owned();
    let ip = client_ip(&s, req.headers());
    let response = studio.respond(req).await;
    let status = response.status().as_u16();
    match ip {
        Some(ip) => tracing::info!(status, path, client_ip = %ip, "static_request"),
        None => tracing::info!(status, path, "static_request"),
    }
    response
}

/// 화면 파일 하나 (원본 + 있으면 brotli 사이드카)
struct Asset {
    body: Bytes,
    br: Option<Bytes>,
    content_type: &'static str,
    etag: String,
}

/// `_headers`의 규칙 하나: `/*`·`/assets/*`처럼 끝의 `*`는 접두어, 나머지는 정확히 같은 경로
struct Rule {
    pattern: String,
    set: Vec<(HeaderName, HeaderValue)>,
    remove: Vec<HeaderName>,
}
impl Rule {
    fn matches(&self, path: &str) -> bool {
        match self.pattern.strip_suffix('*') {
            Some(prefix) => path.starts_with(prefix),
            None => path == self.pattern,
        }
    }
}

/// 한 릴리스 폴더를 읽은 것
struct Snapshot {
    /// 심링크를 따라간 실제 폴더: 바뀌면 다시 읽는다
    target: PathBuf,
    files: HashMap<String, Asset>,
    rules: Vec<Rule>,
    redirects: HashMap<String, (String, StatusCode)>,
}

pub struct Studio {
    root: PathBuf,
    snapshot: RwLock<Option<Arc<Snapshot>>>,
    reload: tokio::sync::Mutex<()>,
}

/// 메모리에 올리는 폴더 크기 상한 (지금 빌드는 압축 포함 5 MB 정도)
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

impl Studio {
    /// `STUDIO_DIR`가 있으면 화면을 서빙한다. 폴더가 아직 없어도 뜬다(첫 배포 전에는 503).
    pub fn from_env() -> Option<Arc<Self>> {
        let dir = std::env::var("STUDIO_DIR").ok()?;
        let dir = dir.trim();
        (!dir.is_empty()).then(|| Self::new(dir))
    }

    pub fn new(root: impl Into<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            root: root.into(),
            snapshot: RwLock::new(None),
            reload: tokio::sync::Mutex::new(()),
        })
    }

    fn cached(&self) -> Option<Arc<Snapshot>> {
        self.snapshot
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 지금 릴리스. 심링크가 바뀌었으면 새로 읽고, 읽기에 실패하면 이전 것을 계속 쓴다.
    async fn current(&self) -> Option<Arc<Snapshot>> {
        let target = match tokio::fs::canonicalize(&self.root).await {
            Ok(t) => t,
            Err(_) => return self.cached(),
        };
        if let Some(snap) = self.cached().filter(|s| s.target == target) {
            return Some(snap);
        }
        let _one = self.reload.lock().await;
        if let Some(snap) = self.cached().filter(|s| s.target == target) {
            return Some(snap);
        }
        let dir = target.clone();
        match tokio::task::spawn_blocking(move || load(&dir)).await {
            Ok(Ok(snap)) => {
                tracing::info!(dir = %target.display(), files = snap.files.len(), "studio release loaded");
                let snap = Arc::new(snap);
                *self.snapshot.write().unwrap_or_else(|e| e.into_inner()) = Some(snap.clone());
                Some(snap)
            }
            Ok(Err(error)) => {
                tracing::error!(dir = %target.display(), %error, "studio release not loaded; keeping the previous one");
                self.cached()
            }
            Err(error) => {
                tracing::error!(%error, "studio release load panicked");
                self.cached()
            }
        }
    }

    pub async fn respond(&self, req: Request) -> Response {
        if !matches!(*req.method(), Method::GET | Method::HEAD) {
            let mut r = fail(StatusCode::METHOD_NOT_ALLOWED, "METHOD_NOT_ALLOWED");
            r.headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
            return r;
        }
        let Some(snap) = self.current().await else {
            return fail(StatusCode::SERVICE_UNAVAILABLE, "STUDIO_NOT_DEPLOYED");
        };
        snap.respond(&req)
    }
}

impl Snapshot {
    fn respond(&self, req: &Request) -> Response {
        let raw = req.uri().path();
        if let Some((to, status)) = self.redirects.get(raw) {
            let location = match req.uri().query() {
                Some(q) => format!("{to}?{q}"),
                None => to.clone(),
            };
            let mut r = Response::new(Body::empty());
            *r.status_mut() = *status;
            if let Ok(v) = HeaderValue::from_str(&location) {
                r.headers_mut().insert(header::LOCATION, v);
            }
            r.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            return r;
        }
        let Some(path) = decode_path(raw) else {
            return not_found();
        };
        let key = if path.ends_with('/') {
            format!("{path}index.html")
        } else {
            path.clone()
        };
        if let Some(asset) = self.files.get(&key) {
            return self.serve(req, asset, &path, false);
        }
        if path.starts_with("/.well-known/") {
            // AI 클라이언트가 찾아보는 다른 발견 문서에 화면 HTML을 200으로 주면 404로 넘어가지 못한다
            return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
        }
        let last = path.rsplit('/').next().unwrap_or("");
        if last.contains('.') {
            return not_found();
        }
        match self.files.get("/index.html") {
            Some(index) => self.serve(req, index, "/index.html", true),
            None => fail(StatusCode::SERVICE_UNAVAILABLE, "STUDIO_NOT_DEPLOYED"),
        }
    }

    fn serve(&self, req: &Request, asset: &Asset, rule_path: &str, fallback: bool) -> Response {
        let mut headers = HeaderMap::new();
        for rule in self.rules.iter().filter(|r| r.matches(rule_path)) {
            for name in &rule.remove {
                headers.remove(name);
            }
            for (name, value) in &rule.set {
                headers.insert(name.clone(), value.clone());
            }
        }
        if fallback || asset.content_type.starts_with("text/html") {
            // 화면 HTML은 늘 새로 확인한다: 새 배포의 스크립트 이름을 바로 받게
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        } else if !headers.contains_key(header::CACHE_CONTROL) {
            let policy = if rule_path.starts_with("/assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "public, max-age=0, must-revalidate"
            };
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
        }
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(asset.content_type),
        );
        let br = asset.br.as_ref().filter(|_| {
            let accept = req
                .headers()
                .get(header::ACCEPT_ENCODING)
                .and_then(|v| v.to_str().ok());
            let q = encoding_quality(accept, "br");
            q > 0.0 && q >= encoding_quality(accept, "gzip")
        });
        if asset.br.is_some() {
            headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        }
        let etag = match br {
            Some(_) => format!("\"{}-br\"", asset.etag),
            None => format!("\"{}\"", asset.etag),
        };
        if let Ok(v) = HeaderValue::from_str(&etag) {
            headers.insert(header::ETAG, v);
        }
        if br.is_some() {
            headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("br"));
        }
        if etag_matches(req.headers(), &etag) {
            headers.remove(header::CONTENT_ENCODING);
            headers.remove(header::CONTENT_TYPE);
            let mut r = Response::new(Body::empty());
            *r.status_mut() = StatusCode::NOT_MODIFIED;
            *r.headers_mut() = headers;
            return r;
        }
        let body = br.unwrap_or(&asset.body).clone();
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(body.len()));
        let mut r = if *req.method() == Method::HEAD {
            Response::new(Body::empty())
        } else {
            Response::new(Body::from(body))
        };
        *r.headers_mut() = headers;
        r
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        "Not found",
    )
        .into_response()
}

/// `%XX`를 풀고 `..`·빈 조각·역슬래시가 있는 경로는 버린다 (파일은 메모리 표에서만 찾지만 이름이 어긋나지 않게)
fn decode_path(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(out).ok()?;
    let bad = !path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.split('/').any(|seg| seg == ".." || seg == ".");
    (!bad).then_some(path)
}

/// Accept-Encoding의 q 값 (web/shared/transport.ts encodingQuality와 같은 규칙)
fn encoding_quality(accept: Option<&str>, encoding: &str) -> f32 {
    let mut named: Option<f32> = None;
    let mut star: Option<f32> = None;
    for part in accept.unwrap_or("").to_ascii_lowercase().split(',') {
        let mut items = part.split(';').map(str::trim);
        let name = items.next().unwrap_or("");
        let mut q = 1.0;
        for p in items {
            if let Some(raw) = p.strip_prefix("q=").or_else(|| p.strip_prefix("q =")) {
                q = raw
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .filter(|q| (0.0..=1.0).contains(q))
                    .unwrap_or(0.0);
            }
        }
        // 같은 이름이 여러 번 오면 낮은 쪽
        let slot = if name == encoding {
            &mut named
        } else if name == "*" {
            &mut star
        } else {
            continue;
        };
        *slot = Some(slot.map_or(q, |old: f32| old.min(q)));
    }
    named.or(star).unwrap_or(0.0)
}

fn etag_matches(h: &HeaderMap, etag: &str) -> bool {
    let Some(v) = h.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    v.split(',')
        .map(|t| t.trim().trim_start_matches("W/"))
        .any(|t| t == "*" || t == etag)
}

fn content_type(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("webmanifest") => "application/manifest+json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("wasm") => "application/wasm",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("pdf") => "application/pdf",
        Some("mp3") => "audio/mpeg",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// 릴리스 폴더를 읽는다. 심링크·숨김 파일(`.well-known` 제외)은 건너뛴다.
fn load(dir: &Path) -> std::io::Result<Snapshot> {
    let mut raw: HashMap<String, Vec<u8>> = HashMap::new();
    let mut total = 0u64;
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((path, prefix)) = stack.pop() {
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name != ".well-known" {
                continue;
            }
            let kind = entry.file_type()?;
            let key = format!("{prefix}/{name}");
            if kind.is_dir() {
                stack.push((entry.path(), key));
            } else if kind.is_file() {
                total += entry.metadata()?.len();
                if total > MAX_TOTAL_BYTES {
                    return Err(std::io::Error::other("studio release is too large"));
                }
                raw.insert(key, std::fs::read(entry.path())?);
            }
        }
    }
    if !raw.contains_key("/index.html") {
        return Err(std::io::Error::other("index.html missing"));
    }
    let rules = raw
        .remove("/_headers")
        .map(|b| parse_headers(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let redirects = raw
        .remove("/_redirects")
        .map(|b| parse_redirects(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let mut sidecars = HashMap::new();
    raw.retain(|key, body| match key.strip_suffix(".br") {
        Some(original) => {
            sidecars.insert(original.to_owned(), std::mem::take(body));
            false
        }
        None => true,
    });
    let files = raw
        .into_iter()
        .map(|(key, body)| {
            let digest = Sha256::digest(&body);
            let asset = Asset {
                content_type: content_type(&key),
                etag: crate::hex::encode(&digest[..12]),
                br: sidecars.remove(&key).map(Bytes::from),
                body: Bytes::from(body),
            };
            (key, asset)
        })
        .collect();
    Ok(Snapshot {
        target: dir.to_path_buf(),
        files,
        rules,
        redirects,
    })
}

/// Cloudflare `_headers` 형식: 들여쓰지 않은 줄이 경로, 들여쓴 `Name: value` 줄이 헤더, `! Name`은 지우기
fn parse_headers(text: &str) -> Vec<Rule> {
    let mut rules: Vec<Rule> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with([' ', '\t']) {
            rules.push(Rule {
                pattern: trimmed.to_owned(),
                set: Vec::new(),
                remove: Vec::new(),
            });
            continue;
        }
        let Some(rule) = rules.last_mut() else {
            continue;
        };
        if let Some(name) = trimmed.strip_prefix('!') {
            if let Ok(name) = HeaderName::try_from(name.trim()) {
                rule.remove.push(name);
            }
        } else if let Some((name, value)) = trimmed.split_once(':') {
            match (
                HeaderName::try_from(name.trim()),
                HeaderValue::from_str(value.trim()),
            ) {
                (Ok(n), Ok(v)) => rule.set.push((n, v)),
                _ => tracing::warn!(line = trimmed, "studio _headers line ignored"),
            }
        }
    }
    rules
}

/// Cloudflare `_redirects` 형식 중 정확한 경로만: `/from /to [301|302|303|307|308]`
fn parse_redirects(text: &str) -> HashMap<String, (String, StatusCode)> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let status = match parts.get(2) {
            Some(s) => s
                .parse::<u16>()
                .ok()
                .and_then(|c| StatusCode::from_u16(c).ok()),
            None => Some(StatusCode::FOUND),
        };
        match (parts.first(), parts.get(1), status) {
            (Some(from), Some(to), Some(status))
                if status.is_redirection() && !from.contains(['*', ':']) =>
            {
                out.insert((*from).to_owned(), ((*to).to_owned(), status));
            }
            _ => tracing::warn!(line, "studio _redirects line ignored"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트용 임시 폴더 (끝나면 지운다)
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("audeniq-studio-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn release(files: &[(&str, &[u8])]) -> TempDir {
        let dir = TempDir::new();
        for (name, body) in files {
            let path = dir.path().join(name.trim_start_matches('/'));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    const HEADERS: &str = "/*\n  X-Content-Type-Options: nosniff\n  Content-Security-Policy: default-src 'self'\n/\n  Cache-Control: no-cache\n/index.html\n  Cache-Control: no-cache\n/assets/*\n  Cache-Control: public, max-age=31536000, immutable\n/static/*\n  Cache-Control: public, max-age=86400\n";

    fn studio_files() -> TempDir {
        release(&[
            ("index.html", b"<!doctype html>app"),
            ("index.html.br", b"BR-HTML"),
            ("assets/app-abc.js", b"console.log(1)"),
            ("assets/app-abc.js.br", b"BR-JS"),
            ("static/logo.svg", b"<svg/>"),
            ("_headers", HEADERS.as_bytes()),
            ("_redirects", b"/index.html / 308\n/studio / 308\n"),
        ])
    }

    async fn get(studio: &Studio, path: &str, headers: &[(&str, &str)]) -> Response {
        let mut b = axum::http::Request::builder().uri(path);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        studio.respond(b.body(Body::empty()).unwrap()).await
    }

    async fn text(r: Response) -> String {
        let b = axum::body::to_bytes(r.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(b.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn serves_files_with_headers_rules_and_cache_policy() {
        let dir = studio_files();
        let studio = Studio::new(dir.path());
        let r = get(&studio, "/", &[]).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["cache-control"], "no-cache");
        assert_eq!(r.headers()["content-security-policy"], "default-src 'self'");
        assert_eq!(r.headers()["content-type"], "text/html; charset=utf-8");
        assert_eq!(text(r).await, "<!doctype html>app");

        let r = get(&studio, "/assets/app-abc.js", &[]).await;
        assert_eq!(
            r.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(r.headers()["x-content-type-options"], "nosniff");
        assert_eq!(r.headers()["vary"], "Accept-Encoding");
        assert_eq!(text(r).await, "console.log(1)");

        let r = get(&studio, "/static/logo.svg", &[]).await;
        assert_eq!(r.headers()["cache-control"], "public, max-age=86400");
        assert_eq!(r.headers()["content-type"], "image/svg+xml");
        // _headers·_redirects·사이드카는 파일로 내보내지 않는다
        // (확장자 없는 /_headers는 화면 HTML로 폴백)
        for hidden in ["/_headers", "/_redirects"] {
            assert_eq!(
                text(get(&studio, hidden, &[]).await).await,
                "<!doctype html>app",
                "{hidden}"
            );
        }
        assert_eq!(
            get(&studio, "/assets/app-abc.js.br", &[]).await.status(),
            404
        );
    }

    #[tokio::test]
    async fn spa_fallback_and_missing_files() {
        let dir = studio_files();
        let studio = Studio::new(dir.path());
        for path in ["/releases", "/releases/123/edit", "/settings/"] {
            let r = get(&studio, path, &[]).await;
            assert_eq!(r.status(), 200, "{path}");
            assert_eq!(r.headers()["cache-control"], "no-cache");
            assert_eq!(r.headers()["content-security-policy"], "default-src 'self'");
            assert_eq!(text(r).await, "<!doctype html>app");
        }
        // 예전 배포의 스크립트는 HTML이 아니라 404
        let r = get(&studio, "/assets/old-123.js", &[]).await;
        assert_eq!(r.status(), 404);
        assert_eq!(r.headers()["content-type"], "text/plain; charset=utf-8");
        // 다른 발견 문서는 JSON 404
        let r = get(&studio, "/.well-known/openid-configuration", &[]).await;
        assert_eq!(r.status(), 404);
        assert!(text(r).await.contains("NOT_FOUND"));
        // 경로 조작
        for path in ["/%2e%2e/etc/passwd", "/assets/..%2fapp-abc.js", "/a%ZZ"] {
            assert_eq!(get(&studio, path, &[]).await.status(), 404, "{path}");
        }
    }

    #[tokio::test]
    async fn redirects_keep_the_query() {
        let dir = studio_files();
        let studio = Studio::new(dir.path());
        let r = get(&studio, "/studio?x=1", &[]).await;
        assert_eq!(r.status(), 308);
        assert_eq!(r.headers()["location"], "/?x=1");
        assert_eq!(
            get(&studio, "/index.html", &[]).await.headers()["location"],
            "/"
        );
    }

    #[tokio::test]
    async fn brotli_sidecars_and_conditional_requests() {
        let dir = studio_files();
        let studio = Studio::new(dir.path());
        let r = get(
            &studio,
            "/assets/app-abc.js",
            &[("accept-encoding", "gzip, br")],
        )
        .await;
        assert_eq!(r.headers()["content-encoding"], "br");
        assert_eq!(
            r.headers()["content-type"],
            "application/javascript; charset=utf-8"
        );
        let etag = r.headers()["etag"].to_str().unwrap().to_owned();
        assert!(etag.ends_with("-br\""));
        assert_eq!(text(r).await, "BR-JS");
        let r = get(
            &studio,
            "/assets/app-abc.js",
            &[("accept-encoding", "br;q=0.5, gzip")],
        )
        .await;
        assert!(r.headers().get("content-encoding").is_none());
        let r = get(
            &studio,
            "/assets/app-abc.js",
            &[("accept-encoding", "br"), ("if-none-match", &etag)],
        )
        .await;
        assert_eq!(r.status(), 304);
        assert_eq!(
            r.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(text(r).await, "");
        // SPA 폴백도 압축본
        let r = get(&studio, "/releases", &[("accept-encoding", "br")]).await;
        assert_eq!(text(r).await, "BR-HTML");
    }

    #[tokio::test]
    async fn defaults_without_rules_and_methods() {
        let dir = release(&[
            ("index.html", b"x"),
            ("assets/a.js", b"a"),
            ("robots.txt", b"r"),
        ]);
        let studio = Studio::new(dir.path());
        assert_eq!(
            get(&studio, "/", &[]).await.headers()["cache-control"],
            "no-cache"
        );
        assert_eq!(
            get(&studio, "/assets/a.js", &[]).await.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            get(&studio, "/robots.txt", &[]).await.headers()["cache-control"],
            "public, max-age=0, must-revalidate"
        );
        let post = axum::http::Request::builder()
            .method("POST")
            .uri("/")
            .body(Body::empty())
            .unwrap();
        assert_eq!(studio.respond(post).await.status(), 405);
        let head = axum::http::Request::builder()
            .method("HEAD")
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let r = studio.respond(head).await;
        assert_eq!(r.headers()["content-length"], "1");
        assert_eq!(text(r).await, "");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn switches_release_when_the_symlink_moves() {
        let base = TempDir::new();
        for (name, body) in [("a", "first"), ("b", "second")] {
            std::fs::create_dir_all(base.path().join("releases").join(name)).unwrap();
            std::fs::write(
                base.path().join("releases").join(name).join("index.html"),
                body,
            )
            .unwrap();
        }
        let current = base.path().join("current");
        let studio = Studio::new(&current);
        assert_eq!(get(&studio, "/", &[]).await.status(), 503);
        std::os::unix::fs::symlink("releases/a", &current).unwrap();
        assert_eq!(text(get(&studio, "/", &[]).await).await, "first");
        std::fs::remove_file(&current).unwrap();
        std::os::unix::fs::symlink("releases/b", &current).unwrap();
        assert_eq!(text(get(&studio, "/x", &[]).await).await, "second");
        // 깨진 릴리스(index.html 없음)로 바뀌면 이전 것을 계속 쓴다
        std::fs::create_dir_all(base.path().join("releases/c")).unwrap();
        std::fs::remove_file(&current).unwrap();
        std::os::unix::fs::symlink("releases/c", &current).unwrap();
        assert_eq!(text(get(&studio, "/", &[]).await).await, "second");
    }

    #[test]
    fn api_paths_stay_with_the_api() {
        for p in [
            "/api",
            "/api/me",
            "/ready",
            "/health",
            "/.well-known/oauth-authorization-server",
        ] {
            assert!(api_path(p), "{p}");
        }
        for p in [
            "/",
            "/apiary",
            "/assets/a.js",
            "/.well-known/security.txt",
            "/release/1",
        ] {
            assert!(!api_path(p), "{p}");
        }
    }

    const SECRET: &str = "ssssssssssssssssssssssssssssssss";

    async fn state() -> AppState {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let config = crate::config::Config {
            database_url: String::new(),
            origin: "https://studio.audeniq.test".into(),
            service_secret: SECRET.into(),
            secure_cookie: true,
            bind: String::new(),
            session_seconds: 60,
            test_only_bypass_dsp_gate: false,
        };
        AppState::new(pool, config, Arc::new(crate::storage::DisabledStore))
            .await
            .unwrap()
    }

    /// 실제 라우터: 화면 파일은 서비스 비밀 없이도 뜨고, /api는 지금처럼 비밀이 없으면 403
    #[tokio::test]
    async fn static_files_are_outside_the_boundary_and_the_api_is_not() {
        use tower::ServiceExt;
        let dir = studio_files();
        let app = super::app(state().await, Some(Studio::new(dir.path())));
        let call = |path: &str, headers: &[(&str, &str)]| {
            let mut b = axum::http::Request::builder().uri(path);
            for (k, v) in headers {
                b = b.header(*k, *v);
            }
            app.clone().oneshot(b.body(Body::empty()).unwrap())
        };
        // 화면: 비밀 없이 200 (boundary 바깥), SPA 폴백 포함
        for path in ["/", "/releases/1", "/assets/app-abc.js"] {
            assert_eq!(call(path, &[]).await.unwrap().status(), 200, "{path}");
        }
        let r = call("/releases/1", &[]).await.unwrap();
        assert_eq!(r.headers()["cache-control"], "no-cache");
        // API는 그대로: 비밀이 없거나 위조면 403, 맞으면 통과
        assert_eq!(call("/health", &[]).await.unwrap().status(), 403);
        let forged = [("x-audeniq-service", "forged")];
        assert_eq!(call("/api/me", &forged).await.unwrap().status(), 403);
        let ok = [("x-audeniq-service", SECRET)];
        assert_eq!(call("/health", &ok).await.unwrap().status(), 200);
        // OAuth 발견 문서는 API로 (비밀 필요)
        let discovery = "/.well-known/oauth-authorization-server";
        assert_eq!(call(discovery, &[]).await.unwrap().status(), 403);
        // 관리자 사이트 요청에는 화면 파일을 주지 않는다 (API의 sites 규칙이 그대로 404)
        let admin = [("x-audeniq-site", "admin")];
        let r = call("/", &admin).await.unwrap();
        assert_eq!(r.status(), 404);
        assert_eq!(r.headers()["cache-control"], "no-store");
        // STUDIO_DIR 없이: 지금 동작 그대로 (비밀 없는 요청은 모두 403)
        let bare = super::app(state().await, None);
        let req = axum::http::Request::builder().uri("/").body(Body::empty());
        assert_eq!(bare.oneshot(req.unwrap()).await.unwrap().status(), 403);
    }

    #[tokio::test]
    async fn client_ip_is_trusted_only_with_the_service_secret() {
        let s = state().await;
        let mut h = HeaderMap::new();
        h.insert(CLIENT_IP_HEADER, HeaderValue::from_static("198.51.100.7"));
        assert_eq!(client_ip(&s, &h), None);
        h.insert("x-audeniq-service", HeaderValue::from_static("forged"));
        assert_eq!(client_ip(&s, &h), None);
        h.insert("x-audeniq-service", HeaderValue::from_static(SECRET));
        assert_eq!(client_ip(&s, &h).as_deref(), Some("198.51.100.7"));
        h.insert(CLIENT_IP_HEADER, HeaderValue::from_static("not-an-ip"));
        assert_eq!(client_ip(&s, &h), None);
    }
}
