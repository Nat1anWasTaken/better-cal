//! Turns runner output + expected answers into verdicts. Pure functions.

use ccal_core::compare::{CompareMode, compare};
use ccal_core::problem::{ProblemContent, ProblemLimits, Visibility};
use diagnostics::{RunObservation, TestRef};
use judge_proto::{CompileResult, RunStatus, TestRunResult};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Accepted,
    WrongAnswer,
    CompileError,
    RuntimeError,
    TimeLimit,
    MemoryLimit,
    OutputLimit,
    /// Custom-input run finished (nothing to compare against).
    Completed,
    /// The judging service failed; not the student's fault.
    SystemError,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Accepted => "accepted",
            Verdict::WrongAnswer => "wrong_answer",
            Verdict::CompileError => "compile_error",
            Verdict::RuntimeError => "runtime_error",
            Verdict::TimeLimit => "time_limit",
            Verdict::MemoryLimit => "memory_limit",
            Verdict::OutputLimit => "output_limit",
            Verdict::Completed => "completed",
            Verdict::SystemError => "system_error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestVerdict {
    Accepted,
    WrongAnswer,
    RuntimeError,
    TimeLimit,
    MemoryLimit,
    OutputLimit,
    Skipped,
    Completed,
}

/// What we run for one submission.
#[derive(Debug, Clone)]
pub struct TestSpec {
    /// Index into the revision's tests (None for custom stdin).
    pub case_index: Option<usize>,
    pub name: Option<String>,
    pub hidden: bool,
    pub input: String,
    pub expected: Option<String>,
}

/// Full result stored server-side (never sent to students as-is).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredResult {
    pub compile: Option<CompileResult>,
    pub tests: Vec<StoredTest>,
    pub compare_mode: CompareMode,
    pub limits: ProblemLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTest {
    pub number: u32,
    pub case_index: Option<usize>,
    pub name: Option<String>,
    pub hidden: bool,
    pub custom: bool,
    pub status: RunStatus,
    pub verdict: TestVerdict,
    pub time_ms: u32,
    pub memory_kb: u32,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub wall_low_cpu: bool,
    pub whitespace_only: bool,
    pub stdout: String,
    pub stdout_truncated: bool,
    pub stderr: String,
}

/// Tests to run for a submission kind.
pub fn select_tests(content: &ProblemContent, kind: &str, custom_stdin: Option<&str>) -> Vec<TestSpec> {
    if let Some(stdin) = custom_stdin {
        return vec![TestSpec { case_index: None, name: Some("自訂輸入".into()), hidden: false, input: stdin.to_string(), expected: None }];
    }
    content
        .tests
        .iter()
        .enumerate()
        .filter(|(_, t)| kind != "run" || t.visibility == Visibility::Public)
        .map(|(i, t)| TestSpec {
            case_index: Some(i),
            name: t.name.clone(),
            hidden: t.visibility == Visibility::Hidden,
            input: t.input.clone(),
            expected: Some(t.expected_output.clone()),
        })
        .collect()
}

fn test_verdict(r: &TestRunResult, spec: &TestSpec, mode: CompareMode) -> (TestVerdict, bool) {
    match r.status {
        RunStatus::Ok => match &spec.expected {
            None => (TestVerdict::Completed, false),
            Some(exp) => {
                let o = compare(exp, &r.stdout, mode);
                if o.matches && !r.stdout_truncated { (TestVerdict::Accepted, false) } else { (TestVerdict::WrongAnswer, o.whitespace_only) }
            }
        },
        RunStatus::NonZeroExit | RunStatus::Signaled => (TestVerdict::RuntimeError, false),
        RunStatus::TimeLimit => (TestVerdict::TimeLimit, false),
        RunStatus::MemoryLimit => (TestVerdict::MemoryLimit, false),
        RunStatus::OutputLimit => (TestVerdict::OutputLimit, false),
        RunStatus::Skipped | RunStatus::SandboxError => (TestVerdict::Skipped, false),
    }
}

pub struct Evaluation {
    pub verdict: Verdict,
    pub passed: i32,
    pub total: i32,
    pub stored: StoredResult,
}

pub fn evaluate(compile: CompileResult, runs: &[TestRunResult], specs: &[TestSpec], mode: CompareMode, limits: ProblemLimits) -> Evaluation {
    let total = specs.len() as i32;
    if !compile.success {
        return Evaluation {
            verdict: Verdict::CompileError,
            passed: 0,
            total,
            stored: StoredResult { compile: Some(compile), tests: vec![], compare_mode: mode, limits },
        };
    }
    let mut tests = vec![];
    for (i, (spec, r)) in specs.iter().zip(runs).enumerate() {
        let (verdict, whitespace_only) = test_verdict(r, spec, mode);
        tests.push(StoredTest {
            number: i as u32 + 1,
            case_index: spec.case_index,
            name: spec.name.clone(),
            hidden: spec.hidden,
            custom: spec.case_index.is_none(),
            status: r.status,
            verdict,
            time_ms: r.time_ms,
            memory_kb: r.memory_kb,
            exit_code: r.exit_code,
            signal: r.signal,
            wall_low_cpu: r.wall_limit_with_low_cpu,
            whitespace_only,
            stdout: r.stdout.clone(),
            stdout_truncated: r.stdout_truncated,
            stderr: r.stderr.clone(),
        });
    }
    let passed = tests.iter().filter(|t| matches!(t.verdict, TestVerdict::Accepted | TestVerdict::Completed)).count() as i32;
    // The first failing test decides the overall verdict (tests run in order).
    let verdict = tests
        .iter()
        .find_map(|t| match t.verdict {
            TestVerdict::RuntimeError => Some(Verdict::RuntimeError),
            TestVerdict::TimeLimit => Some(Verdict::TimeLimit),
            TestVerdict::MemoryLimit => Some(Verdict::MemoryLimit),
            TestVerdict::OutputLimit => Some(Verdict::OutputLimit),
            TestVerdict::WrongAnswer => Some(Verdict::WrongAnswer),
            _ => None,
        })
        .unwrap_or(if specs.iter().all(|s| s.expected.is_none()) { Verdict::Completed } else { Verdict::Accepted });
    Evaluation { verdict, passed, total, stored: StoredResult { compile: Some(compile), tests, compare_mode: mode, limits } }
}

/// Observations for the diagnostics analyzer. Hidden tests contribute only
/// their status: no stdout, no stderr, nothing derived from their input.
pub fn observations(stored: &StoredResult) -> Vec<RunObservation> {
    stored
        .tests
        .iter()
        .map(|t| RunObservation {
            test: TestRef { number: t.number, hidden: t.hidden, name: if t.hidden { None } else { t.name.clone() } },
            status: t.status,
            exit_code: t.exit_code,
            signal: t.signal,
            time_ms: t.time_ms,
            time_limit_ms: stored.limits.time_ms,
            memory_limit_kb: stored.limits.memory_kb,
            wall_limit_with_low_cpu: t.wall_low_cpu,
            answer_correct: match t.verdict {
                TestVerdict::Accepted => Some(true),
                TestVerdict::WrongAnswer => Some(false),
                _ => None,
            },
            whitespace_only_difference: t.whitespace_only,
            stderr_excerpt: if t.hidden { String::new() } else { t.stderr.chars().take(500).collect() },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_run(stdout: &str) -> TestRunResult {
        TestRunResult {
            id: "t".into(),
            status: RunStatus::Ok,
            exit_code: Some(0),
            signal: None,
            time_ms: 1,
            wall_time_ms: 1,
            memory_kb: 1000,
            wall_limit_with_low_cpu: false,
            stdout: stdout.into(),
            stdout_truncated: false,
            stderr: String::new(),
            stderr_truncated: false,
        }
    }

    fn compile_ok() -> CompileResult {
        CompileResult { success: true, exit_code: Some(0), timed_out: false, diagnostics_json: serde_json::json!([]), stderr_text: String::new() }
    }

    fn spec(expected: &str, hidden: bool) -> TestSpec {
        TestSpec { case_index: Some(0), name: None, hidden, input: "SECRET-INPUT".into(), expected: Some(expected.into()) }
    }

    #[test]
    fn first_failure_decides_verdict() {
        let specs = vec![spec("3\n", false), spec("5\n", true), spec("7\n", true)];
        let mut tle = ok_run("");
        tle.status = RunStatus::TimeLimit;
        let e = evaluate(compile_ok(), &[ok_run("3\n"), ok_run("6\n"), tle], &specs, CompareMode::default(), ProblemLimits::default());
        assert_eq!(e.verdict, Verdict::WrongAnswer);
        assert_eq!((e.passed, e.total), (1, 3));
    }

    #[test]
    fn hidden_observations_carry_no_output() {
        let specs = vec![spec("5\n", true)];
        let mut r = ok_run("SECRET-OUTPUT");
        r.stderr = "SECRET-ERR".into();
        let e = evaluate(compile_ok(), &[r], &specs, CompareMode::default(), ProblemLimits::default());
        let obs = observations(&e.stored);
        let s = format!("{obs:?}");
        assert!(!s.contains("SECRET"));
    }

    #[test]
    fn truncated_output_is_never_accepted() {
        let mut r = ok_run("3\n");
        r.stdout_truncated = true;
        let e = evaluate(compile_ok(), &[r], &[spec("3\n", false)], CompareMode::default(), ProblemLimits::default());
        assert_eq!(e.verdict, Verdict::WrongAnswer);
    }
}
