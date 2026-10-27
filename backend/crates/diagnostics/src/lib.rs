//! Beginner-oriented diagnostics for C programs.
//!
//! The pipeline is split into two pure stages so it can be tested in isolation:
//!
//! 1. [`analyze`] turns real toolchain output (GCC JSON diagnostics, linker
//!    text, sandbox run results) plus the student's source into structured
//!    [`Finding`]s: a rule id, evidence variables, a location, a confidence
//!    level and the untouched raw diagnostics. No prose is produced here.
//! 2. [`render`] turns findings into teaching cards using a [`Catalog`] of
//!    editable Traditional Chinese text, picking wording variants based on the
//!    concepts the student has already learned.
//!
//! Matching logic lives in code (so it can be tested against real compiler
//! output); wording lives in the catalog (so owners can edit it online).

pub mod catalog;
pub mod gcc;
pub mod linker;
mod rank;
pub mod render;
mod rules;
pub mod source;
pub mod template;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use catalog::{Catalog, CatalogError, CustomRule, RuleText};
pub use render::{Card, RenderContext, render};
pub use rules::{COMMON_VARS, RuleSpec, SPECS, spec};

/// Where a problem was detected.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// The compiler refused to compile the program.
    CompileError,
    /// The linker could not build the executable.
    LinkError,
    /// The program compiled but the compiler (or our own lint) has a concern.
    Warning,
    /// The program crashed, ran too long, or used too much memory/output.
    Runtime,
    /// The program ran but produced a different answer.
    WrongAnswer,
    /// The judging service failed; never the student's fault.
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// How sure we are about the *cause* described on the card.
///
/// The existence of a compiler error is always a fact; this describes the
/// system's interpretation of why it happened.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Directly stated by the compiler/sandbox, or directly visible in the code.
    Confirmed,
    /// Strong evidence from the diagnostic plus the code.
    Likely,
    /// A plausible guess; the card must say so.
    Possible,
}

/// How precise the reported position is.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    /// The position points at the offending code.
    Exact,
    /// The compiler noticed the problem here; the cause may be earlier.
    Near,
    /// We could only guess a position (e.g. linker errors).
    Approximate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl Span {
    pub fn point(line: u32, column: u32) -> Span {
        Span { line, column, end_line: line, end_column: column }
    }
    pub fn line(line: u32) -> Span {
        Span { line, column: 1, end_line: line, end_column: 1 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RawOrigin {
    Compiler,
    Linker,
    Lint,
    Sandbox,
}

/// One untouched diagnostic as the tool reported it, kept for the
/// "show original message" section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawDiagnostic {
    pub origin: RawOrigin,
    pub kind: String,
    pub message: String,
    pub option: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    /// Stable within one analysis; used for hint-reveal requests.
    pub id: u32,
    /// Catalog key, e.g. `syntax.missing_semicolon`.
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    pub confidence: Confidence,
    pub span: Option<Span>,
    pub precision: Precision,
    /// Evidence used by the wording templates (identifier names, lines…).
    pub vars: BTreeMap<String, String>,
    pub raw: Vec<RawDiagnostic>,
    /// The single finding we recommend fixing first.
    pub primary: bool,
    /// Set when this finding may be a knock-on effect of an earlier one.
    pub maybe_cascade_of: Option<u32>,
    /// The location is inside code the student cannot edit (fill-in problems).
    pub in_fixed_region: bool,
    /// Test case this finding refers to (runtime / wrong answer).
    pub test: Option<TestRef>,
    /// Ordering weight (higher = more important); catalog may adjust it.
    pub weight: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestRef {
    /// 1-based position among the tests that were run.
    pub number: u32,
    pub hidden: bool,
    pub name: Option<String>,
}

/// One executed test, as the analyzer may see it. For hidden tests callers
/// must leave `stdout`/`stderr` empty so nothing can leak into a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunObservation {
    pub test: TestRef,
    pub status: judge_proto::RunStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub time_ms: u32,
    pub time_limit_ms: u32,
    pub memory_limit_kb: u32,
    pub wall_limit_with_low_cpu: bool,
    /// Only meaningful for completed runs: did the output match?
    pub answer_correct: Option<bool>,
    /// The outputs differ only in whitespace/newlines (exact compare mode).
    pub whitespace_only_difference: bool,
    pub stderr_excerpt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FixedRegion {
    pub start_line: u32,
    pub end_line: u32,
}

/// Everything the analyzer needs. All fields are student-safe.
#[derive(Debug, Clone)]
pub struct AnalysisInput<'a> {
    /// Source exactly as the student sees it in the editor.
    pub source: &'a str,
    pub compile: Option<&'a judge_proto::CompileResult>,
    pub runs: &'a [RunObservation],
    pub fixed_regions: &'a [FixedRegion],
    /// The judging service failed (after retries).
    pub system_error: bool,
    pub custom_rules: &'a [CustomRule],
    /// Per-rule ordering adjustments from the catalog (owner-tunable).
    pub priority_adjust: &'a BTreeMap<String, i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Analysis {
    pub findings: Vec<Finding>,
    pub compile_succeeded: bool,
}

/// Runs every rule and returns ranked findings.
pub fn analyze(input: &AnalysisInput<'_>) -> Analysis {
    let src = source::Source::new(input.source);
    let mut b = rules::Builder::new(&src, input.fixed_regions);

    if input.system_error {
        b.push(rules::FindingDraft::new("judge.system_error", Category::System, Severity::Error, Confidence::Confirmed).weight(1000));
    }

    // Source lints run first so compiler warnings that say the same thing can
    // be attached to them instead of producing a second card.
    rules::lint::run(&mut b);
    let mut compile_succeeded = true;
    if let Some(compile) = input.compile {
        compile_succeeded = compile.success;
        let diags = gcc::parse(&compile.diagnostics_json);
        let link = linker::parse(&compile.stderr_text);
        rules::compile::run(&mut b, &diags, &link, compile, input.custom_rules);
    }
    if compile_succeeded {
        rules::runtime::run(&mut b, input.runs);
    }

    let mut findings = b.finish();
    for f in &mut findings {
        if let Some(adj) = input.priority_adjust.get(&f.rule) {
            f.weight += adj;
        }
    }
    rank::rank(&mut findings, compile_succeeded, &src);
    Analysis { findings, compile_succeeded }
}

/// Concept ids that problems, sets, cohorts and students may declare as learned.
pub const CONCEPTS: &[(&str, &str)] = &[
    ("io", "輸入與輸出（printf、scanf）"),
    ("variables", "變數與型別"),
    ("conditionals", "條件判斷（if、else）"),
    ("loops", "迴圈（for、while）"),
    ("arrays", "陣列"),
    ("strings", "字串（char 陣列）"),
    ("functions", "自訂函式"),
    ("pointers", "指標與位址"),
    ("math", "數學函式庫（math.h）"),
];
