use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Method, header};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::SESSION_COOKIE;
use super::tokens::{constant_time_eq, digest};
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Student,
    Owner,
}

/// Least-privilege permissions for API tokens. Owner sessions hold all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Hash, PartialOrd, Ord)]
pub enum Scope {
    #[serde(rename = "problems:read")]
    ProblemsRead,
    #[serde(rename = "problems:write")]
    ProblemsWrite,
    #[serde(rename = "sets:write")]
    SetsWrite,
    /// Publish/unpublish problems and sets, and switch the active set.
    #[serde(rename = "publish")]
    Publish,
    #[serde(rename = "submissions:read")]
    SubmissionsRead,
    /// Edit teaching text (diagnostics catalog).
    #[serde(rename = "content:write")]
    ContentWrite,
    #[serde(rename = "students:read")]
    StudentsRead,
    #[serde(rename = "students:write")]
    StudentsWrite,
    #[serde(rename = "settings:write")]
    SettingsWrite,
}

impl Scope {
    pub const ALL: &[Scope] = &[
        Scope::ProblemsRead,
        Scope::ProblemsWrite,
        Scope::SetsWrite,
        Scope::Publish,
        Scope::SubmissionsRead,
        Scope::ContentWrite,
        Scope::StudentsRead,
        Scope::StudentsWrite,
        Scope::SettingsWrite,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::ProblemsRead => "problems:read",
            Scope::ProblemsWrite => "problems:write",
            Scope::SetsWrite => "sets:write",
            Scope::Publish => "publish",
            Scope::SubmissionsRead => "submissions:read",
            Scope::ContentWrite => "content:write",
            Scope::StudentsRead => "students:read",
            Scope::StudentsWrite => "students:write",
            Scope::SettingsWrite => "settings:write",
        }
    }
    pub fn parse(s: &str) -> Option<Scope> {
        Scope::ALL.iter().copied().find(|x| x.as_str() == s)
    }
}

#[derive(Debug, Clone)]
pub struct Principal {
    /// The acting user (for tokens: the owner who created the token).
    pub user_id: Uuid,
    pub role: Role,
    pub token_id: Option<Uuid>,
    pub token_scopes: Vec<Scope>,
    /// Hash of the session token (cookie sessions only).
    pub session_hash: Option<Vec<u8>>,
}

impl Principal {
    pub fn is_owner(&self) -> bool {
        self.role == Role::Owner
    }

    /// Owner-only operation that additionally needs `scope` when using a token.
    pub fn require(&self, scope: Scope) -> Result<(), ApiError> {
        if self.role != Role::Owner {
            return Err(ApiError::forbidden("需要擁有者權限"));
        }
        if self.token_id.is_some() && !self.token_scopes.contains(&scope) {
            return Err(ApiError::forbidden(format!("這個 API token 沒有 `{}` 權限", scope.as_str())));
        }
        Ok(())
    }

    /// Operations only an owner's browser session may perform (managing tokens).
    pub fn require_owner_session(&self) -> Result<(), ApiError> {
        if self.role != Role::Owner || self.token_id.is_some() {
            return Err(ApiError::forbidden("只有擁有者登入後才能執行（API token 不能管理 token）"));
        }
        Ok(())
    }

    /// Key used for idempotency records and rate limits.
    pub fn key(&self) -> String {
        match self.token_id {
            Some(t) => format!("token:{t}"),
            None => format!("user:{}", self.user_id),
        }
    }
}

fn cookie_value(parts: &Parts, name: &str) -> Option<String> {
    parts.headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    user_id: Uuid,
    csrf_token: String,
    role: String,
    disabled: bool,
    stale: bool,
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    id: Uuid,
    owner_id: Uuid,
    scopes: Vec<String>,
    role: String,
    disabled: bool,
}

impl FromRequestParts<AppState> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if let Some(p) = parts.extensions.get::<Principal>() {
            return Ok(p.clone());
        }
        let principal = authenticate(parts, state).await?;
        parts.extensions.insert(principal.clone());
        Ok(principal)
    }
}

/// Optional principal (for endpoints usable anonymously).
pub struct MaybePrincipal(pub Option<Principal>);

impl FromRequestParts<AppState> for MaybePrincipal {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        match Principal::from_request_parts(parts, state).await {
            Ok(p) => Ok(MaybePrincipal(Some(p))),
            Err(e) if e.status == axum::http::StatusCode::UNAUTHORIZED => Ok(MaybePrincipal(None)),
            Err(e) => Err(e),
        }
    }
}

async fn authenticate(parts: &Parts, state: &AppState) -> Result<Principal, ApiError> {
    if let Some(auth) = parts.headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        let token = auth.strip_prefix("Bearer ").ok_or_else(ApiError::unauthorized)?.trim();
        let row: Option<TokenRow> = sqlx::query_as(
            "SELECT t.id, t.owner_id, t.scopes, u.role, u.disabled FROM api_tokens t JOIN users u ON u.id = t.owner_id
             WHERE t.token_hash = $1 AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at > now())",
        )
        .bind(digest(token))
        .fetch_optional(&state.db)
        .await?;
        let row = row.ok_or_else(ApiError::unauthorized)?;
        if row.disabled || row.role != "owner" {
            return Err(ApiError::unauthorized());
        }
        // Best effort; not worth failing the request over.
        let _ =
            sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1 AND (last_used_at IS NULL OR last_used_at < now() - interval '1 minute')")
                .bind(row.id)
                .execute(&state.db)
                .await;
        return Ok(Principal {
            user_id: row.owner_id,
            role: Role::Owner,
            token_id: Some(row.id),
            token_scopes: row.scopes.iter().filter_map(|s| Scope::parse(s)).collect(),
            session_hash: None,
        });
    }

    let token = cookie_value(parts, SESSION_COOKIE).ok_or_else(ApiError::unauthorized)?;
    let hash = digest(&token);
    let row: Option<SessionRow> = sqlx::query_as(
        "SELECT s.user_id, s.csrf_token, u.role, u.disabled, (s.last_seen_at < now() - interval '5 minutes') AS stale
         FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = $1 AND s.expires_at > now()",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;
    let row = row.ok_or_else(ApiError::unauthorized)?;
    if row.disabled {
        return Err(ApiError::unauthorized());
    }

    // CSRF: state-changing requests with a cookie must echo the session's
    // CSRF token in a header (and come from an allowed origin, if configured).
    if !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
        let sent = parts.headers.get("x-csrf-token").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !constant_time_eq(sent.as_bytes(), row.csrf_token.as_bytes()) {
            return Err(ApiError::forbidden("缺少或錯誤的 CSRF token（X-CSRF-Token 標頭）"));
        }
        if let Some(origin) = parts.headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
            && !state.cfg.public_origins.is_empty()
            && !state.cfg.public_origins.iter().any(|o| o == origin.trim_end_matches('/'))
        {
            return Err(ApiError::forbidden("不允許的來源（Origin）"));
        }
    }

    if row.stale {
        let ttl = state.cfg.session_ttl.as_secs() as i64;
        let _ = sqlx::query("UPDATE sessions SET last_seen_at = now(), expires_at = now() + make_interval(secs => $2) WHERE token_hash = $1")
            .bind(&hash)
            .bind(ttl as f64)
            .execute(&state.db)
            .await;
    }
    Ok(Principal {
        user_id: row.user_id,
        role: if row.role == "owner" { Role::Owner } else { Role::Student },
        token_id: None,
        token_scopes: vec![],
        session_hash: Some(hash),
    })
}
