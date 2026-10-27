//! The owner's side of an OAuth authorization: the SPA consent page loads the
//! pending request and approves (with the scopes the owner picked) or denies.
//! See `crate::oauth` for the protocol endpoints.

use axum::Json;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_OAUTH;
use crate::auth::{Principal, Scope, tokens};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::oauth::{self, CODE_TTL_SECS, redirect_with};
use crate::repo;
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_request, decide))
}

#[derive(Serialize, ToSchema)]
pub struct AuthRequestView {
    pub id: Uuid,
    pub client_id: String,
    pub client_name: String,
    /// `dcr` (registered itself) or `cimd` (identified by a metadata URL).
    pub client_source: String,
    pub redirect_uri: String,
    /// Where the browser goes after approval; show it so the owner can spot impostors.
    pub redirect_host: String,
    pub requested_scopes: Vec<String>,
    pub resource: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RequestRow {
    id: Uuid,
    client_id: String,
    client_name: String,
    client_source: String,
    redirect_uri: String,
    state: Option<String>,
    scopes_requested: Vec<String>,
    resource: String,
    expires_at: DateTime<Utc>,
}

const PENDING: &str =
    "SELECT r.id, r.client_id, c.name AS client_name, c.source AS client_source, r.redirect_uri, r.state, r.scopes_requested, r.resource, r.expires_at
     FROM oauth_auth_requests r JOIN oauth_clients c ON c.id = r.client_id
     WHERE r.id = $1 AND r.code_hash IS NULL AND r.used_at IS NULL AND r.expires_at > now()";

fn host_of(uri: &str) -> String {
    reqwest::Url::parse(uri).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default()
}

fn gate(st: &AppState, p: &Principal) -> ApiResult<oauth::Endpoints> {
    p.require_owner_session()?;
    oauth::endpoints(st).ok_or_else(|| ApiError::not_found("MCP 設定（尚未啟用）"))
}

/// A pending authorization request, for the consent page.
#[utoipa::path(get, path = "/oauth/requests/{id}", tag = TAG_OAUTH, params(("id" = Uuid, Path)),
    responses((status = 200, body = AuthRequestView), (status = 404, body = crate::error::ProblemDetails)), security(("session" = [])))]
async fn get_request(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<Json<AuthRequestView>> {
    gate(&st, &p)?;
    let r: RequestRow = sqlx::query_as(PENDING)
        .bind(id)
        .fetch_optional(&st.db)
        .await?
        .ok_or_else(|| ApiError::not_found("這個授權請求（可能已過期，請回到應用程式重新連線）"))?;
    Ok(Json(AuthRequestView {
        id: r.id,
        redirect_host: host_of(&r.redirect_uri),
        client_id: r.client_id,
        client_name: r.client_name,
        client_source: r.client_source,
        redirect_uri: r.redirect_uri,
        requested_scopes: r.scopes_requested,
        resource: r.resource,
        expires_at: r.expires_at,
    }))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthDecision {
    pub approve: bool,
    /// Scopes to grant (may differ from what the client asked for). Required when approving.
    #[serde(default)]
    pub scopes: Vec<Scope>,
}

#[derive(Serialize, ToSchema)]
pub struct AuthDecisionResult {
    /// Navigate the browser here (the client's redirect URI with a code or an error).
    pub redirect_to: String,
}

/// Approve or deny an authorization request. Approving replaces any earlier
/// connection of the same app, so reconnecting never piles up grants.
#[utoipa::path(post, path = "/oauth/requests/{id}", tag = TAG_OAUTH, params(("id" = Uuid, Path)), request_body = AuthDecision,
    responses((status = 200, body = AuthDecisionResult), (status = 404, body = crate::error::ProblemDetails), (status = 422, body = crate::error::ProblemDetails)),
    security(("session" = [])))]
async fn decide(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>, JsonBody(req): JsonBody<AuthDecision>) -> ApiResult<Json<AuthDecisionResult>> {
    let e = gate(&st, &p)?;
    let mut tx = st.db.begin().await?;
    let r: RequestRow = sqlx::query_as(&format!("{PENDING} FOR UPDATE OF r"))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("這個授權請求（可能已過期，請回到應用程式重新連線）"))?;
    let mut params: Vec<(&str, &str)> = vec![("iss", e.issuer.as_str())];
    if let Some(s) = &r.state {
        params.push(("state", s));
    }

    if !req.approve {
        sqlx::query("UPDATE oauth_auth_requests SET used_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        repo::audit(&st, &p, "oauth.deny", &r.client_name, serde_json::json!({"client_id": r.client_id})).await;
        params.push(("error", "access_denied"));
        return Ok(Json(AuthDecisionResult { redirect_to: redirect_with(&r.redirect_uri, &params) }));
    }

    if req.scopes.is_empty() {
        return Err(ApiError::validation(vec![ccal_core::problem::FieldError::new("/scopes", "required", "至少選擇一個權限")]));
    }
    let mut scopes: Vec<String> = req.scopes.iter().map(|s| s.as_str().to_string()).collect();
    scopes.sort();
    scopes.dedup();
    sqlx::query("UPDATE api_tokens SET revoked_at = now() WHERE owner_id = $1 AND client_id = $2 AND kind = 'oauth' AND revoked_at IS NULL")
        .bind(p.user_id)
        .bind(&r.client_id)
        .execute(&mut *tx)
        .await?;
    let grant_id = Uuid::now_v7();
    sqlx::query("INSERT INTO api_tokens (id, owner_id, name, scopes, kind, client_id) VALUES ($1, $2, $3, $4, 'oauth', $5)")
        .bind(grant_id)
        .bind(p.user_id)
        .bind(&r.client_name)
        .bind(&scopes)
        .bind(&r.client_id)
        .execute(&mut *tx)
        .await?;
    let code = tokens::random_token(32);
    sqlx::query("UPDATE oauth_auth_requests SET user_id = $2, grant_id = $3, code_hash = $4, expires_at = now() + make_interval(secs => $5) WHERE id = $1")
        .bind(id)
        .bind(p.user_id)
        .bind(grant_id)
        .bind(tokens::digest(&code))
        .bind(CODE_TTL_SECS as f64)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    repo::audit(&st, &p, "oauth.grant", &r.client_name, serde_json::json!({"client_id": r.client_id, "grant_id": grant_id, "scopes": scopes})).await;
    params.push(("code", &code));
    Ok(Json(AuthDecisionResult { redirect_to: redirect_with(&r.redirect_uri, &params) }))
}
