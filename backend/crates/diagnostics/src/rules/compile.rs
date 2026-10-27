//! Rules over compiler (GCC JSON) and linker diagnostics.

use std::sync::LazyLock;

use judge_proto::CompileResult;
use regex::Regex;

use super::{Builder, FindingDraft};
use crate::gcc::Diag;
use crate::linker::LinkIssue;
use crate::source::{Source, TokKind, ascii_lookalike, similar_name};
use crate::{Category, Confidence, CustomRule, Precision, RawDiagnostic, RawOrigin, Severity, Span};

macro_rules! re {
    ($name:ident, $pat:literal) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

re!(RE_SEMI, r"^expected (?:',' or )?';' before (.+)$");
re!(RE_CLOSE_PAREN, r"^expected '\)' before (.+)$");
re!(RE_OPEN_PAREN, r"^expected '\(' before (.+)$");
re!(RE_EXPECTED_EXPR, r"^expected expression before (.+)$");
re!(RE_EXPECTED_IDENT_OR_PAREN, r"^expected identifier or '\(' before (.+)$");
re!(RE_EXPECTED_ANY, r"^expected .+ before (.+)$");
re!(RE_EXPECTED_LIST_WITH_SEMI, r"^expected .*';'.* before (.+)$");
re!(RE_STRAY, r"^stray '(.+)' in program$");
re!(RE_UNDECLARED, r"^'(\w+)' undeclared (?:here )?\((?:first use in this function|not in a function)\)(?:; did you mean '(\w+)'\?)?$");
re!(RE_UNKNOWN_TYPE, r"^unknown type name '(\w+)'");
re!(RE_REDEFINITION, r"^redefinition of '(\w+)'$");
re!(RE_PREVIOUS, r"^previous (?:definition|declaration) of '(\w+)'");
re!(RE_IMPLICIT, r"^implicit declaration of function '(\w+)'(?:; did you mean '(\w+)'\?)?$");
re!(RE_INCLUDE_NOTE, r"^include '<([\w./]+)>' or provide a declaration of '(\w+)'$");
re!(RE_INCOMPATIBLE_BUILTIN, r"^incompatible implicit declaration of built-in function '(\w+)'$");
re!(RE_CONFLICTING, r"^conflicting types for '(\w+)'");
re!(RE_ARG_COUNT, r"^too (few|many) arguments to function '(\w+)'$");
re!(RE_FORMAT_TYPE, r"^format '(%[^']*)' expects argument of type '([^']+)', but argument (\d+) has type '([^']+)'");
re!(RE_FORMAT_MISSING, r"^format '(%[^']*)' expects a matching '([^']+)' argument$");
re!(RE_UNUSED_VAR, r"^unused variable '(\w+)'$");
re!(RE_SET_NOT_USED, r"^variable '(\w+)' set but not used$");
re!(RE_UNUSED_PARAM, r"^unused parameter '(\w+)'$");
re!(RE_UNINIT, r"^'(\w+)' (is|may be) used uninitialized");
re!(RE_EMPTY_BODY, r"^suggest braces around empty body in an? '(\w+)' statement$");
re!(RE_MISLEADING, r"^this '(\w+)' clause does not guard");
re!(RE_LOOP_UB, r"^iteration (\d+) invokes undefined behavior$");
re!(RE_CHAR_ARRAY_PTR, r"^char \(\*\)\[\d+\]$");

/// Strips GCC's quoting from a token description: `'}' token` → `}`.
fn clean_token(t: &str) -> String {
    let t = t.trim().trim_end_matches(" token");
    t.trim_matches(|c| c == '\'' || c == '‘' || c == '’').to_string()
}

fn span_of(d: &Diag) -> Option<Span> {
    let loc = d.locations.first()?;
    let start = loc.start.as_ref().unwrap_or(&loc.caret);
    let finish = loc.finish.as_ref().unwrap_or(start);
    Some(Span { line: loc.caret.line, column: loc.caret.col(), end_line: finish.line.max(start.line), end_column: finish.col().max(start.col()) })
}

fn kind_of(d: &Diag) -> (Category, Severity) {
    if d.effective_error() { (Category::CompileError, Severity::Error) } else { (Category::Warning, Severity::Warning) }
}

struct Ctx<'a, 'b> {
    b: &'a mut Builder<'b>,
    diags: &'a [Diag],
    claimed: Vec<bool>,
    link: &'a [LinkIssue],
    link_claimed: Vec<bool>,
}

impl Ctx<'_, '_> {
    fn src(&self) -> &Source<'_> {
        self.b.src
    }

    /// Claims (marks handled) every unclaimed diagnostic matching `pred` and
    /// returns their raw form, so related messages land on one card.
    fn claim_where(&mut self, pred: impl Fn(&Diag) -> bool) -> Vec<RawDiagnostic> {
        let mut raw = vec![];
        for (i, d) in self.diags.iter().enumerate() {
            if !self.claimed[i] && pred(d) {
                self.claimed[i] = true;
                raw.extend(d.to_raw());
            }
        }
        raw
    }

    fn claim_undefined_reference(&mut self, symbol: &str) -> Option<RawDiagnostic> {
        for (i, l) in self.link.iter().enumerate() {
            if let LinkIssue::UndefinedReference { symbol: s, .. } = l
                && s == symbol
                && !self.link_claimed[i]
            {
                self.link_claimed[i] = true;
                return Some(l.to_raw());
            }
        }
        None
    }

    fn call_line(&self, name: &str) -> Option<u32> {
        self.src().calls(name).first().map(|c| c.line)
    }
}

pub(crate) fn run(b: &mut Builder<'_>, diags: &[Diag], link: &[LinkIssue], compile: &CompileResult, custom: &[CustomRule]) {
    let mut cx = Ctx { b, diags, claimed: vec![false; diags.len()], link, link_claimed: vec![false; link.len()] };

    if compile.timed_out {
        cx.b.push(FindingDraft::new("compile.timeout", Category::CompileError, Severity::Error, Confidence::Confirmed));
    }

    fullwidth(&mut cx, compile.success);

    // Indexing is intentional: rules may mark *other* diagnostics as handled.
    #[allow(clippy::needless_range_loop)]
    for i in 0..diags.len() {
        if cx.claimed[i] {
            continue;
        }
        let d = &diags[i];
        if d.is_note() {
            continue;
        }
        cx.claimed[i] = true;
        let handled = syntax(&mut cx, d)
            || identifiers(&mut cx, d)
            || functions(&mut cx, d)
            || format(&mut cx, d)
            || warnings(&mut cx, d)
            || custom_rule(&mut cx, d, custom);
        if !handled {
            fallback(&mut cx, d);
        }
    }

    linker(&mut cx);

    // Never leave a failed compile without a card (e.g. a driver crash or
    // output we could not parse): show the compiler's own words instead.
    if !compile.success && !cx.b.drafts.iter().any(|d| d.severity == Severity::Error) {
        let mut raw: Vec<RawDiagnostic> = diags.iter().flat_map(Diag::to_raw).collect();
        raw.extend(compile.stderr_text.lines().filter(|l| !l.trim().is_empty()).map(|l| RawDiagnostic {
            origin: RawOrigin::Compiler,
            kind: "error".into(),
            message: l.to_string(),
            option: None,
            line: None,
            column: None,
        }));
        cx.b.push(FindingDraft::new("compile.unexplained", Category::CompileError, Severity::Error, Confidence::Confirmed).raw(raw));
    }
}

/// Full-width punctuation from Chinese input methods. This is checked on the
/// source itself, so the location is exact and the finding is confirmed.
fn fullwidth(cx: &mut Ctx<'_, '_>, compile_ok: bool) {
    let mut seen_lines = vec![];
    let hits: Vec<(String, u32, u32)> =
        cx.src().foreign_chars().into_iter().filter(|t| ascii_lookalike(&t.text).is_some()).map(|t| (t.text.clone(), t.line, t.col)).collect();
    for (ch, line, col) in hits {
        if seen_lines.contains(&line) {
            continue;
        }
        seen_lines.push(line);
        let next = cx.src().tokens.iter().find(|t| t.line > line).map(|t| t.line);
        // Messages on this line, and "expected … before" on the next code
        // line, are consequences of the look-alike character.
        let raw = cx.claim_where(|d| d.line() == Some(line) || (d.line() == next && next.is_some() && d.message.starts_with("expected")));
        let (cat, sev) = if compile_ok { (Category::Warning, Severity::Warning) } else { (Category::CompileError, Severity::Error) };
        cx.b.push(
            FindingDraft::new("syntax.fullwidth_char", cat, sev, Confidence::Confirmed)
                .at(Some(Span::point(line, col)), Precision::Exact)
                .var("char", &ch)
                .var("ascii", ascii_lookalike(&ch).unwrap_or("?"))
                .raw(raw),
        );
    }
}

fn syntax(cx: &mut Ctx<'_, '_>, d: &Diag) -> bool {
    let (cat, sev) = kind_of(d);
    let span = span_of(d);
    let m = d.message.as_str();

    if let Some(c) = RE_STRAY.captures(m) {
        let ch = c[1].to_string();
        let line = d.line();
        let raw = cx.claim_where(|o| o.line() == line && RE_STRAY.is_match(&o.message));
        cx.b.push(FindingDraft::new("syntax.stray_char", cat, sev, Confidence::Confirmed).at(span, Precision::Exact).var("char", ch).raw(d.to_raw()).raw(raw));
        return true;
    }
    if m.contains("missing terminating") {
        let line = d.line();
        let raw = cx.claim_where(|o| o.line() == line && o.message.contains("missing terminating"));
        cx.b.push(
            FindingDraft::new("syntax.unterminated_string", Category::CompileError, Severity::Error, Confidence::Confirmed)
                .at(span, Precision::Exact)
                .raw(d.to_raw())
                .raw(raw),
        );
        return true;
    }
    if let Some(c) = RE_SEMI.captures(m) {
        let token = clean_token(&c[1]);
        let caret = d.caret().cloned();
        let prev_line = caret.filter(|p| cx.src().is_first_token_on_line(p.line, p.col())).and_then(|p| cx.src().prev_code_line(p.line));
        cx.b.push(
            FindingDraft::new("syntax.missing_semicolon", cat, sev, Confidence::Likely)
                .at(span, Precision::Near)
                .var("token", token)
                .opt_var("prev_line", prev_line)
                .raw(d.to_raw()),
        );
        return true;
    }
    // "expected '=', ',', ';', 'asm' or '__attribute__' before 'scanf'" after
    // `int a, b` — counts as a missing semicolon only with evidence from the
    // code: the reported token starts a new line and the previous code line
    // does not end a statement or block.
    if let Some(c) = RE_EXPECTED_LIST_WITH_SEMI.captures(m)
        && let Some(p) = d.caret()
        && cx.src().is_first_token_on_line(p.line, p.col())
        && let Some(prev) = cx.src().prev_code_line(p.line)
    {
        let prev_text = cx.src().line_text(prev).trim_end().to_string();
        let ends_open = prev_text.ends_with([';', '{', '}', ',', '(']) || prev_text.starts_with('#');
        if !ends_open {
            cx.b.push(
                FindingDraft::new("syntax.missing_semicolon", cat, sev, Confidence::Likely)
                    .at(span, Precision::Near)
                    .var("token", clean_token(&c[1]))
                    .var("prev_line", prev)
                    .raw(d.to_raw()),
            );
            return true;
        }
    }
    if let Some(c) = RE_CLOSE_PAREN.captures(m) {
        cx.b.push(
            FindingDraft::new("syntax.missing_close_paren", cat, sev, Confidence::Likely)
                .at(span, Precision::Near)
                .var("token", clean_token(&c[1]))
                .raw(d.to_raw()),
        );
        return true;
    }
    if let Some(c) = RE_OPEN_PAREN.captures(m) {
        let keyword = d.caret().and_then(|p| {
            cx.src()
                .tokens
                .iter()
                .rev()
                .find(|t| t.line < p.line || (t.line == p.line && t.col < p.col()))
                .filter(|t| ["if", "while", "for", "switch"].contains(&t.text.as_str()))
                .map(|t| t.text.clone())
        });
        cx.b.push(
            FindingDraft::new("syntax.missing_open_paren", cat, sev, Confidence::Likely)
                .at(span, Precision::Near)
                .var("token", clean_token(&c[1]))
                .opt_var("keyword", keyword)
                .raw(d.to_raw()),
        );
        return true;
    }
    if m == "expected declaration or statement at end of input" {
        let raw = cx.claim_where(|o| o.message == "expected declaration or statement at end of input");
        let report = cx.src().brace_report();
        cx.b.push(
            FindingDraft::new("syntax.unclosed_brace", cat, sev, Confidence::Likely)
                .at(span, Precision::Near)
                .opt_var("open_line", report.unclosed_open_lines.first())
                .raw(d.to_raw())
                .raw(raw),
        );
        return true;
    }
    if RE_EXPECTED_IDENT_OR_PAREN.is_match(m) {
        let report = cx.src().brace_report();
        if let Some(close_line) = report.function_closed_early_line.or(report.first_extra_close_line) {
            let raw = cx.claim_where(|o| RE_EXPECTED_IDENT_OR_PAREN.is_match(&o.message));
            cx.b.push(
                FindingDraft::new("syntax.extra_brace", cat, sev, Confidence::Likely)
                    .at(Some(Span::line(close_line)), Precision::Approximate)
                    .var("close_line", close_line)
                    .raw(d.to_raw())
                    .raw(raw),
            );
            return true;
        }
    }
    if let Some(c) = RE_EXPECTED_EXPR.captures(m) {
        cx.b.push(
            FindingDraft::new("syntax.expected_expression", cat, sev, Confidence::Possible)
                .at(span, Precision::Near)
                .var("token", clean_token(&c[1]))
                .raw(d.to_raw()),
        );
        return true;
    }
    if let Some(c) = RE_EXPECTED_ANY.captures(m) {
        cx.b.push(
            FindingDraft::new("syntax.unexpected_token", cat, sev, Confidence::Possible)
                .at(span, Precision::Near)
                .var("token", clean_token(&c[1]))
                .raw(d.to_raw()),
        );
        return true;
    }
    false
}

fn identifiers(cx: &mut Ctx<'_, '_>, d: &Diag) -> bool {
    let (cat, sev) = kind_of(d);
    let span = span_of(d);
    let m = d.message.as_str();

    if let Some(c) = RE_UNDECLARED.captures(m) {
        let ident = c[1].to_string();
        let line = d.line().unwrap_or(0);
        let declared_line = cx.src().declaration_lines(&ident).into_iter().find(|l| *l != line);
        // An "unused variable" warning for the same name usually means it was
        // declared inside an inner block.
        let mut related = cx.claim_where(|o| RE_UNUSED_VAR.captures(&o.message).is_some_and(|c| c[1] == ident));
        related.extend(cx.b.take_raw(|f| f.rule == "warn.unused_variable" && f.vars.get("ident") == Some(&ident)));
        cx.b.push(
            FindingDraft::new("ident.undeclared", cat, sev, Confidence::Likely)
                .at(span, Precision::Exact)
                .var("ident", &ident)
                .opt_var("suggestion", c.get(2).map(|s| s.as_str()))
                .opt_var("declared_line", declared_line)
                .raw(d.to_raw())
                .raw(related),
        );
        return true;
    }
    if let Some(c) = RE_UNKNOWN_TYPE.captures(m) {
        // GCC's own "did you mean" here is often misleading (string → stdin),
        // so it is intentionally not used.
        let t = c[1].to_string();
        let lower = t.to_ascii_lowercase();
        cx.b.push(
            FindingDraft::new("ident.unknown_type", cat, sev, Confidence::Likely)
                .at(span, Precision::Exact)
                .var("type_name", &t)
                .opt_var("is_string", (lower == "string").then_some("1"))
                .opt_var("is_bool", (lower == "bool" || lower == "boolean").then_some("1"))
                .raw(d.to_raw()),
        );
        return true;
    }
    if let Some(c) = RE_REDEFINITION.captures(m) {
        let previous_line = d.children.iter().find(|ch| RE_PREVIOUS.is_match(&ch.message)).and_then(|ch| ch.line());
        cx.b.push(
            FindingDraft::new("ident.redefinition", cat, sev, Confidence::Confirmed)
                .at(span, Precision::Exact)
                .var("ident", &c[1])
                .opt_var("previous_line", previous_line)
                .raw(d.to_raw()),
        );
        return true;
    }
    false
}

fn functions(cx: &mut Ctx<'_, '_>, d: &Diag) -> bool {
    let span = span_of(d);
    let m = d.message.as_str();

    if let Some(c) = RE_IMPLICIT.captures(m) {
        let func = c[1].to_string();
        let gcc_suggestion = c.get(2).map(|s| s.as_str().to_string());
        let use_line = d.line().unwrap_or(0);
        let header = d.children.iter().find_map(|ch| RE_INCLUDE_NOTE.captures(&ch.message).map(|c| c[1].to_string()));
        let same_func = |o: &Diag| {
            RE_INCOMPATIBLE_BUILTIN.captures(&o.message).is_some_and(|c| c[1] == func)
                || (o.is_note() && RE_INCLUDE_NOTE.captures(&o.message).is_some_and(|c| c[2] == func))
                || RE_IMPLICIT.captures(&o.message).is_some_and(|c| c[1] == func)
        };
        let mut raw = d.to_raw();
        raw.extend(cx.claim_where(same_func));

        if let Some(header) = header {
            cx.b.push(
                FindingDraft::new("func.missing_include", Category::Warning, Severity::Warning, Confidence::Likely)
                    .at(span, Precision::Exact)
                    .var("func", &func)
                    .var("header", header)
                    .raw(raw),
            );
            return true;
        }
        let definition = cx.src().function_definitions().into_iter().find(|(n, _)| *n == func);
        if let Some((_, def_line)) = definition.filter(|(_, l)| *l > use_line) {
            // Implicit declaration makes GCC assume `int f()`; a later
            // definition with other types then conflicts, and format checks on
            // the call see the wrong type.
            raw.extend(cx.claim_where(|o| RE_CONFLICTING.captures(&o.message).is_some_and(|c| c[1] == func)));
            raw.extend(cx.claim_where(|o| o.line() == Some(use_line) && RE_FORMAT_TYPE.captures(&o.message).is_some_and(|c| &c[4] == "int")));
            let failed = raw.iter().any(|r| r.kind.contains("error"));
            let (cat, sev) = if failed { (Category::CompileError, Severity::Error) } else { (Category::Warning, Severity::Warning) };
            cx.b.push(
                FindingDraft::new("func.used_before_definition", cat, sev, Confidence::Likely)
                    .at(span, Precision::Exact)
                    .var("func", &func)
                    .var("def_line", def_line)
                    .raw(raw)
                    .weight(if failed { 100 } else { 60 }),
            );
            return true;
        }
        let suggestion = gcc_suggestion.or_else(|| cx.src().function_definitions().into_iter().map(|(n, _)| n).find(|n| similar_name(&func, n)));
        let link_raw = cx.claim_undefined_reference(&func);
        let (cat, sev) = if link_raw.is_some() { (Category::LinkError, Severity::Error) } else { (Category::Warning, Severity::Warning) };
        raw.extend(link_raw);
        let rule = if suggestion.is_some() { "func.misspelled" } else { "func.undefined" };
        cx.b.push(
            FindingDraft::new(rule, cat, sev, Confidence::Likely)
                .at(span, Precision::Exact)
                .var("func", &func)
                .opt_var("suggestion", suggestion)
                .raw(raw)
                .weight(if sev == Severity::Error { 100 } else { 70 }),
        );
        return true;
    }
    if let Some(c) = RE_ARG_COUNT.captures(m) {
        let rule = if &c[1] == "few" { "func.too_few_args" } else { "func.too_many_args" };
        let decl_line = d.children.iter().find(|ch| ch.message == "declared here").and_then(|ch| ch.line());
        let (cat, sev) = kind_of(d);
        cx.b.push(
            FindingDraft::new(rule, cat, sev, Confidence::Confirmed)
                .at(span, Precision::Exact)
                .var("func", &c[2])
                .opt_var("decl_line", decl_line)
                .raw(d.to_raw()),
        );
        return true;
    }
    if let Some(c) = RE_CONFLICTING.captures(m) {
        let previous_line = d.children.iter().find_map(|ch| ch.line());
        let (cat, sev) = kind_of(d);
        cx.b.push(
            FindingDraft::new("func.conflicting_types", cat, sev, Confidence::Likely)
                .at(span, Precision::Exact)
                .var("func", &c[1])
                .opt_var("previous_line", previous_line)
                .raw(d.to_raw()),
        );
        return true;
    }
    let func = d.line().and_then(|l| cx.src().enclosing_function(l)).map(|(f, _)| f);
    let simple = match m {
        "control reaches end of non-void function" => Some(("func.missing_return", Confidence::Likely)),
        "'return' with a value, in function returning void" => Some(("func.void_return_value", Confidence::Confirmed)),
        "'return' with no value, in function returning non-void" => Some(("func.return_without_value", Confidence::Confirmed)),
        _ => None,
    };
    if let Some((rule, conf)) = simple {
        let (cat, sev) = kind_of(d);
        cx.b.push(FindingDraft::new(rule, cat, sev, conf).at(span, Precision::Exact).opt_var("func", func).raw(d.to_raw()));
        return true;
    }
    false
}

const SCANF_FAMILY: &[&str] = &["scanf", "fscanf", "sscanf"];
const PRINTF_FAMILY: &[&str] = &["printf", "fprintf", "sprintf", "snprintf"];

/// Finds the scanf/printf-family call a format diagnostic refers to.
fn format_call(src: &Source<'_>, line: u32, col: u32) -> Option<(String, Vec<String>)> {
    let mut best: Option<(String, Vec<String>)> = None;
    for name in SCANF_FAMILY.iter().chain(PRINTF_FAMILY) {
        for call in src.calls(name) {
            let start = &src.tokens[call.name_index];
            let end = &src.tokens[call.close_index];
            let after_start = (start.line, start.col) <= (line, col);
            let before_end = (line, col) <= (end.line, end.col);
            if after_start && before_end || (best.is_none() && call.line == line) {
                let args = call.args.iter().map(|r| src.text_of(r.clone())).collect();
                best = Some((name.to_string(), args));
            }
        }
    }
    best
}

fn suggest_spec(actual: &str) -> Option<&'static str> {
    Some(match actual {
        "double *" => "%lf",
        "float *" => "%f",
        "int *" => "%d",
        "long int *" => "%ld",
        "long long int *" => "%lld",
        "double" | "float" => "%f",
        "int" => "%d",
        "long int" => "%ld",
        "long long int" => "%lld",
        "char *" => "%s",
        _ => return None,
    })
}

/// `char *` in scanf is either `&c` (one character → `%c`) or an array (`%s`).
fn scanf_char_spec(is_scanf: bool, actual: &str, arg: Option<&str>) -> Option<&'static str> {
    if !is_scanf || actual != "char *" {
        return None;
    }
    Some(if arg.is_some_and(|a| a.starts_with('&')) { "%c" } else { "%s" })
}

fn format(cx: &mut Ctx<'_, '_>, d: &Diag) -> bool {
    let (cat, sev) = kind_of(d);
    let span = span_of(d);
    let m = d.message.as_str();
    let (line, col) = d.caret().map(|p| (p.line, p.col())).unwrap_or((0, 0));

    if let Some(c) = RE_FORMAT_TYPE.captures(m) {
        let (spec, expected, arg_no, actual) = (c[1].to_string(), c[2].to_string(), c[3].parse::<usize>().unwrap_or(0), c[4].to_string());
        let call = format_call(cx.src(), line, col);
        let is_scanf = call.as_ref().map(|(n, _)| SCANF_FAMILY.contains(&n.as_str())).unwrap_or(expected.ends_with('*') && expected != "char *");
        let arg = call.as_ref().and_then(|(_, args)| args.get(arg_no.saturating_sub(1)).cloned());
        let raw = d.to_raw();

        if is_scanf && expected.strip_suffix(" *") == Some(actual.as_str()) {
            // Evidence: is the argument a plain variable name without `&`?
            let plain_ident =
                arg.as_deref().is_some_and(|a| a.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') && !a.starts_with(|ch: char| ch.is_ascii_digit()));
            let conf = if plain_ident { Confidence::Likely } else { Confidence::Possible };
            let ident = arg.clone().unwrap_or_default();
            let related = cx.claim_where(|o| o.line() == Some(line) && RE_UNINIT.captures(&o.message).is_some_and(|c| c[1] == ident));
            cx.b.push(
                FindingDraft::new("format.scanf_missing_address", cat, sev, conf)
                    .at(span, Precision::Exact)
                    .opt_var("arg", arg)
                    .var("spec", spec)
                    .var("arg_number", arg_no)
                    .raw(raw)
                    .raw(related),
            );
            return true;
        }
        if is_scanf && expected == "char *" && RE_CHAR_ARRAY_PTR.is_match(&actual) {
            cx.b.push(
                FindingDraft::new("format.scanf_string_address", cat, sev, Confidence::Likely)
                    .at(span, Precision::Exact)
                    .opt_var("arg", arg)
                    .var("spec", spec)
                    .raw(raw),
            );
            return true;
        }
        let rule = if is_scanf { "format.scanf_type_mismatch" } else { "format.printf_type_mismatch" };
        let spec_hint = scanf_char_spec(is_scanf, &actual, arg.as_deref()).or(suggest_spec(&actual));
        cx.b.push(
            FindingDraft::new(rule, cat, sev, Confidence::Likely)
                .at(span, Precision::Exact)
                .opt_var("arg", arg)
                .var("spec", spec)
                .var("expected", expected)
                .var("actual", &actual)
                .opt_var("suggest_spec", spec_hint)
                .raw(raw),
        );
        return true;
    }
    if let Some(c) = RE_FORMAT_MISSING.captures(m) {
        let call = format_call(cx.src(), line, col).map(|(n, _)| n);
        cx.b.push(
            FindingDraft::new("format.missing_argument", cat, sev, Confidence::Confirmed)
                .at(span, Precision::Exact)
                .var("spec", &c[1])
                .opt_var("call", call)
                .raw(d.to_raw()),
        );
        return true;
    }
    if m == "too many arguments for format" {
        let call = format_call(cx.src(), line, col).map(|(n, _)| n);
        cx.b.push(FindingDraft::new("format.extra_argument", cat, sev, Confidence::Confirmed).at(span, Precision::Exact).opt_var("call", call).raw(d.to_raw()));
        return true;
    }
    false
}

fn warnings(cx: &mut Ctx<'_, '_>, d: &Diag) -> bool {
    let (cat, sev) = kind_of(d);
    let span = span_of(d);
    let m = d.message.as_str();
    let line = d.line().unwrap_or(0);
    let push = |cx: &mut Ctx<'_, '_>, f: FindingDraft| cx.b.push(f.at(span, Precision::Exact).raw(d.to_raw()));

    if let Some(c) = RE_UNUSED_VAR.captures(m) {
        push(cx, FindingDraft::new("warn.unused_variable", cat, sev, Confidence::Confirmed).var("ident", &c[1]));
        return true;
    }
    if let Some(c) = RE_SET_NOT_USED.captures(m) {
        push(cx, FindingDraft::new("warn.unused_variable", cat, sev, Confidence::Confirmed).var("ident", &c[1]).var("set_only", "1"));
        return true;
    }
    if let Some(c) = RE_UNUSED_PARAM.captures(m) {
        push(cx, FindingDraft::new("warn.unused_parameter", cat, sev, Confidence::Confirmed).var("ident", &c[1]));
        return true;
    }
    if let Some(c) = RE_UNINIT.captures(m) {
        let maybe = &c[2] == "may be";
        let conf = if maybe { Confidence::Possible } else { Confidence::Likely };
        push(cx, FindingDraft::new("warn.uninitialized", cat, sev, conf).var("ident", &c[1]).opt_var("maybe", maybe.then_some("1")));
        return true;
    }
    if m == "suggest parentheses around assignment used as truth value" {
        push(cx, FindingDraft::new("warn.assign_in_condition", cat, sev, Confidence::Likely));
        return true;
    }
    if m.starts_with("comparisons like 'X<=Y<=Z'") {
        let raw = cx.claim_where(|o| o.line() == Some(line) && o.option_is("-Wbool-compare"));
        cx.b.push(FindingDraft::new("warn.chained_comparison", cat, sev, Confidence::Likely).at(span, Precision::Exact).raw(d.to_raw()).raw(raw));
        return true;
    }
    if d.option_is("-Wbool-compare") {
        // Usually accompanies a chained comparison reported later on the same line.
        let chained = cx.diags.iter().any(|o| o.line() == Some(line) && o.message.starts_with("comparisons like"));
        if chained {
            cx.claimed.iter_mut().zip(cx.diags).for_each(|(c, o)| {
                if o.line() == Some(line) && o.message.starts_with("comparisons like") {
                    *c = true;
                }
            });
            let mut raw = d.to_raw();
            raw.extend(cx.diags.iter().filter(|o| o.line() == Some(line) && o.message.starts_with("comparisons like")).flat_map(|o| o.to_raw()));
            cx.b.push(FindingDraft::new("warn.chained_comparison", cat, sev, Confidence::Likely).at(span, Precision::Exact).raw(raw));
            return true;
        }
    }
    if let Some(c) = RE_EMPTY_BODY.captures(m) {
        // The misleading-indentation warning on the same line says the same
        // thing; it may already be marked handled, so collect it directly.
        let raw: Vec<RawDiagnostic> =
            cx.diags.iter().filter(|o| o.line() == Some(line) && o.option_is("-Wmisleading-indentation")).flat_map(|o| o.to_raw()).collect();
        let _ = cx.claim_where(|o| o.line() == Some(line) && o.option_is("-Wmisleading-indentation"));
        cx.b.push(FindingDraft::new("warn.empty_body", cat, sev, Confidence::Likely).at(span, Precision::Exact).var("keyword", &c[1]).raw(d.to_raw()).raw(raw));
        return true;
    }
    if let Some(c) = RE_MISLEADING.captures(m) {
        // Our own source lint may already explain `for (...);` on this line.
        if let Some(existing) = cx.b.find_mut("lint.loop_empty_body", line) {
            existing.raw.extend(d.to_raw());
            return true;
        }
        if cx.diags.iter().any(|o| o.line() == Some(line) && RE_EMPTY_BODY.is_match(&o.message)) {
            // Explained by the empty-body card (which collects this message).
            return true;
        }
        let next_line = d.children.first().and_then(|ch| ch.line());
        push(cx, FindingDraft::new("warn.misleading_indentation", cat, sev, Confidence::Likely).var("keyword", &c[1]).opt_var("next_line", next_line));
        return true;
    }
    if d.option_is("-Wsign-compare") {
        push(cx, FindingDraft::new("warn.sign_compare", cat, sev, Confidence::Confirmed));
        return true;
    }
    if m == "statement with no effect" {
        let has_eq = cx.src().tokens_on_line(line).any(|(_, t)| t.is("=="));
        push(cx, FindingDraft::new("warn.no_effect", cat, sev, Confidence::Likely).opt_var("has_equality", has_eq.then_some("1")));
        return true;
    }
    if let Some(c) = RE_LOOP_UB.captures(m) {
        push(cx, FindingDraft::new("warn.loop_overrun", cat, sev, Confidence::Possible).var("iteration", &c[1]));
        return true;
    }
    false
}

fn custom_rule(cx: &mut Ctx<'_, '_>, d: &Diag, custom: &[CustomRule]) -> bool {
    use crate::catalog::CustomMatchKind;
    let (cat, sev) = kind_of(d);
    for rule in custom.iter().filter(|r| r.enabled) {
        let kind_ok = match rule.kind {
            CustomMatchKind::Error => sev == Severity::Error,
            CustomMatchKind::Warning => sev == Severity::Warning,
            CustomMatchKind::Any => true,
        };
        if !kind_ok || rule.option.as_deref().is_some_and(|o| !d.option_is(o)) {
            continue;
        }
        let Ok(re) = rule.regex() else { continue };
        let Some(caps) = re.captures(&d.message) else { continue };
        let mut f = FindingDraft::new(&rule.id, cat, sev, Confidence::Possible)
            .at(span_of(d), Precision::Near)
            .raw(d.to_raw())
            .weight(if sev == Severity::Error { 95 } else { 30 });
        for name in re.capture_names().flatten() {
            if let Some(v) = caps.name(name) {
                f.vars.insert(name.to_string(), v.as_str().to_string());
            }
        }
        cx.b.push(f);
        return true;
    }
    false
}

fn fallback(cx: &mut Ctx<'_, '_>, d: &Diag) {
    let (cat, sev) = kind_of(d);
    let rule = if sev == Severity::Error { "fallback.compile_error" } else { "fallback.warning" };
    let f = FindingDraft::new(rule, cat, sev, Confidence::Possible).at(span_of(d), Precision::Near).var("message", &d.message).raw(d.to_raw());
    let f = if rule == "fallback.warning" { f.opt_var("option", d.option.clone()) } else { f };
    cx.b.push(f);
}

fn linker(cx: &mut Ctx<'_, '_>) {
    for i in 0..cx.link.len() {
        if cx.link_claimed[i] {
            continue;
        }
        cx.link_claimed[i] = true;
        let issue = &cx.link[i];
        let raw = issue.to_raw();
        let defs: Vec<String> = cx.src().function_definitions().into_iter().map(|(n, _)| n).collect();
        match issue {
            LinkIssue::UndefinedReference { symbol, .. } if symbol == "main" => {
                let suggestion = defs.iter().find(|n| similar_name("main", n)).cloned();
                let span = suggestion
                    .as_ref()
                    .and_then(|s| cx.src().tokens.iter().find(|t| t.kind == TokKind::Ident && &t.text == s).map(|t| Span::point(t.line, t.col)));
                cx.b.push(
                    FindingDraft::new("link.undefined_main", Category::LinkError, Severity::Error, Confidence::Confirmed)
                        .at(span, Precision::Approximate)
                        .opt_var("suggestion", suggestion)
                        .raw(vec![raw]),
                );
            }
            LinkIssue::UndefinedReference { symbol, .. } => {
                let suggestion = defs.iter().find(|n| similar_name(symbol, n)).cloned();
                let call_line = cx.call_line(symbol);
                cx.b.push(
                    FindingDraft::new("link.undefined_function", Category::LinkError, Severity::Error, Confidence::Likely)
                        .at(call_line.map(Span::line), Precision::Approximate)
                        .var("func", symbol)
                        .opt_var("suggestion", suggestion)
                        .opt_var("call_line", call_line)
                        .raw(vec![raw]),
                );
            }
            LinkIssue::MultipleDefinition { symbol, .. } => {
                cx.b.push(
                    FindingDraft::new("link.multiple_definition", Category::LinkError, Severity::Error, Confidence::Confirmed)
                        .var("symbol", symbol)
                        .raw(vec![raw]),
                );
            }
            LinkIssue::Other { raw: text } => {
                cx.b.push(
                    FindingDraft::new("fallback.link_error", Category::LinkError, Severity::Error, Confidence::Possible)
                        .at(None, Precision::Approximate)
                        .var("message", text)
                        .raw(vec![RawDiagnostic {
                            origin: RawOrigin::Linker,
                            kind: "error".into(),
                            message: text.clone(),
                            option: None,
                            line: None,
                            column: None,
                        }]),
                );
            }
        }
    }
}
