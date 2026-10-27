//! JSON import/export format `cycu-cal.import/v1`.
//!
//! Items are keyed by `slug`, the stable external identifier, so re-importing
//! the same document is a no-op and never duplicates content.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::problem::{FieldError, ProblemContent};

pub const FORMAT_VERSION: &str = "cycu-cal.import/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportDocument {
    /// Must be `cycu-cal.import/v1`.
    pub format_version: String,
    #[serde(default)]
    pub problems: Vec<ImportProblem>,
    #[serde(default)]
    pub problem_sets: Vec<ImportSet>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportProblem {
    /// Stable identifier (lower-case letters, digits, hyphens).
    pub slug: String,
    pub content: ProblemContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportSet {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description_md: String,
    /// Concepts students are assumed to know in this set.
    #[serde(default)]
    pub learned_concepts: Vec<String>,
    /// Unlisted sets are only visible to cohorts/students granted access.
    #[serde(default = "yes")]
    pub listed: bool,
    /// Problem slugs in display order (from this document or already in the bank).
    pub problems: Vec<String>,
}

fn yes() -> bool {
    true
}

/// Validates everything that can be checked without the database.
/// `existing_problem_slugs` lets sets reference problems already in the bank.
pub fn validate(doc: &ImportDocument, existing_problem_slugs: &BTreeSet<String>) -> Vec<FieldError> {
    let mut e = vec![];
    if doc.format_version != FORMAT_VERSION {
        e.push(FieldError::new("/format_version", "unsupported_version", format!("不支援的格式版本；請使用 `{FORMAT_VERSION}`")));
        return e;
    }
    if doc.problems.is_empty() && doc.problem_sets.is_empty() {
        e.push(FieldError::new("", "empty", "文件裡沒有任何題目或題目集"));
    }
    let mut slugs = BTreeSet::new();
    for (i, p) in doc.problems.iter().enumerate() {
        if !crate::valid_slug(&p.slug) {
            e.push(FieldError::new(format!("/problems/{i}/slug"), "invalid_slug", "slug 只能使用小寫英文字母、數字與單一連字號，長度 3–64"));
        }
        if !slugs.insert(p.slug.clone()) {
            e.push(FieldError::new(format!("/problems/{i}/slug"), "duplicate", format!("slug `{}` 在文件中重複", p.slug)));
        }
        e.extend(p.content.validate(&format!("/problems/{i}/content")));
    }
    let mut set_slugs = BTreeSet::new();
    for (i, s) in doc.problem_sets.iter().enumerate() {
        let b = format!("/problem_sets/{i}");
        if !crate::valid_slug(&s.slug) {
            e.push(FieldError::new(format!("{b}/slug"), "invalid_slug", "slug 只能使用小寫英文字母、數字與單一連字號，長度 3–64"));
        }
        if !set_slugs.insert(s.slug.clone()) {
            e.push(FieldError::new(format!("{b}/slug"), "duplicate", format!("slug `{}` 在文件中重複", s.slug)));
        }
        if s.title.trim().is_empty() {
            e.push(FieldError::new(format!("{b}/title"), "required", "題目集名稱不可空白"));
        }
        for (j, c) in s.learned_concepts.iter().enumerate() {
            if !crate::valid_concept(c) {
                e.push(FieldError::new(format!("{b}/learned_concepts/{j}"), "unknown_concept", format!("未知的概念 `{c}`")));
            }
        }
        let mut seen = BTreeSet::new();
        for (j, ps) in s.problems.iter().enumerate() {
            if !slugs.contains(ps) && !existing_problem_slugs.contains(ps) {
                e.push(FieldError::new(format!("{b}/problems/{j}"), "unknown_problem", format!("找不到題目 `{ps}`（不在這份文件，也不在題庫中）")));
            }
            if !seen.insert(ps) {
                e.push(FieldError::new(format!("{b}/problems/{j}"), "duplicate", format!("題目 `{ps}` 在同一個題目集中重複")));
            }
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> ImportDocument {
        serde_json::from_value(serde_json::json!({
            "format_version": FORMAT_VERSION,
            "problems": [{
                "slug": "a-plus-b",
                "content": {
                    "title": "A+B",
                    "statement_md": "輸入兩個整數。",
                    "reference_solution": "int main(void){return 0;}",
                    "tests": [{"input": "1 2\n", "expected_output": "3\n"}]
                }
            }],
            "problem_sets": [{"slug": "week-1", "title": "第 1 週", "problems": ["a-plus-b", "hello"]}]
        }))
        .unwrap()
    }

    #[test]
    fn unknown_fields_are_rejected_by_parser() {
        let r: Result<ImportDocument, _> = serde_json::from_value(serde_json::json!({"format_version": FORMAT_VERSION, "problemz": []}));
        assert!(r.is_err());
    }

    #[test]
    fn reports_unknown_problem_with_pointer() {
        let e = validate(&doc(), &BTreeSet::new());
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].pointer, "/problem_sets/0/problems/1");
        assert_eq!(e[0].code, "unknown_problem");
        let existing: BTreeSet<String> = ["hello".to_string()].into();
        assert!(validate(&doc(), &existing).is_empty());
    }

    #[test]
    fn rejects_wrong_version_and_duplicates() {
        let mut d = doc();
        d.format_version = "v0".into();
        assert_eq!(validate(&d, &BTreeSet::new())[0].code, "unsupported_version");
        let mut d = doc();
        d.problems.push(d.problems[0].clone());
        let e = validate(&d, &["hello".to_string()].into());
        assert!(e.iter().any(|x| x.pointer == "/problems/1/slug" && x.code == "duplicate"));
    }

    #[test]
    fn nested_content_errors_have_full_pointer() {
        let mut d = doc();
        d.problems[0].content.limits.output_kb = 0;
        let e = validate(&d, &["hello".to_string()].into());
        assert_eq!(e[0].pointer, "/problems/0/content/limits/output_kb");
    }
}
