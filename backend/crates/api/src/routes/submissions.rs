//! Run / Submit, viewing results, progressive hints and re-judging.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use ccal_core::fillin::Template;
use ccal_core::problem::{FieldError, ProblemMode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_SUBMISSIONS;
use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::judge::view::{self, Audience, SUBMISSION_SELECT, SubmissionRow, SubmissionView, ViewContext};
use crate::repo::{self, Page, PageParams};
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_submission))
        .routes(routes!(get_submission))
        .routes(routes!(reveal_hint))
        .routes(routes!(rejudge))
        .routes(routes!(my_submissions))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionKind {
    /// Public tests, or your own stdin. Not counted as an attempt.
    Run,
    /// All tests (public + hidden); recorded as an official attempt.
    Submit,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSubmission {
    pub problem_id: Uuid,
    /// The set you were working in (used for wording and statistics).
    pub set_id: Option<Uuid>,
    pub kind: SubmissionKind,
    /// Full-program problems only.
    pub code: Option<String>,
    /// Fill-in problems only: blank id → your code. Fixed code always comes
    /// from the server's copy of the template.
    pub blanks: Option<BTreeMap<String, String>>,
    /// `run` only: use this as stdin instead of the public tests.
    pub stdin: Option<String>,
}

pub async fn load_row(st: &AppState, id: Uuid) -> ApiResult<SubmissionRow> {
    sqlx::query_as(&format!("{SUBMISSION_SELECT} WHERE s.id = $1")).bind(id).fetch_optional(&st.db).await?.ok_or_else(|| ApiError::not_found("這筆提交"))
}

pub async fn render_view(st: &AppState, row: &SubmissionRow, audience: Audience, use_drafts: bool) -> ApiResult<SubmissionView> {
    let snap = st.snapshot();
    let policy = repo::policy_for(st, row.user_id).await?;
    let set_concepts: Vec<String> = match row.set_id {
        Some(s) => sqlx::query_scalar("SELECT learned_concepts FROM problem_sets WHERE id = $1").bind(s).fetch_optional(&st.db).await?.unwrap_or_default(),
        None => vec![],
    };
    let revealed: Vec<(i32, i64)> =
        sqlx::query_as("SELECT finding_id, count(*) FROM hint_reveals WHERE submission_id = $1 GROUP BY finding_id").bind(row.id).fetch_all(&st.db).await?;
    let revealed: BTreeMap<u32, usize> = revealed.into_iter().map(|(f, n)| (f as u32, n as usize)).collect();
    let ctx = ViewContext { snapshot: &snap, policy: &policy, set_concepts: &set_concepts, revealed: &revealed, use_drafts };
    Ok(view::build(row, audience, &ctx))
}

/// Builds the program to compile. Fill-in problems accept only blanks.
pub fn assemble(
    mode: ProblemMode,
    template: Option<&str>,
    code: Option<String>,
    blanks: Option<BTreeMap<String, String>>,
) -> ApiResult<(String, String, Option<serde_json::Value>)> {
    match mode {
        ProblemMode::Full => {
            if blanks.is_some() {
                return Err(ApiError::validation(vec![FieldError::new("/blanks", "not_allowed", "這一題是完整作答題，請送出 code")]));
            }
            let code = code.ok_or_else(|| ApiError::validation(vec![FieldError::new("/code", "required", "缺少程式碼")]))?;
            if code.len() > ccal_core::problem::MAX_CODE_BYTES {
                return Err(ApiError::validation(vec![FieldError::new("/code", "too_long", "程式碼最多 64 KiB")]));
            }
            Ok((code.clone(), code, None))
        }
        ProblemMode::FillIn => {
            if code.is_some() {
                return Err(ApiError::validation(vec![FieldError::new(
                    "/code",
                    "not_allowed",
                    "這一題是局部補寫題，只接受 blanks；固定的程式碼由伺服器提供，不能修改",
                )]));
            }
            let blanks = blanks.ok_or_else(|| ApiError::validation(vec![FieldError::new("/blanks", "required", "缺少填空內容")]))?;
            let tpl = Template::parse(template.unwrap_or_default()).map_err(ApiError::internal)?;
            let assembled = tpl.assemble(&blanks).map_err(|e| ApiError::validation(vec![FieldError::new("/blanks", "invalid", e.to_string())]))?;
            Ok((assembled.source.clone(), assembled.source, Some(serde_json::to_value(&blanks).unwrap())))
        }
    }
}

/// Queue a Run or Submit. Returns immediately (202); poll the submission.
/// Send an `Idempotency-Key` header to make retries safe.
#[utoipa::path(post, path = "/submissions", tag = TAG_SUBMISSIONS, request_body = CreateSubmission,
    params(("Idempotency-Key" = Option<String>, Header, description = "Reusing a key returns the original submission instead of creating a new one")),
    responses((status = 202, body = SubmissionView), (status = 200, body = SubmissionView, description = "Replayed (same Idempotency-Key)"),
              (status = 422, body = crate::error::ProblemDetails), (status = 429, body = crate::error::ProblemDetails)))]
async fn create_submission(
    State(st): State<AppState>,
    p: Principal,
    headers: HeaderMap,
    JsonBody(req): JsonBody<CreateSubmission>,
) -> ApiResult<(StatusCode, Json<SubmissionView>)> {
    let idem = repo::idempotency_key(&headers);
    if let Some(key) = &idem {
        let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM submissions WHERE user_id = $1 AND idempotency_key = $2")
            .bind(p.user_id)
            .bind(key)
            .fetch_optional(&st.db)
            .await?;
        if let Some(id) = existing {
            let row = load_row(&st, id).await?;
            return Ok((StatusCode::OK, Json(render_view(&st, &row, Audience::Student, false).await?)));
        }
    }

    let pr = repo::find_problem(&st.db, &req.problem_id.to_string()).await?;
    let policy = repo::policy_for(&st, p.user_id).await?;
    let (Some(rev_id), true) = (pr.published_revision_id, pr.status == "published") else {
        return Err(ApiError::conflict("problem_unavailable", "這個題目目前沒有開放作答"));
    };
    if !repo::student_can_see_problem(&st, &p, &policy, pr.id).await? {
        return Err(ApiError::not_found("這個題目"));
    }
    if let Some(set_id) = req.set_id {
        let in_set: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM problem_set_items WHERE set_id = $1 AND problem_id = $2)")
            .bind(set_id)
            .bind(pr.id)
            .fetch_one(&st.db)
            .await?;
        if !in_set {
            return Err(ApiError::validation(vec![FieldError::new("/set_id", "not_in_set", "這個題目不在指定的題目集裡")]));
        }
    }
    if req.kind == SubmissionKind::Submit && req.stdin.is_some() {
        return Err(ApiError::validation(vec![FieldError::new("/stdin", "not_allowed", "正式提交使用題目的測資，不能自訂輸入")]));
    }
    if req.stdin.as_ref().is_some_and(|s| s.len() > 64 * 1024) {
        return Err(ApiError::validation(vec![FieldError::new("/stdin", "too_long", "自訂輸入最多 64 KiB")]));
    }

    let rev = repo::revision(&st.db, rev_id).await?;
    let content = rev.content()?;
    let (code, source, blanks) = assemble(content.mode, content.template.as_deref(), req.code, req.blanks)?;
    // Rate limits count only well-formed submissions, so a rejected request
    // (e.g. invalid blanks) does not use up the student's quota.
    let (kind, limit) = match req.kind {
        SubmissionKind::Run => ("run", policy.runs_per_minute.value),
        SubmissionKind::Submit => ("submit", policy.submits_per_minute.value),
    };
    if !st.limiter.check(&format!("{kind}:{}", p.user_id), limit, std::time::Duration::from_secs(60)).await {
        return Err(ApiError::too_many(format!("操作太頻繁了，每分鐘最多 {limit} 次，請稍等一下再試")));
    }
    let in_flight: i64 = sqlx::query_scalar("SELECT count(*) FROM submissions WHERE user_id = $1 AND status IN ('queued', 'compiling', 'running')")
        .bind(p.user_id)
        .fetch_one(&st.db)
        .await?;
    if in_flight >= 2 {
        return Err(ApiError::too_many("你已經有兩筆程式正在評測，請等它們完成再送出"));
    }

    let id = Uuid::now_v7();
    let inserted = sqlx::query(
        "INSERT INTO submissions (id, user_id, problem_id, revision_id, set_id, kind, code, blanks, assembled_source, custom_stdin, idempotency_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT (user_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING",
    )
    .bind(id)
    .bind(p.user_id)
    .bind(pr.id)
    .bind(rev.id)
    .bind(req.set_id)
    .bind(kind)
    .bind(&code)
    .bind(&blanks)
    .bind(&source)
    .bind(&req.stdin)
    .bind(&idem)
    .execute(&st.db)
    .await?;
    let id = if inserted.rows_affected() == 0 {
        // Lost a race with a concurrent retry using the same key.
        sqlx::query_scalar("SELECT id FROM submissions WHERE user_id = $1 AND idempotency_key = $2").bind(p.user_id).bind(&idem).fetch_one(&st.db).await?
    } else {
        id
    };
    st.judge_notify.notify_waiters();
    let row = load_row(&st, id).await?;
    Ok((StatusCode::ACCEPTED, Json(render_view(&st, &row, Audience::Student, false).await?)))
}

fn check_access(p: &Principal, row: &SubmissionRow) -> ApiResult<Audience> {
    if row.user_id == p.user_id && p.token_id.is_none() {
        return Ok(if p.is_owner() { Audience::Owner } else { Audience::Student });
    }
    if p.is_owner() {
        p.require(crate::auth::Scope::SubmissionsRead)?;
        return Ok(Audience::Owner);
    }
    // Do not reveal whether someone else's submission exists.
    Err(ApiError::not_found("這筆提交"))
}

/// Status, test results and teaching cards. Poll until `status` is `done`
/// or `system_error`. Hidden tests show only their verdict.
#[utoipa::path(get, path = "/submissions/{id}", tag = TAG_SUBMISSIONS, params(("id" = Uuid, Path)),
    responses((status = 200, body = SubmissionView), (status = 404, body = crate::error::ProblemDetails)))]
async fn get_submission(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<Json<SubmissionView>> {
    let row = load_row(&st, id).await?;
    let audience = check_access(&p, &row)?;
    Ok(Json(render_view(&st, &row, audience, false).await?))
}

#[derive(Serialize, ToSchema)]
pub struct HintReveal {
    pub finding_id: u32,
    pub hints: Vec<String>,
    pub hints_total: usize,
    pub hints_allowed: usize,
}

/// Reveal the next hint level for one card (recorded for teaching statistics).
#[utoipa::path(post, path = "/submissions/{id}/findings/{finding_id}/hints", tag = TAG_SUBMISSIONS,
    params(("id" = Uuid, Path), ("finding_id" = u32, Path)),
    responses((status = 200, body = HintReveal), (status = 409, body = crate::error::ProblemDetails)))]
async fn reveal_hint(State(st): State<AppState>, p: Principal, Path((id, finding_id)): Path<(Uuid, u32)>) -> ApiResult<Json<HintReveal>> {
    let row = load_row(&st, id).await?;
    if row.user_id != p.user_id {
        return Err(ApiError::not_found("這筆提交"));
    }
    let view = render_view(&st, &row, Audience::Student, false).await?;
    let card = view.cards.iter().find(|c| c.finding_id == finding_id).ok_or_else(|| ApiError::not_found("這張說明卡"))?;
    if card.hints.len() >= card.hints_allowed {
        return Err(ApiError::conflict("no_more_hints", "已經沒有更多提示了"));
    }
    let level = card.hints.len() as i32 + 1;
    sqlx::query("INSERT INTO hint_reveals (submission_id, finding_id, level, rule) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING")
        .bind(id)
        .bind(finding_id as i32)
        .bind(level)
        .bind(&card.rule)
        .execute(&st.db)
        .await?;
    // Earlier levels shown automatically count as revealed too.
    for l in 1..level {
        sqlx::query("INSERT INTO hint_reveals (submission_id, finding_id, level, rule) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(finding_id as i32)
            .bind(l)
            .bind(&card.rule)
            .execute(&st.db)
            .await?;
    }
    let view = render_view(&st, &row, Audience::Student, false).await?;
    let card = view.cards.into_iter().find(|c| c.finding_id == finding_id).unwrap();
    Ok(Json(HintReveal { finding_id, hints: card.hints, hints_total: card.hints_total, hints_allowed: card.hints_allowed }))
}

/// Re-queue a submission that failed because of the judging service.
/// It is judged against the same problem revision as before.
#[utoipa::path(post, path = "/submissions/{id}/rejudge", tag = TAG_SUBMISSIONS, params(("id" = Uuid, Path)),
    responses((status = 202, body = SubmissionView), (status = 409, body = crate::error::ProblemDetails)))]
async fn rejudge(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<(StatusCode, Json<SubmissionView>)> {
    let row = load_row(&st, id).await?;
    let audience = check_access(&p, &row)?;
    let r = sqlx::query(
        "UPDATE submissions SET status = 'queued', verdict = NULL, attempts = 0, next_attempt_at = now(), last_error = NULL, finished_at = NULL
          WHERE id = $1 AND status = 'system_error'",
    )
    .bind(id)
    .execute(&st.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::conflict("not_retryable", "只有因評測服務問題而失敗的提交可以重新評測"));
    }
    st.judge_notify.notify_waiters();
    let row = load_row(&st, id).await?;
    Ok((StatusCode::ACCEPTED, Json(render_view(&st, &row, audience, false).await?)))
}

#[derive(Serialize, ToSchema)]
pub struct SubmissionSummary {
    pub id: Uuid,
    pub problem_id: Uuid,
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
pub struct MyFilter {
    pub problem_id: Option<Uuid>,
    /// `run` or `submit`.
    pub kind: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(sqlx::FromRow)]
pub struct SummaryRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub problem_id: Uuid,
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

pub const SUMMARY_SELECT: &str =
    "SELECT s.id, s.user_id, u.username, s.problem_id, p.slug AS problem_slug, r.title AS problem_title, r.number AS revision_number,
        s.kind, s.status, s.verdict, s.passed, s.total, s.created_at
   FROM submissions s JOIN problems p ON p.id = s.problem_id JOIN problem_revisions r ON r.id = s.revision_id JOIN users u ON u.id = s.user_id";

impl From<SummaryRow> for SubmissionSummary {
    fn from(r: SummaryRow) -> Self {
        SubmissionSummary {
            id: r.id,
            problem_id: r.problem_id,
            problem_slug: r.problem_slug,
            problem_title: r.problem_title,
            revision_number: r.revision_number,
            kind: r.kind,
            status: r.status,
            verdict: r.verdict,
            passed: r.passed,
            total: r.total,
            created_at: r.created_at,
        }
    }
}

/// Your submission history (newest first).
#[utoipa::path(get, path = "/me/submissions", tag = TAG_SUBMISSIONS, params(MyFilter), responses((status = 200, body = Page<SubmissionSummary>)))]
async fn my_submissions(State(st): State<AppState>, p: Principal, Query(f): Query<MyFilter>) -> ApiResult<Json<Page<SubmissionSummary>>> {
    let (limit, offset) = PageParams { limit: f.limit, cursor: f.cursor.clone() }.resolve();
    let rows: Vec<SummaryRow> = sqlx::query_as(&format!(
        "{SUMMARY_SELECT} WHERE s.user_id = $1 AND s.kind <> 'test_run' AND ($2::uuid IS NULL OR s.problem_id = $2) AND ($3::text IS NULL OR s.kind = $3)
         ORDER BY s.created_at DESC, s.id DESC LIMIT $4 OFFSET $5"
    ))
    .bind(p.user_id)
    .bind(f.problem_id)
    .bind(&f.kind)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(repo::page(rows.into_iter().map(Into::into).collect(), limit, offset)))
}
