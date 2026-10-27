//! Durable judging queue on PostgreSQL.
//!
//! Jobs are rows in `submissions`. A worker claims one with
//! `FOR UPDATE SKIP LOCKED` and a short lease that it keeps renewing while it
//! works. If the API process dies mid-job the lease expires (≤ 30 s) and
//! another worker (or the restarted process) picks the job up again.
//! Results are written with a guard on (status, attempts) so a stale worker
//! can never overwrite a newer result or score a submission twice.

use std::collections::BTreeMap;
use std::time::Duration;

use ccal_core::problem::ProblemContent;
use judge_proto::{ExecuteRequest, TestInput};
use uuid::Uuid;

use super::client::RunnerFailure;
use super::evaluate::{TestSpec, Verdict, evaluate, observations, select_tests};
use crate::state::{AppState, JudgeHealth};

/// A claimed job is owned for this long; the worker renews it every
/// `HEARTBEAT` while it runs, so a crashed process's jobs are reclaimed quickly.
const LEASE_SECS: i64 = 30;
const HEARTBEAT: Duration = Duration::from_secs(10);

#[derive(sqlx::FromRow, Debug)]
struct Job {
    id: Uuid,
    kind: String,
    assembled_source: String,
    custom_stdin: Option<String>,
    attempts: i32,
    content: serde_json::Value,
}

pub fn spawn_workers(state: &AppState) {
    for n in 0..state.cfg.judge_workers {
        let st = state.clone();
        tokio::spawn(async move {
            tracing::info!(worker = n, "judge worker started");
            loop {
                match claim(&st).await {
                    Ok(Some(job)) => process(&st, job).await,
                    Ok(None) => {
                        // Woken early when a submission is created.
                        let _ = tokio::time::timeout(Duration::from_secs(2), st.judge_notify.notified()).await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "claiming judge job failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        });
    }
    let st = state.clone();
    tokio::spawn(async move { health_loop(st).await });
}

async fn claim(st: &AppState) -> anyhow::Result<Option<Job>> {
    let job: Option<Job> = sqlx::query_as(
        "WITH next AS (
             SELECT id FROM submissions
             WHERE (status = 'queued' AND next_attempt_at <= now())
                OR (status IN ('compiling', 'running') AND lease_until < now())
             ORDER BY next_attempt_at, created_at
             FOR UPDATE SKIP LOCKED
             LIMIT 1
         )
         UPDATE submissions s
            SET status = 'compiling', attempts = s.attempts + 1,
                lease_until = now() + make_interval(secs => $1),
                started_at = COALESCE(s.started_at, now())
           FROM next, problem_revisions r
          WHERE s.id = next.id AND r.id = s.revision_id
      RETURNING s.id, s.kind, s.assembled_source, s.custom_stdin, s.attempts, r.content",
    )
    .bind(LEASE_SECS as f64)
    .fetch_optional(&st.db)
    .await?;
    Ok(job)
}

async fn set_phase(st: &AppState, id: Uuid, attempts: i32, phase: &str) {
    let _ = sqlx::query("UPDATE submissions SET status = $3 WHERE id = $1 AND attempts = $2 AND status IN ('compiling', 'running')")
        .bind(id)
        .bind(attempts)
        .bind(phase)
        .execute(&st.db)
        .await;
}

async fn process(st: &AppState, job: Job) {
    let content: ProblemContent = match serde_json::from_value(job.content.clone()) {
        Ok(c) => c,
        Err(e) => {
            fail_permanently(st, &job, &format!("revision content unreadable: {e}")).await;
            return;
        }
    };
    let specs = select_tests(&content, &job.kind, job.custom_stdin.as_deref());
    let heartbeat = {
        let (st, id, attempts) = (st.clone(), job.id, job.attempts);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(HEARTBEAT).await;
                let _ = sqlx::query("UPDATE submissions SET lease_until = now() + make_interval(secs => $3) WHERE id = $1 AND attempts = $2 AND status IN ('compiling', 'running')")
                    .bind(id)
                    .bind(attempts)
                    .bind(LEASE_SECS as f64)
                    .execute(&st.db)
                    .await;
            }
        })
    };
    let result = judge(st, &job, &content, &specs).await;
    heartbeat.abort();
    if let Err(failure) = result {
        retry_or_fail(st, &job, &failure.to_string()).await;
    }
}

/// Runs the two phases (compile, then compile+run) so the student sees an
/// honest "compiling" → "running" progression.
async fn judge(st: &AppState, job: &Job, content: &ProblemContent, specs: &[TestSpec]) -> Result<(), RunnerFailure> {
    let limits = content.limits.to_runner();
    let compile_only = ExecuteRequest { source: job.assembled_source.clone(), tests: vec![], limits, stop_on_failure: false };
    let compiled = st.runner.execute(&compile_only).await;
    mark_health(st, &compiled).await;
    let compiled = compiled?;
    let final_resp = if compiled.compile.success && !specs.is_empty() {
        set_phase(st, job.id, job.attempts, "running").await;
        let tests = specs.iter().enumerate().map(|(i, s)| TestInput { id: format!("t{}", i + 1), stdin: s.input.clone() }).collect();
        let req = ExecuteRequest { source: job.assembled_source.clone(), tests, limits, stop_on_failure: false };
        let r = st.runner.execute(&req).await;
        mark_health(st, &r).await;
        let r = r?;
        if !r.compile.success || r.tests.len() != specs.len() {
            return Err(RunnerFailure::Failed("inconsistent runner response".into()));
        }
        r
    } else {
        compiled
    };

    let eval = evaluate(final_resp.compile, &final_resp.tests, specs, content.compare_mode, content.limits);
    // Rules hit, for the teaching statistics in the dashboard.
    let adj = BTreeMap::new();
    let obs = observations(&eval.stored);
    let analysis = diagnostics::analyze(&diagnostics::AnalysisInput {
        source: &job.assembled_source,
        compile: eval.stored.compile.as_ref(),
        runs: &obs,
        fixed_regions: &[],
        system_error: false,
        custom_rules: &[],
        priority_adjust: &adj,
    });
    let rules: Vec<String> = analysis.findings.iter().map(|f| f.rule.clone()).collect();
    let stored = serde_json::to_value(&eval.stored).expect("serializable");
    let res = sqlx::query(
        "UPDATE submissions SET status = 'done', verdict = $3, passed = $4, total = $5, result = $6, finding_rules = $7,
                lease_until = NULL, finished_at = now(), last_error = NULL
          WHERE id = $1 AND attempts = $2 AND status IN ('compiling', 'running')",
    )
    .bind(job.id)
    .bind(job.attempts)
    .bind(eval.verdict.as_str())
    .bind(eval.passed)
    .bind(eval.total)
    .bind(stored)
    .bind(&rules)
    .execute(&st.db)
    .await;
    match res {
        Ok(r) if r.rows_affected() == 0 => tracing::warn!(id = %job.id, "result discarded: job was re-claimed (stale lease)"),
        Ok(_) => tracing::info!(id = %job.id, verdict = eval.verdict.as_str(), "judged"),
        Err(e) => tracing::error!(id = %job.id, error = %e, "storing result failed; lease will expire and the job will be retried"),
    }
    Ok(())
}

async fn retry_or_fail(st: &AppState, job: &Job, err: &str) {
    if job.attempts >= st.cfg.max_attempts {
        fail_permanently(st, job, err).await;
        return;
    }
    let backoff = 2f64.powi(job.attempts) * 2.0;
    tracing::warn!(id = %job.id, attempt = job.attempts, error = err, "judge attempt failed; retrying in {backoff}s");
    let _ = sqlx::query(
        "UPDATE submissions SET status = 'queued', lease_until = NULL, last_error = $3, next_attempt_at = now() + make_interval(secs => $4)
          WHERE id = $1 AND attempts = $2 AND status IN ('compiling', 'running')",
    )
    .bind(job.id)
    .bind(job.attempts)
    .bind(err)
    .bind(backoff)
    .execute(&st.db)
    .await;
}

async fn fail_permanently(st: &AppState, job: &Job, err: &str) {
    tracing::error!(id = %job.id, error = err, "judging failed permanently");
    let _ = sqlx::query(
        "UPDATE submissions SET status = 'system_error', verdict = $3, lease_until = NULL, last_error = $4, finished_at = now()
          WHERE id = $1 AND attempts = $2 AND status IN ('compiling', 'running')",
    )
    .bind(job.id)
    .bind(job.attempts)
    .bind(Verdict::SystemError.as_str())
    .bind(err)
    .execute(&st.db)
    .await;
}

async fn mark_health<T>(st: &AppState, r: &Result<T, RunnerFailure>) {
    let h = match r {
        Ok(_) | Err(RunnerFailure::Busy) => JudgeHealth::Ok,
        Err(_) => JudgeHealth::Down,
    };
    *st.judge_health.write().await = (h, Some(chrono::Utc::now()));
}

async fn health_loop(st: AppState) {
    loop {
        let r = st.runner.info().await;
        mark_health(&st, &r).await;
        tokio::time::sleep(Duration::from_secs(15)).await;
    }
}
