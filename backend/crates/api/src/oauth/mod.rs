//! Embedded OAuth 2.1 authorization server for MCP clients (docs/mcp.md).
//!
//! The MCP server at `/mcp` is the protected resource. Clients discover this
//! server through RFC 9728 / RFC 8414 metadata, register dynamically (RFC 7591)
//! or identify themselves with a Client ID Metadata Document, send the owner to
//! `/oauth/authorize` (PKCE S256 only), and exchange the code at `/oauth/token`.
//! The owner approves on the SPA consent page (`routes::oauth_consent`).
//!
//! An approved connection is a *grant*: an `api_tokens` row with
//! `kind = 'oauth'`. Access tokens (1 h) and rotating refresh tokens point at
//! it, so revoking the grant cuts the client off at once.

pub mod cimd;
pub mod jwt;

use std::collections::HashMap;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::auth::{Scope, tokens};
use crate::routes::auth::client_ip;
use crate::state::AppState;

pub const ACCESS_TTL_SECS: i64 = 3600;
pub const REFRESH_TTL_DAYS: i32 = 30;
pub const CODE_TTL_SECS: i64 = 600;

/// Scopes pre-selected on the consent screen when a client asks for none.
pub const DEFAULT_SCOPES: &[Scope] = &[Scope::ProblemsRead, Scope::ProblemsWrite, Scope::SetsWrite, Scope::Publish];

/// Issuer and resource URLs, when MCP is configured (`PUBLIC_URL`) and enabled.
pub struct Endpoints {
    pub issuer: String,
    pub resource: String,
}

pub fn endpoints(st: &AppState) -> Option<Endpoints> {
    let issuer = st.cfg.public_url.clone()?;
    if !st.snapshot().settings.mcp_enabled {
        return None;
    }
    Some(Endpoints { resource: format!("{issuer}/mcp"), issuer })
}

pub fn resource_metadata_url(issuer: &str) -> String {
    format!("{issuer}/.well-known/oauth-protected-resource/mcp")
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/.well-known/oauth-protected-resource", get(protected_resource))
        .route("/.well-known/oauth-protected-resource/mcp", get(protected_resource))
        .route("/.well-known/oauth-authorization-server", get(authorization_server))
        .route("/.well-known/openid-configuration", get(authorization_server))
        // Some clients insert the resource path when probing (RFC 8414 §3.1 style).
        .route("/.well-known/oauth-authorization-server/mcp", get(authorization_server))
        .route("/.well-known/openid-configuration/mcp", get(authorization_server))
        .route("/mcp/.well-known/openid-configuration", get(authorization_server))
        // Anything else under /.well-known is a real 404, not the SPA's index.html.
        .route("/.well-known/{*rest}", get(|| async { StatusCode::NOT_FOUND }))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
}

fn disabled() -> Response {
    (StatusCode::NOT_FOUND, "MCP is not enabled on this server").into_response()
}

fn scope_names(scopes: &[Scope]) -> Vec<&'static str> {
    scopes.iter().map(|s| s.as_str()).collect()
}

async fn protected_resource(State(st): State<AppState>) -> Response {
    let Some(e) = endpoints(&st) else { return disabled() };
    axum::Json(json!({
        "resource": e.resource,
        "authorization_servers": [e.issuer],
        "scopes_supported": scope_names(Scope::ALL),
        "bearer_methods_supported": ["header"],
        "resource_name": st.snapshot().settings.site_name,
        "resource_documentation": format!("{}/api/docs", e.issuer),
    }))
    .into_response()
}

async fn authorization_server(State(st): State<AppState>) -> Response {
    let Some(e) = endpoints(&st) else { return disabled() };
    let i = &e.issuer;
    axum::Json(json!({
        "issuer": i,
        "authorization_endpoint": format!("{i}/oauth/authorize"),
        "token_endpoint": format!("{i}/oauth/token"),
        "registration_endpoint": format!("{i}/oauth/register"),
        "revocation_endpoint": format!("{i}/oauth/revoke"),
        "scopes_supported": scope_names(Scope::ALL),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic", "private_key_jwt"],
        "token_endpoint_auth_signing_alg_values_supported": jwt::SIGNING_ALGS.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>(),
        "revocation_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
        "client_id_metadata_document_supported": true,
        "authorization_response_iss_parameter_supported": true,
    }))
    .into_response()
}

/// RFC 6749 §5.2 error body.
pub struct OAuthError {
    status: StatusCode,
    error: &'static str,
    description: String,
}

impl OAuthError {
    fn new(status: StatusCode, error: &'static str, description: impl Into<String>) -> OAuthError {
        OAuthError { status, error, description: description.into() }
    }
    fn bad(error: &'static str, description: impl Into<String>) -> OAuthError {
        OAuthError::new(StatusCode::BAD_REQUEST, error, description)
    }
    fn grant(description: impl Into<String>) -> OAuthError {
        OAuthError::bad("invalid_grant", description)
    }
}

impl From<sqlx::Error> for OAuthError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = %e, "oauth database error");
        OAuthError::new(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "internal error")
    }
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let mut res = (self.status, axum::Json(json!({"error": self.error, "error_description": self.description}))).into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            res.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"oauth\""));
        }
        res
    }
}

fn no_store(mut res: Response) -> Response {
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res.headers_mut().insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    res
}

/// A registered (or CIMD-resolved) client.
#[derive(sqlx::FromRow, Clone)]
pub struct ClientRow {
    pub id: String,
    pub source: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub secret_hash: Option<Vec<u8>>,
    /// Registration metadata (`token_endpoint_auth_method`, `jwks_uri`…).
    pub metadata: sqlx::types::Json<Value>,
}

const CLIENT_COLUMNS: &str = "id, source, name, redirect_uris, secret_hash, metadata";

pub async fn find_client(st: &AppState, id: &str) -> Result<Option<ClientRow>, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {CLIENT_COLUMNS} FROM oauth_clients WHERE id = $1")).bind(id).fetch_optional(&st.db).await
}

/// Redirect URIs must be https, or plain http on a loopback host (RFC 8252 native apps).
pub fn valid_redirect_uri(uri: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(uri) else { return false };
    if u.fragment().is_some() || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    match u.scheme() {
        "https" => u.host().is_some(),
        "http" => matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        _ => false,
    }
}

/// Exact match, except that loopback redirects may use any port (RFC 8252 §7.3).
fn redirect_matches(registered: &str, given: &str) -> bool {
    if registered == given {
        return true;
    }
    let (Ok(a), Ok(b)) = (reqwest::Url::parse(registered), reqwest::Url::parse(given)) else { return false };
    a.scheme() == "http"
        && b.scheme() == "http"
        && matches!(a.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        && a.host_str() == b.host_str()
        && a.path() == b.path()
        && a.query() == b.query()
}

/// Appends query parameters to a client's redirect URI.
pub fn redirect_with(uri: &str, params: &[(&str, &str)]) -> String {
    let mut u = reqwest::Url::parse(uri).expect("redirect URIs are validated on registration");
    {
        let mut q = u.query_pairs_mut();
        for (k, v) in params {
            q.append_pair(k, v);
        }
    }
    u.to_string()
}

/// Housekeeping, piggybacked on registration and authorization requests.
async fn prune(st: &AppState) {
    let _ = sqlx::query(
        "DELETE FROM oauth_clients c WHERE c.source = 'dcr' AND c.created_at < now() - interval '7 days'
           AND NOT EXISTS (SELECT 1 FROM api_tokens t WHERE t.client_id = c.id)",
    )
    .execute(&st.db)
    .await;
    let _ = sqlx::query("DELETE FROM oauth_auth_requests WHERE expires_at < now() - interval '1 day'").execute(&st.db).await;
    let _ = sqlx::query("DELETE FROM oauth_tokens WHERE expires_at < now() - interval '1 day'").execute(&st.db).await;
}

/// Dynamic client registration (RFC 7591). Open by design: registering grants
/// nothing; the owner still has to log in and approve on the consent page.
async fn register(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if endpoints(&st).is_none() {
        return disabled();
    }
    if !st.limiter.check(&format!("oauth-register:{}", client_ip(&st.cfg, &headers)), 30, Duration::from_secs(3600)).await {
        return OAuthError::new(StatusCode::TOO_MANY_REQUESTS, "slow_down", "too many registrations from this address").into_response();
    }
    match register_inner(&st, &body).await {
        Ok(v) => no_store((StatusCode::CREATED, axum::Json(v)).into_response()),
        Err(e) => e.into_response(),
    }
}

async fn register_inner(st: &AppState, body: &[u8]) -> Result<Value, OAuthError> {
    let meta: serde_json::Map<String, Value> =
        serde_json::from_slice(body).map_err(|e| OAuthError::bad("invalid_client_metadata", format!("body must be a JSON object: {e}")))?;
    let redirect_uris: Vec<String> =
        meta.get("redirect_uris").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if redirect_uris.is_empty() || redirect_uris.len() > 10 {
        return Err(OAuthError::bad("invalid_redirect_uri", "redirect_uris must list 1 to 10 URIs"));
    }
    if let Some(bad) = redirect_uris.iter().find(|u| !valid_redirect_uri(u)) {
        return Err(OAuthError::bad("invalid_redirect_uri", format!("{bad}: must be https, or http on localhost")));
    }
    let auth_method = meta.get("token_endpoint_auth_method").and_then(Value::as_str).unwrap_or("client_secret_basic").to_string();
    if !matches!(auth_method.as_str(), "none" | "client_secret_post" | "client_secret_basic") {
        return Err(OAuthError::bad("invalid_client_metadata", "token_endpoint_auth_method must be none, client_secret_post or client_secret_basic"));
    }
    let list = |k: &str| meta.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>());
    if list("grant_types").is_some_and(|g| g.iter().any(|g| g != "authorization_code" && g != "refresh_token")) {
        return Err(OAuthError::bad("invalid_client_metadata", "only authorization_code and refresh_token grants are supported"));
    }
    if list("response_types").is_some_and(|r| r.iter().any(|r| r != "code")) {
        return Err(OAuthError::bad("invalid_client_metadata", "only the code response type is supported"));
    }
    let name: String = meta.get("client_name").and_then(Value::as_str).unwrap_or("MCP client").trim().chars().take(80).collect();
    let name = if name.is_empty() { "MCP client".to_string() } else { name };

    prune(st).await;
    let id = format!("ccalc_{}", tokens::random_token(18));
    let secret = (auth_method != "none").then(|| tokens::random_token(32));
    let stored = json!({
        "client_name": name,
        "client_uri": meta.get("client_uri"),
        "logo_uri": meta.get("logo_uri"),
        "token_endpoint_auth_method": auth_method,
        "software_id": meta.get("software_id"),
        "software_version": meta.get("software_version"),
    });
    sqlx::query("INSERT INTO oauth_clients (id, source, name, redirect_uris, secret_hash, metadata) VALUES ($1, 'dcr', $2, $3, $4, $5)")
        .bind(&id)
        .bind(&name)
        .bind(&redirect_uris)
        .bind(secret.as_deref().map(tokens::digest))
        .bind(&stored)
        .execute(&st.db)
        .await?;
    let mut out = json!({
        "client_id": id,
        "client_id_issued_at": chrono::Utc::now().timestamp(),
        "client_name": name,
        "redirect_uris": redirect_uris,
        "token_endpoint_auth_method": auth_method,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
    });
    if let Some(s) = secret {
        out["client_secret"] = json!(s);
        out["client_secret_expires_at"] = json!(0);
    }
    Ok(out)
}

/// Shown instead of redirecting when the client or redirect URI can't be trusted.
fn error_page(msg: &str) -> Response {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>授權失敗</title>\
         <body style=\"font-family:sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem\">\
         <h1>無法授權這個應用程式</h1><p>{}</p></body>",
        html_escape(msg)
    );
    (StatusCode::BAD_REQUEST, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `resource` (RFC 8707) must name this MCP server; tolerate a trailing slash.
fn resource_ok(given: Option<&String>, resource: &str) -> bool {
    given.is_none_or(|r| r.trim_end_matches('/') == resource)
}

/// Starts an authorization: validates the request, stores it, and sends the
/// browser to the SPA consent page (which requires an owner login).
async fn authorize(State(st): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(e) = endpoints(&st) else { return disabled() };
    let Some(client_id) = q.get("client_id") else { return error_page("缺少 client_id。") };
    let client = match resolve_client(&st, client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => return error_page("找不到這個應用程式（client_id 未註冊）。請在應用程式中重新連線。"),
        Err(msg) => return error_page(&msg),
    };
    let redirect_uri = match q.get("redirect_uri") {
        Some(r) if client.redirect_uris.iter().any(|reg| redirect_matches(reg, r)) => r.clone(),
        Some(_) => return error_page("redirect_uri 與應用程式註冊的不符。"),
        None if client.redirect_uris.len() == 1 => client.redirect_uris[0].clone(),
        None => return error_page("缺少 redirect_uri。"),
    };
    let state = q.get("state").cloned();
    let fail = |error: &str, desc: &str| {
        let mut p = vec![("error", error), ("error_description", desc), ("iss", e.issuer.as_str())];
        if let Some(s) = &state {
            p.push(("state", s));
        }
        Redirect::to(&redirect_with(&redirect_uri, &p)).into_response()
    };
    if q.get("response_type").map(String::as_str) != Some("code") {
        return fail("unsupported_response_type", "response_type must be code");
    }
    let Some(challenge) = q.get("code_challenge").filter(|c| (43..=128).contains(&c.len())) else {
        return fail("invalid_request", "PKCE code_challenge is required");
    };
    if q.get("code_challenge_method").map(String::as_str) != Some("S256") {
        return fail("invalid_request", "code_challenge_method must be S256");
    }
    if !resource_ok(q.get("resource"), &e.resource) {
        return fail("invalid_target", "resource must be this server's MCP endpoint");
    }
    let mut scopes: Vec<&str> = q.get("scope").map(|s| s.split_whitespace().filter_map(Scope::parse).map(Scope::as_str).collect()).unwrap_or_default();
    if scopes.is_empty() {
        scopes = scope_names(DEFAULT_SCOPES);
    }
    scopes.sort();
    scopes.dedup();

    prune(&st).await;
    let id = Uuid::now_v7();
    let r = sqlx::query(
        "INSERT INTO oauth_auth_requests (id, client_id, redirect_uri, state, code_challenge, scopes_requested, resource, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now() + make_interval(secs => $8))",
    )
    .bind(id)
    .bind(&client.id)
    .bind(&redirect_uri)
    .bind(&state)
    .bind(challenge)
    .bind(&scopes)
    .bind(&e.resource)
    .bind(CODE_TTL_SECS as f64)
    .execute(&st.db)
    .await;
    if let Err(err) = r {
        tracing::error!(error = %err, "storing authorization request");
        return fail("server_error", "internal error");
    }
    Redirect::to(&format!("/oauth/consent?request={id}")).into_response()
}

/// Registered client, or a Client ID Metadata Document fetched on first use.
async fn resolve_client(st: &AppState, client_id: &str) -> Result<Option<ClientRow>, String> {
    if client_id.starts_with("https://") {
        return cimd::resolve(st, client_id).await.map(Some);
    }
    find_client(st, client_id).await.map_err(|e| {
        tracing::error!(error = %e, "loading oauth client");
        "伺服器內部錯誤，請稍後再試。".to_string()
    })
}

/// Client authentication at the token endpoint: a signed `client_assertion`
/// (private_key_jwt), HTTP Basic, form secret, or none for public clients.
/// Clients registered with a secret or keys must use them.
async fn authenticate_client(st: &AppState, e: &Endpoints, headers: &HeaderMap, form: &HashMap<String, String>) -> Result<ClientRow, OAuthError> {
    let invalid = |msg: String| OAuthError::new(StatusCode::UNAUTHORIZED, "invalid_client", msg);
    if let Some(assertion) = form.get("client_assertion") {
        if form.get("client_assertion_type").map(String::as_str) != Some(jwt::ASSERTION_TYPE) {
            return Err(invalid("unsupported client_assertion_type".into()));
        }
        let id = form.get("client_id").cloned().or_else(|| jwt::unverified_issuer(assertion)).ok_or_else(|| invalid("client_id is required".into()))?;
        let client = find_client(st, &id).await?.ok_or_else(|| invalid("unknown client".into()))?;
        let audiences = [e.issuer.clone(), format!("{}/oauth/token", e.issuer)];
        jwt::verify(st, &client, assertion, &audiences).await.map_err(invalid)?;
        return Ok(client);
    }
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b.trim()).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.split_once(':').map(|(a, b)| (pct_decode(a), pct_decode(b))));
    let (id, secret) = match basic {
        Some((id, secret)) => (id, Some(secret)),
        None => {
            let id = form.get("client_id").cloned().ok_or_else(|| OAuthError::new(StatusCode::UNAUTHORIZED, "invalid_client", "client_id is required"))?;
            (id, form.get("client_secret").cloned())
        }
    };
    let client = find_client(st, &id).await?.ok_or_else(|| OAuthError::new(StatusCode::UNAUTHORIZED, "invalid_client", "unknown client"))?;
    if jwt::requires_assertion(&client) {
        return Err(invalid("this client must authenticate with a client_assertion (private_key_jwt)".into()));
    }
    if let Some(hash) = &client.secret_hash {
        let ok = secret.is_some_and(|s| tokens::constant_time_eq(&tokens::digest(&s), hash));
        if !ok {
            return Err(OAuthError::new(StatusCode::UNAUTHORIZED, "invalid_client", "client authentication failed"));
        }
    }
    Ok(client)
}

/// Percent-decoding for Basic credentials (RFC 6749 §2.3.1).
fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn pkce_s256(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

async fn token(State(st): State<AppState>, headers: HeaderMap, axum::Form(form): axum::Form<HashMap<String, String>>) -> Response {
    let Some(e) = endpoints(&st) else { return disabled() };
    let r = match form.get("grant_type").map(String::as_str) {
        Some("authorization_code") => exchange_code(&st, &e, &headers, &form).await,
        Some("refresh_token") => refresh(&st, &e, &headers, &form).await,
        _ => Err(OAuthError::bad("unsupported_grant_type", "grant_type must be authorization_code or refresh_token")),
    };
    no_store(match r {
        Ok(v) => axum::Json(v).into_response(),
        Err(e) => e.into_response(),
    })
}

async fn revoke_grant(db: impl sqlx::PgExecutor<'_>, grant_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE api_tokens SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL").bind(grant_id).execute(db).await.map(|_| ())
}

#[derive(sqlx::FromRow)]
struct CodeRow {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    resource: String,
    grant_id: Option<Uuid>,
    expired: bool,
    used: bool,
}

async fn exchange_code(st: &AppState, e: &Endpoints, headers: &HeaderMap, form: &HashMap<String, String>) -> Result<Value, OAuthError> {
    let client = authenticate_client(st, e, headers, form).await?;
    let code = form.get("code").ok_or_else(|| OAuthError::bad("invalid_request", "code is required"))?;
    let verifier = form.get("code_verifier").ok_or_else(|| OAuthError::bad("invalid_request", "code_verifier is required"))?;
    let mut tx = st.db.begin().await?;
    let row: Option<CodeRow> = sqlx::query_as(
        "SELECT client_id, redirect_uri, code_challenge, resource, grant_id, expires_at < now() AS expired, used_at IS NOT NULL AS used
         FROM oauth_auth_requests WHERE code_hash = $1 FOR UPDATE",
    )
    .bind(tokens::digest(code))
    .fetch_optional(&mut *tx)
    .await?;
    let row = row.ok_or_else(|| OAuthError::grant("unknown authorization code"))?;
    let grant_id = row.grant_id.ok_or_else(|| OAuthError::grant("unknown authorization code"))?;
    if row.used {
        // A replayed code may have been stolen: cut off everything it produced.
        revoke_grant(&mut *tx, grant_id).await?;
        tx.commit().await?;
        return Err(OAuthError::grant("authorization code was already used; the connection has been revoked"));
    }
    sqlx::query("UPDATE oauth_auth_requests SET used_at = now() WHERE code_hash = $1").bind(tokens::digest(code)).execute(&mut *tx).await?;
    if row.expired {
        tx.commit().await?;
        return Err(OAuthError::grant("authorization code expired"));
    }
    if row.client_id != client.id {
        tx.commit().await?;
        return Err(OAuthError::grant("code was issued to another client"));
    }
    if form.get("redirect_uri").is_some_and(|r| *r != row.redirect_uri) {
        tx.commit().await?;
        return Err(OAuthError::grant("redirect_uri does not match the authorization request"));
    }
    if !tokens::constant_time_eq(pkce_s256(verifier).as_bytes(), row.code_challenge.as_bytes()) {
        tx.commit().await?;
        return Err(OAuthError::grant("PKCE verification failed"));
    }
    if !resource_ok(form.get("resource"), &row.resource) || row.resource != e.resource {
        tx.commit().await?;
        return Err(OAuthError::bad("invalid_target", "resource must be this server's MCP endpoint"));
    }
    let out = issue(&mut tx, grant_id).await?;
    tx.commit().await?;
    touch_client(st, &client.id).await;
    Ok(out)
}

#[derive(sqlx::FromRow)]
struct RefreshRow {
    grant_id: Uuid,
    client_id: Option<String>,
    rotated: bool,
    expired: bool,
    revoked: bool,
}

async fn refresh(st: &AppState, e: &Endpoints, headers: &HeaderMap, form: &HashMap<String, String>) -> Result<Value, OAuthError> {
    let client = authenticate_client(st, e, headers, form).await?;
    let rt = form.get("refresh_token").ok_or_else(|| OAuthError::bad("invalid_request", "refresh_token is required"))?;
    let mut tx = st.db.begin().await?;
    let row: Option<RefreshRow> = sqlx::query_as(
        "SELECT o.grant_id, t.client_id, o.rotated_at IS NOT NULL AS rotated, o.expires_at < now() AS expired,
                (t.revoked_at IS NOT NULL OR (t.expires_at IS NOT NULL AND t.expires_at < now())) AS revoked
         FROM oauth_tokens o JOIN api_tokens t ON t.id = o.grant_id
         WHERE o.token_hash = $1 AND o.kind = 'refresh' FOR UPDATE OF o",
    )
    .bind(tokens::digest(rt))
    .fetch_optional(&mut *tx)
    .await?;
    let row = row.ok_or_else(|| OAuthError::grant("unknown refresh token"))?;
    if row.client_id.as_deref() != Some(client.id.as_str()) {
        return Err(OAuthError::grant("refresh token was issued to another client"));
    }
    if row.rotated {
        // Reuse of a rotated refresh token: assume theft and revoke the grant.
        revoke_grant(&mut *tx, row.grant_id).await?;
        tx.commit().await?;
        return Err(OAuthError::grant("refresh token was already used; the connection has been revoked"));
    }
    if row.expired || row.revoked {
        return Err(OAuthError::grant("refresh token expired or the connection was revoked"));
    }
    sqlx::query("UPDATE oauth_tokens SET rotated_at = now() WHERE token_hash = $1").bind(tokens::digest(rt)).execute(&mut *tx).await?;
    let out = issue(&mut tx, row.grant_id).await?;
    tx.commit().await?;
    touch_client(st, &client.id).await;
    Ok(out)
}

/// New access + refresh token pair for a grant.
async fn issue(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, grant_id: Uuid) -> Result<Value, OAuthError> {
    let access = tokens::new_oauth_access();
    let refresh = tokens::new_oauth_refresh();
    sqlx::query("INSERT INTO oauth_tokens (token_hash, grant_id, kind, expires_at) VALUES ($1, $2, 'access', now() + make_interval(secs => $3))")
        .bind(tokens::digest(&access))
        .bind(grant_id)
        .bind(ACCESS_TTL_SECS as f64)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO oauth_tokens (token_hash, grant_id, kind, expires_at)
         SELECT $1, id, 'refresh', LEAST(now() + make_interval(days => $3), COALESCE(expires_at, 'infinity')) FROM api_tokens WHERE id = $2",
    )
    .bind(tokens::digest(&refresh))
    .bind(grant_id)
    .bind(REFRESH_TTL_DAYS)
    .execute(&mut **tx)
    .await?;
    let scopes: Vec<String> = sqlx::query_scalar("SELECT scopes FROM api_tokens WHERE id = $1").bind(grant_id).fetch_one(&mut **tx).await?;
    Ok(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_TTL_SECS,
        "refresh_token": refresh,
        "scope": scopes.join(" "),
    }))
}

async fn touch_client(st: &AppState, id: &str) {
    let _ = sqlx::query("UPDATE oauth_clients SET last_used_at = now() WHERE id = $1").bind(id).execute(&st.db).await;
}

/// Token revocation (RFC 7009): revoking either token ends the whole grant.
/// Always answers 200, as the RFC requires for unknown tokens.
async fn revoke(State(st): State<AppState>, axum::Form(form): axum::Form<HashMap<String, String>>) -> Response {
    if endpoints(&st).is_none() {
        return disabled();
    }
    if let Some(t) = form.get("token") {
        let grant: Option<Uuid> =
            sqlx::query_scalar("SELECT grant_id FROM oauth_tokens WHERE token_hash = $1").bind(tokens::digest(t)).fetch_optional(&st.db).await.ok().flatten();
        if let Some(g) = grant {
            let _ = revoke_grant(&st.db, g).await;
        }
    }
    no_store(StatusCode::OK.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uri_rules() {
        assert!(valid_redirect_uri("https://claude.ai/api/mcp/auth_callback"));
        assert!(valid_redirect_uri("http://localhost:6274/oauth/callback"));
        assert!(valid_redirect_uri("http://127.0.0.1/cb"));
        assert!(!valid_redirect_uri("http://evil.example/cb"));
        assert!(!valid_redirect_uri("https://a.example/cb#frag"));
        assert!(!valid_redirect_uri("javascript:alert(1)"));
        assert!(redirect_matches("http://127.0.0.1:1234/cb", "http://127.0.0.1:5555/cb"));
        assert!(!redirect_matches("https://a.example/cb", "https://a.example/cb2"));
    }

    #[test]
    fn pkce_matches_rfc_7636_example() {
        assert_eq!(pkce_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn basic_credentials_are_percent_decoded() {
        assert_eq!(pct_decode("a%3Ab+c"), "a:b c");
    }
}
