//! Parser for GNU ld / collect2 text output (the linker has no JSON mode).

use std::sync::LazyLock;

use regex::Regex;

use crate::{RawDiagnostic, RawOrigin};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkIssue {
    UndefinedReference {
        symbol: String,
        raw: String,
    },
    MultipleDefinition {
        symbol: String,
        raw: String,
    },
    /// Any other linker or driver error line we do not specifically understand.
    Other {
        raw: String,
    },
}

impl LinkIssue {
    pub fn raw_line(&self) -> &str {
        match self {
            LinkIssue::UndefinedReference { raw, .. } | LinkIssue::MultipleDefinition { raw, .. } | LinkIssue::Other { raw } => raw,
        }
    }
    pub fn to_raw(&self) -> RawDiagnostic {
        RawDiagnostic { origin: RawOrigin::Linker, kind: "error".into(), message: self.raw_line().to_string(), option: None, line: None, column: None }
    }
}

static UNDEFINED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"undefined reference to [`'‘]([A-Za-z_][A-Za-z0-9_]*)['’]").unwrap());
static MULTIPLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"multiple definition of [`'‘]([A-Za-z_][A-Za-z0-9_]*)['’]").unwrap());

/// Lines that only add context ("in function `main':", the collect2 summary).
fn is_context(line: &str) -> bool {
    line.contains(": in function `")
        || line.contains(": in function '")
        || line.starts_with("collect2: error: ld returned")
        || line.contains("first defined here")
        || line.trim().is_empty()
}

pub fn parse(text: &str) -> Vec<LinkIssue> {
    let mut out: Vec<LinkIssue> = Vec::new();
    for line in text.lines() {
        if let Some(c) = UNDEFINED.captures(line) {
            let symbol = c[1].to_string();
            // ld repeats the error once per call site; one card per symbol is enough.
            if !out.iter().any(|i| matches!(i, LinkIssue::UndefinedReference { symbol: s, .. } if *s == symbol)) {
                out.push(LinkIssue::UndefinedReference { symbol, raw: line.to_string() });
            }
        } else if let Some(c) = MULTIPLE.captures(line) {
            out.push(LinkIssue::MultipleDefinition { symbol: c[1].to_string(), raw: line.to_string() });
        } else if !is_context(line) {
            out.push(LinkIssue::Other { raw: line.to_string() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_undefined_reference_and_dedups() {
        let text = "/usr/bin/ld: /tmp/ccXXXXXX.o: in function `main':\nmain.c:(.text.startup+0xe): undefined reference to `Printf'\nmain.c:(.text.startup+0x1e): undefined reference to `Printf'\ncollect2: error: ld returned 1 exit status\n";
        let issues = parse(text);
        assert_eq!(issues.len(), 1);
        assert!(matches!(&issues[0], LinkIssue::UndefinedReference { symbol, .. } if symbol == "Printf"));
    }

    #[test]
    fn parses_multiple_definition() {
        // Captured from GNU ld 2.40 (runner image) linking two files that both define `count`.
        let text = "/usr/bin/ld: /tmp/ccPO4gUM.o:(.data+0x0): multiple definition of `count'; /tmp/ccDU97NU.o:(.data+0x0): first defined here\ncollect2: error: ld returned 1 exit status\n";
        let issues = parse(text);
        assert_eq!(issues, vec![LinkIssue::MultipleDefinition { symbol: "count".into(), raw: text.lines().next().unwrap().into() }]);
    }

    #[test]
    fn unknown_lines_are_kept() {
        let issues = parse("gcc-12: fatal error: cannot execute 'cc1': execvp: No such file or directory\n");
        assert!(matches!(&issues[0], LinkIssue::Other { .. }));
    }

    #[test]
    fn empty_text_has_no_issues() {
        assert!(parse("").is_empty());
    }
}
