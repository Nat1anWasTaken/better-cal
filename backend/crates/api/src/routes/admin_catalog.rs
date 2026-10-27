//! Online editing of the teaching text (diagnostics catalog).
//!
//! Edits are saved as drafts, previewed against real compiler output, and
//! published together. Publishing takes effect immediately (no restart).

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use ccal_core::problem::FieldError;
use chrono::{DateTime, Utc};
use diagnostics::catalog::{RuleEntry, validate_custom_rule, validate_entry};
use diagnostics::{Card, CatalogError, CustomRule, RenderContext};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_CATALOG;
use crate::auth::{Principal, Scope};
use crate::catalog_store::EntryRow;
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::judge::evaluate::observations;
use crate::repo;
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(overview))
        .routes(routes!(get_rule, put_rule))
        .routes(routes!(discard_draft))
        .routes(routes!(reset_rule))
        .routes(routes!(get_texts, put_texts))
        .routes(routes!(get_custom_rule, put_custom_rule, delete_custom_rule))
        .routes(routes!(publish))
        .routes(routes!(history))
        .routes(routes!(preview))
}

fn catalog_errors(errs: Vec<CatalogError>) -> ApiError {
    ApiError::validation(errs.into_iter().map(|e| FieldError::new(e.pointer, "invalid", e.message)).collect())
}

#[derive(Serialize, ToSchema)]
pub struct RuleListItem {
    pub id: String,
    /// `builtin` (unchanged), `edited` (published override) or `custom`.
    pub state: String,
    pub has_draft: bool,
    pub enabled: bool,
    pub priority: i32,
    pub title: String,
    /// Variables available in this rule's templates.
    pub vars: Vec<String>,
    /// Submissions in the last 7 days that hit this rule.
    pub hits_7d: i64,
}

#[derive(Serialize, ToSchema)]
pub struct CatalogOverview {
    pub rules: Vec<RuleListItem>,
    pub texts_have_draft: bool,
    /// Number of entries with unpublished changes.
    pub pending_changes: usize,
    pub version: i64,
}

async fn rows(st: &AppState) -> ApiResult<Vec<EntryRow>> {
    Ok(sqlx::query_as("SELECT key, draft, published, draft_delete FROM catalog_entries ORDER BY key").fetch_all(&st.db).await?)
}

/// All rules with their state, unpublished drafts and recent usage.
#[utoipa::path(get, path = "/admin/catalog", tag = TAG_CATALOG, responses((status = 200, body = CatalogOverview)), security(("token" = ["content:write"]), ("session" = [])))]
async fn overview(State(st): State<AppState>, p: Principal) -> ApiResult<Json<CatalogOverview>> {
    p.require(Scope::ContentWrite)?;
    let snap = st.snapshot();
    let rows = rows(&st).await?;
    let pending: BTreeSet<&str> = rows.iter().filter(|r| r.draft.is_some() || r.draft_delete).map(|r| r.key.as_str()).collect();
    let edited: BTreeSet<&str> = rows.iter().filter(|r| r.published.is_some()).map(|r| r.key.as_str()).collect();
    let hits: Vec<(String, i64)> =
        sqlx::query_as("SELECT r, count(*) FROM submissions s, unnest(s.finding_rules) r WHERE s.created_at > now() - interval '7 days' GROUP BY r")
            .fetch_all(&st.db)
            .await?;
    let hits: BTreeMap<String, i64> = hits.into_iter().collect();
    let mut items = vec![];
    for spec in diagnostics::SPECS {
        let key = format!("rule:{}", spec.id);
        let e = snap.catalog.rules.get(spec.id);
        items.push(RuleListItem {
            id: spec.id.to_string(),
            state: if edited.contains(key.as_str()) { "edited" } else { "builtin" }.into(),
            has_draft: pending.contains(key.as_str()),
            enabled: e.is_none_or(|e| e.enabled),
            priority: e.map(|e| e.priority).unwrap_or(0),
            title: e.map(|e| e.title.clone()).unwrap_or_default(),
            vars: snap.catalog.allowed_vars(spec.id).unwrap_or_default(),
            hits_7d: hits.get(spec.id).copied().unwrap_or(0),
        });
    }
    let mut custom_ids: BTreeSet<String> = snap.catalog_with_drafts.custom_rules.iter().map(|c| c.id.clone()).collect();
    custom_ids.extend(snap.catalog.custom_rules.iter().map(|c| c.id.clone()));
    for id in custom_ids {
        let c = snap.catalog_with_drafts.custom_rules.iter().chain(snap.catalog.custom_rules.iter()).find(|c| c.id == id).unwrap();
        items.push(RuleListItem {
            id: id.clone(),
            state: "custom".into(),
            has_draft: pending.contains(format!("custom:{id}").as_str()),
            enabled: c.enabled,
            priority: c.priority,
            title: c.title.clone(),
            vars: snap.catalog_with_drafts.allowed_vars(&id).unwrap_or_default(),
            hits_7d: hits.get(&id).copied().unwrap_or(0),
        });
    }
    Ok(Json(CatalogOverview { rules: items, texts_have_draft: pending.contains("texts"), pending_changes: pending.len(), version: snap.version }))
}

#[derive(Serialize, ToSchema)]
pub struct RuleEditor {
    pub id: String,
    pub vars: Vec<String>,
    /// Wording shipped with the system.
    #[schema(value_type = Option<Object>)]
    pub builtin: Option<RuleEntry>,
    /// Wording students currently see.
    #[schema(value_type = Object)]
    pub published: RuleEntry,
    /// Unpublished edit, if any.
    #[schema(value_type = Option<Object>)]
    pub draft: Option<RuleEntry>,
    /// Publishing will revert this rule to the built-in wording.
    pub reset_pending: bool,
}

/// One rule's built-in, published and draft wording.
#[utoipa::path(get, path = "/admin/catalog/rules/{id}", tag = TAG_CATALOG, params(("id" = String, Path, description = "Rule id, e.g. syntax.missing_semicolon")),
    responses((status = 200, body = RuleEditor), (status = 404, body = crate::error::ProblemDetails)), security(("token" = ["content:write"]), ("session" = [])))]
async fn get_rule(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<Json<RuleEditor>> {
    p.require(Scope::ContentWrite)?;
    if diagnostics::spec(&id).is_none() {
        return Err(ApiError::not_found("這個規則"));
    }
    let snap = st.snapshot();
    let row: Option<EntryRow> = sqlx::query_as("SELECT key, draft, published, draft_delete FROM catalog_entries WHERE key = $1")
        .bind(format!("rule:{id}"))
        .fetch_optional(&st.db)
        .await?;
    Ok(Json(RuleEditor {
        vars: snap.catalog.allowed_vars(&id).unwrap_or_default(),
        builtin: snap.builtin.rules.get(&id).cloned(),
        published: snap.catalog.rules.get(&id).cloned().ok_or_else(|| ApiError::not_found("這個規則"))?,
        draft: row.as_ref().and_then(|r| r.draft.clone()).and_then(|d| serde_json::from_value(d).ok()),
        reset_pending: row.is_some_and(|r| r.draft_delete),
        id,
    }))
}

async fn save_draft(st: &AppState, p: &Principal, key: &str, draft: Option<serde_json::Value>, delete: bool) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO catalog_entries (key, draft, draft_delete, updated_by) VALUES ($1, $2, $3, $4)
         ON CONFLICT (key) DO UPDATE SET draft = EXCLUDED.draft, draft_delete = EXCLUDED.draft_delete, updated_at = now(), updated_by = EXCLUDED.updated_by",
    )
    .bind(key)
    .bind(draft)
    .bind(delete)
    .bind(p.user_id)
    .execute(&st.db)
    .await?;
    // Drafts are visible in previews immediately.
    st.reload_config().await?;
    Ok(())
}

/// Save a draft of a rule's wording. Templates are validated against the
/// variables the rule provides. Students see it only after publishing.
#[utoipa::path(put, path = "/admin/catalog/rules/{id}", tag = TAG_CATALOG, params(("id" = String, Path)), request_body(content = Object, description = "A RuleEntry (same shape as `published`)"),
    responses((status = 200, body = RuleEditor), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["content:write"]), ("session" = [])))]
async fn put_rule(State(st): State<AppState>, p: Principal, Path(id): Path<String>, JsonBody(entry): JsonBody<RuleEntry>) -> ApiResult<Json<RuleEditor>> {
    p.require(Scope::ContentWrite)?;
    let Some(allowed) = st.snapshot().catalog.allowed_vars(&id).filter(|_| diagnostics::spec(&id).is_some()) else {
        return Err(ApiError::not_found("這個規則"));
    };
    let errs = validate_entry("", &entry, &allowed);
    if !errs.is_empty() {
        return Err(catalog_errors(errs));
    }
    save_draft(&st, &p, &format!("rule:{id}"), Some(serde_json::to_value(&entry).unwrap()), false).await?;
    repo::audit(&st, &p, "catalog.draft", &id, serde_json::json!({})).await;
    get_rule(State(st), p, Path(id)).await
}

/// Discard an unpublished draft (rule or custom rule; use id `texts` for shared texts).
#[utoipa::path(delete, path = "/admin/catalog/drafts/{key}", tag = TAG_CATALOG, params(("key" = String, Path, description = "`rule:<id>`, `custom:<id>` or `texts`")),
    responses((status = 204)), security(("token" = ["content:write"]), ("session" = [])))]
async fn discard_draft(State(st): State<AppState>, p: Principal, Path(key): Path<String>) -> ApiResult<StatusCode> {
    p.require(Scope::ContentWrite)?;
    sqlx::query("UPDATE catalog_entries SET draft = NULL, draft_delete = false, updated_at = now() WHERE key = $1").bind(&key).execute(&st.db).await?;
    sqlx::query("DELETE FROM catalog_entries WHERE key = $1 AND published IS NULL AND draft IS NULL").bind(&key).execute(&st.db).await?;
    st.reload_config().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Stage a revert to the built-in wording (applied on publish).
#[utoipa::path(post, path = "/admin/catalog/rules/{id}/reset", tag = TAG_CATALOG, params(("id" = String, Path)),
    responses((status = 200, body = RuleEditor)), security(("token" = ["content:write"]), ("session" = [])))]
async fn reset_rule(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<Json<RuleEditor>> {
    p.require(Scope::ContentWrite)?;
    if diagnostics::spec(&id).is_none() {
        return Err(ApiError::not_found("這個規則"));
    }
    save_draft(&st, &p, &format!("rule:{id}"), None, true).await?;
    get_rule(State(st), p, Path(id)).await
}

#[derive(Serialize, ToSchema)]
pub struct TextsEditor {
    pub builtin: BTreeMap<String, String>,
    pub published: BTreeMap<String, String>,
    pub draft: Option<BTreeMap<String, String>>,
    pub required_keys: Vec<String>,
}

/// Shared texts: confidence labels, notes, comparison-rule descriptions.
#[utoipa::path(get, path = "/admin/catalog/texts", tag = TAG_CATALOG, responses((status = 200, body = TextsEditor)), security(("token" = ["content:write"]), ("session" = [])))]
async fn get_texts(State(st): State<AppState>, p: Principal) -> ApiResult<Json<TextsEditor>> {
    p.require(Scope::ContentWrite)?;
    let snap = st.snapshot();
    let draft: Option<serde_json::Value> = sqlx::query_scalar("SELECT draft FROM catalog_entries WHERE key = 'texts'").fetch_optional(&st.db).await?.flatten();
    Ok(Json(TextsEditor {
        builtin: snap.builtin.texts.clone(),
        published: snap.catalog.texts.clone(),
        draft: draft.and_then(|d| serde_json::from_value(d).ok()),
        required_keys: diagnostics::catalog::REQUIRED_TEXTS.iter().map(|s| s.to_string()).collect(),
    }))
}

/// Save a draft of the shared texts (full map).
#[utoipa::path(put, path = "/admin/catalog/texts", tag = TAG_CATALOG, request_body = BTreeMap<String, String>,
    responses((status = 200, body = TextsEditor), (status = 422, body = crate::error::ProblemDetails)), security(("token" = ["content:write"]), ("session" = [])))]
async fn put_texts(State(st): State<AppState>, p: Principal, JsonBody(texts): JsonBody<BTreeMap<String, String>>) -> ApiResult<Json<TextsEditor>> {
    p.require(Scope::ContentWrite)?;
    let mut errors = vec![];
    for k in diagnostics::catalog::REQUIRED_TEXTS {
        if texts.get(*k).is_none_or(|v| v.trim().is_empty()) {
            errors.push(FieldError::new(format!("/{k}"), "required", "這個文字不可空白"));
        }
    }
    for (k, v) in &texts {
        if let Err(e) = diagnostics::template::validate(v, diagnostics::COMMON_VARS) {
            errors.push(FieldError::new(format!("/{k}"), "invalid", e.to_string()));
        }
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    save_draft(&st, &p, "texts", Some(serde_json::to_value(&texts).unwrap()), false).await?;
    get_texts(State(st), p).await
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomRuleInput {
    #[schema(value_type = Object)]
    pub rule: CustomRule,
    /// Programs that must produce a diagnostic matched by this rule
    /// (compiled with the real toolchain). At least one is required.
    pub positive_examples: Vec<String>,
    /// Programs that must NOT be matched by this rule.
    #[serde(default)]
    pub negative_examples: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CustomRuleEditor {
    pub id: String,
    /// Version students currently get (absent if never published).
    #[schema(value_type = Option<Object>)]
    pub published: Option<CustomRule>,
    /// Unpublished edit, if any.
    #[schema(value_type = Option<Object>)]
    pub draft: Option<CustomRule>,
    /// Publishing will remove this rule.
    pub delete_pending: bool,
}

/// One custom rule: published value and unpublished draft.
#[utoipa::path(get, path = "/admin/catalog/custom-rules/{id}", tag = TAG_CATALOG, params(("id" = String, Path)),
    responses((status = 200, body = CustomRuleEditor), (status = 404, body = crate::error::ProblemDetails)), security(("token" = ["content:write"]), ("session" = [])))]
async fn get_custom_rule(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<Json<CustomRuleEditor>> {
    p.require(Scope::ContentWrite)?;
    let row: Option<EntryRow> = sqlx::query_as("SELECT key, draft, published, draft_delete FROM catalog_entries WHERE key = $1")
        .bind(format!("custom:{id}"))
        .fetch_optional(&st.db)
        .await?;
    let row = row.ok_or_else(|| ApiError::not_found("這個自訂規則"))?;
    let parse = |v: Option<serde_json::Value>| v.and_then(|v| serde_json::from_value::<CustomRule>(v).ok());
    Ok(Json(CustomRuleEditor { id, published: parse(row.published), draft: parse(row.draft), delete_pending: row.draft_delete }))
}

#[derive(Serialize, ToSchema)]
pub struct CustomRuleResult {
    pub id: String,
    /// For each positive example: the compiler message that matched.
    pub matched_messages: Vec<String>,
}

async fn compile_messages(st: &AppState, source: &str) -> ApiResult<(judge_proto::CompileResult, Vec<diagnostics::gcc::Diag>)> {
    let req = judge_proto::ExecuteRequest { source: source.to_string(), tests: vec![], limits: judge_proto::Limits::default(), stop_on_failure: false };
    let resp = st.runner.execute(&req).await.map_err(|e| ApiError::unavailable(format!("評測服務無法使用，無法編譯範例：{e}")))?;
    let diags = diagnostics::gcc::parse(&resp.compile.diagnostics_json);
    Ok((resp.compile, diags))
}

/// Create or update a custom rule (draft). Its regex is checked against real
/// compiler output from the examples; custom rules are always shown as guesses.
#[utoipa::path(put, path = "/admin/catalog/custom-rules/{id}", tag = TAG_CATALOG, params(("id" = String, Path, description = "Must start with `custom.`")), request_body = CustomRuleInput,
    responses((status = 200, body = CustomRuleResult), (status = 422, body = crate::error::ProblemDetails), (status = 503, body = crate::error::ProblemDetails)),
    security(("token" = ["content:write"]), ("session" = [])))]
async fn put_custom_rule(
    State(st): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<CustomRuleInput>,
) -> ApiResult<Json<CustomRuleResult>> {
    p.require(Scope::ContentWrite)?;
    let mut errors: Vec<FieldError> = validate_custom_rule("/rule", &req.rule).into_iter().map(|e| FieldError::new(e.pointer, "invalid", e.message)).collect();
    if req.rule.id != id {
        errors.push(FieldError::new("/rule/id", "mismatch", "rule.id 必須和網址中的 id 相同"));
    }
    if req.positive_examples.is_empty() {
        errors.push(FieldError::new("/positive_examples", "required", "至少需要一個會觸發這個規則的範例程式"));
    }
    if req.positive_examples.len() + req.negative_examples.len() > 10 {
        errors.push(FieldError::new("/positive_examples", "too_many", "範例最多 10 個"));
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    let re = req.rule.regex().map_err(ApiError::internal)?;
    let mut matched = vec![];
    for (i, src) in req.positive_examples.iter().enumerate() {
        let (_, diags) = compile_messages(&st, src).await?;
        match diags.iter().find(|d| re.is_match(&d.message)) {
            Some(d) => matched.push(d.message.clone()),
            None => errors.push(FieldError::new(
                format!("/positive_examples/{i}"),
                "no_match",
                format!("編譯器的訊息沒有符合這個規則（實際訊息：{}）", diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join(" | ")),
            )),
        }
    }
    for (i, src) in req.negative_examples.iter().enumerate() {
        let (compile, _) = compile_messages(&st, src).await?;
        let adj = BTreeMap::new();
        let customs = vec![req.rule.clone()];
        let a = diagnostics::analyze(&diagnostics::AnalysisInput {
            source: src,
            compile: Some(&compile),
            runs: &[],
            fixed_regions: &[],
            system_error: false,
            custom_rules: &customs,
            priority_adjust: &adj,
        });
        if a.findings.iter().any(|f| f.rule == id) {
            errors.push(FieldError::new(format!("/negative_examples/{i}"), "unexpected_match", "這個反例也被規則比對到了，請讓規則更精確"));
        }
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    save_draft(&st, &p, &format!("custom:{id}"), Some(serde_json::to_value(&req.rule).unwrap()), false).await?;
    repo::audit(&st, &p, "catalog.custom_rule", &id, serde_json::json!({})).await;
    Ok(Json(CustomRuleResult { id, matched_messages: matched }))
}

/// Stage removal of a custom rule (applied on publish).
#[utoipa::path(delete, path = "/admin/catalog/custom-rules/{id}", tag = TAG_CATALOG, params(("id" = String, Path)), responses((status = 204)), security(("token" = ["content:write"]), ("session" = [])))]
async fn delete_custom_rule(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<StatusCode> {
    p.require(Scope::ContentWrite)?;
    save_draft(&st, &p, &format!("custom:{id}"), None, true).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct PublishResult {
    pub published_keys: Vec<String>,
    pub version: i64,
}

/// Publish every pending draft at once. Takes effect immediately for all
/// students, including when they re-open earlier submissions.
#[utoipa::path(post, path = "/admin/catalog/publish", tag = TAG_CATALOG, responses((status = 200, body = PublishResult), (status = 422, body = crate::error::ProblemDetails)),
    security(("token" = ["content:write"]), ("session" = [])))]
async fn publish(State(st): State<AppState>, p: Principal) -> ApiResult<Json<PublishResult>> {
    p.require(Scope::ContentWrite)?;
    let snap = st.snapshot();
    let errs = snap.catalog_with_drafts.validate();
    if !errs.is_empty() {
        return Err(catalog_errors(errs));
    }
    let mut tx = st.db.begin().await?;
    let pending: Vec<EntryRow> =
        sqlx::query_as("SELECT key, draft, published, draft_delete FROM catalog_entries WHERE draft IS NOT NULL OR draft_delete FOR UPDATE")
            .fetch_all(&mut *tx)
            .await?;
    let mut keys = vec![];
    for r in pending {
        let new_published = if r.draft_delete { None } else { r.draft.clone() };
        sqlx::query("INSERT INTO catalog_history (key, published, created_by) VALUES ($1, $2, $3)")
            .bind(&r.key)
            .bind(&new_published)
            .bind(p.user_id)
            .execute(&mut *tx)
            .await?;
        if new_published.is_none() {
            sqlx::query("DELETE FROM catalog_entries WHERE key = $1").bind(&r.key).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE catalog_entries SET published = draft, draft = NULL, draft_delete = false, updated_at = now() WHERE key = $1")
                .bind(&r.key)
                .execute(&mut *tx)
                .await?;
        }
        keys.push(r.key);
    }
    tx.commit().await?;
    st.config_changed().await?;
    repo::audit(&st, &p, "catalog.publish", "", serde_json::json!({"keys": keys})).await;
    Ok(Json(PublishResult { published_keys: keys, version: st.snapshot().version }))
}

#[derive(Serialize, ToSchema)]
pub struct HistoryEntry {
    pub id: i64,
    pub key: String,
    /// The published value (null = reverted to built-in / removed).
    #[schema(value_type = Option<Object>)]
    pub published: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<String>,
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct HistoryParams {
    pub key: Option<String>,
}

/// Publish history (newest first). To roll back, PUT an older value as a draft and publish.
#[utoipa::path(get, path = "/admin/catalog/history", tag = TAG_CATALOG, params(HistoryParams), responses((status = 200, body = Vec<HistoryEntry>)), security(("token" = ["content:write"]), ("session" = [])))]
async fn history(State(st): State<AppState>, p: Principal, Query(q): Query<HistoryParams>) -> ApiResult<Json<Vec<HistoryEntry>>> {
    p.require(Scope::ContentWrite)?;
    let rows: Vec<(i64, String, Option<serde_json::Value>, DateTime<Utc>, Option<String>)> = sqlx::query_as(
        "SELECT h.id, h.key, h.published, h.created_at, u.username FROM catalog_history h LEFT JOIN users u ON u.id = h.created_by
          WHERE ($1::text IS NULL OR h.key = $1) ORDER BY h.id DESC LIMIT 200",
    )
    .bind(&q.key)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows.into_iter().map(|(id, key, published, created_at, created_by)| HistoryEntry { id, key, published, created_at, created_by }).collect()))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    /// Compile this program with the real toolchain and explain it.
    pub source: Option<String>,
    /// Or re-explain an existing submission (student identity is not shown).
    pub submission_id: Option<Uuid>,
    /// Pretend the student has learned these concepts.
    #[serde(default)]
    pub concepts: Vec<String>,
    /// Include unpublished drafts (default true).
    pub use_drafts: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct PreviewResult {
    pub compile_success: bool,
    /// Every card with all hint levels.
    #[schema(value_type = Vec<Object>)]
    pub cards: Vec<Card>,
}

/// See exactly what students would see, with drafts applied.
#[utoipa::path(post, path = "/admin/catalog/preview", tag = TAG_CATALOG, request_body = PreviewRequest,
    responses((status = 200, body = PreviewResult), (status = 503, body = crate::error::ProblemDetails)), security(("token" = ["content:write"]), ("session" = [])))]
async fn preview(State(st): State<AppState>, p: Principal, JsonBody(req): JsonBody<PreviewRequest>) -> ApiResult<Json<PreviewResult>> {
    p.require(Scope::ContentWrite)?;
    let snap = st.snapshot();
    let catalog = if req.use_drafts.unwrap_or(true) { &snap.catalog_with_drafts } else { &snap.catalog };
    let learned: BTreeSet<String> = req.concepts.iter().cloned().collect();
    let adj = crate::state::priority_map(catalog);
    let (source, compile, obs) = match (&req.source, req.submission_id) {
        (Some(src), None) => {
            let (compile, _) = compile_messages(&st, src).await?;
            (src.clone(), compile, vec![])
        }
        (None, Some(id)) => {
            let row = super::submissions::load_row(&st, id).await?;
            let stored: crate::judge::evaluate::StoredResult =
                row.result.clone().and_then(|r| serde_json::from_value(r).ok()).ok_or_else(|| ApiError::conflict("not_judged", "這筆提交還沒有評測結果"))?;
            let compile = stored.compile.clone().ok_or_else(|| ApiError::conflict("not_judged", "這筆提交沒有編譯結果"))?;
            (row.assembled_source.clone(), compile, observations(&stored))
        }
        _ => return Err(ApiError::validation(vec![FieldError::new("", "one_of", "請提供 source 或 submission_id 其中一個")])),
    };
    let a = diagnostics::analyze(&diagnostics::AnalysisInput {
        source: &source,
        compile: Some(&compile),
        runs: &obs,
        fixed_regions: &[],
        system_error: false,
        custom_rules: &catalog.custom_rules,
        priority_adjust: &adj,
    });
    let cards = diagnostics::render(&a.findings, &RenderContext { catalog, builtin: &snap.builtin, learned: &learned });
    Ok(Json(PreviewResult { compile_success: a.compile_succeeded, cards }))
}
