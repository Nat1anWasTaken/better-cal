use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

use crate::error::ApiError;
use crate::routes;
use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "CYCU-CAL Judge API",
        version = "1.0.0",
        description = "C 語言教學與自動評測平台 API。\n\n\
            - 學生使用瀏覽器 session（HttpOnly cookie）；所有非 GET 請求需帶 `X-CSRF-Token`（由 `GET /api/v1/auth/me` 取得）。\n\
            - Agent / 腳本使用擁有者建立的 API token：`Authorization: Bearer ccal_…`，權限依 scope 限制。\n\
            - 錯誤一律為 RFC 9457 `application/problem+json`，欄位問題在 `errors[].pointer`（JSON Pointer）。\n\
            - 建立類請求可帶 `Idempotency-Key` 以安全重試。\n\
            - 詳細流程見 docs/api-guide.md。"
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Login, logout, current user, password"),
        (name = "student", description = "What students see: sets, problems, drafts, progress, leaderboard"),
        (name = "submissions", description = "Run / Submit and results with teaching cards"),
        (name = "playground", description = "Free-form C: compile and run any program, with teaching cards"),
        (name = "admin: problems", description = "Problem bank with immutable revisions"),
        (name = "admin: problem sets", description = "Ordered sets of bank problems; active set; leaderboard rounds"),
        (name = "admin: import/export", description = "JSON import (cycu-cal.import/v1) with dry-run"),
        (name = "admin: teaching text", description = "Online editing of diagnostic explanations"),
        (name = "admin: students", description = "Accounts, cohorts, overrides, progress"),
        (name = "admin: system", description = "Settings, API tokens, MCP, audit log, submissions"),
        (name = "oauth", description = "Owner consent for MCP clients (the OAuth protocol endpoints live under /oauth and /.well-known)")
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let c = openapi.components.get_or_insert_with(Default::default);
        c.add_security_scheme("token", SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).bearer_format("ccal_<secret>").build()));
        c.add_security_scheme("session", SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(crate::auth::SESSION_COOKIE))));
    }
}

pub fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi()).nest(
        "/api/v1",
        OpenApiRouter::new()
            .merge(routes::auth::router())
            .merge(routes::student::router())
            .merge(routes::leaderboard::router())
            .merge(routes::submissions::router())
            .merge(routes::playground::router())
            .merge(routes::admin_problems::router())
            .merge(routes::admin_sets::router())
            .merge(routes::admin_students::router())
            .merge(routes::admin_catalog::router())
            .merge(routes::admin_system::router())
            .merge(routes::oauth_consent::router()),
    )
}

pub fn openapi_json() -> String {
    let (_, api) = api_router().split_for_parts();
    api.to_pretty_json().expect("openapi serializes")
}

async fn security_headers(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    if path.starts_with("/api/") || path.starts_with("/oauth/") || path == "/mcp" {
        h.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-store"));
    }
    if !path.starts_with("/api/docs") {
        h.insert(
            "content-security-policy",
            HeaderValue::from_static("default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"),
        );
    }
    res
}

async fn api_not_found() -> ApiError {
    ApiError::not_found("這個 API 路徑")
}

pub fn build(state: AppState) -> Router {
    let (router, api) = api_router().split_for_parts();
    let spec = api.clone();
    // MCP tool calls run through the API router in-process.
    let mcp_api = router.clone().with_state(state.clone());
    // Machine-to-machine endpoints may be called from browser-based MCP
    // clients (e.g. MCP Inspector). They never use cookies, so any origin is fine.
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("mcp-protocol-version"),
            HeaderName::from_static("mcp-session-id"),
        ])
        .expose_headers([header::WWW_AUTHENTICATE]);
    let mut app = router
        .merge(crate::oauth::router().layer(cors.clone()))
        .merge(crate::mcp::router(mcp_api).layer(cors))
        .route("/api/v1/openapi.json", axum::routing::get(move || async move { axum::Json(spec.clone()) }))
        .merge(Scalar::with_url("/api/docs", api))
        .route("/api/{*rest}", axum::routing::any(api_not_found))
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .with_state(state.clone());
    if let Some(dir) = &state.cfg.static_dir {
        let index = format!("{dir}/index.html");
        // `.fallback` (not `.not_found_service`) so client-side routes get 200.
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }
    app.layer(middleware::from_fn(security_headers))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(16 * 1024 * 1024))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
}
