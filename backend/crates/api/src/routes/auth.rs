use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_AUTH;
use crate::auth::principal::Role;
use crate::auth::{Principal, SESSION_COOKIE, password, tokens};
use crate::config::Config;
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::preferences::UserPreferences;
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(login)).routes(routes!(logout)).routes(routes!(me)).routes(routes!(change_password)).routes(routes!(put_preferences))
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
pub struct UserInfo {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub must_change_password: bool,
    pub preferences: UserPreferences,
}

#[derive(Serialize, ToSchema)]
pub struct Me {
    pub user: UserInfo,
    /// Send as `X-CSRF-Token` on every non-GET request made with the session cookie.
    pub csrf_token: Option<String>,
    /// Present when authenticated with an API token.
    pub token_scopes: Option<Vec<String>>,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    username: String,
    display_name: String,
    role: String,
    password_hash: String,
    disabled: bool,
    must_change_password: bool,
    preferences: serde_json::Value,
}

fn user_info(u: &UserRow) -> UserInfo {
    UserInfo {
        id: u.id,
        username: u.username.clone(),
        display_name: u.display_name.clone(),
        role: if u.role == "owner" { Role::Owner } else { Role::Student },
        must_change_password: u.must_change_password,
        preferences: UserPreferences::from_stored(u.preferences.clone()),
    }
}

fn session_cookie(st: &AppState, token: &str, max_age: i64) -> HeaderValue {
    let secure = if st.cfg.cookie_secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")).unwrap()
}

/// Client address as reported by the reverse proxy, for rate-limit keys.
/// Only proxy-written values are used: `CLIENT_IP_HEADER` when configured,
/// otherwise the last `X-Forwarded-For` hop (earlier hops are client-supplied).
/// A whole classroom behind one NAT shares this key, so the per-address limit
/// is generous; the per-account limit is what slows down password guessing.
pub(crate) fn client_ip(cfg: &Config, headers: &HeaderMap) -> String {
    let value = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let ip = match &cfg.client_ip_header {
        Some(h) => value(h),
        None => value("x-forwarded-for").and_then(|v| v.rsplit(',').next()),
    };
    ip.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("direct").to_string()
}

/// Log in with username and password; sets an HttpOnly session cookie.
#[utoipa::path(post, path = "/auth/login", tag = TAG_AUTH, request_body = LoginRequest,
    responses((status = 200, body = Me), (status = 401, body = crate::error::ProblemDetails), (status = 429, body = crate::error::ProblemDetails)))]
async fn login(State(st): State<AppState>, headers: HeaderMap, JsonBody(req): JsonBody<LoginRequest>) -> ApiResult<Response> {
    let uname = req.username.trim().to_lowercase();
    let ip = client_ip(&st.cfg, &headers);
    let window = std::time::Duration::from_secs(300);
    // The account-wide limit holds even when guesses come from many addresses.
    if !st.limiter.check(&format!("login:{ip}:{uname}"), 10, window).await
        || !st.limiter.check(&format!("login-user:{uname}"), 30, window).await
        || !st.limiter.check(&format!("login-ip:{ip}"), 300, window).await
    {
        return Err(ApiError::too_many("登入嘗試次數過多，請 5 分鐘後再試"));
    }
    let user: Option<UserRow> = sqlx::query_as(
        "SELECT id, username, display_name, role, password_hash, disabled, must_change_password, preferences FROM users WHERE lower(username) = $1",
    )
    .bind(&uname)
    .fetch_optional(&st.db)
    .await?;
    let ok = match &user {
        Some(u) => password::verify(&req.password, &u.password_hash) && !u.disabled,
        None => {
            password::verify(&req.password, password::dummy_hash());
            false
        }
    };
    let Some(user) = user.filter(|_| ok) else {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid_credentials", "帳號或密碼不正確"));
    };
    let token = tokens::random_token(32);
    let csrf = tokens::random_token(24);
    let ttl = st.cfg.session_ttl.as_secs() as i64;
    sqlx::query("INSERT INTO sessions (token_hash, user_id, csrf_token, expires_at) VALUES ($1, $2, $3, now() + make_interval(secs => $4))")
        .bind(tokens::digest(&token))
        .bind(user.id)
        .bind(&csrf)
        .bind(ttl as f64)
        .execute(&st.db)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()").execute(&st.db).await?;
    let body = Me { user: user_info(&user), csrf_token: Some(csrf), token_scopes: None };
    let mut res = Json(body).into_response();
    res.headers_mut().insert(header::SET_COOKIE, session_cookie(&st, &token, ttl));
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

/// End the current session.
#[utoipa::path(post, path = "/auth/logout", tag = TAG_AUTH, responses((status = 204)))]
async fn logout(State(st): State<AppState>, p: Principal) -> ApiResult<Response> {
    if let Some(h) = &p.session_hash {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1").bind(h).execute(&st.db).await?;
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::SET_COOKIE, session_cookie(&st, "", 0));
    Ok(res)
}

/// Current user, plus the CSRF token for cookie sessions.
#[utoipa::path(get, path = "/auth/me", tag = TAG_AUTH, responses((status = 200, body = Me), (status = 401, body = crate::error::ProblemDetails)))]
async fn me(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Me>> {
    let user: UserRow =
        sqlx::query_as("SELECT id, username, display_name, role, password_hash, disabled, must_change_password, preferences FROM users WHERE id = $1")
            .bind(p.user_id)
            .fetch_one(&st.db)
            .await?;
    let csrf: Option<String> = match &p.session_hash {
        Some(h) => sqlx::query_scalar("SELECT csrf_token FROM sessions WHERE token_hash = $1").bind(h).fetch_optional(&st.db).await?,
        None => None,
    };
    Ok(Json(Me { user: user_info(&user), csrf_token: csrf, token_scopes: p.token_id.map(|_| p.token_scopes.iter().map(|s| s.as_str().to_string()).collect()) }))
}

#[derive(Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Change your password. All of your other sessions are signed out.
#[utoipa::path(post, path = "/auth/password", tag = TAG_AUTH, request_body = ChangePasswordRequest,
    responses((status = 204), (status = 422, body = crate::error::ProblemDetails)))]
async fn change_password(State(st): State<AppState>, p: Principal, JsonBody(req): JsonBody<ChangePasswordRequest>) -> ApiResult<StatusCode> {
    if p.token_id.is_some() {
        return Err(ApiError::forbidden("API token 不能修改密碼"));
    }
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1").bind(p.user_id).fetch_one(&st.db).await?;
    if !password::verify(&req.current_password, &hash) {
        return Err(ApiError::validation(vec![ccal_core::problem::FieldError::new("/current_password", "incorrect", "目前的密碼不正確")]));
    }
    if let Err(msg) = password::check_strength(&req.new_password) {
        return Err(ApiError::validation(vec![ccal_core::problem::FieldError::new("/new_password", "weak", msg)]));
    }
    let new_hash = password::hash(&req.new_password)?;
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = $2, must_change_password = false, updated_at = now() WHERE id = $1")
        .bind(p.user_id)
        .bind(new_hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND token_hash <> $2")
        .bind(p.user_id)
        .bind(p.session_hash.clone().unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Replace your preferences (editor theme, keymap, font and layout).
#[utoipa::path(put, path = "/auth/preferences", tag = TAG_AUTH, request_body = UserPreferences,
    responses((status = 200, body = UserPreferences), (status = 422, body = crate::error::ProblemDetails)))]
async fn put_preferences(State(st): State<AppState>, p: Principal, JsonBody(prefs): JsonBody<UserPreferences>) -> ApiResult<Json<UserPreferences>> {
    if p.token_id.is_some() {
        return Err(ApiError::forbidden("API token 不能修改偏好設定"));
    }
    let errors = prefs.validate();
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    sqlx::query("UPDATE users SET preferences = $2, updated_at = now() WHERE id = $1")
        .bind(p.user_id)
        .bind(serde_json::to_value(&prefs).map_err(ApiError::internal)?)
        .execute(&st.db)
        .await?;
    Ok(Json(prefs))
}
