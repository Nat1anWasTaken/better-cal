//! Ordering, "fix this first" selection and cascade marking.

use crate::source::Source;
use crate::{Category, Finding, Severity};

fn is_error(f: &Finding) -> bool {
    f.severity == Severity::Error && matches!(f.category, Category::CompileError | Category::LinkError)
}

/// Rules after which later errors in the same function are often knock-on
/// effects (the parser lost track of the program structure).
fn may_cause_cascade(rule: &str) -> bool {
    rule.starts_with("syntax.") || rule == "ident.unknown_type" || rule == "func.used_before_definition"
}

/// Warnings that typically appear only because of an earlier syntax error.
fn cascade_prone_warning(rule: &str) -> bool {
    matches!(rule, "warn.no_effect" | "warn.unused_variable" | "fallback.warning" | "format.printf_type_mismatch" | "format.scanf_type_mismatch")
}

pub(crate) fn rank(findings: &mut [Finding], compile_succeeded: bool, src: &Source<'_>) {
    if findings.is_empty() {
        return;
    }
    let line_key = |f: &Finding| f.span.map(|s| (s.line, s.column)).unwrap_or((u32::MAX, 0));

    let system = findings.iter().position(|f| f.category == Category::System);
    let primary_idx = if let Some(i) = system {
        Some(i)
    } else if !compile_succeeded && findings.iter().any(is_error) {
        // A confirmed look-alike character is almost always the root cause.
        findings
            .iter()
            .position(|f| f.rule == "syntax.fullwidth_char" && is_error(f))
            .or_else(|| findings.iter().enumerate().filter(|(_, f)| is_error(f)).min_by_key(|(_, f)| (line_key(f), -f.weight)).map(|(i, _)| i))
    } else {
        findings
            .iter()
            .enumerate()
            .filter(|(_, f)| f.severity != Severity::Info)
            .max_by_key(|(_, f)| (f.weight, std::cmp::Reverse(line_key(f))))
            .map(|(i, _)| i)
    };

    if let Some(p) = primary_idx {
        findings[p].primary = true;
        let primary = findings[p].clone();
        if !compile_succeeded && may_cause_cascade(&primary.rule) {
            let p_line = primary.span.map(|s| s.line).unwrap_or(0);
            let p_func = src.enclosing_function(p_line).map(|f| f.1);
            for f in findings.iter_mut() {
                if f.id == primary.id || f.span.is_none() {
                    continue;
                }
                let line = f.span.unwrap().line;
                let same_func = src.enclosing_function(line).map(|x| x.1) == p_func;
                // Unclosed braces / strings confuse everything after them.
                let global = matches!(primary.rule.as_str(), "syntax.unclosed_brace" | "syntax.unterminated_string" | "syntax.extra_brace");
                let after = line >= p_line;
                let eligible = if is_error(f) { true } else { cascade_prone_warning(&f.rule) };
                // "unused variable x" while x clearly appears again in the code
                // means the parser lost track of the code that uses it.
                let unused_but_used = f.rule == "warn.unused_variable" && f.vars.get("ident").is_some_and(|i| src.ident_count(i) > 1);
                if ((after && eligible) || unused_but_used) && (same_func || global) && f.rule != "syntax.fullwidth_char" {
                    f.maybe_cascade_of = Some(primary.id);
                }
            }
        }
    }

    // Order: primary, other errors (by position), cascades, runtime/answer,
    // warnings (by weight), info.
    findings.sort_by_key(|f| {
        let group = if f.primary {
            0
        } else if f.category == Category::System {
            1
        } else if is_error(f) && f.maybe_cascade_of.is_none() {
            2
        } else if f.maybe_cascade_of.is_some() {
            3
        } else if matches!(f.category, Category::Runtime | Category::WrongAnswer) {
            4
        } else if f.severity == Severity::Warning {
            5
        } else {
            6
        };
        let weight_key = if group == 5 { -f.weight } else { 0 };
        (group, weight_key, line_key(f))
    });
}
