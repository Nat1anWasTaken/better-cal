//! judge-runner: compiles and runs untrusted C code inside isolate sandboxes.
//!
//! Only reachable on the internal Docker network and authenticated with a
//! shared bearer token. It never sees expected outputs.

mod meta;
mod sandbox;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use judge_proto::{ExecuteRequest, ExecuteResponse, Limits, PROTOCOL_VERSION, RunStatus, RunnerError, RunnerInfo};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex, Semaphore};

struct Config {
    token: String,
    max: Limits,
    max_tests: usize,
    queue_wait: Duration,
}

struct AppState {
    cfg: Config,
    slots: Semaphore,
    free_boxes: Mutex<Vec<u32>>,
    info: RunnerInfo,
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        return healthcheck();
    }
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();

    let token = std::env::var("RUNNER_TOKEN").unwrap_or_default();
    if token.len() < 24 {
        anyhow::bail!("RUNNER_TOKEN must be set to a random string of at least 24 characters");
    }
    let concurrency = env_u32("RUNNER_CONCURRENCY", 2).clamp(1, 32) as usize;
    let cfg = Config {
        token,
        max: Limits {
            time_ms: env_u32("RUNNER_MAX_TIME_MS", 5000),
            wall_time_ms: env_u32("RUNNER_MAX_WALL_TIME_MS", 10000),
            memory_kb: env_u32("RUNNER_MAX_MEMORY_KB", 256 * 1024),
            output_kb: env_u32("RUNNER_MAX_OUTPUT_KB", 1024),
        },
        max_tests: env_u32("RUNNER_MAX_TESTS", 50) as usize,
        queue_wait: Duration::from_secs(env_u32("RUNNER_QUEUE_WAIT_S", 30) as u64),
    };

    let info = RunnerInfo {
        protocol_version: PROTOCOL_VERSION,
        compiler: sandbox::COMPILER.into(),
        compiler_version: sandbox::command_version(sandbox::COMPILER, "--version").await,
        compile_flags: sandbox::COMPILE_FLAGS.iter().map(|s| s.to_string()).collect(),
        isolate_version: sandbox::command_version("isolate", "--version").await,
        max_limits: cfg.max,
        concurrency,
    };
    tracing::info!(compiler = %info.compiler_version, isolate = %info.isolate_version, concurrency, "judge-runner starting");

    let state = Arc::new(AppState { slots: Semaphore::new(concurrency), free_boxes: Mutex::new((0..concurrency as u32).collect()), cfg, info });

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/info", get(info_handler))
        .route("/v1/execute", post(execute_handler))
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .with_state(state);

    let bind = std::env::var("RUNNER_BIND").unwrap_or_else(|_| "0.0.0.0:7070".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

/// Used by the container HEALTHCHECK; avoids shipping curl in the image.
fn healthcheck() -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect_timeout(&"127.0.0.1:7070".parse()?, Duration::from_secs(2))?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(b"GET /healthz HTTP/1.0\r\n\r\n")?;
    let mut buf = String::new();
    s.read_to_string(&mut buf)?;
    if buf.starts_with("HTTP/1.0 200") || buf.starts_with("HTTP/1.1 200") { Ok(()) } else { anyhow::bail!("unhealthy: {}", buf.lines().next().unwrap_or("")) }
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

fn err(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (status, Json(RunnerError { code: code.into(), message: message.into() })).into_response()
}

fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) else { return false };
    let Some(tok) = v.strip_prefix("Bearer ") else { return false };
    tok.as_bytes().ct_eq(state.cfg.token.as_bytes()).into()
}

async fn info_handler(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid runner token");
    }
    Json(state.info.clone()).into_response()
}

fn clamp(req: Limits, max: Limits) -> Limits {
    let time_ms = req.time_ms.clamp(100, max.time_ms);
    // Wall time must leave room for the CPU limit plus process start-up.
    let min_wall = time_ms + 500;
    Limits {
        time_ms,
        wall_time_ms: req.wall_time_ms.clamp(min_wall, max.wall_time_ms.max(min_wall)),
        memory_kb: req.memory_kb.clamp(8 * 1024, max.memory_kb),
        output_kb: req.output_kb.clamp(1, max.output_kb),
    }
}

async fn execute_handler(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<ExecuteRequest>) -> Response {
    if !authorized(&state, &headers) {
        return err(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid runner token");
    }
    if req.tests.len() > state.cfg.max_tests {
        return err(StatusCode::UNPROCESSABLE_ENTITY, "too_many_tests", format!("at most {} tests per request", state.cfg.max_tests));
    }
    if req.source.len() > 256 * 1024 {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "source_too_large", "source exceeds 256 KiB");
    }
    let Ok(permit) = tokio::time::timeout(state.cfg.queue_wait, state.slots.acquire()).await else {
        return err(StatusCode::SERVICE_UNAVAILABLE, "busy", "all sandboxes are busy");
    };
    let _permit = permit.expect("semaphore never closed");
    let box_id = state.free_boxes.lock().await.pop().expect("a free box per permit");
    let limits = clamp(req.limits, state.cfg.max);
    let result = run_job(box_id, &req, limits).await;
    state.free_boxes.lock().await.push(box_id);
    match result {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "sandbox failure");
            err(StatusCode::INTERNAL_SERVER_ERROR, "sandbox_error", e.to_string())
        }
    }
}

async fn run_job(box_id: u32, req: &ExecuteRequest, limits: Limits) -> anyhow::Result<ExecuteResponse> {
    let sb = sandbox::Sandbox::init(box_id).await?;
    let out = async {
        let compile = sb.compile(&req.source).await?;
        let mut tests = Vec::with_capacity(req.tests.len());
        if compile.success {
            let mut stop = false;
            for t in &req.tests {
                if stop {
                    tests.push(skipped(&t.id));
                    continue;
                }
                let r = sb.run_test(t, &limits).await?;
                if r.status == RunStatus::SandboxError {
                    anyhow::bail!("sandbox error while running test {}", t.id);
                }
                stop = req.stop_on_failure && r.status != RunStatus::Ok;
                tests.push(r);
            }
        }
        Ok(ExecuteResponse { protocol_version: PROTOCOL_VERSION, compile, tests })
    }
    .await;
    sb.cleanup().await;
    out
}

fn skipped(id: &str) -> judge_proto::TestRunResult {
    judge_proto::TestRunResult {
        id: id.to_string(),
        status: RunStatus::Skipped,
        exit_code: None,
        signal: None,
        time_ms: 0,
        wall_time_ms: 0,
        memory_kb: 0,
        wall_limit_with_low_cpu: false,
        stdout: String::new(),
        stdout_truncated: false,
        stderr: String::new(),
        stderr_truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_requested_limits_to_caps() {
        let max = Limits { time_ms: 2000, wall_time_ms: 5000, memory_kb: 131072, output_kb: 256 };
        let l = clamp(Limits { time_ms: 99999, wall_time_ms: 99999, memory_kb: 1 << 30, output_kb: 1 << 20 }, max);
        assert_eq!(l, Limits { time_ms: 2000, wall_time_ms: 5000, memory_kb: 131072, output_kb: 256 });
        let l = clamp(Limits { time_ms: 1, wall_time_ms: 1, memory_kb: 1, output_kb: 0 }, max);
        assert_eq!(l.time_ms, 100);
        assert!(l.wall_time_ms >= 600);
        assert_eq!(l.memory_kb, 8192);
        assert_eq!(l.output_kb, 1);
    }
}
