//! What students (and owners) see of a submission. Every student-facing
//! response is built here, so hidden-test protection lives in one place.

use std::collections::{BTreeMap, BTreeSet};

use ccal_core::compare::{CompareMode, DiffLine, diff};
use ccal_core::policy::EffectivePolicy;
use ccal_core::problem::ProblemContent;
use chrono::{DateTime, Utc};
use diagnostics::{Category, Confidence, Precision, RawDiagnostic, RenderContext, Severity, Span, TestRef};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::evaluate::{StoredResult, TestVerdict, observations};
use crate::state::ConfigSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Student,
    Owner,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SubmissionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub problem_id: Uuid,
    pub revision_id: Uuid,
    pub set_id: Option<Uuid>,
    pub kind: String,
    pub code: String,
    pub blanks: Option<serde_json::Value>,
    pub assembled_source: String,
    pub custom_stdin: Option<String>,
    pub status: String,
    pub verdict: Option<String>,
    pub passed: Option<i32>,
    pub total: Option<i32>,
    pub result: Option<serde_json::Value>,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub revision_number: i32,
    pub problem_slug: String,
    pub problem_title: String,
    pub content: serde_json::Value,
}

pub const SUBMISSION_SELECT: &str = "SELECT s.id, s.user_id, s.problem_id, s.revision_id, s.set_id, s.kind, s.code, s.blanks, s.assembled_source,
        s.custom_stdin, s.status, s.verdict, s.passed, s.total, s.result, s.attempts, s.last_error, s.created_at, s.finished_at,
        r.number AS revision_number, p.slug AS problem_slug, r.title AS problem_title, r.content
   FROM submissions s JOIN problem_revisions r ON r.id = s.revision_id JOIN problems p ON p.id = s.problem_id";

#[derive(Debug, Serialize, ToSchema)]
pub struct TestView {
    pub number: u32,
    pub name: Option<String>,
    pub hidden: bool,
    /// Custom stdin run (no expected output).
    pub custom: bool,
    pub verdict: TestVerdict,
    pub time_ms: u32,
    pub memory_kb: u32,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    /// Omitted for hidden tests (students).
    pub input: Option<String>,
    pub expected_output: Option<String>,
    pub actual_output: Option<String>,
    pub output_truncated: bool,
    pub stderr: Option<String>,
    /// Line diff with visible whitespace (public tests that failed).
    pub diff: Option<Vec<DiffLine>>,
    pub diff_truncated: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CardView {
    pub finding_id: u32,
    pub rule: String,
    #[schema(value_type = String)]
    pub category: Category,
    #[schema(value_type = String)]
    pub severity: Severity,
    #[schema(value_type = String)]
    pub confidence: Confidence,
    pub confidence_label: String,
    pub title: String,
    pub what: String,
    pub cause: Option<String>,
    pub where_text: Option<String>,
    pub why: Option<String>,
    pub next_steps: Vec<String>,
    /// Hints revealed so far (progressive).
    pub hints: Vec<String>,
    pub hints_total: usize,
    /// How many hints this student may reveal in total (policy).
    pub hints_allowed: usize,
    #[schema(value_type = Option<Object>)]
    pub span: Option<Span>,
    #[schema(value_type = String)]
    pub precision: Precision,
    pub precision_note: Option<String>,
    pub in_fixed_region: bool,
    pub fixed_region_note: Option<String>,
    pub primary: bool,
    pub maybe_cascade_of: Option<u32>,
    pub cascade_note: Option<String>,
    #[schema(value_type = Option<Object>)]
    pub test: Option<TestRef>,
    #[schema(value_type = Vec<Object>)]
    pub raw: Vec<RawDiagnostic>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SubmissionView {
    pub id: Uuid,
    pub problem_id: Uuid,
    pub problem_slug: String,
    pub problem_title: String,
    /// The immutable problem revision this submission was judged against.
    pub revision_number: i32,
    pub set_id: Option<Uuid>,
    /// `run`, `submit` or `test_run` (owner).
    pub kind: String,
    /// `queued`, `compiling`, `running`, `done` or `system_error`.
    pub status: String,
    /// Human-readable status in Traditional Chinese.
    pub status_text: String,
    pub verdict: Option<String>,
    pub passed: Option<i32>,
    pub total: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub code: String,
    #[schema(value_type = Option<Object>)]
    pub blanks: Option<serde_json::Value>,
    pub custom_stdin: Option<String>,
    pub compile_success: Option<bool>,
    pub tests: Vec<TestView>,
    pub cards: Vec<CardView>,
    pub compare_mode: CompareMode,
    pub compare_description: String,
    /// The judging service failed; the student may ask for a re-judge.
    pub retryable: bool,
    /// Policy: expand original compiler messages by default.
    pub raw_expanded: bool,
    /// Owner only: the exact program that was compiled and judge diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assembled_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempts: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,
}

pub fn status_text(status: &str) -> &'static str {
    match status {
        "queued" => "等待評測中",
        "compiling" => "編譯中",
        "running" => "執行測資中",
        "done" => "評測完成",
        "system_error" => "評測服務發生問題（不是你的程式造成的）",
        _ => "未知狀態",
    }
}

pub struct ViewContext<'a> {
    pub snapshot: &'a ConfigSnapshot,
    pub policy: &'a EffectivePolicy,
    /// Extra concepts from the problem set the submission was made in.
    pub set_concepts: &'a [String],
    /// finding id → number of hints revealed so far.
    pub revealed: &'a BTreeMap<u32, usize>,
    /// Use catalog drafts (dashboard preview).
    pub use_drafts: bool,
}

pub fn build(row: &SubmissionRow, audience: Audience, ctx: &ViewContext<'_>) -> SubmissionView {
    let content: Option<ProblemContent> = serde_json::from_value(row.content.clone()).ok();
    let stored: Option<StoredResult> = row.result.clone().and_then(|r| serde_json::from_value(r).ok());
    let compare_mode = content.as_ref().map(|c| c.compare_mode).unwrap_or_default();
    let catalog = if ctx.use_drafts { &ctx.snapshot.catalog_with_drafts } else { &ctx.snapshot.catalog };
    let compare_description = catalog.text(match compare_mode {
        CompareMode::Exact => "compare.exact",
        CompareMode::TrimTrailingWhitespace => "compare.trim_trailing_whitespace",
    });

    let mut tests = vec![];
    if let Some(st) = &stored {
        for t in &st.tests {
            let show = !t.hidden || audience == Audience::Owner;
            let case = t.case_index.and_then(|i| content.as_ref().and_then(|c| c.tests.get(i)));
            let input = if t.custom { row.custom_stdin.clone() } else { case.map(|c| c.input.clone()) };
            let expected = case.map(|c| c.expected_output.clone());
            let (d, dt) = match (&expected, t.verdict) {
                (Some(e), TestVerdict::WrongAnswer) if show => {
                    let (d, tr) = diff(e, &t.stdout, 200);
                    (Some(d), tr)
                }
                _ => (None, false),
            };
            tests.push(TestView {
                number: t.number,
                name: if show { t.name.clone() } else { None },
                hidden: t.hidden,
                custom: t.custom,
                verdict: t.verdict,
                time_ms: t.time_ms,
                memory_kb: t.memory_kb,
                exit_code: t.exit_code,
                signal: t.signal,
                input: show.then_some(input).flatten(),
                expected_output: show.then_some(expected).flatten(),
                actual_output: show.then(|| t.stdout.clone()),
                output_truncated: show && t.stdout_truncated,
                stderr: show.then(|| t.stderr.clone()).filter(|s| !s.is_empty()),
                diff: d,
                diff_truncated: dt,
            });
        }
    }

    let mut cards = vec![];
    let system_error = row.status == "system_error";
    if stored.is_some() || system_error {
        let obs = stored.as_ref().map(observations).unwrap_or_default();
        let fixed = content.as_ref().and_then(|c| fixed_regions(c, row)).unwrap_or_default();
        let adj = crate::state::priority_map(catalog);
        let analysis = diagnostics::analyze(&diagnostics::AnalysisInput {
            source: &row.code_for_analysis(),
            compile: stored.as_ref().and_then(|s| s.compile.as_ref()),
            runs: &obs,
            fixed_regions: &fixed,
            system_error,
            custom_rules: &catalog.custom_rules,
            priority_adjust: &adj,
        });
        let mut learned: BTreeSet<String> = ctx.policy.learned_concepts.iter().cloned().collect();
        learned.extend(ctx.set_concepts.iter().cloned());
        if let Some(c) = &content {
            learned.extend(c.concepts.iter().cloned());
        }
        let rendered = diagnostics::render(&analysis.findings, &RenderContext { catalog, builtin: &ctx.snapshot.builtin, learned: &learned });
        let auto = ctx.policy.auto_reveal_hints.value as usize;
        for c in rendered {
            let total = c.hints.len();
            let allowed = ctx.policy.max_hint_levels.value.map(|m| m as usize).unwrap_or(total).min(total);
            let shown = ctx.revealed.get(&c.finding_id).copied().unwrap_or(0).max(auto).min(allowed);
            cards.push(CardView {
                finding_id: c.finding_id,
                rule: c.rule,
                category: c.category,
                severity: c.severity,
                confidence: c.confidence,
                confidence_label: c.confidence_label,
                title: c.title,
                what: c.what,
                cause: c.cause,
                where_text: c.where_text,
                why: c.why,
                next_steps: c.next_steps,
                hints: c.hints.into_iter().take(shown).collect(),
                hints_total: total,
                hints_allowed: allowed,
                span: c.span,
                precision: c.precision,
                precision_note: c.precision_note,
                in_fixed_region: c.in_fixed_region,
                fixed_region_note: c.fixed_region_note,
                primary: c.primary,
                maybe_cascade_of: c.maybe_cascade_of,
                cascade_note: c.cascade_note,
                test: c.test,
                raw: c.raw,
            });
        }
    }

    let owner = audience == Audience::Owner;
    SubmissionView {
        id: row.id,
        problem_id: row.problem_id,
        problem_slug: row.problem_slug.clone(),
        problem_title: row.problem_title.clone(),
        revision_number: row.revision_number,
        set_id: row.set_id,
        kind: row.kind.clone(),
        status: row.status.clone(),
        status_text: status_text(&row.status).to_string(),
        verdict: row.verdict.clone(),
        passed: row.passed,
        total: row.total,
        created_at: row.created_at,
        finished_at: row.finished_at,
        code: row.code.clone(),
        blanks: row.blanks.clone(),
        custom_stdin: row.custom_stdin.clone(),
        compile_success: stored.as_ref().and_then(|s| s.compile.as_ref()).map(|c| c.success),
        tests,
        cards,
        compare_mode,
        compare_description,
        retryable: system_error && row.kind != "test_run",
        raw_expanded: ctx.policy.raw_expanded.value,
        assembled_source: owner.then(|| row.assembled_source.clone()),
        last_error: if owner { row.last_error.clone() } else { None },
        attempts: owner.then_some(row.attempts),
        user_id: owner.then_some(row.user_id),
    }
}

impl SubmissionRow {
    /// The assembled source equals what the student saw in the editor
    /// (fill-in templates keep the fixed text verbatim), so diagnostics line
    /// numbers map 1:1 to the editor.
    fn code_for_analysis(&self) -> String {
        self.assembled_source.clone()
    }
}

fn fixed_regions(c: &ProblemContent, row: &SubmissionRow) -> Option<Vec<diagnostics::FixedRegion>> {
    let tpl = ccal_core::fillin::Template::parse(c.template.as_deref()?).ok()?;
    let blanks: BTreeMap<String, String> = serde_json::from_value(row.blanks.clone()?).ok()?;
    Some(tpl.assemble(&blanks).ok()?.fixed_regions)
}
