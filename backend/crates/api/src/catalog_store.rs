//! Loads the editable catalog and site settings from the database.

use diagnostics::catalog::RuleEntry;
use diagnostics::{Catalog, CustomRule};
use sqlx::PgPool;
use uuid::Uuid;

use crate::settings::SiteSettings;
use crate::state::ConfigSnapshot;

#[derive(sqlx::FromRow)]
pub struct EntryRow {
    pub key: String,
    pub draft: Option<serde_json::Value>,
    pub published: Option<serde_json::Value>,
    pub draft_delete: bool,
}

/// Applies one stored entry to a catalog. Invalid stored JSON is skipped
/// (and logged) so a bad row can never break rendering for students.
pub fn apply(cat: &mut Catalog, key: &str, value: &serde_json::Value) {
    if let Some(id) = key.strip_prefix("rule:") {
        match serde_json::from_value::<RuleEntry>(value.clone()) {
            Ok(e) => {
                cat.rules.insert(id.to_string(), e);
            }
            Err(err) => tracing::warn!(key, %err, "ignoring invalid catalog rule entry"),
        }
    } else if key.starts_with("custom:") {
        match serde_json::from_value::<CustomRule>(value.clone()) {
            Ok(c) => {
                cat.custom_rules.retain(|x| x.id != c.id);
                cat.custom_rules.push(c);
            }
            Err(err) => tracing::warn!(key, %err, "ignoring invalid custom rule"),
        }
    } else if key == "texts" {
        match serde_json::from_value::<std::collections::BTreeMap<String, String>>(value.clone()) {
            Ok(t) => cat.texts.extend(t),
            Err(err) => tracing::warn!(%err, "ignoring invalid catalog texts"),
        }
    }
}

pub async fn load_snapshot(db: &PgPool) -> anyhow::Result<ConfigSnapshot> {
    let (version, settings, active_set_id): (i64, serde_json::Value, Option<Uuid>) =
        sqlx::query_as("SELECT version, settings, active_set_id FROM site_config WHERE id = 1").fetch_one(db).await?;
    let settings: SiteSettings = serde_json::from_value(settings).unwrap_or_else(|e| {
        tracing::warn!(%e, "invalid site settings in database; using defaults");
        SiteSettings::default()
    });
    let rows: Vec<EntryRow> = sqlx::query_as("SELECT key, draft, published, draft_delete FROM catalog_entries ORDER BY key").fetch_all(db).await?;
    let builtin = Catalog::builtin();
    let mut catalog = builtin.clone();
    for r in &rows {
        if let Some(p) = &r.published {
            apply(&mut catalog, &r.key, p);
        }
    }
    let mut with_drafts = catalog.clone();
    for r in &rows {
        if r.draft_delete {
            if let Some(id) = r.key.strip_prefix("rule:") {
                if let Some(b) = builtin.rules.get(id) {
                    with_drafts.rules.insert(id.to_string(), b.clone());
                }
            } else if let Some(id) = r.key.strip_prefix("custom:") {
                with_drafts.custom_rules.retain(|c| c.id != id);
            }
        } else if let Some(d) = &r.draft {
            apply(&mut with_drafts, &r.key, d);
        }
    }
    Ok(ConfigSnapshot { version, settings, active_set_id, catalog, catalog_with_drafts: with_drafts, builtin })
}

/// Background task: reload when another process changes config (LISTEN),
/// with a periodic version check as a fallback.
pub async fn watch(state: crate::state::AppState) {
    let mut listener = match sqlx::postgres::PgListener::connect_with(&state.db).await {
        Ok(mut l) => {
            if l.listen("ccal_config").await.is_err() {
                None
            } else {
                Some(l)
            }
        }
        Err(e) => {
            tracing::warn!(%e, "config LISTEN unavailable; falling back to polling");
            None
        }
    };
    loop {
        let notified = match listener.as_mut() {
            Some(l) => tokio::time::timeout(std::time::Duration::from_secs(10), l.recv()).await.is_ok(),
            None => {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                false
            }
        };
        let current: Option<i64> = sqlx::query_scalar("SELECT version FROM site_config WHERE id = 1").fetch_optional(&state.db).await.ok().flatten();
        if (notified || current.is_some_and(|v| v != state.snapshot().version))
            && let Err(e) = state.reload_config().await
        {
            tracing::warn!(%e, "config reload failed");
        }
    }
}
