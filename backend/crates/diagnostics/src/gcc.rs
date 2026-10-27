//! Parser for GCC's `-fdiagnostics-format=json` output (GCC 12 schema).

use serde::Deserialize;

use crate::{RawDiagnostic, RawOrigin};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Pos {
    #[serde(default)]
    pub file: String,
    pub line: u32,
    #[serde(default, deserialize_with = "column")]
    pub column: u32,
    /// 1-based column in "display" units (what an editor shows).
    #[serde(rename = "display-column", default, deserialize_with = "opt_column")]
    pub display_column: Option<u32>,
}

/// GCC reports column -1 when it has no column (e.g. "at end of input").
fn opt_column<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    Ok(Option::<i64>::deserialize(d)?.and_then(|c| u32::try_from(c).ok()))
}

fn column<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    opt_column(d).map(Option::unwrap_or_default)
}

impl Pos {
    pub fn col(&self) -> u32 {
        self.display_column.unwrap_or(self.column)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Location {
    pub caret: Pos,
    pub start: Option<Pos>,
    pub finish: Option<Pos>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Fixit {
    pub start: Pos,
    pub next: Pos,
    pub string: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Diag {
    /// "error", "warning", "note", "fatal error", "pedantic warning", ...
    pub kind: String,
    pub message: String,
    pub option: Option<String>,
    #[serde(default)]
    pub locations: Vec<Location>,
    #[serde(default)]
    pub children: Vec<Diag>,
    #[serde(default)]
    pub fixits: Vec<Fixit>,
}

impl Diag {
    pub fn is_error(&self) -> bool {
        self.kind.contains("error")
    }
    pub fn is_warning(&self) -> bool {
        self.kind.contains("warning")
    }
    pub fn is_note(&self) -> bool {
        self.kind == "note"
    }
    /// The caret of the first location, if it is in the student's file.
    pub fn caret(&self) -> Option<&Pos> {
        self.locations.first().map(|l| &l.caret)
    }
    pub fn line(&self) -> Option<u32> {
        self.caret().map(|p| p.line)
    }
    pub fn option_is(&self, opt: &str) -> bool {
        self.option.as_deref() == Some(opt)
    }
    /// Labels attached to secondary locations (e.g. argument types).
    pub fn labels(&self) -> Vec<&str> {
        self.locations.iter().filter_map(|l| l.label.as_deref()).collect()
    }
    /// Effective severity: a warning whose child is an error is an error
    /// (GCC reports e.g. "missing terminating \" character" that way).
    pub fn effective_error(&self) -> bool {
        self.is_error() || self.children.iter().any(|c| c.is_error())
    }

    pub fn to_raw(&self) -> Vec<RawDiagnostic> {
        let mut out = vec![RawDiagnostic {
            origin: RawOrigin::Compiler,
            kind: self.kind.clone(),
            message: self.message.clone(),
            option: self.option.clone(),
            line: self.caret().map(|p| p.line),
            column: self.caret().map(|p| p.col()),
        }];
        for c in &self.children {
            out.extend(c.to_raw());
        }
        out
    }
}

/// Parses the JSON array. Unknown fields are ignored and an entry that does
/// not fit the schema is skipped on its own; a malformed document yields no
/// diagnostics (the caller still sees `compile.success == false` and falls
/// back to an honest generic card).
pub fn parse(value: &serde_json::Value) -> Vec<Diag> {
    let diags: Vec<Diag> = value.as_array().map(|a| a.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()).unwrap_or_default();
    // Top-level notes that just repeat a sibling's child note are noise.
    let mut out: Vec<Diag> = Vec::with_capacity(diags.len());
    for d in diags {
        if d.is_note()
            && let Some(prev) = out.last_mut()
        {
            let dup = prev.children.iter().any(|c| c.message == d.message);
            if !dup {
                prev.children.push(d);
            }
            continue;
        }
        out.push(d);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gcc12_document() {
        let json: serde_json::Value = serde_json::from_str(
            r#"[{"kind": "error", "column-origin": 1, "children": [{"kind": "note", "locations": [{"caret": {"file": "main.c", "line": 3, "display-column": 5, "byte-column": 5, "column": 5}}], "escape-source": false, "message": "declared here"}], "locations": [{"caret": {"file": "main.c", "line": 8, "display-column": 20, "byte-column": 20, "column": 20}, "finish": {"file": "main.c", "line": 8, "display-column": 22, "byte-column": 22, "column": 22}}], "escape-source": false, "message": "too few arguments to function 'add'"}]"#,
        )
        .unwrap();
        let d = parse(&json);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line(), Some(8));
        assert_eq!(d[0].children[0].message, "declared here");
        assert!(d[0].effective_error());
    }

    #[test]
    fn negative_column_does_not_drop_the_document() {
        let json = serde_json::json!([
            {"kind": "error", "message": "a", "locations": [{"caret": {"file": "main.c", "line": 8, "column": 12, "display-column": 12}}]},
            {"kind": "error", "message": "expected '{' at end of input", "locations": [{"caret": {"file": "main.c", "line": 16, "column": -1, "display-column": -1}}]},
            {"kind": "error", "message": "not a diagnostic", "locations": "oops"},
        ]);
        let d = parse(&json);
        assert_eq!(d.len(), 2);
        assert_eq!(d[1].line(), Some(16));
        assert_eq!(d[1].caret().unwrap().col(), 0);
    }

    #[test]
    fn malformed_input_is_empty() {
        assert!(parse(&serde_json::json!({"not": "an array"})).is_empty());
    }

    #[test]
    fn folds_trailing_top_level_note() {
        let json = serde_json::json!([
            {"kind": "warning", "message": "'return' with a value, in function returning void", "option": "-Wreturn-type", "locations": [{"caret": {"line": 5, "column": 12}}]},
            {"kind": "note", "message": "declared here", "locations": [{"caret": {"line": 3, "column": 6}}]}
        ]);
        let d = parse(&json);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].children.len(), 1);
    }
}
