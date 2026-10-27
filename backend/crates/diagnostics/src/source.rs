//! A tiny, forgiving C lexer used to gather evidence from the student's code.
//!
//! It never needs to be a full parser: rules only ask narrow questions such as
//! "what is the argument text of this scanf call" or "which `{` is unclosed".

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
    Ident,
    Number,
    Str,
    Char,
    Punct,
    /// A non-ASCII character outside strings/comments (e.g. full-width `；`).
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokKind,
    pub text: String,
    pub line: u32,
    /// 1-based column counted in characters.
    pub col: u32,
}

impl Token {
    pub fn is(&self, s: &str) -> bool {
        self.text == s
    }
}

pub struct Source<'a> {
    pub text: &'a str,
    lines: Vec<&'a str>,
    pub tokens: Vec<Token>,
}

const TYPE_WORDS: &[&str] = &["int", "long", "short", "char", "float", "double", "unsigned", "signed", "void", "size_t", "_Bool", "bool"];

impl<'a> Source<'a> {
    pub fn new(text: &'a str) -> Source<'a> {
        Source { text, lines: text.lines().collect(), tokens: lex(text) }
    }

    pub fn line_count(&self) -> u32 {
        self.lines.len() as u32
    }

    pub fn line_text(&self, line: u32) -> &str {
        self.lines.get(line.saturating_sub(1) as usize).copied().unwrap_or("")
    }

    /// The nearest earlier line that contains code (skips blank lines).
    pub fn prev_code_line(&self, line: u32) -> Option<u32> {
        let mut l = line.checked_sub(1)?;
        while l >= 1 {
            let t = self.line_text(l).trim();
            if !t.is_empty() && !t.starts_with("//") {
                return Some(l);
            }
            l -= 1;
        }
        None
    }

    /// True when `col` points at the first non-blank character of `line`.
    pub fn is_first_token_on_line(&self, line: u32, col: u32) -> bool {
        self.tokens.iter().find(|t| t.line == line).is_some_and(|t| t.col == col)
    }

    pub fn tokens_on_line(&self, line: u32) -> impl Iterator<Item = (usize, &Token)> {
        self.tokens.iter().enumerate().filter(move |(_, t)| t.line == line)
    }

    /// Index of the token matching the bracket at `open` (`(`/`[`/`{`).
    pub fn matching_close(&self, open: usize) -> Option<usize> {
        let (o, c) = match self.tokens.get(open)?.text.as_str() {
            "(" => ("(", ")"),
            "[" => ("[", "]"),
            "{" => ("{", "}"),
            _ => return None,
        };
        let mut depth = 0i32;
        for (i, t) in self.tokens.iter().enumerate().skip(open) {
            if t.is(o) {
                depth += 1;
            } else if t.is(c) {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Calls to `name(...)`: (index of name token, argument token ranges).
    pub fn calls(&self, name: &str) -> Vec<Call> {
        let mut out = Vec::new();
        for (i, t) in self.tokens.iter().enumerate() {
            // `int add(` is a declaration or definition, not a call.
            let declared_here = i > 0 && (TYPE_WORDS.contains(&self.tokens[i - 1].text.as_str()) || self.tokens[i - 1].is("*"));
            if t.kind == TokKind::Ident && t.text == name && !declared_here && self.tokens.get(i + 1).is_some_and(|n| n.is("(")) {
                let Some(close) = self.matching_close(i + 1) else { continue };
                let mut args = Vec::new();
                let mut depth = 0;
                let mut start = i + 2;
                for j in i + 2..close {
                    let tj = &self.tokens[j];
                    if tj.is("(") || tj.is("[") || tj.is("{") {
                        depth += 1;
                    } else if tj.is(")") || tj.is("]") || tj.is("}") {
                        depth -= 1;
                    } else if tj.is(",") && depth == 0 {
                        args.push(start..j);
                        start = j + 1;
                    }
                }
                if start < close {
                    args.push(start..close);
                }
                out.push(Call { name_index: i, close_index: close, line: t.line, args });
            }
        }
        out
    }

    pub fn text_of(&self, range: std::ops::Range<usize>) -> String {
        let mut s = String::new();
        let mut prev: Option<&Token> = None;
        for t in &self.tokens[range] {
            if let Some(p) = prev {
                let word = |k: TokKind| matches!(k, TokKind::Ident | TokKind::Number);
                if word(p.kind) && word(t.kind) {
                    s.push(' ');
                }
            }
            s.push_str(&t.text);
            prev = Some(t);
        }
        s
    }

    /// Lines where `name` is declared as a variable or parameter (heuristic:
    /// a type word, optional `*`s, then the name, then `=`, `;`, `,`, `[` or `)`).
    pub fn declaration_lines(&self, name: &str) -> Vec<u32> {
        let mut out = Vec::new();
        for (i, t) in self.tokens.iter().enumerate() {
            if t.kind != TokKind::Ident || t.text != name {
                continue;
            }
            let follows_ok = self.tokens.get(i + 1).is_some_and(|n| ["=", ";", ",", "[", ")"].contains(&n.text.as_str()));
            if !follows_ok {
                continue;
            }
            // Walk back over `*` and, for comma-separated declarators, over
            // earlier declarators until a type word appears.
            let mut j = i;
            let mut found = false;
            while j > 0 {
                j -= 1;
                let p = &self.tokens[j];
                if TYPE_WORDS.contains(&p.text.as_str()) {
                    found = true;
                    break;
                }
                let declarator_part = p.is("*") || p.is(",") || p.is("=") || p.is("[") || p.is("]") || matches!(p.kind, TokKind::Ident | TokKind::Number);
                if !declarator_part {
                    break;
                }
            }
            if found && !out.contains(&t.line) {
                out.push(t.line);
            }
        }
        out
    }

    /// Function definitions at file scope: (name, line).
    pub fn function_definitions(&self) -> Vec<(String, u32)> {
        let mut out = Vec::new();
        let mut depth = 0i32;
        for (i, t) in self.tokens.iter().enumerate() {
            if t.is("{") {
                depth += 1;
            } else if t.is("}") {
                depth -= 1;
            } else if depth == 0
                && t.kind == TokKind::Ident
                && self.tokens.get(i + 1).is_some_and(|n| n.is("("))
                && let Some(close) = self.matching_close(i + 1)
                && self.tokens.get(close + 1).is_some_and(|n| n.is("{"))
            {
                out.push((t.text.clone(), t.line));
            }
        }
        out
    }

    /// The function (name, start line) whose body contains `line`.
    pub fn enclosing_function(&self, line: u32) -> Option<(String, u32)> {
        let mut best = None;
        for (name, start) in self.function_definitions() {
            if start <= line {
                best = Some((name, start));
            }
        }
        best
    }

    /// Brace balance check ignoring strings/comments.
    pub fn brace_report(&self) -> BraceReport {
        let mut stack: Vec<&Token> = Vec::new();
        let mut first_extra_close = None;
        let mut misindented_opens = Vec::new();
        for t in &self.tokens {
            if t.is("{") {
                stack.push(t);
            } else if t.is("}") {
                match stack.pop() {
                    // A `}` indented differently from the line that opened the
                    // block hints that the pairing the compiler sees is not
                    // the one the student intended.
                    Some(open) if self.indent(t.line) < self.indent(open.line) => misindented_opens.push(open.line),
                    Some(_) => {}
                    None if first_extra_close.is_none() => first_extra_close = Some(t.line),
                    None => {}
                }
            }
        }
        let unclosed = if stack.is_empty() {
            vec![]
        } else if !misindented_opens.is_empty() {
            misindented_opens
        } else {
            stack.iter().map(|t| t.line).collect()
        };
        BraceReport { unclosed_open_lines: unclosed, first_extra_close_line: first_extra_close, function_closed_early_line: self.function_closed_early() }
    }

    fn indent(&self, line: u32) -> usize {
        let t = self.line_text(line);
        t.len() - t.trim_start().len()
    }

    /// A `}` at depth 0 followed by statement-like code (e.g. `return`) at
    /// file scope suggests a function was closed too early by an extra `}`.
    fn function_closed_early(&self) -> Option<u32> {
        let mut depth = 0i32;
        let mut last_close_to_zero: Option<u32> = None;
        for (i, t) in self.tokens.iter().enumerate() {
            if t.is("{") {
                depth += 1;
            } else if t.is("}") {
                depth -= 1;
                if depth == 0 {
                    last_close_to_zero = Some(t.line);
                }
            } else if depth == 0 {
                let stmt = ["return", "printf", "scanf", "if", "for", "while"].contains(&t.text.as_str());
                let prev_is_close = i > 0 && self.tokens[i - 1].is("}");
                if stmt && prev_is_close {
                    return last_close_to_zero;
                }
            }
        }
        None
    }

    /// How many times `name` appears as an identifier token.
    pub fn ident_count(&self, name: &str) -> usize {
        self.tokens.iter().filter(|t| t.kind == TokKind::Ident && t.text == name).count()
    }

    /// Non-ASCII characters outside strings and comments.
    pub fn foreign_chars(&self) -> Vec<&Token> {
        self.tokens.iter().filter(|t| t.kind == TokKind::Foreign).collect()
    }
}

#[derive(Debug, Clone)]
pub struct Call {
    pub name_index: usize,
    pub close_index: usize,
    pub line: u32,
    pub args: Vec<std::ops::Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BraceReport {
    pub unclosed_open_lines: Vec<u32>,
    pub first_extra_close_line: Option<u32>,
    pub function_closed_early_line: Option<u32>,
}

const PUNCT3: &[&str] = &["<<=", ">>=", "..."];
const PUNCT2: &[&str] = &["==", "!=", "<=", ">=", "&&", "||", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "->", "<<", ">>", "##"];

pub fn lex(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);
    let advance = |i: &mut usize, line: &mut u32, col: &mut u32, chars: &[char]| {
        if chars[*i] == '\n' {
            *line += 1;
            *col = 1;
        } else {
            *col += 1;
        }
        *i += 1;
    };
    while i < chars.len() {
        let c = chars[i];
        let (sl, sc) = (line, col);
        if c == '\n' || c.is_ascii_whitespace() {
            advance(&mut i, &mut line, &mut col, &chars);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                advance(&mut i, &mut line, &mut col, &chars);
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            advance(&mut i, &mut line, &mut col, &chars);
            advance(&mut i, &mut line, &mut col, &chars);
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                advance(&mut i, &mut line, &mut col, &chars);
            }
            for _ in 0..2 {
                if i < chars.len() {
                    advance(&mut i, &mut line, &mut col, &chars);
                }
            }
        } else if c == '#' && out.last().is_none_or(|t: &Token| t.line != line) {
            // Preprocessor line: keep as one punct token, skip the rest.
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                advance(&mut i, &mut line, &mut col, &chars);
            }
            out.push(Token { kind: TokKind::Punct, text: chars[start..i].iter().collect::<String>().trim().to_string(), line: sl, col: sc });
        } else if c == '"' || c == '\'' {
            let start = i;
            advance(&mut i, &mut line, &mut col, &chars);
            while i < chars.len() && chars[i] != c && chars[i] != '\n' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    advance(&mut i, &mut line, &mut col, &chars);
                }
                advance(&mut i, &mut line, &mut col, &chars);
            }
            if i < chars.len() && chars[i] == c {
                advance(&mut i, &mut line, &mut col, &chars);
            }
            let kind = if c == '"' { TokKind::Str } else { TokKind::Char };
            out.push(Token { kind, text: chars[start..i].iter().collect(), line: sl, col: sc });
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                advance(&mut i, &mut line, &mut col, &chars);
            }
            out.push(Token { kind: TokKind::Ident, text: chars[start..i].iter().collect(), line: sl, col: sc });
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                advance(&mut i, &mut line, &mut col, &chars);
            }
            out.push(Token { kind: TokKind::Number, text: chars[start..i].iter().collect(), line: sl, col: sc });
        } else if !c.is_ascii() {
            advance(&mut i, &mut line, &mut col, &chars);
            if !c.is_whitespace() {
                out.push(Token { kind: TokKind::Foreign, text: c.to_string(), line: sl, col: sc });
            }
        } else {
            let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
            let len = if PUNCT3.iter().any(|p| rest.starts_with(p)) {
                3
            } else if PUNCT2.iter().any(|p| rest.starts_with(p)) {
                2
            } else {
                1
            };
            let start = i;
            for _ in 0..len {
                advance(&mut i, &mut line, &mut col, &chars);
            }
            out.push(Token { kind: TokKind::Punct, text: chars[start..i].iter().collect(), line: sl, col: sc });
        }
    }
    out
}

/// ASCII look-alike for common full-width / typographic characters that
/// Chinese input methods produce.
pub fn ascii_lookalike(c: &str) -> Option<&'static str> {
    Some(match c {
        "；" => ";",
        "，" | "、" => ",",
        "（" => "(",
        "）" => ")",
        "｛" => "{",
        "｝" => "}",
        "［" | "【" => "[",
        "］" | "】" => "]",
        "＝" => "=",
        "＋" => "+",
        "－" => "-",
        "＊" => "*",
        "／" => "/",
        "＜" | "《" => "<",
        "＞" | "》" => ">",
        "：" => ":",
        "！" => "!",
        "＆" => "&",
        "％" => "%",
        "＃" => "#",
        "“" | "”" | "＂" => "\"",
        "‘" | "’" | "＇" => "'",
        "\u{3000}" => " ",
        _ => return None,
    })
}

/// Classic Levenshtein distance, used for "did you mean" evidence.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Case-insensitive near match (distance ≤ 2, or equal ignoring case).
pub fn similar_name(a: &str, b: &str) -> bool {
    a != b && (a.eq_ignore_ascii_case(b) || (a.len() >= 3 && edit_distance(a, b) <= 2) || is_transposition(a, b))
}

fn is_transposition(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len() != b.len() {
        return false;
    }
    let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    diff.len() == 2 && diff[1] == diff[0] + 1 && a[diff[0]] == b[diff[1]] && a[diff[1]] == b[diff[0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_strings_comments_and_foreign_chars() {
        let s = Source::new("int a = 5；// 註解；\nprintf(\"；%d\", a);\n/* （ */");
        let foreign: Vec<_> = s.foreign_chars().iter().map(|t| (t.text.clone(), t.line, t.col)).collect();
        assert_eq!(foreign, vec![("；".to_string(), 1, 10)]);
    }

    #[test]
    fn finds_call_arguments() {
        let s = Source::new("int main(void){ int a,b; scanf(\"%d %d\", &a, f(b, 2)); }");
        let calls = s.calls("scanf");
        assert_eq!(calls.len(), 1);
        let args: Vec<String> = calls[0].args.iter().map(|r| s.text_of(r.clone())).collect();
        assert_eq!(args, vec!["\"%d %d\"", "&a", "f(b,2)"]);
    }

    #[test]
    fn finds_declarations_including_comma_lists() {
        let s = Source::new("int main(void) {\n    int a, b;\n    for (int i = 0; i < 3; i++) {\n        int last = i;\n    }\n    b = last;\n}\n");
        assert_eq!(s.declaration_lines("b"), vec![2]);
        assert_eq!(s.declaration_lines("last"), vec![4]);
        assert!(s.declaration_lines("main").is_empty());
    }

    #[test]
    fn reports_unclosed_and_early_closed_braces() {
        let s = Source::new("int main(void) {\n  for (;;) {\n    x();\n  return 0;\n}\n");
        assert_eq!(s.brace_report().unclosed_open_lines, vec![2]);
        let s = Source::new("int main(void) {\n  f();\n  }\n  return 0;\n}\n");
        let r = s.brace_report();
        assert_eq!(r.function_closed_early_line, Some(3));
        assert_eq!(r.first_extra_close_line, Some(5));
    }

    #[test]
    fn similar_names() {
        assert!(similar_name("mian", "main"));
        assert!(similar_name("Printf", "printf"));
        assert!(similar_name("ad", "add") || edit_distance("ad", "add") == 1);
        assert!(!similar_name("sum", "count"));
    }

    #[test]
    fn function_definitions_and_enclosing() {
        let s = Source::new("int add(int a, int b) {\n return a + b;\n}\nint main(void) {\n  return add(1, 2);\n}\n");
        assert_eq!(s.function_definitions(), vec![("add".into(), 1), ("main".into(), 4)]);
        assert_eq!(s.enclosing_function(5), Some(("main".into(), 4)));
    }
}
