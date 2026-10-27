//! Student-facing reads: site info, problem sets, problems, drafts, progress.

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::{Path, State};
use ccal_core::compare::CompareMode;
use ccal_core::fillin::{Segment, Template};
use ccal_core::problem::{Example, ProblemContent, ProblemLimits, ProblemMode, Visibility};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_STUDENT;
use crate::auth::Principal;
use crate::auth::principal::MaybePrincipal;
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::repo::{self, SetRow};
use crate::state::{AppState, JudgeHealth};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(site))
        .routes(routes!(concepts))
        .routes(routes!(system_status))
        .routes(routes!(list_sets))
        .routes(routes!(get_set))
        .routes(routes!(get_problem))
        .routes(routes!(get_draft, put_draft))
        .routes(routes!(progress))
}

#[derive(Serialize, ToSchema)]
pub struct SetSummary {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub description_md: String,
    pub learned_concepts: Vec<String>,
    pub problem_count: i64,
    pub solved_count: i64,
    /// This is the site's currently active set.
    pub active: bool,
}

#[derive(Serialize, ToSchema)]
pub struct SiteInfo {
    pub site_name: String,
    pub announcement_md: String,
    /// The owner-selected active set (the default entry point).
    pub active_set_id: Option<Uuid>,
    /// The set this user should land on (cohort/student settings may differ).
    pub default_set_id: Option<Uuid>,
    /// Bumps whenever online settings change; clients poll it to detect switches.
    pub config_version: i64,
    /// This user may view leaderboards.
    pub leaderboard: bool,
    /// The leaderboard round in progress, if any.
    pub open_round_id: Option<Uuid>,
    /// This user may use the C playground.
    pub playground: bool,
}

/// Site name, announcement and which problem set to open by default.
#[utoipa::path(get, path = "/site", tag = TAG_STUDENT, responses((status = 200, body = SiteInfo)))]
async fn site(State(st): State<AppState>, MaybePrincipal(p): MaybePrincipal) -> ApiResult<Json<SiteInfo>> {
    let snap = st.snapshot();
    let (default_set_id, leaderboard, playground) = match &p {
        Some(p) => {
            let policy = repo::policy_for(&st, p.user_id).await?;
            (policy.default_set_id.value, p.is_owner() || policy.leaderboard.value, p.is_owner() || policy.playground.value)
        }
        None => (snap.active_set_id, false, false),
    };
    let open_round_id = if leaderboard { super::leaderboard::open_round(&st).await?.map(|r| r.id) } else { None };
    Ok(Json(SiteInfo {
        site_name: snap.settings.site_name.clone(),
        announcement_md: if p.is_some() { snap.settings.announcement_md.clone() } else { String::new() },
        active_set_id: snap.active_set_id,
        default_set_id,
        config_version: snap.version,
        leaderboard,
        open_round_id,
        playground,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct Concept {
    pub id: String,
    pub label: String,
}

/// Concept ids usable in `concepts` / `learned_concepts` fields.
#[utoipa::path(get, path = "/concepts", tag = TAG_STUDENT, responses((status = 200, body = Vec<Concept>)))]
async fn concepts() -> Json<Vec<Concept>> {
    Json(ccal_core::CONCEPTS.iter().map(|(id, label)| Concept { id: id.to_string(), label: label.to_string() }).collect())
}

#[derive(Serialize, ToSchema)]
pub struct SystemStatus {
    pub judge: JudgeHealth,
    pub judge_checked_at: Option<DateTime<Utc>>,
    /// Submissions waiting or being judged.
    pub queue_length: i64,
    pub message: Option<String>,
}

/// Health of the judging service, for the status banner.
#[utoipa::path(get, path = "/system/status", tag = TAG_STUDENT, responses((status = 200, body = SystemStatus)))]
async fn system_status(State(st): State<AppState>) -> ApiResult<Json<SystemStatus>> {
    let (judge, at) = *st.judge_health.read().await;
    let queue_length: i64 = sqlx::query_scalar("SELECT count(*) FROM submissions WHERE status IN ('queued', 'compiling', 'running')").fetch_one(&st.db).await?;
    let message = (judge == JudgeHealth::Down).then(|| "評測服務暫時無法連線。你的程式碼與提交都已保存，服務恢復後會自動繼續評測。".to_string());
    Ok(Json(SystemStatus { judge, judge_checked_at: at, queue_length, message }))
}

async fn visible_sets(st: &AppState, p: &Principal) -> ApiResult<Vec<SetRow>> {
    let rows: Vec<SetRow> = sqlx::query_as(&format!("{} WHERE status = 'published' ORDER BY created_at", repo::SET_SELECT)).fetch_all(&st.db).await?;
    if p.is_owner() {
        return Ok(rows);
    }
    let policy = repo::policy_for(st, p.user_id).await?;
    Ok(rows.into_iter().filter(|s| policy.can_see_set(s.id, s.listed)).collect())
}

async fn solved_set(st: &AppState, user: Uuid) -> ApiResult<std::collections::HashSet<Uuid>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT problem_id FROM submissions WHERE user_id = $1 AND kind = 'submit' AND verdict = 'accepted'
         UNION SELECT problem_id FROM progress_credits WHERE user_id = $1",
    )
    .bind(user)
    .fetch_all(&st.db)
    .await?;
    Ok(ids.into_iter().collect())
}

/// Published problem sets visible to you.
#[utoipa::path(get, path = "/problem-sets", tag = TAG_STUDENT, responses((status = 200, body = Vec<SetSummary>)))]
async fn list_sets(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Vec<SetSummary>>> {
    let sets = visible_sets(&st, &p).await?;
    let solved = solved_set(&st, p.user_id).await?;
    let active = st.snapshot().active_set_id;
    let mut out = vec![];
    for s in sets {
        let pids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT i.problem_id FROM problem_set_items i JOIN problems p ON p.id = i.problem_id WHERE i.set_id = $1 AND p.published_revision_id IS NOT NULL AND p.status = 'published'",
        )
        .bind(s.id)
        .fetch_all(&st.db)
        .await?;
        out.push(SetSummary {
            id: s.id,
            slug: s.slug,
            title: s.title,
            description_md: s.description_md,
            learned_concepts: s.learned_concepts,
            problem_count: pids.len() as i64,
            solved_count: pids.iter().filter(|id| solved.contains(id)).count() as i64,
            active: Some(s.id) == active,
        });
    }
    Ok(Json(out))
}

#[derive(Serialize, ToSchema)]
pub struct SetProblemItem {
    pub position: i32,
    pub problem_id: Uuid,
    pub slug: String,
    pub title: String,
    pub mode: ProblemMode,
    /// `solved`, `attempted` or `not_started`.
    pub progress: String,
}

#[derive(Serialize, ToSchema)]
pub struct SetDetail {
    #[serde(flatten)]
    pub summary: SetSummary,
    pub problems: Vec<SetProblemItem>,
}

/// One published problem set with its problems (in order) and your progress.
#[utoipa::path(get, path = "/problem-sets/{set}", tag = TAG_STUDENT, params(("set" = String, Path, description = "Set id or slug")),
    responses((status = 200, body = SetDetail), (status = 404, body = crate::error::ProblemDetails)))]
async fn get_set(State(st): State<AppState>, p: Principal, Path(set): Path<String>) -> ApiResult<Json<SetDetail>> {
    let s = repo::find_set(&st.db, &set).await?;
    let visible = visible_sets(&st, &p).await?;
    if !visible.iter().any(|v| v.id == s.id) {
        return Err(ApiError::not_found("這個題目集"));
    }
    let rows: Vec<(i32, Uuid, String, serde_json::Value)> = sqlx::query_as(
        "SELECT i.position, p.id, p.slug, r.content FROM problem_set_items i
           JOIN problems p ON p.id = i.problem_id JOIN problem_revisions r ON r.id = p.published_revision_id
          WHERE i.set_id = $1 AND p.status = 'published' ORDER BY i.position",
    )
    .bind(s.id)
    .fetch_all(&st.db)
    .await?;
    let attempted: Vec<Uuid> =
        sqlx::query_scalar("SELECT DISTINCT problem_id FROM submissions WHERE user_id = $1 AND kind = 'submit'").bind(p.user_id).fetch_all(&st.db).await?;
    let solved = solved_set(&st, p.user_id).await?;
    let problems: Vec<SetProblemItem> = rows
        .into_iter()
        .map(|(position, id, slug, content)| {
            let c: Option<ProblemContent> = serde_json::from_value(content).ok();
            SetProblemItem {
                position,
                problem_id: id,
                slug,
                title: c.as_ref().map(|c| c.title.clone()).unwrap_or_default(),
                mode: c.map(|c| c.mode).unwrap_or_default(),
                progress: if solved.contains(&id) {
                    "solved"
                } else if attempted.contains(&id) {
                    "attempted"
                } else {
                    "not_started"
                }
                .into(),
            }
        })
        .collect();
    let solved_count = problems.iter().filter(|x| x.progress == "solved").count() as i64;
    Ok(Json(SetDetail {
        summary: SetSummary {
            id: s.id,
            slug: s.slug,
            title: s.title,
            description_md: s.description_md,
            learned_concepts: s.learned_concepts,
            problem_count: problems.len() as i64,
            solved_count,
            active: Some(s.id) == st.snapshot().active_set_id,
        },
        problems,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct PublicTest {
    pub number: u32,
    pub name: Option<String>,
    pub input: String,
    pub expected_output: String,
}

#[derive(Serialize, ToSchema)]
pub struct ProblemView {
    pub id: Uuid,
    pub slug: String,
    /// The published revision number students currently get.
    pub revision_number: i32,
    pub title: String,
    pub statement_md: String,
    pub examples: Vec<Example>,
    pub mode: ProblemMode,
    /// Editor content to start from (full program, or the assembled template).
    pub initial_code: String,
    /// Fill-in only: template segments; only `blank` segments are editable.
    pub segments: Option<Vec<Segment>>,
    pub limits: ProblemLimits,
    pub compare_mode: CompareMode,
    pub compare_description: String,
    pub concepts: Vec<String>,
    pub public_tests: Vec<PublicTest>,
    pub hidden_test_count: usize,
}

pub fn problem_view(id: Uuid, slug: &str, revision_number: i32, c: &ProblemContent, compare_description: String) -> ProblemView {
    let segments = (c.mode == ProblemMode::FillIn).then(|| c.template.as_deref().and_then(|t| Template::parse(t).ok()).map(|t| t.segments)).flatten();
    let mut n = 0;
    let public_tests = c
        .tests
        .iter()
        .filter(|t| t.visibility == Visibility::Public)
        .map(|t| {
            n += 1;
            PublicTest { number: n, name: t.name.clone(), input: t.input.clone(), expected_output: t.expected_output.clone() }
        })
        .collect();
    ProblemView {
        id,
        slug: slug.to_string(),
        revision_number,
        title: c.title.clone(),
        statement_md: c.statement_md.clone(),
        examples: c.examples.clone(),
        mode: c.mode,
        initial_code: c.initial_code(),
        segments,
        limits: c.limits,
        compare_mode: c.compare_mode,
        compare_description,
        concepts: c.concepts.clone(),
        public_tests,
        hidden_test_count: c.tests.iter().filter(|t| t.visibility == Visibility::Hidden).count(),
    }
}

/// A published problem: statement, examples, starter code and public tests.
/// Hidden tests and the reference solution are never included.
#[utoipa::path(get, path = "/problems/{problem}", tag = TAG_STUDENT, params(("problem" = String, Path, description = "Problem id or slug")),
    responses((status = 200, body = ProblemView), (status = 404, body = crate::error::ProblemDetails)))]
async fn get_problem(State(st): State<AppState>, p: Principal, Path(problem): Path<String>) -> ApiResult<Json<ProblemView>> {
    let pr = repo::find_problem(&st.db, &problem).await?;
    let policy = repo::policy_for(&st, p.user_id).await?;
    let (Some(rev_id), true) = (pr.published_revision_id, pr.status == "published") else { return Err(ApiError::not_found("這個題目")) };
    if !repo::student_can_see_problem(&st, &p, &policy, pr.id).await? {
        return Err(ApiError::not_found("這個題目"));
    }
    let rev = repo::revision(&st.db, rev_id).await?;
    let c = rev.content()?;
    let desc = st.snapshot().catalog.text(match c.compare_mode {
        CompareMode::Exact => "compare.exact",
        CompareMode::TrimTrailingWhitespace => "compare.trim_trailing_whitespace",
    });
    Ok(Json(problem_view(pr.id, &pr.slug, rev.number, &c, desc)))
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct Draft {
    /// Full-program problems: the editor content.
    pub code: String,
    /// Fill-in problems: blank id → text.
    pub blanks: Option<BTreeMap<String, String>>,
    /// Increments on every save; send it back as `base_version`.
    pub version: i32,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Your autosaved draft for a problem (independent of problem sets).
#[utoipa::path(get, path = "/me/drafts/{problem_id}", tag = TAG_STUDENT, params(("problem_id" = Uuid, Path)),
    responses((status = 200, body = Draft), (status = 404, body = crate::error::ProblemDetails)))]
async fn get_draft(State(st): State<AppState>, p: Principal, Path(problem_id): Path<Uuid>) -> ApiResult<Json<Draft>> {
    let row: Option<(String, Option<serde_json::Value>, i32, DateTime<Utc>)> =
        sqlx::query_as("SELECT code, blanks, version, updated_at FROM drafts WHERE user_id = $1 AND problem_id = $2")
            .bind(p.user_id)
            .bind(problem_id)
            .fetch_optional(&st.db)
            .await?;
    let (code, blanks, version, updated_at) = row.ok_or_else(|| ApiError::not_found("草稿"))?;
    Ok(Json(Draft { code, blanks: blanks.and_then(|b| serde_json::from_value(b).ok()), version, updated_at: Some(updated_at) }))
}

#[derive(Deserialize, ToSchema)]
pub struct SaveDraft {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub blanks: Option<BTreeMap<String, String>>,
    /// The version you last loaded/saved. Omit to overwrite unconditionally.
    pub base_version: Option<i32>,
}

/// Save your draft. Returns 409 with the stored draft if it changed elsewhere.
#[utoipa::path(put, path = "/me/drafts/{problem_id}", tag = TAG_STUDENT, params(("problem_id" = Uuid, Path)), request_body = SaveDraft,
    responses((status = 200, body = Draft), (status = 409, body = crate::error::ProblemDetails)))]
async fn put_draft(State(st): State<AppState>, p: Principal, Path(problem_id): Path<Uuid>, JsonBody(req): JsonBody<SaveDraft>) -> ApiResult<Json<Draft>> {
    if req.code.len() > ccal_core::problem::MAX_CODE_BYTES
        || req.blanks.as_ref().is_some_and(|b| b.values().map(|v| v.len()).sum::<usize>() > ccal_core::problem::MAX_CODE_BYTES)
    {
        return Err(ApiError::bad_request("too_large", "程式碼最多 64 KiB"));
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM problems WHERE id = $1)").bind(problem_id).fetch_one(&st.db).await?;
    if !exists {
        return Err(ApiError::not_found("這個題目"));
    }
    let blanks = req.blanks.as_ref().map(|b| serde_json::to_value(b).unwrap());
    let row: Option<(i32, DateTime<Utc>)> = sqlx::query_as(
        "INSERT INTO drafts (user_id, problem_id, code, blanks) VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, problem_id) DO UPDATE SET code = EXCLUDED.code, blanks = EXCLUDED.blanks, version = drafts.version + 1, updated_at = now()
           WHERE $5::int IS NULL OR drafts.version = $5
         RETURNING version, updated_at",
    )
    .bind(p.user_id)
    .bind(problem_id)
    .bind(&req.code)
    .bind(&blanks)
    .bind(req.base_version)
    .fetch_optional(&st.db)
    .await?;
    match row {
        Some((version, updated_at)) => Ok(Json(Draft { code: req.code, blanks: req.blanks, version, updated_at: Some(updated_at) })),
        None => {
            let current = get_draft(State(st), p, Path(problem_id)).await?.0;
            Err(ApiError::conflict("draft_conflict", "草稿已在其他分頁或裝置更新").with_data(serde_json::to_value(current).unwrap()))
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ProblemProgress {
    pub problem_id: Uuid,
    pub slug: String,
    pub title: String,
    /// `solved`, `attempted` or `not_started`.
    pub status: String,
    pub submissions: i64,
    pub credited: bool,
    pub last_submitted_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema)]
pub struct Progress {
    pub solved: i64,
    pub attempted: i64,
    pub total: i64,
    pub problems: Vec<ProblemProgress>,
}

/// Your progress over all problems in sets visible to you.
#[utoipa::path(get, path = "/me/progress", tag = TAG_STUDENT, responses((status = 200, body = Progress)))]
async fn progress(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Progress>> {
    let sets = visible_sets(&st, &p).await?;
    let set_ids: Vec<Uuid> = sets.iter().map(|s| s.id).collect();
    let rows: Vec<(Uuid, String, serde_json::Value)> = sqlx::query_as(
        "SELECT DISTINCT p.id, p.slug, r.content FROM problem_set_items i JOIN problems p ON p.id = i.problem_id
           JOIN problem_revisions r ON r.id = p.published_revision_id WHERE i.set_id = ANY($1) AND p.status = 'published'",
    )
    .bind(&set_ids)
    .fetch_all(&st.db)
    .await?;
    let stats: Vec<(Uuid, i64, bool, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT problem_id, count(*), bool_or(verdict = 'accepted'), max(created_at) FROM submissions WHERE user_id = $1 AND kind = 'submit' GROUP BY problem_id",
    )
    .bind(p.user_id)
    .fetch_all(&st.db)
    .await?;
    let credits: Vec<Uuid> = sqlx::query_scalar("SELECT problem_id FROM progress_credits WHERE user_id = $1").bind(p.user_id).fetch_all(&st.db).await?;
    let stats: HashMap<Uuid, (i64, bool, Option<DateTime<Utc>>)> = stats.into_iter().map(|(id, n, ok, at)| (id, (n, ok, at))).collect();
    let mut problems: Vec<ProblemProgress> = rows
        .into_iter()
        .map(|(id, slug, content)| {
            let (n, ok, at) = stats.get(&id).copied().unwrap_or((0, false, None));
            let credited = credits.contains(&id);
            ProblemProgress {
                problem_id: id,
                slug,
                title: content.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
                status: if ok || credited {
                    "solved"
                } else if n > 0 {
                    "attempted"
                } else {
                    "not_started"
                }
                .into(),
                submissions: n,
                credited,
                last_submitted_at: at,
            }
        })
        .collect();
    problems.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(Json(Progress {
        solved: problems.iter().filter(|x| x.status == "solved").count() as i64,
        attempted: problems.iter().filter(|x| x.status == "attempted").count() as i64,
        total: problems.len() as i64,
        problems,
    }))
}
