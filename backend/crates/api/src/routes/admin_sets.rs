//! Problem sets (ordered references to bank problems), the active set,
//! and JSON import/export.

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use ccal_core::import::{FORMAT_VERSION, ImportDocument, ImportProblem, ImportSet};
use ccal_core::problem::{FieldError, ProblemContent};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::admin_problems::save_revision;
use super::{TAG_IMPORT, TAG_SETS};
use crate::auth::{Principal, Scope};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::repo::{self, SetRow};
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_sets, create_set))
        .routes(routes!(get_set, update_set, archive_set))
        .routes(routes!(put_items))
        .routes(routes!(publish_set))
        .routes(routes!(unpublish_set))
        .routes(routes!(set_active))
        .routes(routes!(import))
        .routes(routes!(export))
}

#[derive(Serialize, ToSchema)]
pub struct SetItem {
    pub position: i32,
    pub problem_id: Uuid,
    pub slug: String,
    pub title: String,
    pub problem_status: String,
}

#[derive(Serialize, ToSchema)]
pub struct AdminSet {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description_md: String,
    pub learned_concepts: Vec<String>,
    pub listed: bool,
    /// `draft`, `published` or `archived`.
    pub status: String,
    /// This set is the site's active (default) set.
    pub active: bool,
    pub items: Vec<SetItem>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

async fn admin_set(st: &AppState, s: SetRow) -> ApiResult<AdminSet> {
    let items: Vec<(i32, Uuid, String, String, String)> = sqlx::query_as(
        "SELECT i.position, p.id, p.slug, h.title, p.status FROM problem_set_items i JOIN problems p ON p.id = i.problem_id
           JOIN problem_revisions h ON h.id = p.head_revision_id WHERE i.set_id = $1 ORDER BY i.position",
    )
    .bind(s.id)
    .fetch_all(&st.db)
    .await?;
    Ok(AdminSet {
        active: st.snapshot().active_set_id == Some(s.id),
        id: s.id,
        slug: s.slug,
        title: s.title,
        description_md: s.description_md,
        learned_concepts: s.learned_concepts,
        listed: s.listed,
        status: s.status,
        items: items
            .into_iter()
            .map(|(position, problem_id, slug, title, problem_status)| SetItem { position, problem_id, slug, title, problem_status })
            .collect(),
        created_at: s.created_at,
        updated_at: s.updated_at,
    })
}

/// All problem sets, including drafts.
#[utoipa::path(get, path = "/admin/problem-sets", tag = TAG_SETS, responses((status = 200, body = Vec<AdminSet>)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn list_sets(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Vec<AdminSet>>> {
    p.require(Scope::ProblemsRead)?;
    let rows: Vec<SetRow> = sqlx::query_as(&format!("{} ORDER BY created_at", repo::SET_SELECT)).fetch_all(&st.db).await?;
    let mut out = vec![];
    for r in rows {
        out.push(admin_set(&st, r).await?);
    }
    Ok(Json(out))
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetInput {
    /// Stable identifier (immutable).
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description_md: String,
    #[serde(default)]
    pub learned_concepts: Vec<String>,
    #[serde(default = "yes")]
    pub listed: bool,
    /// Problem ids or slugs, in order. Optional on create.
    #[serde(default)]
    pub problems: Vec<String>,
}

fn yes() -> bool {
    true
}

fn validate_set(s: &SetInput) -> Vec<FieldError> {
    let mut e = vec![];
    if !ccal_core::valid_slug(&s.slug) {
        e.push(FieldError::new("/slug", "invalid_slug", "slug 只能使用小寫英文字母、數字與單一連字號，長度 3–64"));
    }
    if s.title.trim().is_empty() || s.title.chars().count() > 120 {
        e.push(FieldError::new("/title", "invalid", "名稱需為 1–120 字"));
    }
    for (i, c) in s.learned_concepts.iter().enumerate() {
        if !ccal_core::valid_concept(c) {
            e.push(FieldError::new(format!("/learned_concepts/{i}"), "unknown_concept", format!("未知的概念 `{c}`")));
        }
    }
    e
}

/// Resolve ids/slugs to problem ids, reporting unknown ones with pointers.
async fn resolve_problems(st: &AppState, refs: &[String], base: &str) -> ApiResult<Vec<Uuid>> {
    let mut ids = vec![];
    let mut errors = vec![];
    let mut seen = BTreeSet::new();
    for (i, r) in refs.iter().enumerate() {
        match repo::find_problem(&st.db, r).await {
            Ok(p) if p.status == "archived" => errors.push(FieldError::new(format!("{base}/{i}"), "archived", format!("題目 `{r}` 已封存"))),
            Ok(p) => {
                if !seen.insert(p.id) {
                    errors.push(FieldError::new(format!("{base}/{i}"), "duplicate", format!("題目 `{r}` 重複")));
                }
                ids.push(p.id);
            }
            Err(_) => errors.push(FieldError::new(format!("{base}/{i}"), "unknown_problem", format!("找不到題目 `{r}`"))),
        }
    }
    if errors.is_empty() { Ok(ids) } else { Err(ApiError::validation(errors)) }
}

async fn replace_items(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, set_id: Uuid, ids: &[Uuid]) -> ApiResult<()> {
    // Only the association rows change; problems themselves are untouched.
    sqlx::query("DELETE FROM problem_set_items WHERE set_id = $1").bind(set_id).execute(&mut **tx).await?;
    for (i, pid) in ids.iter().enumerate() {
        sqlx::query("INSERT INTO problem_set_items (set_id, problem_id, position) VALUES ($1, $2, $3)")
            .bind(set_id)
            .bind(pid)
            .bind(i as i32 + 1)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query("UPDATE problem_sets SET updated_at = now() WHERE id = $1").bind(set_id).execute(&mut **tx).await?;
    Ok(())
}

/// Create a problem set (draft). Use `Idempotency-Key` for safe retries.
#[utoipa::path(post, path = "/admin/problem-sets", tag = TAG_SETS, request_body = SetInput, params(("Idempotency-Key" = Option<String>, Header)),
    responses((status = 201, body = AdminSet), (status = 409, body = crate::error::ProblemDetails), (status = 422, body = crate::error::ProblemDetails)),
    security(("token" = ["sets:write"]), ("session" = [])))]
async fn create_set(
    State(st): State<AppState>,
    p: Principal,
    headers: HeaderMap,
    JsonBody(req): JsonBody<SetInput>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    p.require(Scope::SetsWrite)?;
    let key = repo::idempotency_key(&headers);
    let hash = repo::request_hash(&req);
    let (code, v) = repo::idempotent(&st, &p, key.as_deref(), &hash, || async {
        let errors = validate_set(&req);
        if !errors.is_empty() {
            return Err(ApiError::validation(errors));
        }
        let ids = resolve_problems(&st, &req.problems, "/problems").await?;
        let id = Uuid::now_v7();
        let mut tx = st.db.begin().await?;
        sqlx::query("INSERT INTO problem_sets (id, slug, title, description_md, learned_concepts, listed) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(id)
            .bind(&req.slug)
            .bind(&req.title)
            .bind(&req.description_md)
            .bind(&req.learned_concepts)
            .bind(req.listed)
            .execute(&mut *tx)
            .await
            .map_err(|e| match ApiError::from(e) {
                e if e.code == "already_exists" => ApiError::conflict("slug_taken", format!("slug `{}` 已被使用", req.slug)),
                e => e,
            })?;
        replace_items(&mut tx, id, &ids).await?;
        tx.commit().await?;
        repo::audit(&st, &p, "set.create", &req.slug, serde_json::json!({})).await;
        let s = repo::find_set(&st.db, &id.to_string()).await?;
        Ok((StatusCode::CREATED, serde_json::to_value(admin_set(&st, s).await?).unwrap()))
    })
    .await?;
    Ok((code, Json(v)))
}

/// One set with its ordered problems.
#[utoipa::path(get, path = "/admin/problem-sets/{set}", tag = TAG_SETS, params(("set" = String, Path, description = "Id or slug")),
    responses((status = 200, body = AdminSet), (status = 404, body = crate::error::ProblemDetails)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn get_set(State(st): State<AppState>, p: Principal, Path(set): Path<String>) -> ApiResult<Json<AdminSet>> {
    p.require(Scope::ProblemsRead)?;
    let s = repo::find_set(&st.db, &set).await?;
    Ok(Json(admin_set(&st, s).await?))
}

/// Update title, description, concepts, listing and (optionally) items.
/// The slug cannot change.
#[utoipa::path(put, path = "/admin/problem-sets/{set}", tag = TAG_SETS, params(("set" = String, Path)), request_body = SetInput,
    responses((status = 200, body = AdminSet), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["sets:write"]), ("session" = [])))]
async fn update_set(State(st): State<AppState>, p: Principal, Path(set): Path<String>, JsonBody(req): JsonBody<SetInput>) -> ApiResult<Json<AdminSet>> {
    p.require(Scope::SetsWrite)?;
    let s = repo::find_set(&st.db, &set).await?;
    let mut errors = validate_set(&req);
    if req.slug != s.slug {
        errors.push(FieldError::new("/slug", "immutable", "slug 建立後不能修改"));
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    let ids = resolve_problems(&st, &req.problems, "/problems").await?;
    if s.status == "published" {
        check_publishable(&st, &ids, "/problems").await?;
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE problem_sets SET title = $2, description_md = $3, learned_concepts = $4, listed = $5, updated_at = now() WHERE id = $1")
        .bind(s.id)
        .bind(&req.title)
        .bind(&req.description_md)
        .bind(&req.learned_concepts)
        .bind(req.listed)
        .execute(&mut *tx)
        .await?;
    replace_items(&mut tx, s.id, &ids).await?;
    tx.commit().await?;
    repo::audit(&st, &p, "set.update", &s.slug, serde_json::json!({})).await;
    let s = repo::find_set(&st.db, &s.id.to_string()).await?;
    Ok(Json(admin_set(&st, s).await?))
}

/// Archive a set (hidden from students). Problems are untouched.
#[utoipa::path(delete, path = "/admin/problem-sets/{set}", tag = TAG_SETS, params(("set" = String, Path)),
    responses((status = 204), (status = 409, body = crate::error::ProblemDetails)), security(("token" = ["sets:write"]), ("session" = [])))]
async fn archive_set(State(st): State<AppState>, p: Principal, Path(set): Path<String>) -> ApiResult<StatusCode> {
    p.require(Scope::SetsWrite)?;
    let s = repo::find_set(&st.db, &set).await?;
    if st.snapshot().active_set_id == Some(s.id) {
        return Err(ApiError::conflict("active_set", "這是目前啟用的題目集，請先啟用其他題目集"));
    }
    sqlx::query("UPDATE problem_sets SET status = 'archived', updated_at = now() WHERE id = $1").bind(s.id).execute(&st.db).await?;
    repo::audit(&st, &p, "set.archive", &s.slug, serde_json::json!({})).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemsInput {
    /// Problem ids or slugs in display order. Problems not listed are removed
    /// from this set only.
    pub problems: Vec<String>,
}

/// Replace the ordered list of problems in a set.
#[utoipa::path(put, path = "/admin/problem-sets/{set}/items", tag = TAG_SETS, params(("set" = String, Path)), request_body = ItemsInput,
    responses((status = 200, body = AdminSet), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["sets:write"]), ("session" = [])))]
async fn put_items(State(st): State<AppState>, p: Principal, Path(set): Path<String>, JsonBody(req): JsonBody<ItemsInput>) -> ApiResult<Json<AdminSet>> {
    p.require(Scope::SetsWrite)?;
    let s = repo::find_set(&st.db, &set).await?;
    let ids = resolve_problems(&st, &req.problems, "/problems").await?;
    if s.status == "published" {
        check_publishable(&st, &ids, "/problems").await?;
    }
    let mut tx = st.db.begin().await?;
    replace_items(&mut tx, s.id, &ids).await?;
    tx.commit().await?;
    repo::audit(&st, &p, "set.items", &s.slug, serde_json::json!({"count": ids.len()})).await;
    let s = repo::find_set(&st.db, &s.id.to_string()).await?;
    Ok(Json(admin_set(&st, s).await?))
}

/// Every problem in a published set must itself be published.
async fn check_publishable(st: &AppState, ids: &[Uuid], base: &str) -> ApiResult<()> {
    let mut errors = vec![];
    for (i, id) in ids.iter().enumerate() {
        let (slug, status): (String, String) = sqlx::query_as("SELECT slug, status FROM problems WHERE id = $1").bind(id).fetch_one(&st.db).await?;
        if status != "published" {
            errors.push(FieldError::new(format!("{base}/{i}"), "problem_not_published", format!("題目 `{slug}` 尚未發布")));
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(ApiError::validation(errors)) }
}

/// Publish a set. Every problem in it must already be published.
#[utoipa::path(post, path = "/admin/problem-sets/{set}/publish", tag = TAG_SETS, params(("set" = String, Path)),
    responses((status = 200, body = AdminSet), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["publish"]), ("session" = [])))]
async fn publish_set(State(st): State<AppState>, p: Principal, Path(set): Path<String>) -> ApiResult<Json<AdminSet>> {
    p.require(Scope::Publish)?;
    let s = repo::find_set(&st.db, &set).await?;
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT problem_id FROM problem_set_items WHERE set_id = $1 ORDER BY position").bind(s.id).fetch_all(&st.db).await?;
    if ids.is_empty() {
        return Err(ApiError::validation(vec![FieldError::new("/problems", "empty", "題目集至少需要一個題目")]));
    }
    check_publishable(&st, &ids, "/problems").await?;
    sqlx::query("UPDATE problem_sets SET status = 'published', published_at = now(), updated_at = now() WHERE id = $1").bind(s.id).execute(&st.db).await?;
    repo::audit(&st, &p, "set.publish", &s.slug, serde_json::json!({})).await;
    let s = repo::find_set(&st.db, &s.id.to_string()).await?;
    Ok(Json(admin_set(&st, s).await?))
}

/// Return a set to draft (hidden from students).
#[utoipa::path(post, path = "/admin/problem-sets/{set}/unpublish", tag = TAG_SETS, params(("set" = String, Path)),
    responses((status = 200, body = AdminSet), (status = 409, body = crate::error::ProblemDetails)), security(("token" = ["publish"]), ("session" = [])))]
async fn unpublish_set(State(st): State<AppState>, p: Principal, Path(set): Path<String>) -> ApiResult<Json<AdminSet>> {
    p.require(Scope::Publish)?;
    let s = repo::find_set(&st.db, &set).await?;
    if st.snapshot().active_set_id == Some(s.id) {
        return Err(ApiError::conflict("active_set", "這是目前啟用的題目集，請先啟用其他題目集"));
    }
    sqlx::query("UPDATE problem_sets SET status = 'draft', updated_at = now() WHERE id = $1").bind(s.id).execute(&st.db).await?;
    repo::audit(&st, &p, "set.unpublish", &s.slug, serde_json::json!({})).await;
    let s = repo::find_set(&st.db, &s.id.to_string()).await?;
    Ok(Json(admin_set(&st, s).await?))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActiveSetInput {
    /// Id or slug of a published set.
    pub problem_set: String,
}

#[derive(Serialize, ToSchema)]
pub struct ActiveSet {
    pub active_set_id: Uuid,
    pub slug: String,
    pub config_version: i64,
}

/// Make a published set the site's active (default) set. Takes effect
/// immediately; open editors are told about the switch and keep their code.
#[utoipa::path(put, path = "/admin/site/active-problem-set", tag = TAG_SETS, request_body = ActiveSetInput,
    responses((status = 200, body = ActiveSet), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["publish"]), ("session" = [])))]
async fn set_active(State(st): State<AppState>, p: Principal, JsonBody(req): JsonBody<ActiveSetInput>) -> ApiResult<Json<ActiveSet>> {
    p.require(Scope::Publish)?;
    let s = repo::find_set(&st.db, &req.problem_set).await?;
    if s.status != "published" {
        return Err(ApiError::validation(vec![FieldError::new("/problem_set", "not_published", "只能啟用已發布的題目集")]));
    }
    sqlx::query("UPDATE site_config SET active_set_id = $1 WHERE id = 1").bind(s.id).execute(&st.db).await?;
    sqlx::query("INSERT INTO config_revisions (active_set_id, settings, created_by) SELECT active_set_id, settings, $1 FROM site_config WHERE id = 1")
        .bind(p.user_id)
        .execute(&st.db)
        .await?;
    st.config_changed().await?;
    repo::audit(&st, &p, "site.active_set", &s.slug, serde_json::json!({})).await;
    Ok(Json(ActiveSet { active_set_id: s.id, slug: s.slug, config_version: st.snapshot().version }))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ImportParams {
    /// Validate and report what would happen without writing anything.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, ToSchema)]
pub struct ImportItemResult {
    /// JSON Pointer of the item in the document.
    pub pointer: String,
    pub slug: String,
    /// `create`, `update` (new revision / changed set) or `unchanged`.
    pub action: String,
    pub id: Option<Uuid>,
    pub revision_number: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportReport {
    pub dry_run: bool,
    /// True when the document is valid (and, unless dry-run, was applied).
    pub ok: bool,
    pub errors: Vec<FieldError>,
    pub problems: Vec<ImportItemResult>,
    pub problem_sets: Vec<ImportItemResult>,
}

/// Import problems and sets from `cycu-cal.import/v1` JSON.
///
/// - Items are matched by `slug`: identical content is a no-op, changed
///   content creates a new revision, so retries never duplicate anything.
/// - All-or-nothing: the whole document is applied in one transaction.
/// - Everything imported stays **draft**: publishing and activating are
///   separate, explicit calls.
/// - `?dry_run=true` validates and reports actions without writing.
#[utoipa::path(post, path = "/admin/import", tag = TAG_IMPORT, params(ImportParams, ("Idempotency-Key" = Option<String>, Header)), request_body = ImportDocument,
    responses((status = 200, body = ImportReport), (status = 422, body = crate::error::ProblemDetails, description = "problem+json; `errors` has JSON Pointers, `data` holds the full ImportReport")), security(("token" = ["problems:write", "sets:write"]), ("session" = [])))]
async fn import(
    State(st): State<AppState>,
    p: Principal,
    headers: HeaderMap,
    Query(q): Query<ImportParams>,
    JsonBody(doc): JsonBody<ImportDocument>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    p.require(Scope::ProblemsWrite)?;
    if !doc.problem_sets.is_empty() {
        p.require(Scope::SetsWrite)?;
    }
    let key = if q.dry_run { None } else { repo::idempotency_key(&headers) };
    let hash = repo::request_hash(&doc);
    let (code, v) = repo::idempotent(&st, &p, key.as_deref(), &hash, || async {
        let report = run_import(&st, &p, &doc, q.dry_run).await?;
        if !report.ok {
            // problem+json like every other error; the full report is in `data`.
            let errors = report.errors.clone();
            return Err(ApiError::validation(errors).with_data(serde_json::to_value(&report).unwrap()));
        }
        Ok((StatusCode::OK, serde_json::to_value(report).unwrap()))
    })
    .await?;
    Ok((code, Json(v)))
}

async fn run_import(st: &AppState, p: &Principal, doc: &ImportDocument, dry_run: bool) -> ApiResult<ImportReport> {
    let existing: Vec<(String, Uuid, String, Option<String>, Option<i32>)> =
        sqlx::query_as("SELECT p.slug, p.id, p.status, r.content_hash, r.number FROM problems p LEFT JOIN problem_revisions r ON r.id = p.head_revision_id")
            .fetch_all(&st.db)
            .await?;
    let existing: BTreeMap<String, (Uuid, String, Option<String>, Option<i32>)> =
        existing.into_iter().map(|(s, id, status, h, n)| (s, (id, status, h, n))).collect();
    let slugs: BTreeSet<String> = existing.keys().cloned().collect();
    let mut errors = ccal_core::import::validate(doc, &slugs);
    for (i, pr) in doc.problems.iter().enumerate() {
        if existing.get(&pr.slug).is_some_and(|e| e.1 == "archived") {
            errors.push(FieldError::new(format!("/problems/{i}/slug"), "archived", format!("題目 `{}` 已封存，不能透過匯入修改", pr.slug)));
        }
    }
    let mut report = ImportReport { dry_run, ok: errors.is_empty(), errors, problems: vec![], problem_sets: vec![] };
    if !report.ok {
        return Ok(report);
    }

    let mut tx = st.db.begin().await?;
    let mut ids: BTreeMap<String, Uuid> = existing.iter().map(|(s, e)| (s.clone(), e.0)).collect();
    for (i, ImportProblem { slug, content }) in doc.problems.iter().enumerate() {
        let pointer = format!("/problems/{i}");
        let hash = content.content_hash();
        match existing.get(slug) {
            Some((id, _, h, n)) if h.as_deref() == Some(hash.as_str()) => {
                report.problems.push(ImportItemResult { pointer, slug: slug.clone(), action: "unchanged".into(), id: Some(*id), revision_number: *n });
            }
            Some((id, _, _, n)) => {
                if !dry_run {
                    save_revision(&mut tx, *id, content, p.user_id).await?;
                }
                report.problems.push(ImportItemResult {
                    pointer,
                    slug: slug.clone(),
                    action: "update".into(),
                    id: Some(*id),
                    revision_number: n.map(|n| n + 1),
                });
            }
            None => {
                let id = Uuid::now_v7();
                if !dry_run {
                    sqlx::query("INSERT INTO problems (id, slug) VALUES ($1, $2)").bind(id).bind(slug).execute(&mut *tx).await?;
                    save_revision(&mut tx, id, content, p.user_id).await?;
                }
                ids.insert(slug.clone(), id);
                report.problems.push(ImportItemResult {
                    pointer,
                    slug: slug.clone(),
                    action: "create".into(),
                    id: (!dry_run).then_some(id),
                    revision_number: Some(1),
                });
            }
        }
    }
    for (i, s) in doc.problem_sets.iter().enumerate() {
        let pointer = format!("/problem_sets/{i}");
        let current: Option<SetRow> = sqlx::query_as(&format!("{} WHERE slug = $1", repo::SET_SELECT)).bind(&s.slug).fetch_optional(&mut *tx).await?;
        let problem_ids: Vec<Uuid> = s.problems.iter().map(|ps| ids[ps]).collect();
        match current {
            Some(cur) => {
                let cur_items: Vec<Uuid> =
                    sqlx::query_scalar("SELECT problem_id FROM problem_set_items WHERE set_id = $1 ORDER BY position").bind(cur.id).fetch_all(&mut *tx).await?;
                let same = cur.title == s.title
                    && cur.description_md == s.description_md
                    && cur.learned_concepts == s.learned_concepts
                    && cur.listed == s.listed
                    && cur_items == problem_ids;
                if !same && !dry_run {
                    if cur.status == "published" {
                        // Keep the "published sets only contain published problems" rule.
                        for (j, pid) in problem_ids.iter().enumerate() {
                            let status: String = sqlx::query_scalar("SELECT status FROM problems WHERE id = $1").bind(pid).fetch_one(&mut *tx).await?;
                            if status != "published" {
                                report.errors.push(FieldError::new(
                                    format!("{pointer}/problems/{j}"),
                                    "problem_not_published",
                                    "已發布的題目集只能包含已發布的題目",
                                ));
                            }
                        }
                    }
                    sqlx::query(
                        "UPDATE problem_sets SET title = $2, description_md = $3, learned_concepts = $4, listed = $5, updated_at = now() WHERE id = $1",
                    )
                    .bind(cur.id)
                    .bind(&s.title)
                    .bind(&s.description_md)
                    .bind(&s.learned_concepts)
                    .bind(s.listed)
                    .execute(&mut *tx)
                    .await?;
                    replace_items(&mut tx, cur.id, &problem_ids).await?;
                }
                report.problem_sets.push(ImportItemResult {
                    pointer,
                    slug: s.slug.clone(),
                    action: if same { "unchanged" } else { "update" }.into(),
                    id: Some(cur.id),
                    revision_number: None,
                });
            }
            None => {
                let id = Uuid::now_v7();
                if !dry_run {
                    sqlx::query("INSERT INTO problem_sets (id, slug, title, description_md, learned_concepts, listed) VALUES ($1, $2, $3, $4, $5, $6)")
                        .bind(id)
                        .bind(&s.slug)
                        .bind(&s.title)
                        .bind(&s.description_md)
                        .bind(&s.learned_concepts)
                        .bind(s.listed)
                        .execute(&mut *tx)
                        .await?;
                    replace_items(&mut tx, id, &problem_ids).await?;
                }
                report.problem_sets.push(ImportItemResult {
                    pointer,
                    slug: s.slug.clone(),
                    action: "create".into(),
                    id: (!dry_run).then_some(id),
                    revision_number: None,
                });
            }
        }
    }
    if !report.errors.is_empty() {
        report.ok = false;
        tx.rollback().await?;
        return Ok(report);
    }
    if dry_run {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
        repo::audit(st, p, "import", "", serde_json::json!({"problems": doc.problems.len(), "sets": doc.problem_sets.len()})).await;
    }
    Ok(report)
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct ExportParams {
    /// Export one set (id or slug) and its problems; default: everything.
    pub problem_set: Option<String>,
    /// Use the published revision instead of the head revision.
    #[serde(default)]
    pub published: bool,
}

/// Export problems and sets as an import document (round-trips through `/admin/import`).
#[utoipa::path(get, path = "/admin/export", tag = TAG_IMPORT, params(ExportParams), responses((status = 200, body = ImportDocument)), security(("token" = ["problems:read"]), ("session" = [])))]
async fn export(State(st): State<AppState>, p: Principal, Query(q): Query<ExportParams>) -> ApiResult<Json<ImportDocument>> {
    p.require(Scope::ProblemsRead)?;
    let sets: Vec<SetRow> = match &q.problem_set {
        Some(s) => vec![repo::find_set(&st.db, s).await?],
        None => sqlx::query_as(&format!("{} WHERE status <> 'archived' ORDER BY created_at", repo::SET_SELECT)).fetch_all(&st.db).await?,
    };
    let col = if q.published { "published_revision_id" } else { "head_revision_id" };
    let problem_rows: Vec<(Uuid, String, serde_json::Value)> = if q.problem_set.is_some() {
        sqlx::query_as(&format!(
            "SELECT p.id, p.slug, r.content FROM problem_set_items i JOIN problems p ON p.id = i.problem_id JOIN problem_revisions r ON r.id = p.{col} WHERE i.set_id = $1 ORDER BY i.position"
        ))
        .bind(sets[0].id)
        .fetch_all(&st.db)
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT p.id, p.slug, r.content FROM problems p JOIN problem_revisions r ON r.id = p.{col} WHERE p.status <> 'archived' ORDER BY p.slug"
        ))
        .fetch_all(&st.db)
        .await?
    };
    let mut problems = vec![];
    for (_, slug, content) in problem_rows {
        let content: ProblemContent = serde_json::from_value(content).map_err(ApiError::internal)?;
        problems.push(ImportProblem { slug, content });
    }
    let mut problem_sets = vec![];
    for s in sets {
        let slugs: Vec<String> =
            sqlx::query_scalar("SELECT p.slug FROM problem_set_items i JOIN problems p ON p.id = i.problem_id WHERE i.set_id = $1 ORDER BY i.position")
                .bind(s.id)
                .fetch_all(&st.db)
                .await?;
        problem_sets.push(ImportSet {
            slug: s.slug,
            title: s.title,
            description_md: s.description_md,
            learned_concepts: s.learned_concepts,
            listed: s.listed,
            problems: slugs,
        });
    }
    Ok(Json(ImportDocument { format_version: FORMAT_VERSION.into(), problems, problem_sets }))
}
