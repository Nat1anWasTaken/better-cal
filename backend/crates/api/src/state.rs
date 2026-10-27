use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use diagnostics::Catalog;
use sqlx::PgPool;
use tokio::sync::{Mutex, Notify, RwLock};
use uuid::Uuid;

use crate::config::Config;
use crate::judge::client::RunnerClient;
use crate::settings::SiteSettings;

/// Everything that can be changed online, loaded together so a request sees
/// one consistent version.
pub struct ConfigSnapshot {
    pub version: i64,
    pub settings: SiteSettings,
    pub active_set_id: Option<Uuid>,
    /// Built-in catalog merged with *published* online edits.
    pub catalog: Catalog,
    /// Same, plus unpublished drafts (for previews in the dashboard).
    pub catalog_with_drafts: Catalog,
    /// Pristine built-in catalog (render fallback).
    pub builtin: Catalog,
}

impl ConfigSnapshot {
    pub fn priority_adjust(&self) -> BTreeMap<String, i32> {
        priority_map(&self.catalog)
    }
}

pub fn priority_map(c: &Catalog) -> BTreeMap<String, i32> {
    c.rules.iter().filter(|(_, e)| e.priority != 0).map(|(k, e)| (k.clone(), e.priority)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JudgeHealth {
    Ok,
    Down,
    Unknown,
}

pub struct Inner {
    pub db: PgPool,
    pub cfg: Config,
    pub runner: RunnerClient,
    pub config: ArcSwap<ConfigSnapshot>,
    pub limiter: RateLimiter,
    pub judge_notify: Notify,
    pub judge_health: RwLock<(JudgeHealth, Option<chrono::DateTime<chrono::Utc>>)>,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    pub fn snapshot(&self) -> Arc<ConfigSnapshot> {
        self.config.load_full()
    }

    /// Reloads settings + catalog from the database (after any online edit,
    /// or when another API process announces a change).
    pub async fn reload_config(&self) -> anyhow::Result<()> {
        let snap = crate::catalog_store::load_snapshot(&self.db).await?;
        self.config.store(Arc::new(snap));
        Ok(())
    }

    /// Bumps the config version and notifies other processes.
    pub async fn config_changed(&self) -> anyhow::Result<()> {
        sqlx::query("UPDATE site_config SET version = version + 1, updated_at = now() WHERE id = 1").execute(&self.db).await?;
        let _ = sqlx::query("SELECT pg_notify('ccal_config', 'changed')").execute(&self.db).await;
        self.reload_config().await
    }
}

/// Sliding-window limiter (per key, per minute). In-memory: limits are per
/// API process, which is sufficient for a single-node deployment.
#[derive(Default)]
pub struct RateLimiter {
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub async fn check(&self, key: &str, limit: u32, window: Duration) -> bool {
        let mut map = self.hits.lock().await;
        let now = Instant::now();
        let q = map.entry(key.to_string()).or_default();
        while q.front().is_some_and(|t| now.duration_since(*t) > window) {
            q.pop_front();
        }
        if q.len() as u32 >= limit {
            return false;
        }
        q.push_back(now);
        if map.len() > 50_000 {
            map.retain(|_, v| v.back().is_some_and(|t| now.duration_since(*t) < window));
        }
        true
    }
}
