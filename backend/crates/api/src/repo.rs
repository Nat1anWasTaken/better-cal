//! Shared queries and small services used by several route modules.

use ccal_core::policy::{EffectivePolicy, PolicyPatch, effective};
use ccal_core::problem::ProblemContent;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::Principal;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Effective policy for a user (individual > cohort > site).
pub async fn policy_for(st: &AppState, user_id: Uuid) -> ApiResult<EffectivePolicy> {
    let snap = st.snapshot();
    let row: Option<(Option<serde_json::Value>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT c.policy, o.policy FROM users u LEFT JOIN cohorts c ON c.id = u.cohort_id LEFT JOIN student_overrides o ON o.user_id = u.id WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_optional(&st.db)
    .await?;
    let (cohort, student) = row.unwrap_or((None, None));
    let parse = |v: Option<serde_json::Value>| v.and_then(|v| serde_json::from_value::<PolicyPatch>(v).ok());
    let cohort = parse(cohort);
    let student = parse(student);
    Ok(effective(&snap.settings.default_policy, cohort.as_ref(), student.as_ref(), snap.active_set_id))
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct ProblemRow {
    pub id: Uuid,
    pub slug: String,
    pub status: String,
    pub head_revision_id: Option<Uuid>,
    pub published_revision_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Accepts a UUID or a slug.
pub async fn find_problem(db: &PgPool, id_or_slug: &str) -> ApiResult<ProblemRow> {
    let q = "SELECT id, slug, status, head_revision_id, published_revision_id, created_at, updated_at FROM problems";
    let row: Option<ProblemRow> = match Uuid::parse_str(id_or_slug) {
        Ok(id) => sqlx::query_as(&format!("{q} WHERE id = $1")).bind(id).fetch_optional(db).await?,
        Err(_) => sqlx::query_as(&format!("{q} WHERE slug = $1")).bind(id_or_slug).fetch_optional(db).await?,
    };
    row.ok_or_else(|| ApiError::not_found("這個題目"))
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct RevisionRow {
    pub id: Uuid,
    pub problem_id: Uuid,
    pub number: i32,
    pub content: serde_json::Value,
    pub content_hash: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl RevisionRow {
    pub fn content(&self) -> ApiResult<ProblemContent> {
        serde_json::from_value(self.content.clone()).map_err(ApiError::internal)
    }
}

pub async fn revision(db: &PgPool, id: Uuid) -> ApiResult<RevisionRow> {
    sqlx::query_as("SELECT id, problem_id, number, content, content_hash, created_at FROM problem_revisions WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| ApiError::not_found("這個題目版本"))
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SetRow {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description_md: String,
    pub learned_concepts: Vec<String>,
    pub listed: bool,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub const SET_SELECT: &str = "SELECT id, slug, title, description_md, learned_concepts, listed, status, created_at, updated_at FROM problem_sets";

pub async fn find_set(db: &PgPool, id_or_slug: &str) -> ApiResult<SetRow> {
    let row: Option<SetRow> = match Uuid::parse_str(id_or_slug) {
        Ok(id) => sqlx::query_as(&format!("{SET_SELECT} WHERE id = $1")).bind(id).fetch_optional(db).await?,
        Err(_) => sqlx::query_as(&format!("{SET_SELECT} WHERE slug = $1")).bind(id_or_slug).fetch_optional(db).await?,
    };
    row.ok_or_else(|| ApiError::not_found("這個題目集"))
}

/// Can this principal (as a student) see the published problem through some
/// visible, published set? Owners can see everything.
pub async fn student_can_see_problem(st: &AppState, p: &Principal, policy: &EffectivePolicy, problem_id: Uuid) -> ApiResult<bool> {
    if p.is_owner() {
        return Ok(true);
    }
    let sets: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT s.id, s.listed FROM problem_set_items i JOIN problem_sets s ON s.id = i.set_id WHERE i.problem_id = $1 AND s.status = 'published'",
    )
    .bind(problem_id)
    .fetch_all(&st.db)
    .await?;
    Ok(sets.iter().any(|(id, listed)| policy.can_see_set(*id, *listed)))
}

pub async fn audit(st: &AppState, p: &Principal, action: &str, target: &str, detail: serde_json::Value) {
    let r = sqlx::query("INSERT INTO audit_log (actor_user_id, actor_token_id, action, target, detail) VALUES ($1, $2, $3, $4, $5)")
        .bind(p.user_id)
        .bind(p.token_id)
        .bind(action)
        .bind(target)
        .bind(detail)
        .execute(&st.db)
        .await;
    if let Err(e) = r {
        tracing::warn!(%e, "audit log write failed");
    }
}

/// Opaque offset cursor (documented as opaque; clients must not parse it).
pub fn decode_cursor(c: Option<&str>) -> i64 {
    use base64::Engine;
    c.and_then(|c| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(c).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.strip_prefix("o:").and_then(|n| n.parse().ok()))
        .unwrap_or(0)
}

pub fn encode_cursor(offset: i64) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!("o:{offset}"))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Pass as `cursor` to get the next page; absent on the last page.
    pub next_cursor: Option<String>,
}

pub fn page<T>(mut items: Vec<T>, limit: i64, offset: i64) -> Page<T> {
    let more = items.len() as i64 > limit;
    items.truncate(limit as usize);
    Page { items, next_cursor: more.then(|| encode_cursor(offset + limit)) }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PageParams {
    /// 1–100, default 20.
    pub limit: Option<i64>,
    /// Opaque cursor from `next_cursor`.
    pub cursor: Option<String>,
}

impl PageParams {
    pub fn resolve(&self) -> (i64, i64) {
        (self.limit.unwrap_or(20).clamp(1, 100), decode_cursor(self.cursor.as_deref()))
    }
}

pub fn request_hash(body: &impl Serialize) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(body).unwrap_or_default()))
}

/// Idempotency-Key support for owner writes: the first response for a key is
/// stored and replayed for retries with the same body; a different body with
/// the same key is rejected.
pub async fn idempotent<F, Fut>(
    st: &AppState,
    p: &Principal,
    key: Option<&str>,
    body_hash: &str,
    f: F,
) -> ApiResult<(axum::http::StatusCode, serde_json::Value)>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ApiResult<(axum::http::StatusCode, serde_json::Value)>>,
{
    let Some(key) = key else { return f().await };
    if key.is_empty() || key.len() > 200 {
        return Err(ApiError::bad_request("invalid_idempotency_key", "Idempotency-Key 長度須為 1–200"));
    }
    let principal = p.key();
    // Serialise concurrent retries of the same key.
    let lock_id: i64 = i64::from_be_bytes(Sha256::digest(format!("{principal}|{key}").as_bytes())[..8].try_into().unwrap());
    let mut conn = st.db.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)").bind(lock_id).execute(&mut *conn).await?;
    let result = async {
        let existing: Option<(String, i32, serde_json::Value)> =
            sqlx::query_as("SELECT request_hash, status_code, response FROM idempotency_keys WHERE principal = $1 AND key = $2")
                .bind(&principal)
                .bind(key)
                .fetch_optional(&st.db)
                .await?;
        if let Some((h, code, resp)) = existing {
            if h != body_hash {
                return Err(ApiError::conflict("idempotency_key_reused", "這個 Idempotency-Key 已用於內容不同的請求"));
            }
            return Ok((axum::http::StatusCode::from_u16(code as u16).unwrap_or(axum::http::StatusCode::OK), resp));
        }
        let (code, resp) = f().await?;
        sqlx::query("INSERT INTO idempotency_keys (principal, key, request_hash, status_code, response) VALUES ($1, $2, $3, $4, $5)")
            .bind(&principal)
            .bind(key)
            .bind(body_hash)
            .bind(code.as_u16() as i32)
            .bind(&resp)
            .execute(&st.db)
            .await?;
        Ok((code, resp))
    }
    .await;
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)").bind(lock_id).execute(&mut *conn).await;
    result
}

pub fn idempotency_key(headers: &axum::http::HeaderMap) -> Option<String> {
    headers.get("idempotency-key").and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string())
}
