//! Rule registry and the shared builder used by the rule passes.

pub(crate) mod compile;
pub(crate) mod lint;
pub(crate) mod runtime;

use std::collections::BTreeMap;

use crate::source::Source;
use crate::{Category, Confidence, Finding, FixedRegion, Precision, RawDiagnostic, Severity, Span, TestRef};

/// Variables every rule's templates may use (filled in automatically).
pub const COMMON_VARS: &[&str] = &["line", "column", "function", "test_number", "test_name"];

pub struct RuleSpec {
    pub id: &'static str,
    /// Evidence variables the rule may set.
    pub vars: &'static [&'static str],
    /// Default ordering weight.
    pub weight: i32,
}

macro_rules! specs {
    ($( $id:literal => [$($v:literal),*] @ $w:literal ),* $(,)?) => {
        pub const SPECS: &[RuleSpec] = &[ $( RuleSpec { id: $id, vars: &[$($v),*], weight: $w } ),* ];
    };
}

specs! {
    // Syntax
    "syntax.fullwidth_char" => ["char", "ascii"] @ 120,
    "syntax.stray_char" => ["char"] @ 110,
    "syntax.missing_semicolon" => ["token", "prev_line"] @ 100,
    "syntax.missing_close_paren" => ["token"] @ 100,
    "syntax.missing_open_paren" => ["keyword", "token"] @ 100,
    "syntax.unclosed_brace" => ["open_line"] @ 100,
    "syntax.extra_brace" => ["close_line"] @ 100,
    "syntax.unterminated_string" => [] @ 105,
    "syntax.expected_expression" => ["token"] @ 95,
    "syntax.unexpected_token" => ["token"] @ 90,
    // Identifiers
    "ident.undeclared" => ["ident", "suggestion", "declared_line"] @ 100,
    "ident.unknown_type" => ["type_name", "is_string", "is_bool"] @ 100,
    "ident.redefinition" => ["ident", "previous_line"] @ 100,
    // Functions
    "func.missing_include" => ["func", "header"] @ 50,
    "func.misspelled" => ["func", "suggestion"] @ 100,
    "func.used_before_definition" => ["func", "def_line"] @ 100,
    "func.undefined" => ["func"] @ 100,
    "func.too_few_args" => ["func", "decl_line"] @ 100,
    "func.too_many_args" => ["func", "decl_line"] @ 100,
    "func.conflicting_types" => ["func", "previous_line"] @ 100,
    "func.missing_return" => ["func"] @ 45,
    "func.void_return_value" => ["func"] @ 45,
    "func.return_without_value" => ["func"] @ 45,
    // Format strings
    "format.scanf_missing_address" => ["arg", "spec", "arg_number"] @ 60,
    "format.scanf_string_address" => ["arg", "spec"] @ 55,
    "format.scanf_type_mismatch" => ["arg", "spec", "expected", "actual", "suggest_spec"] @ 60,
    "format.printf_type_mismatch" => ["arg", "spec", "expected", "actual", "suggest_spec"] @ 60,
    "format.missing_argument" => ["spec", "call"] @ 60,
    "format.extra_argument" => ["call"] @ 40,
    // Linker
    "link.undefined_main" => ["suggestion"] @ 100,
    "link.undefined_function" => ["func", "suggestion", "call_line"] @ 100,
    "link.multiple_definition" => ["symbol"] @ 100,
    // Warnings
    "warn.unused_variable" => ["ident", "set_only"] @ 10,
    "warn.unused_parameter" => ["ident"] @ 5,
    "warn.uninitialized" => ["ident", "maybe"] @ 55,
    "warn.assign_in_condition" => [] @ 50,
    "warn.empty_body" => ["keyword"] @ 50,
    "warn.misleading_indentation" => ["keyword", "next_line"] @ 40,
    "warn.sign_compare" => [] @ 15,
    "warn.chained_comparison" => [] @ 45,
    "warn.no_effect" => ["has_equality"] @ 40,
    "warn.loop_overrun" => ["iteration"] @ 45,
    // Source lints (no compiler message)
    "lint.loop_empty_body" => ["keyword"] @ 50,
    "lint.while_scanf_no_compare" => [] @ 45,
    "lint.scanf_trailing_whitespace" => ["format"] @ 5,
    // Runtime and judging
    "run.segfault" => ["count", "warning_lines"] @ 90,
    "run.fpe" => ["count", "warning_lines"] @ 90,
    "run.abort" => ["count"] @ 90,
    "run.signal" => ["signal", "count"] @ 90,
    "run.nonzero_exit" => ["exit_code", "count"] @ 85,
    "run.time_limit" => ["limit_ms", "count", "while_scanf_line", "low_cpu"] @ 90,
    "run.memory_limit" => ["limit_mb", "count"] @ 90,
    "run.output_limit" => ["count"] @ 90,
    "judge.wrong_answer" => ["count", "hidden"] @ 80,
    "judge.whitespace_only" => ["count", "hidden"] @ 80,
    "judge.system_error" => [] @ 1000,
    "compile.timeout" => [] @ 150,
    // Honest fallbacks for anything without a dedicated rule
    "fallback.compile_error" => ["message"] @ 95,
    "fallback.warning" => ["message", "option"] @ 20,
    "fallback.link_error" => ["message"] @ 95,
}

pub fn spec(id: &str) -> Option<&'static RuleSpec> {
    SPECS.iter().find(|s| s.id == id)
}

#[derive(Debug, Clone)]
pub(crate) struct FindingDraft {
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    pub confidence: Confidence,
    pub span: Option<Span>,
    pub precision: Precision,
    pub vars: BTreeMap<String, String>,
    pub raw: Vec<RawDiagnostic>,
    pub test: Option<TestRef>,
    pub weight: i32,
}

impl FindingDraft {
    pub fn new(rule: &str, category: Category, severity: Severity, confidence: Confidence) -> FindingDraft {
        let weight = spec(rule).map(|s| s.weight).unwrap_or(50);
        FindingDraft {
            rule: rule.to_string(),
            category,
            severity,
            confidence,
            span: None,
            precision: Precision::Exact,
            vars: BTreeMap::new(),
            raw: vec![],
            test: None,
            weight,
        }
    }
    pub fn at(mut self, span: Option<Span>, precision: Precision) -> Self {
        self.span = span;
        self.precision = precision;
        self
    }
    pub fn var(mut self, k: &str, v: impl ToString) -> Self {
        debug_assert!(spec(&self.rule).is_none_or(|s| s.vars.contains(&k) || COMMON_VARS.contains(&k)), "rule {} does not declare var {k}", self.rule);
        self.vars.insert(k.to_string(), v.to_string());
        self
    }
    pub fn opt_var(self, k: &str, v: Option<impl ToString>) -> Self {
        match v {
            Some(v) => self.var(k, v),
            None => self,
        }
    }
    pub fn raw(mut self, raw: Vec<RawDiagnostic>) -> Self {
        self.raw.extend(raw);
        self
    }
    pub fn test(mut self, t: TestRef) -> Self {
        self.test = Some(t);
        self
    }
    pub fn weight(mut self, w: i32) -> Self {
        self.weight = w;
        self
    }
    pub fn line(&self) -> Option<u32> {
        self.span.map(|s| s.line)
    }
}

pub(crate) struct Builder<'s> {
    pub src: &'s Source<'s>,
    fixed: &'s [FixedRegion],
    pub drafts: Vec<FindingDraft>,
}

impl<'s> Builder<'s> {
    pub fn new(src: &'s Source<'s>, fixed: &'s [FixedRegion]) -> Builder<'s> {
        Builder { src, fixed, drafts: vec![] }
    }

    pub fn push(&mut self, d: FindingDraft) {
        self.drafts.push(d);
    }

    pub fn find_mut(&mut self, rule: &str, line: u32) -> Option<&mut FindingDraft> {
        self.drafts.iter_mut().find(|d| d.rule == rule && d.line() == Some(line))
    }

    /// Removes already-built findings matching `pred` and returns their raw
    /// diagnostics, so a later, better explanation can absorb them.
    pub fn take_raw(&mut self, pred: impl Fn(&FindingDraft) -> bool) -> Vec<RawDiagnostic> {
        let mut raw = vec![];
        self.drafts.retain(|d| {
            if pred(d) {
                raw.extend(d.raw.clone());
                false
            } else {
                true
            }
        });
        raw
    }

    pub fn has(&self, rule: &str) -> Option<&FindingDraft> {
        self.drafts.iter().find(|d| d.rule == rule)
    }

    pub fn finish(self) -> Vec<Finding> {
        let src = self.src;
        let fixed = self.fixed;
        self.drafts
            .into_iter()
            .enumerate()
            .map(|(i, mut d)| {
                if let Some(s) = d.span {
                    d.vars.entry("line".into()).or_insert_with(|| s.line.to_string());
                    d.vars.entry("column".into()).or_insert_with(|| s.column.to_string());
                    if let Some((f, _)) = src.enclosing_function(s.line) {
                        d.vars.entry("function".into()).or_insert(f);
                    }
                }
                if let Some(t) = &d.test {
                    d.vars.entry("test_number".into()).or_insert_with(|| t.number.to_string());
                    if let Some(n) = &t.name {
                        d.vars.entry("test_name".into()).or_insert_with(|| n.clone());
                    }
                }
                let in_fixed_region = d.span.is_some_and(|s| fixed.iter().any(|r| s.line >= r.start_line && s.line <= r.end_line));
                Finding {
                    id: i as u32 + 1,
                    rule: d.rule,
                    category: d.category,
                    severity: d.severity,
                    confidence: d.confidence,
                    span: d.span,
                    precision: d.precision,
                    vars: d.vars,
                    raw: d.raw,
                    primary: false,
                    maybe_cascade_of: None,
                    in_fixed_region,
                    test: d.test,
                    weight: d.weight,
                }
            })
            .collect()
    }
}
