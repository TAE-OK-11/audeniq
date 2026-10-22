//! AUDENIQ REST API (`/api/v1`) for apps that connect with OAuth 2.0.
//!
//! It opens exactly what the MCP server opens: every route runs one MCP tool
//! through [`tools::call`] (or [`public::call`]), so scopes, input schemas,
//! rate limits, audit rows, retry-safe creates and the Studio approvals are
//! the same. Only the transport differs: REST paths and HTTP status codes
//! instead of JSON-RPC.
//!
//! - Bearer tokens must be issued for the API resource (`{origin}/api/v1`,
//!   RFC 8707); MCP tokens are refused here and API tokens on `/api/mcp`.
//! - Path parameters, then the query string (GET/DELETE) or the JSON body
//!   (POST/PUT/PATCH/DELETE) become the tool's arguments; values in the query
//!   are converted with the tool's input schema.
//! - Success: the tool's structured result (HTTP 200). Failure:
//!   `{"error":{"code","message","details"?}}` with a status from the code.
//! - `GET /api/v1/openapi.json` describes every route (generated from the
//!   tool schemas, no account needed).
//!
//! Off unless `AUDENIQ_MCP_ENABLED` and `AUDENIQ_API_ENABLED` are both on.
use super::{
    Audience, SCOPES,
    oauth::{self, BearerError, Principal},
    public, tools,
};
use crate::{api::AppState, auth, error::Error};
use axum::{
    Json,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};

/// Where the API lives (behind the edge at the Studio origin).
pub const API_PATH: &str = "/api/v1";

/// One REST route: an HTTP method and a path under [`API_PATH`] (`{name}`
/// segments are tool arguments) for one MCP tool.
pub struct Route {
    pub method: &'static str,
    pub path: &'static str,
    pub tool: &'static str,
}

const fn r(method: &'static str, path: &'static str, tool: &'static str) -> Route {
    Route { method, path, tool }
}

/// Every MCP tool has exactly one route (checked in the tests).
pub const ROUTES: &[Route] = &[
    // Account and service
    r("GET", "/me", "get_account_context"),
    r("GET", "/members", "list_members"),
    r("POST", "/members/invitations", "request_member_invite"),
    r("GET", "/platforms", "list_distribution_platforms"),
    r("GET", "/requirements", "get_release_requirements"),
    r("GET", "/status", "get_service_status"),
    r("GET", "/announcements", "list_announcements"),
    r("GET", "/announcements/{kind}/{id}", "get_announcement"),
    r("GET", "/notifications", "list_notifications"),
    r("POST", "/notifications/read", "mark_notifications_read"),
    r("GET", "/documents", "list_documents"),
    r("GET", "/signing-requests", "list_signing_requests"),
    // Releases
    r("GET", "/releases", "list_releases"),
    r("POST", "/releases", "create_release_draft"),
    r("GET", "/releases/{release_id}", "get_release"),
    r("PATCH", "/releases/{release_id}", "update_release_draft"),
    r("DELETE", "/releases/{release_id}", "delete_release_draft"),
    r("POST", "/releases/{release_id}/tracks", "upsert_track"),
    r(
        "PUT",
        "/releases/{release_id}/tracks/{track_id}/credits",
        "set_track_credits",
    ),
    r(
        "PUT",
        "/releases/{release_id}/special-options",
        "set_special_options",
    ),
    r(
        "GET",
        "/releases/{release_id}/validation",
        "validate_release",
    ),
    r(
        "GET",
        "/releases/{release_id}/upload-link",
        "get_upload_link",
    ),
    r(
        "GET",
        "/releases/{release_id}/submission/preview",
        "preview_submission",
    ),
    r(
        "POST",
        "/releases/{release_id}/submission",
        "submit_release",
    ),
    r(
        "GET",
        "/releases/{release_id}/submission",
        "get_submission_status",
    ),
    r(
        "GET",
        "/releases/{release_id}/delivery",
        "get_delivery_status",
    ),
    r(
        "GET",
        "/releases/{release_id}/content-id",
        "get_content_id_status",
    ),
    r(
        "GET",
        "/releases/{release_id}/corrections",
        "get_release_corrections",
    ),
    r(
        "POST",
        "/releases/{release_id}/corrections",
        "request_release_correction",
    ),
    r(
        "DELETE",
        "/releases/{release_id}/corrections",
        "cancel_release_correction",
    ),
    r("GET", "/releases/{release_id}/pitch", "get_playlist_pitch"),
    r(
        "POST",
        "/releases/{release_id}/pitch",
        "submit_playlist_pitch",
    ),
    r(
        "DELETE",
        "/releases/{release_id}/pitch",
        "withdraw_playlist_pitch",
    ),
    r("GET", "/releases/{release_id}/smart-link", "get_smart_link"),
    r(
        "PUT",
        "/releases/{release_id}/smart-link",
        "save_smart_link",
    ),
    r(
        "GET",
        "/releases/{release_id}/smart-link/check",
        "check_smart_link",
    ),
    r("GET", "/releases/{release_id}/splits", "get_release_splits"),
    r(
        "POST",
        "/releases/{release_id}/splits/{share_id}/resend",
        "resend_split_invite",
    ),
    r(
        "POST",
        "/releases/{release_id}/split-change",
        "request_split_change",
    ),
    r(
        "GET",
        "/releases/{release_id}/infringement-claims",
        "list_infringement_claims",
    ),
    r(
        "POST",
        "/infringement-claims/{claim_id}/counter-notice",
        "request_counter_notice",
    ),
    r(
        "POST",
        "/releases/{release_id}/withdrawal",
        "request_release_withdrawal",
    ),
    r(
        "POST",
        "/releases/{release_id}/takedown",
        "request_takedown",
    ),
    r(
        "DELETE",
        "/releases/{release_id}/takedown",
        "cancel_takedown",
    ),
    r("POST", "/releases/{release_id}/end", "request_release_end"),
    r("DELETE", "/releases/{release_id}/end", "cancel_release_end"),
    r("GET", "/release-codes", "list_release_codes"),
    // Files
    r("POST", "/uploads", "start_upload"),
    r("POST", "/uploads/import", "import_file_from_url"),
    r("GET", "/uploads/{upload_session_id}", "get_upload_status"),
    r(
        "POST",
        "/uploads/{upload_session_id}/complete",
        "complete_upload",
    ),
    r("GET", "/assets/{asset_id}/checks", "get_file_checks"),
    r("POST", "/rights-proofs", "attach_rights_proof"),
    // Inquiries and platform requests
    r("GET", "/inquiries", "list_inquiries"),
    r("POST", "/inquiries", "create_inquiry"),
    r("GET", "/inquiries/{inquiry_id}", "get_inquiry"),
    r("POST", "/inquiries/{inquiry_id}/replies", "reply_inquiry"),
    r("POST", "/inquiries/{inquiry_id}/close", "close_inquiry"),
    r("GET", "/platform-requests", "list_platform_requests"),
    r("POST", "/platform-requests", "create_platform_request"),
    r(
        "DELETE",
        "/platform-requests/{request_id}",
        "cancel_platform_request",
    ),
    r("GET", "/artist-pages", "list_artist_pages"),
    r("PUT", "/artist-pages", "set_artist_pages"),
    // Settlement and payouts
    r("GET", "/settlement", "get_settlement_summary"),
    r(
        "GET",
        "/settlement/statements",
        "list_settlement_statements",
    ),
    r("GET", "/settlement/royalty-reports", "get_royalty_reports"),
    r("GET", "/settlement/export", "export_csv"),
    r("GET", "/splits/received", "list_received_splits"),
    r("GET", "/payouts", "list_payouts"),
    r("GET", "/payouts/preview", "preview_payout"),
    r("POST", "/payouts", "request_payout"),
    r("POST", "/payouts/large", "request_large_payout"),
    r("DELETE", "/payouts/{payout_id}", "cancel_payout"),
    // Contract
    r("POST", "/contract/end", "request_contract_end"),
    r("DELETE", "/contract/end", "cancel_contract_end"),
    // Requests the artist approves in the Studio
    r("POST", "/permission-requests", "request_permission"),
    r(
        "GET",
        "/permission-requests/{request_id}",
        "get_permission_request",
    ),
    r("GET", "/action-requests/{request_id}", "get_action_request"),
];

fn enabled(s: &AppState) -> bool {
    s.mcp.enabled && s.mcp.api_enabled
}

/// The route for a method and a path below [`API_PATH`], with its path
/// arguments.
pub fn find(method: &str, rest: &str) -> Option<(&'static Route, Vec<(&'static str, String)>)> {
    let given: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    ROUTES.iter().filter(|r| r.method == method).find_map(|r| {
        let want: Vec<&str> = r.path.split('/').collect();
        if want.len() != given.len() {
            return None;
        }
        let mut params = Vec::new();
        for (w, g) in want.iter().zip(&given) {
            match w.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
                Some(name) if !g.is_empty() => {
                    let value = percent_decode(g)?;
                    params.push((name, value));
                }
                Some(_) => return None,
                None if w == g => {}
                None => return None,
            }
        }
        Some((r, params))
    })
}

/// Whether any route has this path (for 405 instead of 404).
fn path_exists(rest: &str) -> bool {
    ["GET", "POST", "PUT", "PATCH", "DELETE"]
        .iter()
        .any(|m| find(m, rest).is_some())
}

/// A path segment with `%XX` escapes decoded (UTF-8 only).
fn percent_decode(v: &str) -> Option<String> {
    let bytes = v.as_bytes();
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
    String::from_utf8(out).ok()
}

fn property_type(schema: &Value, key: &str) -> Option<String> {
    let p = schema.get("properties")?.get(key)?;
    match p.get("type") {
        Some(Value::String(t)) => Some(t.clone()),
        // ["string","null"] and the like: the first non-null type.
        Some(Value::Array(ts)) => ts
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .map(str::to_owned),
        _ => None,
    }
}

/// A query-string value as the tool's schema types it. Arrays are repeated
/// keys or comma-separated values; anything that does not convert stays a
/// string, so the schema check names the problem.
fn typed(schema: &Value, key: &str, values: Vec<String>) -> Value {
    match property_type(schema, key).as_deref() {
        Some("array") => Value::Array(
            values
                .iter()
                .flat_map(|v| v.split(','))
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| json!(v))
                .collect(),
        ),
        Some(t) => {
            let v = values.into_iter().last().unwrap_or_default();
            match t {
                "integer" => v.parse::<i64>().map(Value::from).unwrap_or(json!(v)),
                "number" => v.parse::<f64>().map(Value::from).unwrap_or(json!(v)),
                "boolean" => match v.as_str() {
                    "true" | "1" => json!(true),
                    "false" | "0" => json!(false),
                    _ => json!(v),
                },
                _ => json!(v),
            }
        }
        None => json!(values.into_iter().last().unwrap_or_default()),
    }
}

/// The tool arguments for a request: path arguments, then the query string
/// or the JSON body. A value given twice with different contents is refused.
fn arguments(
    def: &tools::ToolDef,
    params: Vec<(&'static str, String)>,
    query: Option<&str>,
    body: &[u8],
) -> std::result::Result<Value, Box<Response>> {
    let schema = def.input_schema();
    let mut args = Map::new();
    if !body.iter().all(u8::is_ascii_whitespace) {
        match serde_json::from_slice::<Value>(body) {
            Ok(Value::Object(m)) => args = m,
            _ => {
                return Err(Box::new(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_JSON",
                    "The body must be a JSON object.",
                    None,
                )));
            }
        }
    }
    let mut from_query: Vec<(String, Vec<String>)> = Vec::new();
    for (k, v) in url::form_urlencoded::parse(query.unwrap_or("").as_bytes()) {
        let (k, v) = (k.into_owned(), v.into_owned());
        match from_query.iter_mut().find(|(name, _)| *name == k) {
            Some((_, vs)) => vs.push(v),
            None => from_query.push((k, vec![v])),
        }
    }
    for (k, vs) in from_query {
        let v = typed(&schema, &k, vs);
        if args.get(&k).is_some_and(|old| *old != v) {
            return Err(Box::new(conflict_argument(&k)));
        }
        args.insert(k, v);
    }
    for (k, v) in params {
        let v = typed(&schema, k, vec![v]);
        if args.get(k).is_some_and(|old| *old != v) {
            return Err(Box::new(conflict_argument(k)));
        }
        args.insert(k.to_string(), v);
    }
    Ok(Value::Object(args))
}

fn conflict_argument(name: &str) -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        "INVALID_ARGUMENTS",
        &format!("'{name}' is given twice with different values."),
        None,
    )
}

/// HTTP status for a tool error code.
pub fn status_for(code: &str) -> StatusCode {
    match code {
        "INVALID_ARGUMENTS" | "INVALID_INPUT" | "INVALID_JSON" => StatusCode::BAD_REQUEST,
        "UNAUTHENTICATED" => StatusCode::UNAUTHORIZED,
        "FORBIDDEN" | "INSUFFICIENT_SCOPE" => StatusCode::FORBIDDEN,
        "NOT_FOUND" => StatusCode::NOT_FOUND,
        "CONFLICT" | "INVARIANT_CONFLICT" | "REQUEST_IN_PROGRESS" => StatusCode::CONFLICT,
        "RATE_LIMITED" => StatusCode::TOO_MANY_REQUESTS,
        "NOT_IMPLEMENTED" => StatusCode::NOT_IMPLEMENTED,
        "DATABASE_UNAVAILABLE" | "STORAGE_UNAVAILABLE" | "UPLOAD_BUSY" => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        "REQUEST_TIMEOUT" => StatusCode::GATEWAY_TIMEOUT,
        "INTERNAL_ERROR" => StatusCode::INTERNAL_SERVER_ERROR,
        // Domain rules (a draft that is not complete, a release that is not
        // a draft, ...): the request was understood but cannot be done.
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

fn error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    details: Option<Value>,
) -> Response {
    let mut err = json!({"code": code, "message": message});
    if let Some(d) = details {
        err["details"] = d;
    }
    let mut r = (status, Json(json!({"error": err}))).into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn challenge(s: &AppState, extra: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "Bearer resource_metadata=\"{}\"{extra}",
        super::resource_metadata_url_for(s, Audience::Api)
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("Bearer"))
}

fn unauthorized(s: &AppState, h: &HeaderMap) -> Response {
    let mut r = error_response(
        StatusCode::UNAUTHORIZED,
        "UNAUTHENTICATED",
        "A valid access token for the AUDENIQ API is required.",
        None,
    );
    // RFC 6750 §3.1: no error code when no token was sent.
    let extra = if h.contains_key(header::AUTHORIZATION) {
        ", error=\"invalid_token\""
    } else {
        ""
    };
    r.headers_mut()
        .insert(header::WWW_AUTHENTICATE, challenge(s, extra));
    r
}

fn insufficient_scope(s: &AppState, scope: &str) -> Response {
    let mut r = error_response(
        StatusCode::FORBIDDEN,
        "INSUFFICIENT_SCOPE",
        &format!(
            "This connection was not given the '{scope}' permission. Ask the artist for it (POST /api/v1/permission-requests) or connect again and ask for it."
        ),
        Some(json!({"required_scope": scope})),
    );
    r.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        challenge(
            s,
            &format!(", error=\"insufficient_scope\", scope=\"{scope}\""),
        ),
    );
    r
}

fn server_error(e: Error) -> Response {
    let code = super::protocol::error_code(&e);
    tracing::warn!(code, "api request failed");
    error_response(
        status_for(code),
        code,
        "The request could not be completed.",
        None,
    )
}

/// The tool's MCP result as an HTTP response.
fn tool_response(result: Value) -> Response {
    let mut structured = result
        .get("structuredContent")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if result.get("isError") == Some(&Value::Bool(true)) {
        let err = structured
            .get("error")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let code = err
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("INTERNAL_ERROR");
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("The request could not be completed.");
        return error_response(status_for(code), code, message, err.get("details").cloned());
    }
    // `view` only picks the widget template in AI hosts.
    if let Some(m) = structured.as_object_mut() {
        m.remove("view");
    }
    let mut r = Json(structured).into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// `ANY /api/v1/{*rest}`.
pub async fn handle(
    State(s): State<AppState>,
    method: Method,
    uri: Uri,
    h: HeaderMap,
    RawQuery(q): RawQuery,
    body: Bytes,
) -> Response {
    if !enabled(&s) {
        return Error::NotFound.into_response();
    }
    // Apps call the API from their servers. A browser page of another site
    // gets nothing (no CORS); the Studio itself uses its session API.
    if h.contains_key(header::ORIGIN) && auth::origin(&h, &s.config).is_err() {
        return error_response(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "Call the AUDENIQ API from your server, not from a browser page.",
            None,
        );
    }
    let rest = uri.path().strip_prefix(API_PATH).unwrap_or("").to_string();
    let Some((route, params)) = find(method.as_str(), &rest) else {
        return if path_exists(&rest) {
            error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "METHOD_NOT_ALLOWED",
                "This path does not take that method.",
                None,
            )
        } else {
            error_response(
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "No such API path.",
                None,
            )
        };
    };
    let Some(def) = tools::find(route.tool) else {
        return server_error(Error::NotFound);
    };
    let principal: Principal = match oauth::principal_for(&s, &h, Audience::Api).await {
        Ok(p) => p,
        Err(BearerError::Invalid) => return unauthorized(&s, &h),
        Err(BearerError::Api(e)) => return server_error(e),
    };
    if !tools::permitted(def, &principal) {
        return insufficient_scope(&s, def.scope);
    }
    let body_args = match method {
        Method::GET | Method::HEAD => &[][..],
        _ => &body[..],
    };
    let args = match arguments(def, params, q.as_deref(), body_args) {
        Ok(v) => v,
        Err(r) => return *r,
    };
    if def.public() {
        return match public::call(&s, def, args, Some(&principal)).await {
            Ok(v) => tool_response(v),
            Err(e) => server_error(e),
        };
    }
    tool_response(Box::pin(tools::call(&s, &principal, def, args)).await)
}

/// `GET /api/v1/openapi.json`: every route, generated from the tool schemas.
pub async fn openapi(State(s): State<AppState>) -> Response {
    if !enabled(&s) {
        return Error::NotFound.into_response();
    }
    Json(openapi_document(&super::issuer(&s))).into_response()
}

fn path_names(path: &str) -> Vec<&str> {
    path.split('/')
        .filter_map(|seg| seg.strip_prefix('{').and_then(|v| v.strip_suffix('}')))
        .collect()
}

pub fn openapi_document(issuer: &str) -> Value {
    let scope_text = |scope: &str| match scope {
        super::SCOPE_READ => "Read releases, tracks, checks and status",
        super::SCOPE_WRITE => "Create and edit drafts, files, inquiries and requests",
        super::SCOPE_SUBMIT => {
            "Submit a release for distribution (after the artist's confirmation)"
        }
        super::SCOPE_SETTLEMENT => "Read settlement, reports and payouts",
        super::SCOPE_PAYOUT => "Request small payouts to the registered account",
        _ => "",
    };
    let scopes: Map<String, Value> = SCOPES
        .iter()
        .map(|sc| (sc.to_string(), json!(scope_text(sc))))
        .collect();
    let error_schema = json!({"type":"object","required":["error"],"properties":{"error":{
        "type":"object","required":["code","message"],"properties":{
            "code":{"type":"string"},"message":{"type":"string"},"details":{}}}}});
    let mut paths = Map::new();
    for route in ROUTES {
        let Some(def) = tools::find(route.tool) else {
            continue;
        };
        let schema = def.input_schema();
        let in_path = path_names(route.path);
        let props = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let required: Vec<String> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let mut parameters: Vec<Value> = in_path
            .iter()
            .map(|n| {
                json!({"name": n, "in": "path", "required": true,
                       "schema": props.get(*n).cloned().unwrap_or_else(|| json!({"type":"string"}))})
            })
            .collect();
        let with_body = matches!(route.method, "POST" | "PUT" | "PATCH")
            || (route.method == "DELETE"
                && required.iter().any(|r| !in_path.contains(&r.as_str())));
        let mut op = json!({
            "operationId": route.tool,
            "summary": def.title,
            "description": def.description,
            "security": [{"oauth2": [def.scope]}],
            "responses": {
                "200": {"description": "The result", "content": {"application/json": {"schema": {"type":"object"}}}},
                "default": {"description": "An error", "content": {"application/json": {"schema": error_schema}}},
            },
        });
        if with_body {
            let mut body_schema = schema.clone();
            if let Some(p) = body_schema
                .get_mut("properties")
                .and_then(Value::as_object_mut)
            {
                for n in &in_path {
                    p.remove(*n);
                }
            }
            let rest_required: Vec<&String> = required
                .iter()
                .filter(|r| !in_path.contains(&r.as_str()))
                .collect();
            body_schema["required"] = json!(rest_required);
            op["requestBody"] = json!({"required": !rest_required.is_empty(),
                "content": {"application/json": {"schema": body_schema}}});
        } else {
            for (name, prop) in &props {
                if in_path.contains(&name.as_str()) {
                    continue;
                }
                parameters.push(json!({"name": name, "in": "query",
                    "required": required.contains(name), "schema": prop}));
            }
        }
        if !parameters.is_empty() {
            op["parameters"] = json!(parameters);
        }
        let entry = paths
            .entry(route.path.to_string())
            .or_insert_with(|| json!({}));
        entry[route.method.to_ascii_lowercase()] = op;
    }
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "AUDENIQ API",
            "version": "1",
            "description": "The same actions as the AUDENIQ MCP server, as REST. Connect with OAuth 2.0 (authorization code + PKCE) for the resource {origin}/api/v1; requests that change contracts, payouts above the limit or members are filed for the artist to approve in AUDENIQ Studio.",
        },
        "servers": [{"url": format!("{issuer}{API_PATH}")}],
        "components": {"securitySchemes": {"oauth2": {"type": "oauth2", "flows": {"authorizationCode": {
            "authorizationUrl": format!("{issuer}/api/oauth/authorize"),
            "tokenUrl": format!("{issuer}/api/oauth/token"),
            "refreshUrl": format!("{issuer}/api/oauth/token"),
            "scopes": scopes,
        }}}}},
        "paths": paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_exactly_one_route() {
        let mut seen = std::collections::BTreeSet::new();
        for r in ROUTES {
            assert!(tools::find(r.tool).is_some(), "unknown tool {}", r.tool);
            assert!(seen.insert(r.tool), "{} has two routes", r.tool);
        }
        for d in tools::defs() {
            assert!(seen.contains(d.name), "{} has no REST route", d.name);
        }
    }

    #[test]
    fn path_arguments_are_tool_arguments_and_routes_are_unique() {
        let mut keys = std::collections::BTreeSet::new();
        let mut problems = Vec::new();
        for r in ROUTES {
            let def = tools::find(r.tool).unwrap();
            let schema = def.input_schema();
            for n in path_names(r.path) {
                if schema["properties"].get(n).is_none() {
                    let known: Vec<&String> = schema["properties"]
                        .as_object()
                        .map(|m| m.keys().collect())
                        .unwrap_or_default();
                    problems.push(format!(
                        "{} {}: '{n}' is not an argument of {} {known:?}",
                        r.method, r.path, r.tool
                    ));
                }
            }
            // Same shape (parameters as '*') and method would be ambiguous.
            let shape: String = r
                .path
                .split('/')
                .map(|s| if s.starts_with('{') { "*" } else { s })
                .collect::<Vec<_>>()
                .join("/");
            if !keys.insert((r.method, shape)) {
                problems.push(format!("{} {} clashes", r.method, r.path));
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn routes_match_paths_and_decode_arguments() {
        let (r, p) = find("GET", "/releases/abc%2Fdef").unwrap();
        assert_eq!(r.tool, "get_release");
        assert_eq!(p, vec![("release_id", "abc/def".to_string())]);
        assert_eq!(find("GET", "/releases/").unwrap().0.tool, "list_releases");
        assert!(find("PUT", "/releases").is_none());
        assert!(path_exists("/releases"));
        assert!(!path_exists("/nope"));
        let (r, p) = find("POST", "/releases/r1/splits/s1/resend").unwrap();
        assert_eq!(r.tool, "resend_split_invite");
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn query_values_follow_the_schema() {
        let schema = json!({"properties":{"n":{"type":"integer"},"b":{"type":"boolean"},
            "a":{"type":"array"},"s":{"type":["string","null"]}}});
        assert_eq!(typed(&schema, "n", vec!["5".into()]), json!(5));
        assert_eq!(typed(&schema, "n", vec!["x".into()]), json!("x"));
        assert_eq!(typed(&schema, "b", vec!["true".into()]), json!(true));
        assert_eq!(
            typed(&schema, "a", vec!["x,y".into(), "z".into()]),
            json!(["x", "y", "z"])
        );
        assert_eq!(typed(&schema, "s", vec!["v".into()]), json!("v"));
        assert_eq!(typed(&schema, "other", vec!["v".into()]), json!("v"));
    }

    #[test]
    fn error_codes_have_http_statuses() {
        assert_eq!(status_for("INVALID_ARGUMENTS"), StatusCode::BAD_REQUEST);
        assert_eq!(status_for("NOT_FOUND"), StatusCode::NOT_FOUND);
        assert_eq!(status_for("RATE_LIMITED"), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            status_for("RELEASE_NOT_DRAFT"),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn openapi_lists_every_route_with_oauth() {
        let doc = openapi_document("https://studio.example");
        assert_eq!(doc["servers"][0]["url"], "https://studio.example/api/v1");
        let mut n = 0;
        for r in ROUTES {
            let op = &doc["paths"][r.path][r.method.to_ascii_lowercase()];
            assert_eq!(op["operationId"], r.tool);
            assert!(op["security"][0]["oauth2"].is_array());
            n += 1;
        }
        assert_eq!(n, ROUTES.len());
        // Path arguments are parameters, not body fields.
        let op = &doc["paths"]["/releases/{release_id}"]["patch"];
        assert_eq!(op["parameters"][0]["name"], "release_id");
        assert!(
            op["requestBody"]["content"]["application/json"]["schema"]["properties"]
                .get("release_id")
                .is_none()
        );
    }
}
