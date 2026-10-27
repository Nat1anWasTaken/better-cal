//! Problem bank management: create, revise (immutable revisions), validate
//! with the reference solution, publish.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use ccal_core::problem::{FieldError, ProblemContent, ProblemMode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_PROBLEMS;
use super::submissions::{assemble, load_row, render_view};
use crate::auth::{Principal, Scope};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::judge::view::{Audience, SubmissionView};
use crate::repo::{self, Page, ProblemRow};
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_problems, create_problem))
        .routes(routes!(get_problem, update_problem, archive_problem))
        .routes(routes!(list_revisions))
        .routes(routes!(get_revision))
        .routes(routes!(validate_problem))
        .routes(routes!(publish_problem))
        .routes(routes!(unpublish_problem))
        .routes(routes!(test_run))
}

#[derive(Serialize, ToSchema)]
pub struct RevisionDetail {
    pub number: i32,
    pub content: ProblemContent,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct SetRef {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub status: String,
}

#[derive(Serialize, ToSchema)]
pub struct ProblemDetail {
    pub id: Uuid,
    pub slug: String,
    /// `draft`, `published` or `archived`.
    pub status: String,
    pub head_revision: RevisionDetail,
    /// Revision students currently get (None if never published).
    pub published_revision_number: Option<i32>,
    /// The head revision differs from the published one.
    pub has_unpublished_changes: bool,
    /// Sets that include this problem.
    pub sets: Vec<SetRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Only on writes: whether a new revision was created (false = no change).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_created: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct ProblemSummary {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub status: String,
    pub head_revision_number: i32,
    pub published_revision_number: Option<i32>,
    pub has_unpublished_changes: bool,
    pub set_count: i64,
    pub concepts: Vec<String>,
    pub mode: ProblemMode,
    pub updated_at: DateTime<Utc>,
}

pub async fn detail(st: &AppState, pr: &ProblemRow, revision_created: Option<bool>) -> ApiResult<ProblemDetail> {
    let head = repo::revision(&st.db, pr.head_revision_id.ok_or_else(|| ApiError::internal("problem without head revision"))?).await?;
    let published_number: Option<i32> = match pr.published_revision_id {
        Some(id) => Some(repo::revision(&st.db, id).await?.number),
        None => None,
    };
    let sets: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT s.id, s.slug, s.title, s.status FROM problem_set_items i JOIN problem_sets s ON s.id = i.set_id WHERE i.problem_id = $1 ORDER BY s.created_at",
    )
    .bind(pr.id)
    .fetch_all(&st.db)
    .await?;
    Ok(ProblemDetail {
        id: pr.id,
        slug: pr.slug.clone(),
        status: pr.status.clone(),
        has_unpublished_changes: pr.published_revision_id != pr.head_revision_id,
        published_revision_number: published_number,
        head_revision: RevisionDetail { number: head.number, content: head.content()?, content_hash: head.content_hash, created_at: head.created_at },
        sets: sets.into_iter().map(|(id, slug, title, status)| SetRef { id, slug, title, status }).collect(),
        created_at: pr.created_at,
        updated_at: pr.updated_at,
        revision_created,
    })
}

/// Creates a revision unless the content equals the head revision.
/// Returns (revision id, created).
pub async fn save_revision(tx: &mut Transaction<'_, Postgres>, problem_id: Uuid, content: &ProblemContent, user: Uuid) -> ApiResult<(Uuid, bool)> {
    let hash = content.content_hash();
    let head: Option<(Uuid, String, i32)> = sqlx::query_as(
        "SELECT r.id, r.content_hash, r.number FROM problems p JOIN problem_revisions r ON r.id = p.head_revision_id WHERE p.id = $1 FOR UPDATE OF p",
    )
    .bind(problem_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((id, h, _)) = &head
        && *h == hash
    {
        return Ok((*id, false));
    }
    let number = head.map(|h| h.2).unwrap_or(0) + 1;
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO problem_revisions (id, problem_id, number, content, title, content_hash, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(id)
        .bind(problem_id)
        .bind(number)
        .bind(serde_json::to_value(content).unwrap())
        .bind(&content.title)
        .bind(&hash)
        .bind(user)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE problems SET head_revision_id = $2, updated_at = now() WHERE id = $1").bind(problem_id).bind(id).execute(&mut **tx).await?;
    Ok((id, true))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ProblemFilter {
    /// Search in slug and title.
    pub q: Option<String>,
    /// `draft`, `published` or `archived`.
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

/// List the problem bank.
#[utoipa::path(get, path = "/admin/problems", tag = TAG_PROBLEMS, params(ProblemFilter), responses((status = 200, body = Page<ProblemSummary>)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn list_problems(State(st): State<AppState>, p: Principal, Query(f): Query<ProblemFilter>) -> ApiResult<Json<Page<ProblemSummary>>> {
    p.require(Scope::ProblemsRead)?;
    let (limit, offset) = repo::PageParams { limit: f.limit, cursor: f.cursor.clone() }.resolve();
    let q = f.q.as_ref().map(|q| format!("%{}%", q.replace('%', "\\%").replace('_', "\\_")));
    let rows: Vec<(Uuid, String, String, i32, Option<i32>, serde_json::Value, DateTime<Utc>, i64)> = sqlx::query_as(
        "SELECT p.id, p.slug, p.status, h.number, pub.number, h.content, p.updated_at,
                (SELECT count(*) FROM problem_set_items i WHERE i.problem_id = p.id)
           FROM problems p JOIN problem_revisions h ON h.id = p.head_revision_id LEFT JOIN problem_revisions pub ON pub.id = p.published_revision_id
          WHERE ($1::text IS NULL OR p.slug ILIKE $1 OR h.title ILIKE $1) AND ($2::text IS NULL OR p.status = $2)
          ORDER BY p.updated_at DESC, p.id LIMIT $3 OFFSET $4",
    )
    .bind(&q)
    .bind(&f.status)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&st.db)
    .await?;
    let items = rows
        .into_iter()
        .map(|(id, slug, status, head, published, content, updated_at, set_count)| {
            let c: Option<ProblemContent> = serde_json::from_value(content).ok();
            ProblemSummary {
                id,
                slug,
                title: c.as_ref().map(|c| c.title.clone()).unwrap_or_default(),
                status,
                head_revision_number: head,
                published_revision_number: published,
                has_unpublished_changes: published != Some(head),
                set_count,
                concepts: c.as_ref().map(|c| c.concepts.clone()).unwrap_or_default(),
                mode: c.map(|c| c.mode).unwrap_or_default(),
                updated_at,
            }
        })
        .collect();
    Ok(Json(repo::page(items, limit, offset)))
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProblem {
    /// Stable identifier, immutable after creation.
    pub slug: String,
    pub content: ProblemContent,
}

/// Create a problem (as a draft). Use `Idempotency-Key` for safe retries.
#[utoipa::path(post, path = "/admin/problems", tag = TAG_PROBLEMS, request_body = CreateProblem,
    params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = 201, body = ProblemDetail), (status = 409, body = crate::error::ProblemDetails), (status = 422, body = crate::error::ProblemDetails)),
    security(("token" = ["problems:write"]), ("session" = [])))]
async fn create_problem(
    State(st): State<AppState>,
    p: Principal,
    headers: HeaderMap,
    JsonBody(req): JsonBody<CreateProblem>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    p.require(Scope::ProblemsWrite)?;
    let key = repo::idempotency_key(&headers);
    let hash = repo::request_hash(&req);
    let (code, v) = repo::idempotent(&st, &p, key.as_deref(), &hash, || async {
        let mut errors = vec![];
        if !ccal_core::valid_slug(&req.slug) {
            errors.push(FieldError::new("/slug", "invalid_slug", "slug 只能使用小寫英文字母、數字與單一連字號，長度 3–64"));
        }
        errors.extend(req.content.validate("/content"));
        if !errors.is_empty() {
            return Err(ApiError::validation(errors));
        }
        let mut tx = st.db.begin().await?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO problems (id, slug) VALUES ($1, $2)").bind(id).bind(&req.slug).execute(&mut *tx).await.map_err(|e| {
            match ApiError::from(e) {
                e if e.code == "already_exists" => ApiError::conflict("slug_taken", format!("slug `{}` 已被使用", req.slug)),
                e => e,
            }
        })?;
        save_revision(&mut tx, id, &req.content, p.user_id).await?;
        tx.commit().await?;
        repo::audit(&st, &p, "problem.create", &req.slug, serde_json::json!({})).await;
        let pr = repo::find_problem(&st.db, &id.to_string()).await?;
        Ok((StatusCode::CREATED, serde_json::to_value(detail(&st, &pr, Some(true)).await?).unwrap()))
    })
    .await?;
    Ok((code, Json(v)))
}

/// Full problem including hidden tests and reference solution (head revision).
#[utoipa::path(get, path = "/admin/problems/{problem}", tag = TAG_PROBLEMS, params(("problem" = String, Path, description = "Id or slug")),
    responses((status = 200, body = ProblemDetail), (status = 404, body = crate::error::ProblemDetails)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn get_problem(State(st): State<AppState>, p: Principal, Path(problem): Path<String>) -> ApiResult<Json<ProblemDetail>> {
    p.require(Scope::ProblemsRead)?;
    let pr = repo::find_problem(&st.db, &problem).await?;
    Ok(Json(detail(&st, &pr, None).await?))
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProblem {
    pub content: ProblemContent,
}

/// Replace the problem content. Creates a new immutable revision (unless
/// nothing changed). Published content is unaffected until you publish again.
/// Send `If-Match: <head revision number>` to avoid overwriting someone else's edit.
#[utoipa::path(put, path = "/admin/problems/{problem}", tag = TAG_PROBLEMS, request_body = UpdateProblem,
    params(("problem" = String, Path), ("If-Match" = Option<String>, Header, description = "Expected head revision number")),
    responses((status = 200, body = ProblemDetail), (status = 412, body = crate::error::ProblemDetails), (status = 422, body = crate::error::ProblemDetails)),
    security(("token" = ["problems:write"]), ("session" = [])))]
async fn update_problem(
    State(st): State<AppState>,
    p: Principal,
    headers: HeaderMap,
    Path(problem): Path<String>,
    JsonBody(req): JsonBody<UpdateProblem>,
) -> ApiResult<Json<ProblemDetail>> {
    p.require(Scope::ProblemsWrite)?;
    let errors = req.content.validate("/content");
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    let pr = repo::find_problem(&st.db, &problem).await?;
    if pr.status == "archived" {
        return Err(ApiError::conflict("archived", "已封存的題目不能修改"));
    }
    let mut tx = st.db.begin().await?;
    if let Some(expected) = headers.get("if-match").and_then(|v| v.to_str().ok()) {
        let expected: i32 = expected.trim_matches('"').parse().map_err(|_| ApiError::bad_request("invalid_if_match", "If-Match 必須是版本號碼"))?;
        let head: i32 =
            sqlx::query_scalar("SELECT r.number FROM problems p JOIN problem_revisions r ON r.id = p.head_revision_id WHERE p.id = $1 FOR UPDATE OF p")
                .bind(pr.id)
                .fetch_one(&mut *tx)
                .await?;
        if head != expected {
            return Err(ApiError::new(StatusCode::PRECONDITION_FAILED, "revision_mismatch", format!("題目已被修改（目前版本 {head}），請重新載入後再編輯")));
        }
    }
    let (_, created) = save_revision(&mut tx, pr.id, &req.content, p.user_id).await?;
    tx.commit().await?;
    if created {
        repo::audit(&st, &p, "problem.update", &pr.slug, serde_json::json!({})).await;
    }
    let pr = repo::find_problem(&st.db, &pr.id.to_string()).await?;
    Ok(Json(detail(&st, &pr, Some(created)).await?))
}

/// Archive a problem. Refused while any problem set still includes it.
/// Revisions and submissions are kept for traceability.
#[utoipa::path(delete, path = "/admin/problems/{problem}", tag = TAG_PROBLEMS, params(("problem" = String, Path)),
    responses((status = 204), (status = 409, body = crate::error::ProblemDetails)), security(("token" = ["problems:write"]), ("session" = [])))]
async fn archive_problem(State(st): State<AppState>, p: Principal, Path(problem): Path<String>) -> ApiResult<StatusCode> {
    p.require(Scope::ProblemsWrite)?;
    let pr = repo::find_problem(&st.db, &problem).await?;
    let in_sets: i64 = sqlx::query_scalar("SELECT count(*) FROM problem_set_items WHERE problem_id = $1").bind(pr.id).fetch_one(&st.db).await?;
    if in_sets > 0 {
        return Err(ApiError::conflict("in_use", format!("這個題目還在 {in_sets} 個題目集中，請先從題目集移除")));
    }
    sqlx::query("UPDATE problems SET status = 'archived', published_revision_id = NULL, updated_at = now() WHERE id = $1").bind(pr.id).execute(&st.db).await?;
    repo::audit(&st, &p, "problem.archive", &pr.slug, serde_json::json!({})).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct RevisionSummary {
    pub number: i32,
    pub title: String,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
    pub published: bool,
    /// Student runs/submissions judged against this revision (owner test runs excluded).
    pub submission_count: i64,
}

/// All revisions (newest first).
#[utoipa::path(get, path = "/admin/problems/{problem}/revisions", tag = TAG_PROBLEMS, params(("problem" = String, Path)),
    responses((status = 200, body = Vec<RevisionSummary>)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn list_revisions(State(st): State<AppState>, p: Principal, Path(problem): Path<String>) -> ApiResult<Json<Vec<RevisionSummary>>> {
    p.require(Scope::ProblemsRead)?;
    let pr = repo::find_problem(&st.db, &problem).await?;
    let rows: Vec<(Uuid, i32, String, String, DateTime<Utc>, i64)> = sqlx::query_as(
        "SELECT r.id, r.number, r.title, r.content_hash, r.created_at, (SELECT count(*) FROM submissions s WHERE s.revision_id = r.id AND s.kind <> 'test_run')
           FROM problem_revisions r WHERE r.problem_id = $1 ORDER BY r.number DESC",
    )
    .bind(pr.id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, number, title, content_hash, created_at, submission_count)| RevisionSummary {
                number,
                title,
                content_hash,
                created_at,
                published: Some(id) == pr.published_revision_id,
                submission_count,
            })
            .collect(),
    ))
}

/// One immutable revision (including hidden tests).
#[utoipa::path(get, path = "/admin/problems/{problem}/revisions/{number}", tag = TAG_PROBLEMS, params(("problem" = String, Path), ("number" = i32, Path)),
    responses((status = 200, body = RevisionDetail), (status = 404, body = crate::error::ProblemDetails)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn get_revision(State(st): State<AppState>, p: Principal, Path((problem, number)): Path<(String, i32)>) -> ApiResult<Json<RevisionDetail>> {
    p.require(Scope::ProblemsRead)?;
    let pr = repo::find_problem(&st.db, &problem).await?;
    let rev: repo::RevisionRow =
        sqlx::query_as("SELECT id, problem_id, number, content, content_hash, created_at FROM problem_revisions WHERE problem_id = $1 AND number = $2")
            .bind(pr.id)
            .bind(number)
            .fetch_optional(&st.db)
            .await?
            .ok_or_else(|| ApiError::not_found("這個版本"))?;
    Ok(Json(RevisionDetail { number: rev.number, content: rev.content()?, content_hash: rev.content_hash, created_at: rev.created_at }))
}

#[derive(Serialize, ToSchema)]
pub struct ValidationReport {
    /// True when the revision can be published.
    pub ok: bool,
    pub revision_number: i32,
    pub errors: Vec<FieldError>,
    /// Result of running the reference solution against every test.
    pub reference_run: Option<SubmissionView>,
}

/// Queue an owner test run and wait for it (up to ~2 minutes).
/// An owner test run of some program against a problem revision.
pub struct TestRunJob {
    pub problem_id: Uuid,
    pub revision_id: Uuid,
    pub code: String,
    pub source: String,
    pub blanks: Option<serde_json::Value>,
    pub stdin: Option<String>,
}

pub async fn run_and_wait(st: &AppState, p: &Principal, job: TestRunJob) -> ApiResult<SubmissionView> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO submissions (id, user_id, problem_id, revision_id, kind, code, blanks, assembled_source, custom_stdin) VALUES ($1, $2, $3, $4, 'test_run', $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(p.user_id)
    .bind(job.problem_id)
    .bind(job.revision_id)
    .bind(job.code)
    .bind(job.blanks)
    .bind(job.source)
    .bind(job.stdin)
    .execute(&st.db)
    .await?;
    st.judge_notify.notify_waiters();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(150);
    loop {
        let status: String = sqlx::query_scalar("SELECT status FROM submissions WHERE id = $1").bind(id).fetch_one(&st.db).await?;
        if status == "done" || status == "system_error" {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            return Err(ApiError::unavailable("評測服務忙碌或無法使用，暫時無法驗證；請稍後再試"));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let row = load_row(st, id).await?;
    render_view(st, &row, Audience::Owner, false).await
}

async fn validate_revision(st: &AppState, p: &Principal, pr: &ProblemRow, rev: &repo::RevisionRow) -> ApiResult<ValidationReport> {
    let content = rev.content()?;
    let mut errors = content.validate_for_publish("/content");
    let mut reference_run = None;
    if errors.is_empty() {
        let (code, source, blanks) = match content.mode {
            ProblemMode::Full => assemble(content.mode, None, content.reference_solution.clone(), None)?,
            ProblemMode::FillIn => assemble(content.mode, content.template.as_deref(), None, content.reference_blanks.clone())?,
        };
        let view = run_and_wait(st, p, TestRunJob { problem_id: pr.id, revision_id: rev.id, code, source, blanks, stdin: None }).await?;
        if view.status == "system_error" {
            return Err(ApiError::unavailable("評測服務無法使用，暫時無法用參考解答驗證測資"));
        }
        if view.verdict.as_deref() != Some("accepted") {
            let failing: Vec<String> =
                view.tests.iter().filter(|t| !matches!(t.verdict, crate::judge::evaluate::TestVerdict::Accepted)).map(|t| t.number.to_string()).collect();
            errors.push(FieldError::new(
                "/content/reference_solution",
                "reference_failed",
                format!("參考解答沒有通過所有測資（結果：{}；未通過：第 {} 筆）", view.verdict.clone().unwrap_or_default(), failing.join("、")),
            ));
        }
        reference_run = Some(view);
    }
    Ok(ValidationReport { ok: errors.is_empty(), revision_number: rev.number, errors, reference_run })
}

#[derive(Deserialize, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct RevisionSelector {
    /// Defaults to the head revision.
    pub revision_number: Option<i32>,
}

async fn pick_revision(st: &AppState, pr: &ProblemRow, number: Option<i32>) -> ApiResult<repo::RevisionRow> {
    match number {
        None => repo::revision(&st.db, pr.head_revision_id.unwrap()).await,
        Some(n) => {
            sqlx::query_as("SELECT id, problem_id, number, content, content_hash, created_at FROM problem_revisions WHERE problem_id = $1 AND number = $2")
                .bind(pr.id)
                .bind(n)
                .fetch_optional(&st.db)
                .await?
                .ok_or_else(|| ApiError::not_found("這個版本"))
        }
    }
}

/// Check that a revision is ready to publish (content rules + the reference
/// solution passes every test through the real judge). Changes nothing.
#[utoipa::path(post, path = "/admin/problems/{problem}/validate", tag = TAG_PROBLEMS, params(("problem" = String, Path)), request_body(content = RevisionSelector, content_type = "application/json"),
    responses((status = 200, body = ValidationReport), (status = 503, body = crate::error::ProblemDetails)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn validate_problem(
    State(st): State<AppState>,
    p: Principal,
    Path(problem): Path<String>,
    body: Option<JsonBody<RevisionSelector>>,
) -> ApiResult<Json<ValidationReport>> {
    p.require(Scope::ProblemsRead)?;
    let pr = repo::find_problem(&st.db, &problem).await?;
    let sel = body.map(|b| b.0).unwrap_or_default();
    let rev = pick_revision(&st, &pr, sel.revision_number).await?;
    Ok(Json(validate_revision(&st, &p, &pr, &rev).await?))
}

/// Validate and publish a revision (default: head). Students get this
/// revision from now on; earlier submissions keep pointing at theirs.
#[utoipa::path(post, path = "/admin/problems/{problem}/publish", tag = TAG_PROBLEMS, params(("problem" = String, Path)), request_body(content = RevisionSelector, content_type = "application/json"),
    responses((status = 200, body = ProblemDetail), (status = 422, body = crate::error::ProblemDetails), (status = 503, body = crate::error::ProblemDetails)),
    security(("token" = ["publish"]), ("session" = [])))]
async fn publish_problem(
    State(st): State<AppState>,
    p: Principal,
    Path(problem): Path<String>,
    body: Option<JsonBody<RevisionSelector>>,
) -> ApiResult<Json<ProblemDetail>> {
    p.require(Scope::Publish)?;
    let pr = repo::find_problem(&st.db, &problem).await?;
    if pr.status == "archived" {
        return Err(ApiError::conflict("archived", "已封存的題目不能發布"));
    }
    let sel = body.map(|b| b.0).unwrap_or_default();
    let rev = pick_revision(&st, &pr, sel.revision_number).await?;
    if pr.published_revision_id != Some(rev.id) {
        let report = validate_revision(&st, &p, &pr, &rev).await?;
        if !report.ok {
            return Err(ApiError::validation(report.errors));
        }
    }
    sqlx::query("UPDATE problems SET published_revision_id = $2, status = 'published', updated_at = now() WHERE id = $1")
        .bind(pr.id)
        .bind(rev.id)
        .execute(&st.db)
        .await?;
    repo::audit(&st, &p, "problem.publish", &pr.slug, serde_json::json!({"revision": rev.number})).await;
    let pr = repo::find_problem(&st.db, &pr.id.to_string()).await?;
    Ok(Json(detail(&st, &pr, None).await?))
}

/// Hide the problem from students (sets keep the reference).
#[utoipa::path(post, path = "/admin/problems/{problem}/unpublish", tag = TAG_PROBLEMS, params(("problem" = String, Path)),
    responses((status = 200, body = ProblemDetail)), security(("token" = ["publish"]), ("session" = [])))]
async fn unpublish_problem(State(st): State<AppState>, p: Principal, Path(problem): Path<String>) -> ApiResult<Json<ProblemDetail>> {
    p.require(Scope::Publish)?;
    let pr = repo::find_problem(&st.db, &problem).await?;
    sqlx::query("UPDATE problems SET published_revision_id = NULL, status = 'draft', updated_at = now() WHERE id = $1 AND status <> 'archived'")
        .bind(pr.id)
        .execute(&st.db)
        .await?;
    repo::audit(&st, &p, "problem.unpublish", &pr.slug, serde_json::json!({})).await;
    let pr = repo::find_problem(&st.db, &pr.id.to_string()).await?;
    Ok(Json(detail(&st, &pr, None).await?))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestRunRequest {
    pub revision_number: Option<i32>,
    /// Run the stored reference solution instead of `code`/`blanks`.
    #[serde(default)]
    pub use_reference: bool,
    pub code: Option<String>,
    pub blanks: Option<BTreeMap<String, String>>,
    /// Use this stdin instead of the tests.
    pub stdin: Option<String>,
}

/// Run any program against a revision's tests (including hidden ones) and
/// see full details. Useful while authoring.
#[utoipa::path(post, path = "/admin/problems/{problem}/test-run", tag = TAG_PROBLEMS, params(("problem" = String, Path)), request_body = TestRunRequest,
    responses((status = 200, body = SubmissionView), (status = 503, body = crate::error::ProblemDetails)), security(("token" = ["problems:write"]), ("session" = [])))]
async fn test_run(
    State(st): State<AppState>,
    p: Principal,
    Path(problem): Path<String>,
    JsonBody(req): JsonBody<TestRunRequest>,
) -> ApiResult<Json<SubmissionView>> {
    p.require(Scope::ProblemsWrite)?;
    let pr = repo::find_problem(&st.db, &problem).await?;
    let rev = pick_revision(&st, &pr, req.revision_number).await?;
    let content = rev.content()?;
    let (code, blanks) = if req.use_reference { (content.reference_solution.clone(), content.reference_blanks.clone()) } else { (req.code, req.blanks) };
    let (code, source, blanks) = assemble(content.mode, content.template.as_deref(), code, blanks)?;
    Ok(Json(run_and_wait(&st, &p, TestRunJob { problem_id: pr.id, revision_id: rev.id, code, source, blanks, stdin: req.stdin }).await?))
}
