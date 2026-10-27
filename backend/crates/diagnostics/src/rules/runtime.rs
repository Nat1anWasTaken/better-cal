//! Rules over sandbox run results and answer comparison.
//!
//! These never look at hidden-test inputs or outputs: callers only pass
//! statuses for hidden tests.

use judge_proto::RunStatus;

use super::{Builder, FindingDraft};
use crate::{Category, Confidence, Precision, RawDiagnostic, RawOrigin, RunObservation, Severity};

const SIGABRT: i32 = 6;
const SIGFPE: i32 = 8;
const SIGSEGV: i32 = 11;

/// Warnings that commonly explain a crash or wrong behaviour at run time.
const RISKY_WARNINGS: &[&str] = &[
    "format.scanf_missing_address",
    "format.scanf_string_address",
    "format.scanf_type_mismatch",
    "format.printf_type_mismatch",
    "format.missing_argument",
    "warn.uninitialized",
    "warn.loop_overrun",
    "warn.assign_in_condition",
    "warn.empty_body",
    "lint.loop_empty_body",
    "func.missing_return",
    "func.used_before_definition",
];

fn sandbox_raw(_o: &RunObservation, message: String) -> RawDiagnostic {
    RawDiagnostic { origin: RawOrigin::Sandbox, kind: "run".into(), message, option: None, line: None, column: None }
}

fn describe(o: &RunObservation) -> String {
    let base = match o.status {
        RunStatus::TimeLimit => format!("time limit exceeded ({} ms CPU used, limit {} ms)", o.time_ms, o.time_limit_ms),
        RunStatus::MemoryLimit => format!("memory limit exceeded (limit {} KiB)", o.memory_limit_kb),
        RunStatus::OutputLimit => "output limit exceeded (SIGXFSZ)".to_string(),
        RunStatus::Signaled => format!("killed by signal {}", o.signal.unwrap_or(0)),
        RunStatus::NonZeroExit => format!("exited with code {}", o.exit_code.unwrap_or(0)),
        _ => "finished".to_string(),
    };
    let mut s = format!("test #{}{}: {base}", o.test.number, if o.test.hidden { " (hidden)" } else { "" });
    // stderr of hidden tests is never passed in, so this is safe to show.
    if !o.test.hidden && !o.stderr_excerpt.is_empty() {
        s.push_str("\nstderr: ");
        s.push_str(&o.stderr_excerpt);
    }
    s
}

pub(crate) fn run(b: &mut Builder<'_>, runs: &[RunObservation]) {
    let warning_lines: Vec<String> =
        b.drafts.iter().filter(|d| RISKY_WARNINGS.contains(&d.rule.as_str())).filter_map(|d| d.line()).map(|l| l.to_string()).collect();
    let warning_lines = (!warning_lines.is_empty()).then(|| warning_lines.join("、"));
    let while_scanf_line = b.has("lint.while_scanf_no_compare").and_then(|d| d.line());

    // A public test failing for any reason means the public set did not pass.
    let mut public = runs.iter().filter(|o| !o.test.hidden).peekable();
    let public_passed = public.peek().is_some() && public.all(|o| o.status == RunStatus::Ok && o.answer_correct == Some(true));

    // One card per kind of problem; remember how many tests hit it and how
    // many of those were hidden.
    let mut drafts: Vec<FindingDraft> = vec![];
    let mut hidden_counts: Vec<u32> = vec![];
    for o in runs {
        let (rule, cat, conf) = match o.status {
            RunStatus::Signaled if o.signal == Some(SIGSEGV) => ("run.segfault", Category::Runtime, Confidence::Possible),
            RunStatus::Signaled if o.signal == Some(SIGFPE) => ("run.fpe", Category::Runtime, Confidence::Likely),
            RunStatus::Signaled if o.signal == Some(SIGABRT) => ("run.abort", Category::Runtime, Confidence::Possible),
            RunStatus::Signaled => ("run.signal", Category::Runtime, Confidence::Possible),
            RunStatus::NonZeroExit => ("run.nonzero_exit", Category::Runtime, Confidence::Likely),
            RunStatus::TimeLimit => ("run.time_limit", Category::Runtime, Confidence::Possible),
            RunStatus::MemoryLimit => ("run.memory_limit", Category::Runtime, Confidence::Possible),
            RunStatus::OutputLimit => ("run.output_limit", Category::Runtime, Confidence::Possible),
            RunStatus::Ok if o.answer_correct == Some(false) && o.whitespace_only_difference => {
                ("judge.whitespace_only", Category::WrongAnswer, Confidence::Confirmed)
            }
            RunStatus::Ok if o.answer_correct == Some(false) => ("judge.wrong_answer", Category::WrongAnswer, Confidence::Confirmed),
            RunStatus::SandboxError => ("judge.system_error", Category::System, Confidence::Confirmed),
            _ => continue,
        };
        if let Some(i) = drafts.iter().position(|d| d.rule == rule) {
            hidden_counts[i] += u32::from(o.test.hidden);
            let existing = &mut drafts[i];
            let n: u32 = existing.vars.get("count").and_then(|c| c.parse().ok()).unwrap_or(1) + 1;
            existing.vars.insert("count".into(), n.to_string());
            existing.raw.push(sandbox_raw(o, describe(o)));
            continue;
        }
        // `count` is only set once a second test hits the same problem.
        let f =
            FindingDraft::new(rule, cat, Severity::Error, conf).at(None, Precision::Approximate).test(o.test.clone()).raw(vec![sandbox_raw(o, describe(o))]);
        let f = match rule {
            "run.segfault" | "run.fpe" => f.opt_var("warning_lines", warning_lines.clone()),
            "run.signal" => f.opt_var("signal", o.signal),
            "run.nonzero_exit" => f.opt_var("exit_code", o.exit_code),
            "run.time_limit" => {
                f.var("limit_ms", o.time_limit_ms).opt_var("while_scanf_line", while_scanf_line).opt_var("low_cpu", o.wall_limit_with_low_cpu.then_some("1"))
            }
            "run.memory_limit" => f.var("limit_mb", o.memory_limit_kb / 1024),
            "judge.wrong_answer" | "judge.whitespace_only" => f.opt_var("hidden", o.test.hidden.then_some("1")),
            _ => f,
        };
        drafts.push(f);
        hidden_counts.push(u32::from(o.test.hidden));
    }
    for (d, hidden) in drafts.iter_mut().zip(hidden_counts) {
        let first_hidden = d.test.as_ref().is_some_and(|t| t.hidden);
        match d.rule.as_str() {
            "judge.wrong_answer" | "judge.whitespace_only" => {
                // The card shows the first failing test; say when hidden tests failed too.
                if !first_hidden && hidden > 0 {
                    d.vars.insert("also_hidden".into(), hidden.to_string());
                }
                if first_hidden && public_passed {
                    d.vars.insert("public_passed".into(), "1".into());
                }
            }
            "run.time_limit" if public_passed => {
                d.vars.insert("public_passed".into(), "1".into());
            }
            _ => {}
        }
    }
    for d in drafts {
        b.push(d);
    }
}
