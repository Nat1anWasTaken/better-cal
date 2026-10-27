//! Source lints for beginner mistakes GCC does not (always) report.

use super::{Builder, FindingDraft};
use crate::{Category, Confidence, Precision, RawDiagnostic, RawOrigin, Severity, Span};

fn lint_raw(message: &str, line: u32, col: u32) -> RawDiagnostic {
    RawDiagnostic { origin: RawOrigin::Lint, kind: "lint".into(), message: message.into(), option: None, line: Some(line), column: Some(col) }
}

pub(crate) fn run(b: &mut Builder<'_>) {
    let src = b.src;
    let toks = &src.tokens;
    let mut found: Vec<FindingDraft> = vec![];

    for (i, t) in toks.iter().enumerate() {
        // `for (...);` / `while (...);` — but not the tail of `do { } while (...);`
        if (t.is("for") || t.is("while")) && toks.get(i + 1).is_some_and(|n| n.is("(")) {
            let do_while_tail = t.is("while") && i > 0 && toks[i - 1].is("}");
            if let Some(close) = src.matching_close(i + 1)
                && !do_while_tail
                && toks.get(close + 1).is_some_and(|n| n.is(";"))
            {
                let semi = &toks[close + 1];
                found.push(
                    FindingDraft::new("lint.loop_empty_body", Category::Warning, Severity::Warning, Confidence::Likely)
                        .at(Some(Span { line: t.line, column: t.col, end_line: semi.line, end_column: semi.col + 1 }), Precision::Exact)
                        .var("keyword", &t.text)
                        .raw(vec![lint_raw(&format!("`{}` header is directly followed by `;` (empty loop body)", t.text), semi.line, semi.col)]),
                );
            }
        }
        // `while (scanf(...))` without comparing the return value: at end of
        // input scanf returns EOF (-1), which is "true", so the loop never ends.
        if t.is("while")
            && toks.get(i + 1).is_some_and(|n| n.is("("))
            && toks.get(i + 2).is_some_and(|n| n.is("scanf"))
            && let (Some(while_close), Some(scanf_close)) = (src.matching_close(i + 1), src.matching_close(i + 3))
            && scanf_close + 1 == while_close
        {
            found.push(
                FindingDraft::new("lint.while_scanf_no_compare", Category::Warning, Severity::Warning, Confidence::Possible)
                    .at(Some(Span { line: t.line, column: t.col, end_line: toks[while_close].line, end_column: toks[while_close].col + 1 }), Precision::Exact)
                    .raw(vec![lint_raw("loop condition uses the return value of scanf without comparing it", t.line, t.col)]),
            );
        }
    }

    // scanf("%d\n") / scanf("%d ") waits for more non-blank input when typed interactively.
    for call in src.calls("scanf") {
        let Some(first) = call.args.first() else { continue };
        if first.len() != 1 {
            continue;
        }
        let fmt = &toks[first.start];
        let inner = fmt.text.trim_matches('"');
        if fmt.text.starts_with('"') && inner.contains('%') && (inner.ends_with("\\n") || inner.ends_with(' ') || inner.ends_with("\\t")) {
            found.push(
                FindingDraft::new("lint.scanf_trailing_whitespace", Category::Warning, Severity::Info, Confidence::Possible)
                    .at(
                        Some(Span { line: fmt.line, column: fmt.col, end_line: fmt.line, end_column: fmt.col + fmt.text.chars().count() as u32 }),
                        Precision::Exact,
                    )
                    .var("format", &fmt.text)
                    .raw(vec![lint_raw("scanf format ends with whitespace", fmt.line, fmt.col)]),
            );
        }
    }

    for f in found {
        b.push(f);
    }
}
