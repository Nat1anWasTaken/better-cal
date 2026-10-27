//! Free-form C playground: compile and run any program once, with the same
//! teaching cards as a submission. Nothing is stored. Runs share the judge
//! runner with real judging, so they are capped per user and overall.

use std::collections::{BTreeSet, HashSet};
use std::sync::Mutex;

use axum::Json;
use axum::extract::State;
use ccal_core::problem::FieldError;
use diagnostics::{RenderContext, RunObservation, TestRef};
use judge_proto::{ExecuteRequest, Limits, RunStatus, TestInput};
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, SemaphorePermit};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::TAG_PLAYGROUND;
use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::judge::client::RunnerFailure;
use crate::judge::view::{CardView, card_view};
use crate::repo;
use crate::state::AppState;

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_STDIN_BYTES: usize = 64 * 1024;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(run))
}

/// Limits how many playground runs reach the runner: a few at once overall,
/// and one at a time per user.
pub struct Gate {
    slots: Semaphore,
    users: Mutex<HashSet<Uuid>>,
}

pub struct Pass<'a> {
    gate: &'a Gate,
    user: Uuid,
    _slot: SemaphorePermit<'a>,
}

impl Drop for Pass<'_> {
    fn drop(&mut self) {
        self.gate.users.lock().unwrap().remove(&self.user);
    }
}

pub enum Refused {
    UserBusy,
    Full,
}

impl Gate {
    pub fn new(concurrency: usize) -> Gate {
        Gate { slots: Semaphore::new(concurrency), users: Mutex::new(HashSet::new()) }
    }

    pub fn enter(&self, user: Uuid) -> Result<Pass<'_>, Refused> {
        if !self.users.lock().unwrap().insert(user) {
            return Err(Refused::UserBusy);
        }
        match self.slots.try_acquire() {
            Ok(slot) => Ok(Pass { gate: self, user, _slot: slot }),
            Err(_) => {
                self.users.lock().unwrap().remove(&user);
                Err(Refused::Full)
            }
        }
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaygroundRequest {
    /// A complete C program (C17, compiled like submissions).
    pub source: String,
    /// Standard input for the program.
    #[serde(default)]
    pub stdin: String,
}

#[derive(Serialize, ToSchema)]
pub struct PlaygroundRun {
    /// `ok`, `non_zero_exit`, `signaled`, `time_limit`, `memory_limit`, `output_limit` or `sandbox_error`.
    #[schema(value_type = String)]
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub time_ms: u32,
    pub memory_kb: u32,
    pub stdout: String,
    pub stdout_truncated: bool,
    pub stderr: String,
    pub stderr_truncated: bool,
}

#[derive(Serialize, ToSchema)]
pub struct PlaygroundLimits {
    pub time_ms: u32,
    pub memory_kb: u32,
    pub output_kb: u32,
}

#[derive(Serialize, ToSchema)]
pub struct PlaygroundResult {
    pub compile_success: bool,
    /// Absent when the program did not compile.
    pub run: Option<PlaygroundRun>,
    /// Teaching cards. `hints` holds every hint this user may see; show the
    /// first `auto_reveal_hints` and reveal the rest on request.
    pub cards: Vec<CardView>,
    pub auto_reveal_hints: usize,
    pub raw_expanded: bool,
    pub limits: PlaygroundLimits,
}

fn validate(req: &PlaygroundRequest) -> Vec<FieldError> {
    let mut e = vec![];
    if req.source.trim().is_empty() {
        e.push(FieldError::new("/source", "required", "請輸入程式碼"));
    } else if req.source.len() > MAX_SOURCE_BYTES {
        e.push(FieldError::new("/source", "too_long", format!("程式碼最多 {} KiB", MAX_SOURCE_BYTES / 1024)));
    }
    if req.stdin.len() > MAX_STDIN_BYTES {
        e.push(FieldError::new("/stdin", "too_long", format!("輸入最多 {} KiB", MAX_STDIN_BYTES / 1024)));
    }
    e
}

/// Compile and run a program once with the real toolchain, and explain any
/// problems with teaching cards. There is no expected output, so only compile,
/// warning and runtime cards (crash, time/memory/output limit) can appear.
#[utoipa::path(post, path = "/playground/run", tag = TAG_PLAYGROUND, request_body = PlaygroundRequest,
    responses((status = 200, body = PlaygroundResult), (status = 403, body = crate::error::ProblemDetails), (status = 422, body = crate::error::ProblemDetails),
              (status = 429, body = crate::error::ProblemDetails), (status = 503, body = crate::error::ProblemDetails)))]
async fn run(State(st): State<AppState>, p: Principal, JsonBody(req): JsonBody<PlaygroundRequest>) -> ApiResult<Json<PlaygroundResult>> {
    let policy = repo::policy_for(&st, p.user_id).await?;
    if !p.is_owner() && !policy.playground.value {
        return Err(ApiError::forbidden("練習場目前沒有開放"));
    }
    let errors = validate(&req);
    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }
    let limit = policy.runs_per_minute.value;
    if !st.limiter.check(&format!("playground:{}", p.user_id), limit, std::time::Duration::from_secs(60)).await {
        return Err(ApiError::too_many(format!("執行太頻繁了，每分鐘最多 {limit} 次，請稍等一下再試")));
    }
    let _pass = st.playground.enter(p.user_id).map_err(|r| match r {
        Refused::UserBusy => ApiError::too_many("上一次執行還沒結束，請等它完成"),
        Refused::Full => ApiError::unavailable("練習場目前忙碌中，請稍後再試"),
    })?;

    let limits = Limits::default();
    let exec =
        ExecuteRequest { source: req.source.clone(), tests: vec![TestInput { id: "playground".into(), stdin: req.stdin }], limits, stop_on_failure: false };
    let resp = st.runner.execute(&exec).await.map_err(|e| match e {
        RunnerFailure::Busy => ApiError::unavailable("評測服務忙碌中，請稍後再試"),
        e => ApiError::unavailable(format!("評測服務無法使用：{e}")),
    })?;
    let test = if resp.compile.success { resp.tests.into_iter().next() } else { None };

    let obs: Vec<RunObservation> = test
        .iter()
        .map(|t| RunObservation {
            test: TestRef { number: 1, hidden: false, name: None },
            status: t.status,
            exit_code: t.exit_code,
            signal: t.signal,
            time_ms: t.time_ms,
            time_limit_ms: limits.time_ms,
            memory_limit_kb: limits.memory_kb,
            wall_limit_with_low_cpu: t.wall_limit_with_low_cpu,
            answer_correct: None,
            whitespace_only_difference: false,
            stderr_excerpt: t.stderr.chars().take(500).collect(),
        })
        .collect();
    let snap = st.snapshot();
    let catalog = &snap.catalog;
    let adj = snap.priority_adjust();
    let analysis = diagnostics::analyze(&diagnostics::AnalysisInput {
        source: &req.source,
        compile: Some(&resp.compile),
        runs: &obs,
        fixed_regions: &[],
        system_error: false,
        custom_rules: &catalog.custom_rules,
        priority_adjust: &adj,
    });
    let learned: BTreeSet<String> = policy.learned_concepts.iter().cloned().collect();
    let rendered = diagnostics::render(&analysis.findings, &RenderContext { catalog, builtin: &snap.builtin, learned: &learned });
    let cards = rendered
        .into_iter()
        .map(|c| {
            let allowed = policy.max_hint_levels.value.map(|m| m as usize).unwrap_or(c.hints.len()).min(c.hints.len());
            card_view(c, allowed, allowed)
        })
        .collect();

    Ok(Json(PlaygroundResult {
        compile_success: analysis.compile_succeeded,
        run: test.map(|t| PlaygroundRun {
            status: t.status,
            exit_code: t.exit_code,
            signal: t.signal,
            time_ms: t.time_ms,
            memory_kb: t.memory_kb,
            stdout: t.stdout,
            stdout_truncated: t.stdout_truncated,
            stderr: t.stderr,
            stderr_truncated: t.stderr_truncated,
        }),
        cards,
        auto_reveal_hints: policy.auto_reveal_hints.value as usize,
        raw_expanded: policy.raw_expanded.value,
        limits: PlaygroundLimits { time_ms: limits.time_ms, memory_kb: limits.memory_kb, output_kb: limits.output_kb },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_limits_per_user_and_overall() {
        let gate = Gate::new(2);
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let pa = gate.enter(a).ok().expect("first run");
        assert!(matches!(gate.enter(a), Err(Refused::UserBusy)));
        let _pb = gate.enter(b).ok().expect("second user");
        assert!(matches!(gate.enter(c), Err(Refused::Full)));
        drop(pa);
        assert!(gate.enter(c).is_ok(), "a slot frees up when a run ends");
        assert!(gate.enter(a).is_ok());
    }
}
