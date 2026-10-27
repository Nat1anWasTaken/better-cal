//! Site settings, API tokens, audit log, all submissions, dashboard overview.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_SYSTEM;
use super::submissions::{SUMMARY_SELECT, SummaryRow, load_row, render_view};
use crate::auth::{Principal, Scope, tokens};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::judge::view::{Audience, SubmissionView};
use crate::repo::{self, Page};
use crate::settings::SiteSettings;
use crate::state::{AppState, JudgeHealth};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_settings, put_settings))
        .routes(routes!(list_tokens, create_token))
        .routes(routes!(revoke_token))
        .routes(routes!(mcp_status))
        .routes(routes!(audit_log))
        .routes(routes!(list_submissions))
        .routes(routes!(get_submission))
        .routes(routes!(overview))
}

#[derive(Serialize, ToSchema)]
pub struct SettingsView {
    pub settings: SiteSettings,
    pub active_set_id: Option<Uuid>,
    pub version: i64,
}

/// Current site settings.
#[utoipa::path(get, path = "/admin/settings", tag = TAG_SYSTEM, responses((status = 200, body = SettingsView)), security(("token" = ["settings:write"]), ("session" = [])))]
async fn get_settings(State(st): State<AppState>, p: Principal) -> ApiResult<Json<SettingsView>> {
    p.require(Scope::SettingsWrite)?;
    let snap = st.snapshot();
    Ok(Json(SettingsView { settings: snap.settings.clone(), active_set_id: snap.active_set_id, version: snap.version }))
}

/// Replace site settings. Applies immediately without a restart.
/// The rule that hidden tests are never shown to students is not configurable.
#[utoipa::path(put, path = "/admin/settings", tag = TAG_SYSTEM, request_body = SiteSettings,
    responses((status = 200, body = SettingsView), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["settings:write"]), ("session" = [])))]
async fn put_settings(State(st): State<AppState>, p: Principal, JsonBody(s): JsonBody<SiteSettings>) -> ApiResult<Json<SettingsView>> {
    p.require(Scope::SettingsWrite)?;
    let errors = s.validate();
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    let v = serde_json::to_value(&s).unwrap();
    sqlx::query("UPDATE site_config SET settings = $1 WHERE id = 1").bind(&v).execute(&st.db).await?;
    sqlx::query("INSERT INTO config_revisions (active_set_id, settings, created_by) SELECT active_set_id, settings, $1 FROM site_config WHERE id = 1")
        .bind(p.user_id)
        .execute(&st.db)
        .await?;
    st.config_changed().await?;
    repo::audit(&st, &p, "settings.update", "", v).await;
    get_settings(State(st), p).await
}

#[derive(Serialize, ToSchema)]
pub struct TokenInfo {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    /// `api` (a token you created) or `oauth` (an app connected over MCP).
    pub kind: String,
    /// The connected app, for `oauth` connections.
    pub client: Option<TokenClient>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema)]
pub struct TokenClient {
    pub id: String,
    pub name: String,
    /// `dcr` (registered itself) or `cimd` (identified by a metadata URL).
    pub source: String,
    pub redirect_hosts: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    id: Uuid,
    name: String,
    scopes: Vec<String>,
    kind: String,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    client_id: Option<String>,
    client_name: Option<String>,
    client_source: Option<String>,
    redirect_uris: Option<Vec<String>>,
}

/// API tokens you created and apps connected over MCP (secrets are never shown again).
#[utoipa::path(get, path = "/admin/tokens", tag = TAG_SYSTEM, responses((status = 200, body = Vec<TokenInfo>)), security(("session" = [])))]
async fn list_tokens(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Vec<TokenInfo>>> {
    p.require_owner_session()?;
    let rows: Vec<TokenRow> = sqlx::query_as(
        "SELECT t.id, t.name, t.scopes, t.kind, t.created_at, t.expires_at, t.revoked_at, t.last_used_at,
                c.id AS client_id, c.name AS client_name, c.source AS client_source, c.redirect_uris
         FROM api_tokens t LEFT JOIN oauth_clients c ON c.id = t.client_id WHERE t.owner_id = $1 ORDER BY t.created_at DESC",
    )
    .bind(p.user_id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| TokenInfo {
                client: r.client_id.map(|id| TokenClient {
                    id,
                    name: r.client_name.unwrap_or_default(),
                    source: r.client_source.unwrap_or_default(),
                    redirect_hosts: {
                        let mut h: Vec<String> = r
                            .redirect_uris
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|u| reqwest::Url::parse(u).ok().and_then(|u| u.host_str().map(str::to_string)))
                            .collect();
                        h.dedup();
                        h
                    },
                }),
                id: r.id,
                name: r.name,
                scopes: r.scopes,
                kind: r.kind,
                created_at: r.created_at,
                expires_at: r.expires_at,
                revoked_at: r.revoked_at,
                last_used_at: r.last_used_at,
            })
            .collect(),
    ))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateToken {
    pub name: String,
    /// Least privilege: grant only what the agent needs.
    pub scopes: Vec<Scope>,
    /// Days until expiry (1–365). Omit for no expiry.
    pub expires_in_days: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedToken {
    pub info: TokenInfo,
    /// The secret. Shown only once; store it securely. Use as `Authorization: Bearer <token>`.
    pub token: String,
}

/// Create an API token for an agent or script. Only an owner's browser
/// session can do this (tokens cannot mint tokens).
#[utoipa::path(post, path = "/admin/tokens", tag = TAG_SYSTEM, request_body = CreateToken,
    responses((status = 201, body = CreatedToken), (status = 422, body = crate::error::ProblemDetails)), security(("session" = [])))]
async fn create_token(State(st): State<AppState>, p: Principal, JsonBody(req): JsonBody<CreateToken>) -> ApiResult<(StatusCode, Json<CreatedToken>)> {
    p.require_owner_session()?;
    let mut errors = vec![];
    if req.name.trim().is_empty() || req.name.chars().count() > 80 {
        errors.push(ccal_core::problem::FieldError::new("/name", "invalid", "名稱需為 1–80 字"));
    }
    if req.scopes.is_empty() {
        errors.push(ccal_core::problem::FieldError::new("/scopes", "required", "至少選擇一個權限"));
    }
    if req.expires_in_days.is_some_and(|d| !(1..=365).contains(&d)) {
        errors.push(ccal_core::problem::FieldError::new("/expires_in_days", "out_of_range", "有效天數需為 1–365"));
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    let secret = tokens::new_api_token();
    let id = Uuid::now_v7();
    let mut scopes: Vec<String> = req.scopes.iter().map(|s| s.as_str().to_string()).collect();
    scopes.sort();
    scopes.dedup();
    sqlx::query(
        "INSERT INTO api_tokens (id, owner_id, name, token_hash, scopes, expires_at) VALUES ($1, $2, $3, $4, $5, now() + make_interval(days => $6::int))",
    )
    .bind(id)
    .bind(p.user_id)
    .bind(&req.name)
    .bind(tokens::digest(&secret))
    .bind(&scopes)
    .bind(req.expires_in_days.map(|d| d as i32))
    .execute(&st.db)
    .await?;
    repo::audit(&st, &p, "token.create", &req.name, serde_json::json!({"scopes": scopes})).await;
    let info = list_tokens(State(st.clone()), p).await?.0.into_iter().find(|t| t.id == id).unwrap();
    Ok((StatusCode::CREATED, Json(CreatedToken { info, token: secret })))
}

/// Revoke a token, or disconnect an MCP app, immediately.
#[utoipa::path(delete, path = "/admin/tokens/{id}", tag = TAG_SYSTEM, params(("id" = Uuid, Path)), responses((status = 204)), security(("session" = [])))]
async fn revoke_token(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    p.require_owner_session()?;
    let r = sqlx::query("UPDATE api_tokens SET revoked_at = now() WHERE id = $1 AND owner_id = $2 AND revoked_at IS NULL")
        .bind(id)
        .bind(p.user_id)
        .execute(&st.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("這個 token"));
    }
    repo::audit(&st, &p, "token.revoke", &id.to_string(), serde_json::json!({})).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct AuditEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub actor: Option<String>,
    pub via_token: Option<String>,
    pub action: String,
    pub target: String,
    #[schema(value_type = Object)]
    pub detail: serde_json::Value,
}

/// Who changed what (newest first).
#[utoipa::path(get, path = "/admin/audit", tag = TAG_SYSTEM, params(repo::PageParams), responses((status = 200, body = Page<AuditEntry>)), security(("token" = ["settings:write"]), ("session" = [])))]
async fn audit_log(State(st): State<AppState>, p: Principal, Query(pp): Query<repo::PageParams>) -> ApiResult<Json<Page<AuditEntry>>> {
    p.require(Scope::SettingsWrite)?;
    let (limit, offset) = pp.resolve();
    let rows: Vec<(i64, DateTime<Utc>, Option<String>, Option<String>, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT a.id, a.at, u.username, t.name, a.action, a.target, a.detail FROM audit_log a
           LEFT JOIN users u ON u.id = a.actor_user_id LEFT JOIN api_tokens t ON t.id = a.actor_token_id
          ORDER BY a.id DESC LIMIT $1 OFFSET $2",
    )
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&st.db)
    .await?;
    let items =
        rows.into_iter().map(|(id, at, actor, via_token, action, target, detail)| AuditEntry { id, at, actor, via_token, action, target, detail }).collect();
    Ok(Json(repo::page(items, limit, offset)))
}

#[derive(Serialize, ToSchema)]
pub struct McpStatus {
    /// The `mcp_enabled` site setting.
    pub enabled: bool,
    /// `PUBLIC_URL`; MCP cannot work without it.
    pub public_url: Option<String>,
    /// The URL to paste into Claude / ChatGPT (`{PUBLIC_URL}/mcp`).
    pub server_url: Option<String>,
    pub tools: Vec<McpToolInfo>,
    pub registered_clients: i64,
}

#[derive(Serialize, ToSchema)]
pub struct McpToolInfo {
    pub name: String,
    pub title: String,
    pub scopes: Vec<String>,
    pub read_only: bool,
    /// A hand-written workflow tool (the rest mirror REST operations).
    pub curated: bool,
}

/// MCP server status and the tools it offers.
#[utoipa::path(get, path = "/admin/mcp", tag = TAG_SYSTEM, responses((status = 200, body = McpStatus)), security(("session" = [])))]
async fn mcp_status(State(st): State<AppState>, p: Principal) -> ApiResult<Json<McpStatus>> {
    p.require_owner_session()?;
    let names = |s: &[Scope]| s.iter().map(|s| s.as_str().to_string()).collect::<Vec<_>>();
    let mut tools: Vec<McpToolInfo> = crate::mcp::curated::CURATED
        .iter()
        .map(|t| McpToolInfo { name: t.name.into(), title: t.title.into(), scopes: names(t.scopes), read_only: false, curated: true })
        .collect();
    tools.extend(crate::mcp::tools::CATALOG.iter().map(|t| McpToolInfo {
        name: t.name.clone(),
        title: t.title.clone(),
        scopes: names(&t.scopes),
        read_only: t.read_only(),
        curated: false,
    }));
    Ok(Json(McpStatus {
        enabled: st.snapshot().settings.mcp_enabled,
        public_url: st.cfg.public_url.clone(),
        server_url: st.cfg.mcp_url(),
        tools,
        registered_clients: sqlx::query_scalar("SELECT count(*) FROM oauth_clients").fetch_one(&st.db).await?,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct AdminSubmission {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub problem_slug: String,
    pub problem_title: String,
    pub revision_number: i32,
    pub kind: String,
    pub status: String,
    pub verdict: Option<String>,
    pub passed: Option<i32>,
    pub total: Option<i32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct SubmissionFilter {
    pub user_id: Option<Uuid>,
    pub problem_id: Option<Uuid>,
    pub verdict: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// All submissions (newest first).
#[utoipa::path(get, path = "/admin/submissions", tag = TAG_SYSTEM, params(SubmissionFilter), responses((status = 200, body = Page<AdminSubmission>)), security(("token" = ["submissions:read"]), ("session" = [])))]
async fn list_submissions(State(st): State<AppState>, p: Principal, Query(f): Query<SubmissionFilter>) -> ApiResult<Json<Page<AdminSubmission>>> {
    p.require(Scope::SubmissionsRead)?;
    let (limit, offset) = repo::PageParams { limit: f.limit, cursor: f.cursor.clone() }.resolve();
    let rows: Vec<SummaryRow> = sqlx::query_as(&format!(
        "{SUMMARY_SELECT} WHERE ($1::uuid IS NULL OR s.user_id = $1) AND ($2::uuid IS NULL OR s.problem_id = $2) AND ($3::text IS NULL OR s.verdict = $3)
           AND ($4::text IS NULL OR s.kind = $4) ORDER BY s.created_at DESC, s.id DESC LIMIT $5 OFFSET $6"
    ))
    .bind(f.user_id)
    .bind(f.problem_id)
    .bind(&f.verdict)
    .bind(&f.kind)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&st.db)
    .await?;
    let items = rows
        .into_iter()
        .map(|r| AdminSubmission {
            id: r.id,
            user_id: r.user_id,
            username: r.username,
            problem_slug: r.problem_slug,
            problem_title: r.problem_title,
            revision_number: r.revision_number,
            kind: r.kind,
            status: r.status,
            verdict: r.verdict,
            passed: r.passed,
            total: r.total,
            created_at: r.created_at,
        })
        .collect();
    Ok(Json(repo::page(items, limit, offset)))
}

/// Any submission with full detail (hidden tests, compiled source, judge errors)
/// and the exact problem revision it was judged against.
#[utoipa::path(get, path = "/admin/submissions/{id}", tag = TAG_SYSTEM, params(("id" = Uuid, Path)), responses((status = 200, body = SubmissionView)), security(("token" = ["submissions:read"]), ("session" = [])))]
async fn get_submission(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<Json<SubmissionView>> {
    p.require(Scope::SubmissionsRead)?;
    let row = load_row(&st, id).await?;
    Ok(Json(render_view(&st, &row, Audience::Owner, false).await?))
}

#[derive(Serialize, ToSchema)]
pub struct Overview {
    pub students: i64,
    pub submissions_today: i64,
    pub accepted_today: i64,
    pub queue_length: i64,
    pub system_errors_24h: i64,
    pub judge: JudgeHealth,
    #[schema(value_type = Option<Object>)]
    pub runner: Option<judge_proto::RunnerInfo>,
    pub draft_problems: i64,
    pub problems_with_unpublished_changes: i64,
    pub catalog_pending_changes: i64,
    pub active_set_id: Option<Uuid>,
}

/// Dashboard summary.
#[utoipa::path(get, path = "/admin/overview", tag = TAG_SYSTEM, responses((status = 200, body = Overview)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn overview(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Overview>> {
    p.require(Scope::ProblemsRead)?;
    let q = |sql: &'static str| sqlx::query_scalar::<_, i64>(sql).fetch_one(&st.db);
    let runner = st.runner.info().await.ok();
    Ok(Json(Overview {
        students: q("SELECT count(*) FROM users WHERE role = 'student' AND NOT disabled").await?,
        submissions_today: q("SELECT count(*) FROM submissions WHERE created_at > date_trunc('day', now()) AND kind IN ('run', 'submit')").await?,
        accepted_today: q("SELECT count(*) FROM submissions WHERE created_at > date_trunc('day', now()) AND kind = 'submit' AND verdict = 'accepted'").await?,
        queue_length: q("SELECT count(*) FROM submissions WHERE status IN ('queued', 'compiling', 'running')").await?,
        system_errors_24h: q("SELECT count(*) FROM submissions WHERE status = 'system_error' AND created_at > now() - interval '1 day'").await?,
        judge: if runner.is_some() { JudgeHealth::Ok } else { JudgeHealth::Down },
        runner,
        draft_problems: q("SELECT count(*) FROM problems WHERE status = 'draft'").await?,
        problems_with_unpublished_changes: q("SELECT count(*) FROM problems WHERE status = 'published' AND published_revision_id <> head_revision_id").await?,
        catalog_pending_changes: q("SELECT count(*) FROM catalog_entries WHERE draft IS NOT NULL OR draft_delete").await?,
        active_set_id: st.snapshot().active_set_id,
    }))
}
