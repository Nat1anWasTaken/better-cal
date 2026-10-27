//! Leaderboard rounds: students see the open round's ranking and past rounds
//! of sets they can see. Scoring rules live in `ccal_core::leaderboard`.

use axum::Json;
use axum::extract::{Path, State};
use ccal_core::leaderboard::{self, HintReveal, Standing, Submit};
use ccal_core::policy::{PolicyPatch, effective};
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_STUDENT;
use crate::auth::Principal;
use crate::error::{ApiError, ApiResult};
use crate::repo;
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(current_leaderboard)).routes(routes!(list_rounds)).routes(routes!(round_leaderboard))
}

#[derive(sqlx::FromRow, Serialize, ToSchema, Clone)]
pub struct Round {
    pub id: Uuid,
    pub set_id: Uuid,
    pub set_slug: String,
    pub set_title: String,
    pub title: String,
    pub started_at: DateTime<Utc>,
    /// `None` while the round is open.
    pub ended_at: Option<DateTime<Utc>>,
    /// While set, students see the board as of this moment.
    pub frozen_at: Option<DateTime<Utc>>,
}

pub const ROUND_SELECT: &str = "SELECT r.id, r.set_id, s.slug AS set_slug, s.title AS set_title, r.title, r.started_at, r.ended_at, r.frozen_at
     FROM rounds r JOIN problem_sets s ON s.id = r.set_id";

pub async fn find_round(st: &AppState, id: Uuid) -> ApiResult<Round> {
    sqlx::query_as(&format!("{ROUND_SELECT} WHERE r.id = $1")).bind(id).fetch_optional(&st.db).await?.ok_or_else(|| ApiError::not_found("這一輪排行"))
}

pub async fn open_round(st: &AppState) -> ApiResult<Option<Round>> {
    Ok(sqlx::query_as(&format!("{ROUND_SELECT} WHERE r.ended_at IS NULL")).fetch_optional(&st.db).await?)
}

#[derive(Serialize, ToSchema)]
pub struct BoardProblem {
    pub id: Uuid,
    pub title: String,
    pub position: i32,
}

#[derive(Serialize, ToSchema)]
pub struct BoardRow {
    pub name: String,
    #[serde(flatten)]
    pub standing: Standing,
}

#[derive(Serialize, ToSchema)]
pub struct Leaderboard {
    pub round: Round,
    pub problems: Vec<BoardProblem>,
    /// Only students who submitted during the round, best first.
    pub rows: Vec<BoardRow>,
    /// Results after `round.frozen_at` are hidden from this viewer.
    pub frozen: bool,
    /// Minutes added per wrong try.
    pub penalty_per_wrong: i64,
}

/// Students listed on leaderboards (effective policy `leaderboard`), with
/// their display names, in name order.
async fn board_users(st: &AppState) -> ApiResult<Vec<(Uuid, String)>> {
    let snap = st.snapshot();
    let rows: Vec<(Uuid, String, Option<serde_json::Value>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT u.id, CASE WHEN u.display_name = '' THEN u.username ELSE u.display_name END, c.policy, o.policy
         FROM users u LEFT JOIN cohorts c ON c.id = u.cohort_id LEFT JOIN student_overrides o ON o.user_id = u.id
         WHERE u.role = 'student' AND NOT u.disabled ORDER BY 2, u.id",
    )
    .fetch_all(&st.db)
    .await?;
    let parse = |v: Option<serde_json::Value>| v.and_then(|v| serde_json::from_value::<PolicyPatch>(v).ok());
    Ok(rows
        .into_iter()
        .filter(|(_, _, c, o)| {
            effective(&snap.settings.default_policy, parse(c.clone()).as_ref(), parse(o.clone()).as_ref(), snap.active_set_id).leaderboard.value
        })
        .map(|(id, name, _, _)| (id, name))
        .collect())
}

/// Students need the leaderboard enabled and the round's set visible.
async fn check_access(st: &AppState, p: &Principal, set_id: Uuid) -> ApiResult<()> {
    if p.is_owner() {
        return Ok(());
    }
    let policy = repo::policy_for(st, p.user_id).await?;
    let set = repo::find_set(&st.db, &set_id.to_string()).await?;
    if !policy.leaderboard.value || set.status != "published" || !policy.can_see_set(set.id, set.listed) {
        return Err(ApiError::not_found("這一輪排行"));
    }
    Ok(())
}

pub async fn board(st: &AppState, p: &Principal, round: Round) -> ApiResult<Leaderboard> {
    check_access(st, p, round.set_id).await?;
    let problems: Vec<BoardProblem> = sqlx::query_as::<_, (Uuid, String, i32)>(
        "SELECT p.id, COALESCE(r.title, p.slug), i.position FROM problem_set_items i JOIN problems p ON p.id = i.problem_id
         LEFT JOIN problem_revisions r ON r.id = p.published_revision_id WHERE i.set_id = $1 ORDER BY i.position",
    )
    .bind(round.set_id)
    .fetch_all(&st.db)
    .await?
    .into_iter()
    .map(|(id, title, position)| BoardProblem { id, title, position })
    .collect();
    let problem_ids: Vec<Uuid> = problems.iter().map(|x| x.id).collect();
    let end = round.ended_at;
    let secs = |at: DateTime<Utc>| (at - round.started_at).num_seconds();

    let submits: Vec<Submit> = sqlx::query_as::<_, (Uuid, Uuid, String, DateTime<Utc>)>(
        "SELECT user_id, problem_id, COALESCE(verdict, ''), created_at FROM submissions
         WHERE kind = 'submit' AND status = 'done' AND problem_id = ANY($1) AND created_at >= $2 AND ($3::timestamptz IS NULL OR created_at < $3)",
    )
    .bind(&problem_ids)
    .bind(round.started_at)
    .bind(end)
    .fetch_all(&st.db)
    .await?
    .into_iter()
    .map(|(user, problem, verdict, at)| Submit { user, problem, verdict, at: secs(at) })
    .collect();
    let hints: Vec<HintReveal> = sqlx::query_as::<_, (Uuid, Uuid, DateTime<Utc>)>(
        "SELECT s.user_id, s.problem_id, h.revealed_at FROM hint_reveals h JOIN submissions s ON s.id = h.submission_id
         WHERE s.problem_id = ANY($1) AND h.revealed_at >= $2 AND ($3::timestamptz IS NULL OR h.revealed_at < $3)",
    )
    .bind(&problem_ids)
    .bind(round.started_at)
    .bind(end)
    .fetch_all(&st.db)
    .await?
    .into_iter()
    .map(|(user, problem, at)| HintReveal { user, problem, at: secs(at) })
    .collect();

    let users = board_users(st).await?;
    let ids: Vec<Uuid> = users.iter().map(|(id, _)| *id).collect();
    let frozen = round.frozen_at.is_some() && !p.is_owner();
    let cutoff = round.frozen_at.filter(|_| frozen).map(secs);
    let names: std::collections::HashMap<Uuid, String> = users.into_iter().collect();
    let rows = leaderboard::rank(&problem_ids, &ids, &submits, &hints, cutoff)
        .into_iter()
        .map(|standing| BoardRow { name: names[&standing.user_id].clone(), standing })
        .collect();
    Ok(Leaderboard { round, problems, rows, frozen, penalty_per_wrong: leaderboard::PENALTY_PER_WRONG })
}

/// Ranking of the round in progress.
#[utoipa::path(get, path = "/leaderboard", tag = TAG_STUDENT,
    responses((status = 200, body = Leaderboard), (status = 404, body = crate::error::ProblemDetails)))]
async fn current_leaderboard(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Leaderboard>> {
    let round = open_round(&st).await?.ok_or_else(|| ApiError::not_found("進行中的排行"))?;
    Ok(Json(board(&st, &p, round).await?))
}

/// Rounds you can view, newest first.
#[utoipa::path(get, path = "/rounds", tag = TAG_STUDENT, responses((status = 200, body = Vec<Round>)))]
async fn list_rounds(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Vec<Round>>> {
    let rounds: Vec<Round> = sqlx::query_as(&format!("{ROUND_SELECT} ORDER BY r.started_at DESC")).fetch_all(&st.db).await?;
    if p.is_owner() {
        return Ok(Json(rounds));
    }
    let policy = repo::policy_for(&st, p.user_id).await?;
    if !policy.leaderboard.value {
        return Ok(Json(vec![]));
    }
    let visible: Vec<(Uuid, bool)> = sqlx::query_as("SELECT id, listed FROM problem_sets WHERE status = 'published'").fetch_all(&st.db).await?;
    Ok(Json(rounds.into_iter().filter(|r| visible.iter().any(|(id, listed)| *id == r.set_id && policy.can_see_set(*id, *listed))).collect()))
}

/// Ranking of one round, open or ended.
#[utoipa::path(get, path = "/rounds/{id}/leaderboard", tag = TAG_STUDENT, params(("id" = Uuid, Path)),
    responses((status = 200, body = Leaderboard), (status = 404, body = crate::error::ProblemDetails)))]
async fn round_leaderboard(State(st): State<AppState>, p: Principal, Path(id): Path<Uuid>) -> ApiResult<Json<Leaderboard>> {
    let round = find_round(&st, id).await?;
    Ok(Json(board(&st, &p, round).await?))
}
