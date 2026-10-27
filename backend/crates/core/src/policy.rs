//! Effective per-student policy: individual override > cohort > site default.
//! Learned concepts are additive across all levels (including the problem's
//! and the problem set's own concepts).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Every field is optional; unset fields fall through to the next level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyPatch {
    /// Concepts added at this level (union with other levels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned_concepts: Option<Vec<String>>,
    /// Restrict visible published sets to these (cohort level). `None` = all listed sets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_set_ids: Option<Vec<Uuid>>,
    /// Additional sets (including unlisted ones) visible at this level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_set_ids: Option<Vec<Uuid>>,
    /// Set shown on entry instead of the site's active set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_set_id: Option<Uuid>,
    /// Hint levels shown without clicking (0 = only the card).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_reveal_hints: Option<u8>,
    /// Maximum hint levels a student may reveal (`None` = all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_hint_levels: Option<u8>,
    /// Expand the original compiler message by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_expanded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_per_minute: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submits_per_minute: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Site,
    Cohort,
    Student,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Sourced<T> {
    pub value: T,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EffectivePolicy {
    /// Union of site, cohort and student concepts (problem/set concepts are added per request).
    pub learned_concepts: Vec<String>,
    pub visible_set_ids: Sourced<Option<Vec<Uuid>>>,
    pub extra_set_ids: Vec<Uuid>,
    pub default_set_id: Sourced<Option<Uuid>>,
    pub auto_reveal_hints: Sourced<u8>,
    pub max_hint_levels: Sourced<Option<u8>>,
    pub raw_expanded: Sourced<bool>,
    pub runs_per_minute: Sourced<u32>,
    pub submits_per_minute: Sourced<u32>,
}

/// System-wide hard caps that online settings cannot exceed.
pub const MAX_RUNS_PER_MINUTE: u32 = 30;
pub const MAX_SUBMITS_PER_MINUTE: u32 = 15;

pub fn validate_patch(p: &PolicyPatch, base: &str) -> Vec<crate::problem::FieldError> {
    use crate::problem::FieldError;
    let mut e = vec![];
    for (i, c) in p.learned_concepts.iter().flatten().enumerate() {
        if !crate::valid_concept(c) {
            e.push(FieldError::new(format!("{base}/learned_concepts/{i}"), "unknown_concept", format!("未知的概念 `{c}`")));
        }
    }
    if p.runs_per_minute.is_some_and(|r| r == 0 || r > MAX_RUNS_PER_MINUTE) {
        e.push(FieldError::new(format!("{base}/runs_per_minute"), "out_of_range", format!("必須介於 1 與 {MAX_RUNS_PER_MINUTE}")));
    }
    if p.submits_per_minute.is_some_and(|r| r == 0 || r > MAX_SUBMITS_PER_MINUTE) {
        e.push(FieldError::new(format!("{base}/submits_per_minute"), "out_of_range", format!("必須介於 1 與 {MAX_SUBMITS_PER_MINUTE}")));
    }
    if p.auto_reveal_hints.is_some_and(|h| h > 10) || p.max_hint_levels.is_some_and(|h| h > 10) {
        e.push(FieldError::new(format!("{base}/max_hint_levels"), "out_of_range", "提示層數必須介於 0 與 10"));
    }
    e
}

fn pick<T: Clone>(site: T, cohort: Option<T>, student: Option<T>) -> Sourced<T> {
    match (student, cohort) {
        (Some(v), _) => Sourced { value: v, source: Source::Student },
        (None, Some(v)) => Sourced { value: v, source: Source::Cohort },
        (None, None) => Sourced { value: site, source: Source::Site },
    }
}

/// Site defaults must be complete; use [`site_defaults`] as a base.
pub fn effective(site: &PolicyPatch, cohort: Option<&PolicyPatch>, student: Option<&PolicyPatch>, site_active_set: Option<Uuid>) -> EffectivePolicy {
    let empty = PolicyPatch::default();
    let c = cohort.unwrap_or(&empty);
    let s = student.unwrap_or(&empty);
    let d = site_defaults();
    let mut concepts: BTreeSet<String> = BTreeSet::new();
    for level in [site, c, s] {
        concepts.extend(level.learned_concepts.iter().flatten().cloned());
    }
    let mut extra: Vec<Uuid> = c.extra_set_ids.iter().flatten().chain(s.extra_set_ids.iter().flatten()).copied().collect();
    extra.sort();
    extra.dedup();
    let site_default_set = site.default_set_id.or(site_active_set);
    EffectivePolicy {
        learned_concepts: concepts.into_iter().collect(),
        visible_set_ids: pick(site.visible_set_ids.clone(), c.visible_set_ids.clone().map(Some), s.visible_set_ids.clone().map(Some)),
        extra_set_ids: extra,
        default_set_id: pick(site_default_set, c.default_set_id.map(Some), s.default_set_id.map(Some)),
        auto_reveal_hints: pick(site.auto_reveal_hints.or(d.auto_reveal_hints).unwrap_or(0), c.auto_reveal_hints, s.auto_reveal_hints),
        max_hint_levels: pick(site.max_hint_levels, c.max_hint_levels.map(Some), s.max_hint_levels.map(Some)),
        raw_expanded: pick(site.raw_expanded.or(d.raw_expanded).unwrap_or(false), c.raw_expanded, s.raw_expanded),
        runs_per_minute: pick(site.runs_per_minute.or(d.runs_per_minute).unwrap_or(10), c.runs_per_minute, s.runs_per_minute).clamp_value(MAX_RUNS_PER_MINUTE),
        submits_per_minute: pick(site.submits_per_minute.or(d.submits_per_minute).unwrap_or(5), c.submits_per_minute, s.submits_per_minute)
            .clamp_value(MAX_SUBMITS_PER_MINUTE),
    }
}

impl Sourced<u32> {
    fn clamp_value(mut self, max: u32) -> Self {
        self.value = self.value.clamp(1, max);
        self
    }
}

pub fn site_defaults() -> PolicyPatch {
    PolicyPatch {
        learned_concepts: Some(vec!["io".into(), "variables".into()]),
        visible_set_ids: None,
        extra_set_ids: None,
        default_set_id: None,
        auto_reveal_hints: Some(0),
        max_hint_levels: None,
        raw_expanded: Some(false),
        runs_per_minute: Some(10),
        submits_per_minute: Some(5),
    }
}

impl EffectivePolicy {
    /// Can this student see a published set?
    pub fn can_see_set(&self, set_id: Uuid, listed: bool) -> bool {
        if self.extra_set_ids.contains(&set_id) {
            return true;
        }
        match &self.visible_set_ids.value {
            Some(ids) => ids.contains(&set_id),
            None => listed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_student_over_cohort_over_site() {
        let site = site_defaults();
        let cohort = PolicyPatch { auto_reveal_hints: Some(1), learned_concepts: Some(vec!["loops".into()]), ..Default::default() };
        let student = PolicyPatch { auto_reveal_hints: Some(2), learned_concepts: Some(vec!["pointers".into()]), ..Default::default() };
        let e = effective(&site, Some(&cohort), Some(&student), None);
        assert_eq!(e.auto_reveal_hints, Sourced { value: 2, source: Source::Student });
        assert_eq!(e.learned_concepts, vec!["io", "loops", "pointers", "variables"]);
        let e = effective(&site, Some(&cohort), None, None);
        assert_eq!(e.auto_reveal_hints, Sourced { value: 1, source: Source::Cohort });
        let e = effective(&site, None, None, None);
        assert_eq!(e.auto_reveal_hints.source, Source::Site);
    }

    #[test]
    fn default_set_falls_back_to_active_set() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let e = effective(&site_defaults(), None, None, Some(a));
        assert_eq!(e.default_set_id.value, Some(a));
        let cohort = PolicyPatch { default_set_id: Some(b), ..Default::default() };
        assert_eq!(effective(&site_defaults(), Some(&cohort), None, Some(a)).default_set_id.value, Some(b));
    }

    #[test]
    fn visibility_with_cohort_restriction_and_student_extra() {
        let (s1, s2, s3) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let cohort = PolicyPatch { visible_set_ids: Some(vec![s1]), ..Default::default() };
        let student = PolicyPatch { extra_set_ids: Some(vec![s3]), ..Default::default() };
        let e = effective(&site_defaults(), Some(&cohort), Some(&student), None);
        assert!(e.can_see_set(s1, true));
        assert!(!e.can_see_set(s2, true));
        assert!(e.can_see_set(s3, false), "extra sets may be unlisted");
        let plain = effective(&site_defaults(), None, None, None);
        assert!(plain.can_see_set(s2, true));
        assert!(!plain.can_see_set(s3, false));
    }

    #[test]
    fn rates_are_clamped_and_validated() {
        let student = PolicyPatch { runs_per_minute: Some(999), ..Default::default() };
        assert_eq!(effective(&site_defaults(), None, Some(&student), None).runs_per_minute.value, MAX_RUNS_PER_MINUTE);
        assert_eq!(validate_patch(&student, "/policy")[0].pointer, "/policy/runs_per_minute");
    }
}
