//! CYCU-CAL API server.
// Row tuples from `sqlx::query_as` are clearer inline than as one-off aliases.
#![allow(clippy::type_complexity)]

pub mod app;
pub mod auth;
pub mod catalog_store;
pub mod config;
pub mod error;
pub mod judge;
pub mod mcp;
pub mod oauth;
pub mod preferences;
pub mod repo;
pub mod routes;
pub mod settings;
pub mod state;

use std::sync::Arc;

use arc_swap::ArcSwap;
use sqlx::postgres::PgPoolOptions;

use crate::config::Config;
use crate::judge::client::RunnerClient;
use crate::state::{AppState, Inner, JudgeHealth, RateLimiter};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub async fn connect(cfg: &Config) -> anyhow::Result<sqlx::PgPool> {
    let mut last = None;
    // The database may still be starting (docker compose); retry for a while.
    for _ in 0..30 {
        match PgPoolOptions::new().max_connections(20).acquire_timeout(std::time::Duration::from_secs(5)).connect(&cfg.database_url).await {
            Ok(pool) => return Ok(pool),
            Err(e) => {
                tracing::warn!(error = %e, "database not ready, retrying");
                last = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
    Err(anyhow::anyhow!("could not connect to database: {}", last.map(|e| e.to_string()).unwrap_or_default()))
}

/// Builds the shared state (runs migrations, loads config).
pub async fn init_state(cfg: Config) -> anyhow::Result<AppState> {
    let db = connect(&cfg).await?;
    MIGRATOR.run(&db).await?;
    let snapshot = catalog_store::load_snapshot(&db).await?;
    let runner = RunnerClient::new(&cfg.runner_url, &cfg.runner_token, cfg.runner_timeout);
    Ok(AppState(Arc::new(Inner {
        db,
        cfg,
        runner,
        config: ArcSwap::from_pointee(snapshot),
        limiter: RateLimiter::default(),
        judge_notify: tokio::sync::Notify::new(),
        judge_health: tokio::sync::RwLock::new((JudgeHealth::Unknown, None)),
    })))
}
