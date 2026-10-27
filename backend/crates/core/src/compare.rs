//! Output comparison and a whitespace-visible, line-based diff.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompareMode {
    /// Ignore trailing spaces/tabs on each line and trailing blank lines.
    #[default]
    TrimTrailingWhitespace,
    /// Byte-for-byte (after normalising CRLF to LF).
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub matches: bool,
    /// Differs, but would match if all whitespace were ignored.
    pub whitespace_only: bool,
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn trimmed_lines(s: &str) -> Vec<String> {
    let s = normalize_newlines(s);
    let mut lines: Vec<String> = s.split('\n').map(|l| l.trim_end_matches([' ', '\t']).to_string()).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

pub fn compare(expected: &str, actual: &str, mode: CompareMode) -> Outcome {
    let matches = match mode {
        CompareMode::Exact => normalize_newlines(expected) == normalize_newlines(actual),
        CompareMode::TrimTrailingWhitespace => trimmed_lines(expected) == trimmed_lines(actual),
    };
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    Outcome { matches, whitespace_only: !matches && squash(expected) == squash(actual) }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Same,
    /// Present in the expected output only (missing from yours).
    Missing,
    /// Present in your output only (extra).
    Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DiffLine {
    pub kind: DiffKind,
    /// 1-based line number in the expected output (for Same/Missing).
    pub expected_line: Option<u32>,
    /// 1-based line number in the actual output (for Same/Extra).
    pub actual_line: Option<u32>,
    /// Line content with whitespace made visible: space → `·`, tab → `→`,
    /// and a trailing `↵` when the line ended with a newline.
    pub text: String,
}

/// Makes whitespace visible so students can see spaces/newlines differ.
pub fn visible(line: &str, has_newline: bool) -> String {
    let mut s: String = line
        .chars()
        .map(|c| match c {
            ' ' => '·',
            '\t' => '→',
            '\r' => '␍',
            c => c,
        })
        .collect();
    if has_newline {
        s.push('↵');
    }
    s
}

/// Line diff limited to `max_lines` entries (long outputs are cut).
pub fn diff(expected: &str, actual: &str, max_lines: usize) -> (Vec<DiffLine>, bool) {
    let e = normalize_newlines(expected);
    let a = normalize_newlines(actual);
    let d = TextDiff::from_lines(e.as_str(), a.as_str());
    let mut out = vec![];
    let mut truncated = false;
    let (mut el, mut al) = (0u32, 0u32);
    for change in d.iter_all_changes() {
        let raw = change.value();
        let has_nl = raw.ends_with('\n');
        let text = visible(raw.strip_suffix('\n').unwrap_or(raw), has_nl);
        let line = match change.tag() {
            ChangeTag::Equal => {
                el += 1;
                al += 1;
                DiffLine { kind: DiffKind::Same, expected_line: Some(el), actual_line: Some(al), text }
            }
            ChangeTag::Delete => {
                el += 1;
                DiffLine { kind: DiffKind::Missing, expected_line: Some(el), actual_line: None, text }
            }
            ChangeTag::Insert => {
                al += 1;
                DiffLine { kind: DiffKind::Extra, expected_line: None, actual_line: Some(al), text }
            }
        };
        if out.len() >= max_lines {
            truncated = true;
            break;
        }
        out.push(line);
    }
    (out, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_mode_ignores_trailing_whitespace_only() {
        let m = CompareMode::TrimTrailingWhitespace;
        assert!(compare("3\n", "3", m).matches);
        assert!(compare("1 2\n", "1 2  \n\n", m).matches);
        assert!(compare("a\r\nb\r\n", "a\nb\n", m).matches);
        assert!(!compare("1 2\n", "1  2\n", m).matches);
        assert!(compare("1 2\n", "1  2\n", m).whitespace_only);
        assert!(!compare("1 2\n", " 1 2\n", m).matches, "leading spaces matter");
        assert!(!compare("3\n", "4\n", m).whitespace_only);
    }

    #[test]
    fn exact_mode_counts_every_byte() {
        assert!(!compare("3\n", "3", CompareMode::Exact).matches);
        assert!(compare("3\n", "3", CompareMode::Exact).whitespace_only);
        assert!(compare("a\r\n", "a\n", CompareMode::Exact).matches);
    }

    #[test]
    fn diff_marks_missing_and_extra_with_visible_whitespace() {
        let (d, truncated) = diff("Sum: 3\nDone\n", "Sum:  3\n", 100);
        assert!(!truncated);
        assert_eq!(d[0], DiffLine { kind: DiffKind::Missing, expected_line: Some(1), actual_line: None, text: "Sum:·3↵".into() });
        assert!(d.iter().any(|l| l.kind == DiffKind::Extra && l.text == "Sum:··3↵"));
        assert!(d.iter().any(|l| l.kind == DiffKind::Missing && l.text == "Done↵"));
    }

    #[test]
    fn diff_is_truncated() {
        let e: String = (0..50).map(|i| format!("{i}\n")).collect();
        let (d, truncated) = diff(&e, "", 10);
        assert_eq!(d.len(), 10);
        assert!(truncated);
    }
}
