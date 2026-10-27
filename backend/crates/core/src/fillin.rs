//! Fill-in-the-blank programs.
//!
//! A template marks editable regions with `/*@blank id@*/ default /*@end@*/`.
//! Everything else is fixed. Students submit only `{id: text}`; the server
//! assembles the program from the *stored* template, so fixed code cannot be
//! altered by calling the API directly.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

static OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/\*@blank ([a-z][a-z0-9_]{0,31})@\*/").unwrap());
const CLOSE: &str = "/*@end@*/";
const MARKER_PREFIX: &str = "/*@";

pub const MAX_BLANK_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    Fixed { text: String },
    Blank { id: String, default: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Template {
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
pub enum TemplateError {
    #[error("找不到結束標記 /*@end@*/（區塊 `{0}`）")]
    Unclosed(String),
    #[error("多出一個 /*@end@*/")]
    StrayEnd,
    #[error("區塊 id `{0}` 重複")]
    DuplicateId(String),
    #[error("填空區塊不可巢狀")]
    Nested,
    #[error("無法辨識的標記（格式應為 /*@blank 名稱@*/，名稱為小寫英數字與底線）")]
    BadMarker,
    #[error("範本至少需要一個填空區塊")]
    NoBlanks,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
pub enum AssembleError {
    #[error("缺少填空區塊 `{0}`")]
    Missing(String),
    #[error("沒有名為 `{0}` 的填空區塊")]
    Unknown(String),
    #[error("填空區塊 `{0}` 內不可包含 `/*@` 標記")]
    MarkerInBlank(String),
    #[error("填空區塊 `{0}` 太長")]
    TooLong(String),
}

impl Template {
    pub fn parse(src: &str) -> Result<Template, TemplateError> {
        // Every `/*@` must start a well-formed open or close marker.
        for (i, _) in src.match_indices(MARKER_PREFIX) {
            let at = &src[i..];
            if !at.starts_with(CLOSE) && OPEN.find(at).is_none_or(|m| m.start() != 0) {
                return Err(TemplateError::BadMarker);
            }
        }
        let mut segments = vec![];
        let mut rest = src;
        let mut ids: Vec<String> = vec![];
        loop {
            let open = OPEN.find(rest);
            let stray_end = rest.find(CLOSE);
            match (open, stray_end) {
                (None, Some(_)) => return Err(TemplateError::StrayEnd),
                (Some(o), Some(e)) if e < o.start() => return Err(TemplateError::StrayEnd),
                (None, None) => {
                    if rest.contains(MARKER_PREFIX) {
                        return Err(TemplateError::BadMarker);
                    }
                    if !rest.is_empty() {
                        segments.push(Segment::Fixed { text: rest.to_string() });
                    }
                    break;
                }
                (Some(o), _) => {
                    let before = &rest[..o.start()];
                    if before.contains(MARKER_PREFIX) {
                        return Err(TemplateError::BadMarker);
                    }
                    if !before.is_empty() {
                        segments.push(Segment::Fixed { text: before.to_string() });
                    }
                    let id = OPEN.captures(&rest[o.start()..]).unwrap()[1].to_string();
                    if ids.contains(&id) {
                        return Err(TemplateError::DuplicateId(id));
                    }
                    let after = &rest[o.end()..];
                    let Some(end) = after.find(CLOSE) else { return Err(TemplateError::Unclosed(id)) };
                    let body = &after[..end];
                    if body.contains(MARKER_PREFIX) {
                        return Err(TemplateError::Nested);
                    }
                    segments.push(Segment::Blank { id: id.clone(), default: body.to_string() });
                    ids.push(id);
                    rest = &after[end + CLOSE.len()..];
                }
            }
        }
        if ids.is_empty() {
            return Err(TemplateError::NoBlanks);
        }
        Ok(Template { segments })
    }

    pub fn blank_ids(&self) -> Vec<&str> {
        self.segments
            .iter()
            .filter_map(|s| match s {
                Segment::Blank { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn defaults(&self) -> BTreeMap<String, String> {
        self.segments
            .iter()
            .filter_map(|s| match s {
                Segment::Blank { id, default } => Some((id.clone(), default.clone())),
                _ => None,
            })
            .collect()
    }

    /// Builds the program from the stored template plus the student's blanks.
    /// Returns the source and the 1-based line ranges of fixed code.
    pub fn assemble(&self, blanks: &BTreeMap<String, String>) -> Result<Assembled, AssembleError> {
        let ids = self.blank_ids();
        for k in blanks.keys() {
            if !ids.contains(&k.as_str()) {
                return Err(AssembleError::Unknown(k.clone()));
            }
        }
        let mut source = String::new();
        // Lines a blank touches (even an empty one) are editable.
        let mut editable = std::collections::BTreeSet::new();
        let mut line = 1u32;
        for seg in &self.segments {
            match seg {
                Segment::Fixed { text } => {
                    source.push_str(text);
                    line += text.matches('\n').count() as u32;
                }
                Segment::Blank { id, .. } => {
                    let text = blanks.get(id).ok_or_else(|| AssembleError::Missing(id.clone()))?;
                    if text.contains(MARKER_PREFIX) {
                        return Err(AssembleError::MarkerInBlank(id.clone()));
                    }
                    if text.len() > MAX_BLANK_LEN {
                        return Err(AssembleError::TooLong(id.clone()));
                    }
                    source.push_str(text);
                    let nl = text.matches('\n').count() as u32;
                    // A blank ending in "\n" does not reach into the next line.
                    let last = if nl > 0 && text.ends_with('\n') { line + nl - 1 } else { line + nl };
                    editable.extend(line..=last);
                    line += nl;
                }
            }
        }
        let total = source.lines().count() as u32;
        let mut fixed_regions = vec![];
        let mut run_start: Option<u32> = None;
        for l in 1..=total + 1 {
            let fixed = l <= total && !editable.contains(&l);
            match (fixed, run_start) {
                (true, None) => run_start = Some(l),
                (false, Some(s)) => {
                    fixed_regions.push(diagnostics::FixedRegion { start_line: s, end_line: l - 1 });
                    run_start = None;
                }
                _ => {}
            }
        }
        Ok(Assembled { source, fixed_regions })
    }
}

#[derive(Debug, Clone)]
pub struct Assembled {
    pub source: String,
    pub fixed_regions: Vec<diagnostics::FixedRegion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "#include <stdio.h>\n\nint max(int a, int b) {\n/*@blank body@*/    return 0;\n/*@end@*/}\n\nint main(void) {\n    printf(\"%d\\n\", max(3, 5));\n    return 0;\n}\n";

    #[test]
    fn parses_and_assembles() {
        let t = Template::parse(T).unwrap();
        assert_eq!(t.blank_ids(), vec!["body"]);
        let mut b = BTreeMap::new();
        b.insert("body".to_string(), "    return a > b ? a : b;\n".to_string());
        let a = t.assemble(&b).unwrap();
        assert!(a.source.contains("return a > b ? a : b;"));
        assert!(!a.source.contains("/*@"));
        // Lines 1-3 fixed, line 4 is the blank, lines 5-10 fixed.
        assert_eq!(a.fixed_regions.first().map(|r| (r.start_line, r.end_line)), Some((1, 3)));
        assert_eq!(a.fixed_regions.last().map(|r| (r.start_line, r.end_line)), Some((5, 10)));
    }

    #[test]
    fn rejects_unknown_missing_and_marker_injection() {
        let t = Template::parse(T).unwrap();
        let mut b = BTreeMap::new();
        assert!(matches!(t.assemble(&b), Err(AssembleError::Missing(id)) if id == "body"));
        b.insert("body".to_string(), "/*@end@*/ } int main(void){ return 0; } /*@blank x@*/".to_string());
        assert!(matches!(t.assemble(&b), Err(AssembleError::MarkerInBlank(_))));
        b.clear();
        b.insert("body".into(), "x".into());
        b.insert("main".into(), "evil".into());
        assert!(matches!(t.assemble(&b), Err(AssembleError::Unknown(_))));
    }

    #[test]
    fn template_errors() {
        assert_eq!(Template::parse("int x;"), Err(TemplateError::NoBlanks));
        assert_eq!(Template::parse("/*@blank a@*/ x"), Err(TemplateError::Unclosed("a".into())));
        assert_eq!(Template::parse("x /*@end@*/"), Err(TemplateError::StrayEnd));
        assert_eq!(Template::parse("/*@blank a@*/1/*@end@*//*@blank a@*/2/*@end@*/"), Err(TemplateError::DuplicateId("a".into())));
        assert_eq!(Template::parse("/*@blank A@*/1/*@end@*/"), Err(TemplateError::BadMarker));
    }
}
