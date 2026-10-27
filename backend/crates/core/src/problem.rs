//! Problem content, validation and hashing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::compare::CompareMode;
use crate::fillin::Template;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProblemMode {
    /// The student writes the whole program.
    #[default]
    Full,
    /// The student fills in marked regions of a fixed template.
    FillIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Students see input and expected output; used by "Run".
    #[default]
    Public,
    /// Never shown to students; used only by "Submit".
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Example {
    pub input: String,
    pub output: String,
    #[serde(default)]
    pub explanation_md: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProblemLimits {
    /// CPU time per test, milliseconds (100–5000).
    pub time_ms: u32,
    /// Memory per test, KiB (16384–262144).
    pub memory_kb: u32,
    /// Output size per test, KiB (1–1024).
    pub output_kb: u32,
}

impl Default for ProblemLimits {
    fn default() -> Self {
        ProblemLimits { time_ms: 1000, memory_kb: 65536, output_kb: 64 }
    }
}

impl ProblemLimits {
    pub fn to_runner(self) -> judge_proto::Limits {
        judge_proto::Limits { time_ms: self.time_ms, wall_time_ms: self.time_ms * 3 + 1000, memory_kb: self.memory_kb, output_kb: self.output_kb }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestCaseInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub visibility: Visibility,
    pub input: String,
    pub expected_output: String,
}

/// Everything that defines one revision of a problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProblemContent {
    pub title: String,
    /// Markdown. Rendered safely (no raw HTML).
    pub statement_md: String,
    #[serde(default)]
    pub examples: Vec<Example>,
    #[serde(default)]
    pub mode: ProblemMode,
    /// Initial editor content (mode `full`).
    #[serde(default)]
    pub starter_code: String,
    /// Fill-in template with `/*@blank id@*/ … /*@end@*/` regions (mode `fill_in`).
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub limits: ProblemLimits,
    /// Concepts students are assumed to know for this problem.
    #[serde(default)]
    pub concepts: Vec<String>,
    #[serde(default)]
    pub compare_mode: CompareMode,
    /// Owner-only reference program (mode `full`); must pass every test to publish.
    #[serde(default)]
    pub reference_solution: Option<String>,
    /// Owner-only reference blanks (mode `fill_in`).
    #[serde(default)]
    pub reference_blanks: Option<BTreeMap<String, String>>,
    pub tests: Vec<TestCaseInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FieldError {
    /// JSON Pointer (RFC 6901) to the offending field, relative to the request body.
    pub pointer: String,
    /// Stable machine-readable code, e.g. `required`, `too_long`, `invalid`.
    pub code: String,
    /// Human-readable explanation (Traditional Chinese).
    pub message: String,
}

impl FieldError {
    pub fn new(pointer: impl Into<String>, code: &str, message: impl Into<String>) -> FieldError {
        FieldError { pointer: pointer.into(), code: code.into(), message: message.into() }
    }
}

pub const MAX_TESTS: usize = 50;
pub const MAX_TEST_BYTES: usize = 1024 * 1024;
pub const MAX_STATEMENT_BYTES: usize = 64 * 1024;
pub const MAX_CODE_BYTES: usize = 64 * 1024;

impl ProblemContent {
    /// Structural validation, applied on every save (drafts included).
    /// `base` is prefixed to every pointer (e.g. `/problems/3`).
    pub fn validate(&self, base: &str) -> Vec<FieldError> {
        let mut e = vec![];
        let p = |f: &str| format!("{base}/{f}");
        if self.title.trim().is_empty() {
            e.push(FieldError::new(p("title"), "required", "題目名稱不可空白"));
        } else if self.title.chars().count() > 120 {
            e.push(FieldError::new(p("title"), "too_long", "題目名稱最多 120 字"));
        }
        if self.statement_md.len() > MAX_STATEMENT_BYTES {
            e.push(FieldError::new(p("statement_md"), "too_long", "題敘最多 64 KiB"));
        }
        let l = self.limits;
        if !(100..=5000).contains(&l.time_ms) {
            e.push(FieldError::new(p("limits/time_ms"), "out_of_range", "時間限制必須介於 100 與 5000 毫秒之間"));
        }
        if !(16384..=262144).contains(&l.memory_kb) {
            e.push(FieldError::new(p("limits/memory_kb"), "out_of_range", "記憶體限制必須介於 16384 與 262144 KiB 之間"));
        }
        if !(1..=1024).contains(&l.output_kb) {
            e.push(FieldError::new(p("limits/output_kb"), "out_of_range", "輸出量限制必須介於 1 與 1024 KiB 之間"));
        }
        for (i, c) in self.concepts.iter().enumerate() {
            if !crate::valid_concept(c) {
                e.push(FieldError::new(p(&format!("concepts/{i}")), "unknown_concept", format!("未知的概念 `{c}`")));
            }
        }
        match self.mode {
            ProblemMode::Full => {
                if self.template.is_some() {
                    e.push(FieldError::new(p("template"), "not_allowed", "完整作答題（mode = full）不使用 template"));
                }
                if self.starter_code.len() > MAX_CODE_BYTES {
                    e.push(FieldError::new(p("starter_code"), "too_long", "起始程式碼最多 64 KiB"));
                }
                if self.reference_blanks.is_some() {
                    e.push(FieldError::new(p("reference_blanks"), "not_allowed", "完整作答題請使用 reference_solution"));
                }
            }
            ProblemMode::FillIn => match self.template.as_deref() {
                None => e.push(FieldError::new(p("template"), "required", "局部補寫題（mode = fill_in）需要 template")),
                Some(t) => match Template::parse(t) {
                    Err(err) => e.push(FieldError::new(p("template"), "invalid_template", err.to_string())),
                    Ok(tpl) => {
                        if let Some(rb) = &self.reference_blanks
                            && let Err(err) = tpl.assemble(rb)
                        {
                            e.push(FieldError::new(p("reference_blanks"), "invalid", err.to_string()));
                        }
                        if self.reference_solution.is_some() {
                            e.push(FieldError::new(p("reference_solution"), "not_allowed", "局部補寫題請使用 reference_blanks"));
                        }
                    }
                },
            },
        }
        if self.tests.len() > MAX_TESTS {
            e.push(FieldError::new(p("tests"), "too_many", format!("最多 {MAX_TESTS} 筆測資")));
        }
        for (i, t) in self.tests.iter().enumerate() {
            if t.input.len() > MAX_TEST_BYTES {
                e.push(FieldError::new(p(&format!("tests/{i}/input")), "too_long", "測資輸入最多 1 MiB"));
            }
            if t.expected_output.len() > MAX_TEST_BYTES {
                e.push(FieldError::new(p(&format!("tests/{i}/expected_output")), "too_long", "預期輸出最多 1 MiB"));
            }
            if t.name.as_ref().is_some_and(|n| n.chars().count() > 80) {
                e.push(FieldError::new(p(&format!("tests/{i}/name")), "too_long", "測資名稱最多 80 字"));
            }
        }
        e
    }

    /// Extra checks required before students can see the problem.
    /// Running the reference solution is done by the caller.
    pub fn validate_for_publish(&self, base: &str) -> Vec<FieldError> {
        let mut e = self.validate(base);
        let p = |f: &str| format!("{base}/{f}");
        if self.statement_md.trim().is_empty() {
            e.push(FieldError::new(p("statement_md"), "required", "發布前需要題敘"));
        }
        if !self.tests.iter().any(|t| t.visibility == Visibility::Public) {
            e.push(FieldError::new(p("tests"), "no_public_test", "發布前至少需要 1 筆公開測資（學生按「執行」時使用）"));
        }
        if !self.tests.iter().any(|t| t.visibility == Visibility::Hidden) {
            e.push(FieldError::new(p("tests"), "no_hidden_test", "發布前至少需要 1 筆隱藏測資（正式評測使用）"));
        }
        for (i, t) in self.tests.iter().enumerate() {
            if t.expected_output.trim().is_empty() {
                e.push(FieldError::new(p(&format!("tests/{i}/expected_output")), "empty_output", "預期輸出是空的，請確認這是刻意的"));
            }
        }
        if self.reference_solution.is_none() && self.reference_blanks.is_none() {
            e.push(FieldError::new(
                p(if self.mode == ProblemMode::FillIn { "reference_blanks" } else { "reference_solution" }),
                "required",
                "發布前需要參考解答，系統會用它驗證所有測資",
            ));
        }
        e
    }

    /// Source the editor starts with (template with default blanks for fill-in).
    pub fn initial_code(&self) -> String {
        match (self.mode, self.template.as_deref().and_then(|t| Template::parse(t).ok())) {
            (ProblemMode::FillIn, Some(t)) => t.assemble(&t.defaults()).map(|a| a.source).unwrap_or_default(),
            _ => self.starter_code.clone(),
        }
    }

    /// Stable hash of the content, used to make imports and saves idempotent.
    pub fn content_hash(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("content serializes");
        hex::encode(Sha256::digest(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> ProblemContent {
        ProblemContent {
            title: "A+B".into(),
            statement_md: "輸入兩個整數，輸出它們的和。".into(),
            examples: vec![],
            mode: ProblemMode::Full,
            starter_code: String::new(),
            template: None,
            limits: ProblemLimits::default(),
            concepts: vec!["io".into()],
            compare_mode: CompareMode::default(),
            reference_solution: Some("int main(void){return 0;}".into()),
            reference_blanks: None,
            tests: vec![
                TestCaseInput { name: None, visibility: Visibility::Public, input: "1 2\n".into(), expected_output: "3\n".into() },
                TestCaseInput { name: None, visibility: Visibility::Hidden, input: "5 6\n".into(), expected_output: "11\n".into() },
            ],
        }
    }

    #[test]
    fn valid_sample_has_no_errors() {
        assert!(sample().validate_for_publish("").is_empty());
    }

    #[test]
    fn publish_requires_public_and_hidden_tests() {
        let mut p = sample();
        p.tests.retain(|t| t.visibility == Visibility::Public);
        let e = p.validate_for_publish("/problems/0");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].pointer, "/problems/0/tests");
        assert_eq!(e[0].code, "no_hidden_test");
    }

    #[test]
    fn pointers_identify_fields() {
        let mut p = sample();
        p.limits.time_ms = 99999;
        p.concepts.push("quantum".into());
        let e = p.validate("");
        let ptrs: Vec<&str> = e.iter().map(|x| x.pointer.as_str()).collect();
        assert_eq!(ptrs, vec!["/limits/time_ms", "/concepts/1"]);
    }

    #[test]
    fn fill_in_requires_valid_template() {
        let mut p = sample();
        p.mode = ProblemMode::FillIn;
        p.reference_solution = None;
        p.template = Some("int main(void) { return 0; }".into());
        assert!(p.validate("").iter().any(|e| e.code == "invalid_template"));
        p.template = Some("int main(void) {\n/*@blank body@*/    return 0;\n/*@end@*/}\n".into());
        p.reference_blanks = Some([("body".to_string(), "    return 0;\n".to_string())].into());
        assert!(p.validate("").is_empty());
        assert_eq!(p.initial_code(), "int main(void) {\n    return 0;\n}\n");
    }

    #[test]
    fn hash_changes_with_content() {
        let a = sample();
        let mut b = sample();
        assert_eq!(a.content_hash(), b.content_hash());
        b.tests[1].expected_output = "12\n".into();
        assert_ne!(a.content_hash(), b.content_hash());
    }
}
