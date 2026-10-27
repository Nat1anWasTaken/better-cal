//! Site-wide settings editable online (stored in `site_config.settings`).

use ccal_core::compare::CompareMode;
use ccal_core::policy::PolicyPatch;
use ccal_core::problem::{FieldError, ProblemLimits};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SiteSettings {
    pub site_name: String,
    /// Markdown shown on the student home page (rendered without raw HTML).
    pub announcement_md: String,
    /// Defaults for newly created problems.
    pub default_limits: ProblemLimits,
    pub default_compare_mode: CompareMode,
    /// Site-level policy (lowest precedence; cohorts and students override it).
    pub default_policy: PolicyPatch,
    /// Accept MCP connections at `/mcp` (also requires `PUBLIC_URL`).
    pub mcp_enabled: bool,
}

impl Default for SiteSettings {
    fn default() -> Self {
        SiteSettings {
            site_name: "CYCU-CAL 程式讀書會".into(),
            announcement_md: String::new(),
            default_limits: ProblemLimits::default(),
            default_compare_mode: CompareMode::default(),
            default_policy: ccal_core::policy::site_defaults(),
            mcp_enabled: false,
        }
    }
}

impl SiteSettings {
    pub fn validate(&self) -> Vec<FieldError> {
        let mut e = vec![];
        if self.site_name.trim().is_empty() || self.site_name.chars().count() > 60 {
            e.push(FieldError::new("/site_name", "invalid", "站名需為 1–60 字"));
        }
        if self.announcement_md.len() > 16 * 1024 {
            e.push(FieldError::new("/announcement_md", "too_long", "公告最多 16 KiB"));
        }
        let l = self.default_limits;
        if !(100..=5000).contains(&l.time_ms) || !(16384..=262144).contains(&l.memory_kb) || !(1..=1024).contains(&l.output_kb) {
            e.push(FieldError::new("/default_limits", "out_of_range", "預設限制超出允許範圍"));
        }
        e.extend(ccal_core::policy::validate_patch(&self.default_policy, "/default_policy"));
        e
    }
}
