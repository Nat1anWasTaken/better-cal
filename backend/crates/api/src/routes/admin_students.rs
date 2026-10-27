//! Students, cohorts, per-student overrides and progress.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use ccal_core::policy::{EffectivePolicy, PolicyPatch, validate_patch};
use ccal_core::problem::FieldError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_STUDENTS;
use crate::auth::principal::Role;
use crate::auth::{Principal, Scope, password};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::repo::{self, Page};
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_users, create_user))
        .routes(routes!(batch_create))
        .routes(routes!(get_user, update_user))
        .routes(routes!(reset_password))
        .routes(routes!(revoke_sessions))
        .routes(routes!(get_policy, put_policy))
        .routes(routes!(list_cohorts, create_cohort))
        .routes(routes!(update_cohort, delete_cohort))
        .routes(routes!(progress_matrix))
        .routes(routes!(user_progress))
        .routes(routes!(credit, uncredit))
        .routes(routes!(rule_stats))
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct AdminUser {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub disabled: bool,
    pub must_change_password: bool,
    pub cohort_id: Option<Uuid>,
    pub cohort_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_submission_at: Option<DateTime<Utc>>,
}

const USER_SELECT: &str =
    "SELECT u.id, u.username, u.display_name, u.role, u.disabled, u.must_change_password, u.cohort_id, c.name AS cohort_name, u.created_at,
        (SELECT max(created_at) FROM submissions s WHERE s.user_id = u.id) AS last_submission_at
   FROM users u LEFT JOIN cohorts c ON c.id = u.cohort_id";

#[derive(Deserialize, utoipa::IntoParams)]
pub struct UserFilter {
    pub q: Option<String>,
    pub cohort_id: Option<Uuid>,
    /// `student` or `owner`.
    pub role: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// List accounts.
#[utoipa::path(get, path = "/admin/users", tag = TAG_STUDENTS, params(UserFilter), responses((status = 200, body = Page<AdminUser>)), security(("token" = ["students:read"]), ("session" = [])))]
async fn list_users(State(st): State<AppState>, p: Principal, Query(f): Query<UserFilter>) -> ApiResult<Json<Page<AdminUser>>> {
    p.require(Scope::StudentsRead)?;
    let (limit, offset) = repo::PageParams { limit: f.limit, cursor: f.cursor.clone() }.resolve();
    let q = f.q.as_ref().map(|q| format!("%{}%", q.replace('%', "\\%")));
    let rows: Vec<AdminUser> = sqlx::query_as(&format!(
        "{USER_SELECT} WHERE ($1::text IS NULL OR u.username ILIKE $1 OR u.display_name ILIKE $1) AND ($2::uuid IS NULL OR u.cohort_id = $2) AND ($3::text IS NULL OR u.role = $3)
         ORDER BY u.role DESC, u.username LIMIT $4 OFFSET $5"
    ))
    .bind(&q)
    .bind(f.cohort_id)
    .bind(&f.role)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(repo::page(rows, limit, offset)))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUser {
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    /// Defaults to `student`. Creating owners requires an owner session.
    pub role: Option<Role>,
    /// Omit to generate one (returned once).
    pub password: Option<String>,
    pub cohort_id: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedUser {
    pub user: AdminUser,
    /// Only present when the password was generated; shown once.
    pub initial_password: Option<String>,
}

fn valid_username(u: &str) -> bool {
    (2..=40).contains(&u.chars().count()) && u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

async fn insert_user(st: &AppState, req: &CreateUser, pointer: &str) -> ApiResult<CreatedUser> {
    let mut errors = vec![];
    if !valid_username(&req.username) {
        errors.push(FieldError::new(format!("{pointer}/username"), "invalid", "帳號需為 2–40 個英數字、`_`、`-` 或 `.`"));
    }
    if let Some(pw) = &req.password
        && let Err(m) = password::check_strength(pw)
    {
        errors.push(FieldError::new(format!("{pointer}/password"), "weak", m));
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    let generated = req.password.is_none().then(password::generate);
    let pw = req.password.clone().or(generated.clone()).unwrap();
    let id = Uuid::now_v7();
    let role = match req.role.unwrap_or(Role::Student) {
        Role::Owner => "owner",
        Role::Student => "student",
    };
    sqlx::query("INSERT INTO users (id, username, display_name, role, password_hash, must_change_password, cohort_id) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(id)
        .bind(&req.username)
        .bind(&req.display_name)
        .bind(role)
        .bind(password::hash(&pw)?)
        .bind(generated.is_some())
        .bind(req.cohort_id)
        .execute(&st.db)
        .await
        .map_err(|e| match ApiError::from(e) {
            e if e.code == "already_exists" => {
                ApiError::validation(vec![FieldError::new(format!("{pointer}/username"), "taken", format!("帳號 `{}` 已存在", req.username))])
            }
            e => e,
        })?;
    let user: AdminUser = sqlx::query_as(&format!("{USER_SELECT} WHERE u.id = $1")).bind(id).fetch_one(&st.db).await?;
    Ok(CreatedUser { user, initial_password: generated })
}

/// Create one account.
#[utoipa::path(post, path = "/admin/users", tag = TAG_STUDENTS, request_body = CreateUser,
    responses((status = 201, body = CreatedUser), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["students:write"]), ("session" = [])))]
async fn create_user(State(st): State<AppState>, p: Principal, JsonBody(req): JsonBody<CreateUser>) -> ApiResult<(StatusCode, Json<CreatedUser>)> {
    p.require(Scope::StudentsWrite)?;
    if req.role == Some(Role::Owner) {
        p.require_owner_session()?;
    }
    let created = insert_user(&st, &req, "").await?;
    repo::audit(&st, &p, "user.create", &req.username, serde_json::json!({})).await;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchCreate {
    /// One student per line: `username,display name`. Lines starting with `#` are ignored.
    pub csv: String,
    pub cohort_id: Option<Uuid>,
    /// Validate only.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, ToSchema)]
pub struct BatchResult {
    pub ok: bool,
    pub errors: Vec<FieldError>,
    pub created: Vec<CreatedUser>,
}

/// Create many students from pasted CSV text. All-or-nothing validation;
/// generated passwords are returned once.
#[utoipa::path(post, path = "/admin/users/batch", tag = TAG_STUDENTS, request_body = BatchCreate,
    responses((status = 200, body = BatchResult), (status = 422, body = BatchResult)), security(("token" = ["students:write"]), ("session" = [])))]
async fn batch_create(State(st): State<AppState>, p: Principal, JsonBody(req): JsonBody<BatchCreate>) -> ApiResult<(StatusCode, Json<BatchResult>)> {
    p.require(Scope::StudentsWrite)?;
    let mut rows = vec![];
    let mut errors = vec![];
    let mut seen = std::collections::HashSet::new();
    for (i, line) in req.csv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (u, name) = line.split_once(',').map(|(a, b)| (a.trim(), b.trim())).unwrap_or((line, ""));
        let ptr = format!("/csv/{}", i + 1);
        if !valid_username(u) {
            errors.push(FieldError::new(&ptr, "invalid", format!("第 {} 行：帳號 `{u}` 格式不正確", i + 1)));
        } else if !seen.insert(u.to_lowercase()) {
            errors.push(FieldError::new(&ptr, "duplicate", format!("第 {} 行：帳號 `{u}` 重複", i + 1)));
        } else {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE lower(username) = lower($1))").bind(u).fetch_one(&st.db).await?;
            if exists {
                errors.push(FieldError::new(&ptr, "taken", format!("第 {} 行：帳號 `{u}` 已存在", i + 1)));
            }
        }
        rows.push(CreateUser { username: u.to_string(), display_name: name.to_string(), role: None, password: None, cohort_id: req.cohort_id });
    }
    if rows.is_empty() && errors.is_empty() {
        errors.push(FieldError::new("/csv", "empty", "沒有任何學生資料"));
    }
    if !errors.is_empty() || req.dry_run {
        let code = if errors.is_empty() { StatusCode::OK } else { StatusCode::UNPROCESSABLE_ENTITY };
        return Ok((code, Json(BatchResult { ok: errors.is_empty(), errors, created: vec![] })));
    }
    let mut created = vec![];
    for (i, r) in rows.iter().enumerate() {
        created.push(insert_user(&st, r, &format!("/csv/{i}")).await?);
    }
    repo::audit(&st, &p, "user.batch_create", "", serde_json::json!({"count": created.len()})).await;
    Ok((StatusCode::OK, Json(BatchResult { ok: true, errors: vec![], created })))
}

/// One account.
#[utoipa::path(get, path = "/admin/users/{id}", tag = TAG_STUDENTS, params(("id" = Uuid, Path)),
    responses((status = 200, body = AdminUser), (status = 404, body = crate::error::ProblemDetails)), security(("token" = ["students:read"]), ("session" = [])))]
async fn get_user(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<Json<AdminUser>> {
    p.require(Scope::StudentsRead)?;
    let u: Option<AdminUser> = sqlx::query_as(&format!("{USER_SELECT} WHERE u.id = $1")).bind(id).fetch_optional(&st.db).await?;
    Ok(Json(u.ok_or_else(|| ApiError::not_found("這個帳號"))?))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateUser {
    pub display_name: Option<String>,
    pub disabled: Option<bool>,
    /// Set to move the student; send `"clear_cohort": true` to remove.
    pub cohort_id: Option<Uuid>,
    #[serde(default)]
    pub clear_cohort: bool,
}

/// Rename, disable/enable, or move a user between cohorts.
#[utoipa::path(patch, path = "/admin/users/{id}", tag = TAG_STUDENTS, params(("id" = Uuid, Path)), request_body = UpdateUser,
    responses((status = 200, body = AdminUser)), security(("token" = ["students:write"]), ("session" = [])))]
async fn update_user(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>, JsonBody(req): JsonBody<UpdateUser>) -> ApiResult<Json<AdminUser>> {
    p.require(Scope::StudentsWrite)?;
    if id == p.user_id && req.disabled == Some(true) {
        return Err(ApiError::conflict("self_disable", "不能停用自己的帳號"));
    }
    let r = sqlx::query(
        "UPDATE users SET display_name = COALESCE($2, display_name), disabled = COALESCE($3, disabled),
                cohort_id = CASE WHEN $5 THEN NULL ELSE COALESCE($4, cohort_id) END, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&req.display_name)
    .bind(req.disabled)
    .bind(req.cohort_id)
    .bind(req.clear_cohort)
    .execute(&st.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("這個帳號"));
    }
    if req.disabled == Some(true) {
        sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(id).execute(&st.db).await?;
    }
    repo::audit(&st, &p, "user.update", &id.to_string(), serde_json::json!({"disabled": req.disabled, "cohort_id": req.cohort_id})).await;
    Ok(Json(sqlx::query_as(&format!("{USER_SELECT} WHERE u.id = $1")).bind(id).fetch_one(&st.db).await?))
}

#[derive(Serialize, ToSchema)]
pub struct NewPassword {
    /// Shown once. The user must change it at next login.
    pub password: String,
}

/// Generate a new password for a user and sign them out everywhere.
#[utoipa::path(post, path = "/admin/users/{id}/reset-password", tag = TAG_STUDENTS, params(("id" = Uuid, Path)),
    responses((status = 200, body = NewPassword)), security(("session" = [])))]
async fn reset_password(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<Json<NewPassword>> {
    // Credentials are only handed out to an owner's interactive session.
    p.require_owner_session()?;
    let pw = password::generate();
    let r = sqlx::query("UPDATE users SET password_hash = $2, must_change_password = true, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(password::hash(&pw)?)
        .execute(&st.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("這個帳號"));
    }
    sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(id).execute(&st.db).await?;
    repo::audit(&st, &p, "user.reset_password", &id.to_string(), serde_json::json!({})).await;
    Ok(Json(NewPassword { password: pw }))
}

/// Sign a user out of every session.
#[utoipa::path(post, path = "/admin/users/{id}/revoke-sessions", tag = TAG_STUDENTS, params(("id" = Uuid, Path)),
    responses((status = 204)), security(("token" = ["students:write"]), ("session" = [])))]
async fn revoke_sessions(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    p.require(Scope::StudentsWrite)?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(id).execute(&st.db).await?;
    repo::audit(&st, &p, "user.revoke_sessions", &id.to_string(), serde_json::json!({})).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct PolicyView {
    /// The student's own overrides.
    pub overrides: PolicyPatch,
    /// The cohort's settings (if any).
    pub cohort: Option<PolicyPatch>,
    /// Resulting values with the level each one came from.
    pub effective: EffectivePolicy,
}

async fn policy_view(st: &AppState, id: Uuid) -> ApiResult<PolicyView> {
    let row: Option<(Option<serde_json::Value>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT o.policy, c.policy FROM users u LEFT JOIN student_overrides o ON o.user_id = u.id LEFT JOIN cohorts c ON c.id = u.cohort_id WHERE u.id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let (o, c) = row.ok_or_else(|| ApiError::not_found("這個帳號"))?;
    Ok(PolicyView {
        overrides: o.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
        cohort: c.and_then(|v| serde_json::from_value(v).ok()),
        effective: repo::policy_for(st, id).await?,
    })
}

/// A student's overrides and resulting effective settings (with sources).
#[utoipa::path(get, path = "/admin/users/{id}/policy", tag = TAG_STUDENTS, params(("id" = Uuid, Path)), responses((status = 200, body = PolicyView)), security(("token" = ["students:read"]), ("session" = [])))]
async fn get_policy(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<Json<PolicyView>> {
    p.require(Scope::StudentsRead)?;
    Ok(Json(policy_view(&st, id).await?))
}

/// Replace a student's overrides (unset fields fall back to cohort/site).
#[utoipa::path(put, path = "/admin/users/{id}/policy", tag = TAG_STUDENTS, params(("id" = Uuid, Path)), request_body = PolicyPatch,
    responses((status = 200, body = PolicyView), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["students:write"]), ("session" = [])))]
async fn put_policy(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>, JsonBody(req): JsonBody<PolicyPatch>) -> ApiResult<Json<PolicyView>> {
    p.require(Scope::StudentsWrite)?;
    let errors = validate_patch(&req, "");
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    sqlx::query(
        "INSERT INTO student_overrides (user_id, policy) VALUES ($1, $2) ON CONFLICT (user_id) DO UPDATE SET policy = EXCLUDED.policy, updated_at = now()",
    )
    .bind(id)
    .bind(serde_json::to_value(&req).unwrap())
    .execute(&st.db)
    .await?;
    repo::audit(&st, &p, "user.policy", &id.to_string(), serde_json::to_value(&req).unwrap()).await;
    Ok(Json(policy_view(&st, id).await?))
}

#[derive(Serialize, ToSchema)]
pub struct Cohort {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub policy: PolicyPatch,
    pub member_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CohortInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub policy: PolicyPatch,
}

async fn cohort(st: &AppState, id: Uuid) -> ApiResult<Cohort> {
    let (id, name, description, policy, member_count, created_at): (Uuid, String, String, serde_json::Value, i64, DateTime<Utc>) = sqlx::query_as(
        "SELECT c.id, c.name, c.description, c.policy, (SELECT count(*) FROM users u WHERE u.cohort_id = c.id), c.created_at FROM cohorts c WHERE c.id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or_else(|| ApiError::not_found("這個組別"))?;
    Ok(Cohort { id, name, description, policy: serde_json::from_value(policy).unwrap_or_default(), member_count, created_at })
}

/// Cohorts (groups of students sharing settings).
#[utoipa::path(get, path = "/admin/cohorts", tag = TAG_STUDENTS, responses((status = 200, body = Vec<Cohort>)), security(("token" = ["students:read"]), ("session" = [])))]
async fn list_cohorts(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Vec<Cohort>>> {
    p.require(Scope::StudentsRead)?;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM cohorts ORDER BY name").fetch_all(&st.db).await?;
    let mut out = vec![];
    for id in ids {
        out.push(cohort(&st, id).await?);
    }
    Ok(Json(out))
}

fn validate_cohort(c: &CohortInput) -> Vec<FieldError> {
    let mut e = validate_patch(&c.policy, "/policy");
    if c.name.trim().is_empty() || c.name.chars().count() > 60 {
        e.push(FieldError::new("/name", "invalid", "組別名稱需為 1–60 字"));
    }
    e
}

/// Create a cohort.
#[utoipa::path(post, path = "/admin/cohorts", tag = TAG_STUDENTS, request_body = CohortInput,
    responses((status = 201, body = Cohort), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["students:write"]), ("session" = [])))]
async fn create_cohort(State(st): State<AppState>, p: Principal, JsonBody(req): JsonBody<CohortInput>) -> ApiResult<(StatusCode, Json<Cohort>)> {
    p.require(Scope::StudentsWrite)?;
    let e = validate_cohort(&req);
    if !e.is_empty() {
        return Err(ApiError::validation(e));
    }
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO cohorts (id, name, description, policy) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(&req.name)
        .bind(&req.description)
        .bind(serde_json::to_value(&req.policy).unwrap())
        .execute(&st.db)
        .await?;
    repo::audit(&st, &p, "cohort.create", &req.name, serde_json::json!({})).await;
    Ok((StatusCode::CREATED, Json(cohort(&st, id).await?)))
}

/// Update a cohort's name, description and settings (applies immediately).
#[utoipa::path(put, path = "/admin/cohorts/{id}", tag = TAG_STUDENTS, params(("id" = Uuid, Path)), request_body = CohortInput,
    responses((status = 200, body = Cohort), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["students:write"]), ("session" = [])))]
async fn update_cohort(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>, JsonBody(req): JsonBody<CohortInput>) -> ApiResult<Json<Cohort>> {
    p.require(Scope::StudentsWrite)?;
    let e = validate_cohort(&req);
    if !e.is_empty() {
        return Err(ApiError::validation(e));
    }
    let r = sqlx::query("UPDATE cohorts SET name = $2, description = $3, policy = $4, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(&req.name)
        .bind(&req.description)
        .bind(serde_json::to_value(&req.policy).unwrap())
        .execute(&st.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("這個組別"));
    }
    repo::audit(&st, &p, "cohort.update", &req.name, serde_json::to_value(&req.policy).unwrap()).await;
    Ok(Json(cohort(&st, id).await?))
}

/// Delete a cohort (members keep their accounts, without a cohort).
#[utoipa::path(delete, path = "/admin/cohorts/{id}", tag = TAG_STUDENTS, params(("id" = Uuid, Path)), responses((status = 204)), security(("token" = ["students:write"]), ("session" = [])))]
async fn delete_cohort(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    p.require(Scope::StudentsWrite)?;
    sqlx::query("DELETE FROM cohorts WHERE id = $1").bind(id).execute(&st.db).await?;
    repo::audit(&st, &p, "cohort.delete", &id.to_string(), serde_json::json!({})).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct MatrixProblem {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
}

#[derive(Serialize, ToSchema)]
pub struct MatrixRow {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    /// One entry per problem (same order): `solved`, `credited`, `attempted` or `not_started`.
    pub cells: Vec<String>,
    pub solved: usize,
}

#[derive(Serialize, ToSchema)]
pub struct ProgressMatrix {
    pub problems: Vec<MatrixProblem>,
    pub rows: Vec<MatrixRow>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct MatrixParams {
    /// Set id or slug (required).
    pub problem_set: String,
    pub cohort_id: Option<Uuid>,
}

/// Student × problem progress for one set.
#[utoipa::path(get, path = "/admin/progress", tag = TAG_STUDENTS, params(MatrixParams), responses((status = 200, body = ProgressMatrix)), security(("token" = ["students:read"]), ("session" = [])))]
async fn progress_matrix(State(st): State<AppState>, p: Principal, Query(q): Query<MatrixParams>) -> ApiResult<Json<ProgressMatrix>> {
    p.require(Scope::StudentsRead)?;
    let set = repo::find_set(&st.db, &q.problem_set).await?;
    let problems: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT p.id, p.slug, h.title FROM problem_set_items i JOIN problems p ON p.id = i.problem_id JOIN problem_revisions h ON h.id = p.head_revision_id WHERE i.set_id = $1 ORDER BY i.position",
    )
    .bind(set.id)
    .fetch_all(&st.db)
    .await?;
    let users: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, username, display_name FROM users WHERE role = 'student' AND ($1::uuid IS NULL OR cohort_id = $1) ORDER BY username")
            .bind(q.cohort_id)
            .fetch_all(&st.db)
            .await?;
    let pids: Vec<Uuid> = problems.iter().map(|x| x.0).collect();
    let stats: Vec<(Uuid, Uuid, bool)> = sqlx::query_as(
        "SELECT user_id, problem_id, bool_or(verdict = 'accepted') FROM submissions WHERE kind = 'submit' AND problem_id = ANY($1) GROUP BY user_id, problem_id",
    )
    .bind(&pids)
    .fetch_all(&st.db)
    .await?;
    let credits: Vec<(Uuid, Uuid)> =
        sqlx::query_as("SELECT user_id, problem_id FROM progress_credits WHERE problem_id = ANY($1)").bind(&pids).fetch_all(&st.db).await?;
    let stats: HashMap<(Uuid, Uuid), bool> = stats.into_iter().map(|(u, p, ok)| ((u, p), ok)).collect();
    let rows = users
        .into_iter()
        .map(|(uid, username, display_name)| {
            let cells: Vec<String> = pids
                .iter()
                .map(|pid| match stats.get(&(uid, *pid)) {
                    Some(true) => "solved",
                    _ if credits.contains(&(uid, *pid)) => "credited",
                    Some(false) => "attempted",
                    None => "not_started",
                })
                .map(String::from)
                .collect();
            let solved = cells.iter().filter(|c| *c == "solved" || *c == "credited").count();
            MatrixRow { user_id: uid, username, display_name, cells, solved }
        })
        .collect();
    Ok(Json(ProgressMatrix { problems: problems.into_iter().map(|(id, slug, title)| MatrixProblem { id, slug, title }).collect(), rows }))
}

#[derive(Serialize, ToSchema)]
pub struct UserProblemProgress {
    pub problem_id: Uuid,
    pub slug: String,
    pub title: String,
    pub submissions: i64,
    pub runs: i64,
    pub solved: bool,
    pub credited: bool,
    /// Rules most often hit by this student on this problem.
    pub frequent_rules: Vec<String>,
    /// Hint levels revealed (total).
    pub hints_revealed: i64,
    pub last_verdict: Option<String>,
    pub last_at: Option<DateTime<Utc>>,
}

/// One student's progress per problem, with common error types and hint use.
#[utoipa::path(get, path = "/admin/users/{id}/progress", tag = TAG_STUDENTS, params(("id" = Uuid, Path)), responses((status = 200, body = Vec<UserProblemProgress>)), security(("token" = ["students:read"]), ("session" = [])))]
async fn user_progress(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<UserProblemProgress>>> {
    p.require(Scope::StudentsRead)?;
    let rows: Vec<(Uuid, String, String, i64, i64, bool, Option<String>, Option<DateTime<Utc>>, Vec<String>, i64)> = sqlx::query_as(
        "SELECT p.id, p.slug, h.title,
                count(*) FILTER (WHERE s.kind = 'submit'), count(*) FILTER (WHERE s.kind = 'run'),
                COALESCE(bool_or(s.kind = 'submit' AND s.verdict = 'accepted'), false),
                (array_agg(s.verdict ORDER BY s.created_at DESC) FILTER (WHERE s.kind = 'submit'))[1],
                max(s.created_at),
                COALESCE((SELECT array_agg(r ORDER BY n DESC) FROM (SELECT r, count(*) n FROM submissions s2, unnest(s2.finding_rules) r
                           WHERE s2.user_id = $1 AND s2.problem_id = p.id GROUP BY r ORDER BY n DESC LIMIT 3) t), '{}'),
                (SELECT count(*) FROM hint_reveals hr JOIN submissions s3 ON s3.id = hr.submission_id WHERE s3.user_id = $1 AND s3.problem_id = p.id)
           FROM submissions s JOIN problems p ON p.id = s.problem_id JOIN problem_revisions h ON h.id = p.head_revision_id
          WHERE s.user_id = $1 AND s.kind IN ('run', 'submit') GROUP BY p.id, p.slug, h.title ORDER BY max(s.created_at) DESC",
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    let credits: Vec<Uuid> = sqlx::query_scalar("SELECT problem_id FROM progress_credits WHERE user_id = $1").bind(id).fetch_all(&st.db).await?;
    Ok(Json(
        rows.into_iter()
            .map(|(pid, slug, title, submissions, runs, solved, last_verdict, last_at, frequent_rules, hints_revealed)| UserProblemProgress {
                problem_id: pid,
                slug,
                title,
                submissions,
                runs,
                solved,
                credited: credits.contains(&pid),
                frequent_rules,
                hints_revealed,
                last_verdict,
                last_at,
            })
            .collect(),
    ))
}

#[derive(Deserialize, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct CreditInput {
    #[serde(default)]
    pub note: String,
}

/// Manually credit a problem as completed for a student. Judging results are
/// not modified; the credit is stored separately and can be removed.
#[utoipa::path(put, path = "/admin/users/{id}/credits/{problem_id}", tag = TAG_STUDENTS, params(("id" = Uuid, Path), ("problem_id" = Uuid, Path)),
    request_body = CreditInput, responses((status = 204)), security(("token" = ["students:write"]), ("session" = [])))]
async fn credit(
    State(st): State<AppState>,
    p: Principal,
    Path((id, problem_id)): Path<(Uuid, Uuid)>,
    body: Option<JsonBody<CreditInput>>,
) -> ApiResult<StatusCode> {
    p.require(Scope::StudentsWrite)?;
    let note = body.map(|b| b.0.note).unwrap_or_default();
    sqlx::query("INSERT INTO progress_credits (user_id, problem_id, note, created_by) VALUES ($1, $2, $3, $4) ON CONFLICT (user_id, problem_id) DO UPDATE SET note = EXCLUDED.note")
        .bind(id)
        .bind(problem_id)
        .bind(&note)
        .bind(p.user_id)
        .execute(&st.db)
        .await?;
    repo::audit(&st, &p, "progress.credit", &format!("{id}/{problem_id}"), serde_json::json!({"note": note})).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove a manual credit.
#[utoipa::path(delete, path = "/admin/users/{id}/credits/{problem_id}", tag = TAG_STUDENTS, params(("id" = Uuid, Path), ("problem_id" = Uuid, Path)),
    responses((status = 204)), security(("token" = ["students:write"]), ("session" = [])))]
async fn uncredit(State(st): State<AppState>, p: Principal, Path((id, problem_id)): Path<(Uuid, Uuid)>) -> ApiResult<StatusCode> {
    p.require(Scope::StudentsWrite)?;
    sqlx::query("DELETE FROM progress_credits WHERE user_id = $1 AND problem_id = $2").bind(id).bind(problem_id).execute(&st.db).await?;
    repo::audit(&st, &p, "progress.uncredit", &format!("{id}/{problem_id}"), serde_json::json!({})).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct RuleStat {
    pub rule: String,
    /// Submissions that triggered the rule in the period.
    pub count: i64,
    pub students: i64,
    /// Hint levels revealed on cards of this rule.
    pub hints_revealed: i64,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct StatsParams {
    /// Look-back window in days (default 7).
    pub days: Option<i32>,
    pub problem_id: Option<Uuid>,
}

/// Which explanation rules students hit most (to prioritise teaching text).
#[utoipa::path(get, path = "/admin/stats/rules", tag = TAG_STUDENTS, params(StatsParams), responses((status = 200, body = Vec<RuleStat>)), security(("token" = ["students:read"]), ("session" = [])))]
async fn rule_stats(State(st): State<AppState>, p: Principal, Query(q): Query<StatsParams>) -> ApiResult<Json<Vec<RuleStat>>> {
    p.require(Scope::StudentsRead)?;
    let days = q.days.unwrap_or(7).clamp(1, 365);
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT r, count(*), count(DISTINCT s.user_id) FROM submissions s, unnest(s.finding_rules) r
          WHERE s.created_at > now() - make_interval(days => $1) AND s.kind IN ('run', 'submit') AND ($2::uuid IS NULL OR s.problem_id = $2)
          GROUP BY r ORDER BY count(*) DESC",
    )
    .bind(days)
    .bind(q.problem_id)
    .fetch_all(&st.db)
    .await?;
    let hints: Vec<(String, i64)> =
        sqlx::query_as("SELECT rule, count(*) FROM hint_reveals WHERE revealed_at > now() - make_interval(days => $1) GROUP BY rule")
            .bind(days)
            .fetch_all(&st.db)
            .await?;
    let hints: HashMap<String, i64> = hints.into_iter().collect();
    Ok(Json(
        rows.into_iter().map(|(rule, count, students)| RuleStat { hints_revealed: hints.get(&rule).copied().unwrap_or(0), rule, count, students }).collect(),
    ))
}
