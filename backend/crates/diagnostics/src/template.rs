//! Minimal text templates for catalog wording.
//!
//! Syntax:
//! - `{name}` inserts a variable.
//! - `{?name}…{/name}` keeps the enclosed text only when `name` is present.
//! - `{!name}…{/name}` keeps the enclosed text only when `name` is absent.
//! - `{{` and `}}` produce literal braces. Any other `{`/`}` that does not
//!   form a tag is kept literally, so C code examples can be written as-is.
//!
//! Templates are validated against the variables a rule declares, so a typo
//! in an online edit is rejected before it can reach a student.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Text(String),
    Var(String),
    IfSet(String, Vec<Node>),
    IfUnset(String, Vec<Node>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("未知的變數 `{0}`")]
    UnknownVariable(String),
    #[error("區塊 `{0}` 沒有對應的結束標記 `{{/{0}}}`")]
    Unclosed(String),
    #[error("結束標記 `{{/{0}}}` 沒有對應的開始標記")]
    UnexpectedClose(String),
}

fn is_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_tag(tag: &str) -> bool {
    let name = tag.strip_prefix(['?', '!', '/']).unwrap_or(tag);
    is_name(name)
}

/// Open conditional block (is-set?, variable) and the nodes collected so far.
type Frame = (Option<(bool, String)>, Vec<Node>);

fn parse(src: &str) -> Result<Vec<Node>, TemplateError> {
    let mut stack: Vec<Frame> = vec![(None, vec![])];
    let mut text = String::new();
    let mut chars = src.char_indices().peekable();
    let flush = |text: &mut String, stack: &mut Vec<Frame>| {
        if !text.is_empty() {
            stack.last_mut().unwrap().1.push(Node::Text(std::mem::take(text)));
        }
    };
    while let Some((i, c)) = chars.next() {
        match c {
            '{' if chars.peek().map(|p| p.1) == Some('{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek().map(|p| p.1) == Some('}') => {
                chars.next();
                text.push('}');
            }
            '{' if src[i + 1..].find('}').is_some_and(|end| is_tag(&src[i + 1..i + 1 + end])) => {
                let rest = &src[i + 1..];
                let end = rest.find('}').expect("checked by guard");
                let tag = &rest[..end];
                for _ in 0..tag.chars().count() + 1 {
                    chars.next();
                }
                flush(&mut text, &mut stack);
                if let Some(name) = tag.strip_prefix('?').or_else(|| tag.strip_prefix('!')) {
                    stack.push((Some((tag.starts_with('?'), name.to_string())), vec![]));
                } else if let Some(name) = tag.strip_prefix('/') {
                    if stack.len() == 1 {
                        return Err(TemplateError::UnexpectedClose(name.to_string()));
                    }
                    let (head, body) = stack.pop().unwrap();
                    match head {
                        Some((set, n)) if n == name => {
                            let node = if set { Node::IfSet(n, body) } else { Node::IfUnset(n, body) };
                            stack.last_mut().unwrap().1.push(node);
                        }
                        _ => return Err(TemplateError::UnexpectedClose(name.to_string())),
                    }
                } else {
                    stack.last_mut().unwrap().1.push(Node::Var(tag.to_string()));
                }
            }
            _ => text.push(c),
        }
    }
    flush(&mut text, &mut stack);
    if stack.len() > 1 {
        let (head, _) = stack.pop().unwrap();
        return Err(TemplateError::Unclosed(head.map(|h| h.1).unwrap_or_default()));
    }
    Ok(stack.pop().unwrap().1)
}

fn names(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        match n {
            Node::Text(_) => {}
            Node::Var(v) => out.push(v.clone()),
            Node::IfSet(v, b) | Node::IfUnset(v, b) => {
                out.push(v.clone());
                names(b, out);
            }
        }
    }
}

/// Checks syntax and that every variable is in `allowed`.
pub fn validate(src: &str, allowed: &[&str]) -> Result<(), TemplateError> {
    let nodes = parse(src)?;
    let mut used = vec![];
    names(&nodes, &mut used);
    for u in used {
        if !allowed.contains(&u.as_str()) {
            return Err(TemplateError::UnknownVariable(u));
        }
    }
    Ok(())
}

fn render_nodes(nodes: &[Node], vars: &BTreeMap<String, String>, out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::Var(v) => out.push_str(vars.get(v).map(String::as_str).unwrap_or("")),
            Node::IfSet(v, b) => {
                if vars.get(v).is_some_and(|x| !x.is_empty()) {
                    render_nodes(b, vars, out)
                }
            }
            Node::IfUnset(v, b) => {
                if vars.get(v).is_none_or(|x| x.is_empty()) {
                    render_nodes(b, vars, out)
                }
            }
        }
    }
}

pub fn render(src: &str, vars: &BTreeMap<String, String>) -> Result<String, TemplateError> {
    let nodes = parse(src)?;
    let mut out = String::new();
    render_nodes(&nodes, vars, &mut out);
    Ok(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn renders_vars_and_conditionals() {
        let t = "看第 {line} 行。{?suggestion}你是不是想寫 `{suggestion}`？{/suggestion}{!suggestion}檢查拼字。{/suggestion}";
        assert_eq!(render(t, &vars(&[("line", "5"), ("suggestion", "count")])).unwrap(), "看第 5 行。你是不是想寫 `count`？");
        assert_eq!(render(t, &vars(&[("line", "5")])).unwrap(), "看第 5 行。檢查拼字。");
    }

    #[test]
    fn literal_braces() {
        assert_eq!(render("int main(void) {{ return 0; }}", &vars(&[])).unwrap(), "int main(void) { return 0; }");
        // C code that does not form a tag needs no escaping.
        assert_eq!(render("if (x > 0) { y = 1; }", &vars(&[])).unwrap(), "if (x > 0) { y = 1; }");
        assert_eq!(render("int a[3] = {1, 2, 3};", &vars(&[])).unwrap(), "int a[3] = {1, 2, 3};");
    }

    #[test]
    fn validation_rejects_unknown_and_unbalanced() {
        assert_eq!(validate("{lien}", &["line"]), Err(TemplateError::UnknownVariable("lien".into())));
        assert_eq!(validate("{?line}x", &["line"]), Err(TemplateError::Unclosed("line".into())));
        assert_eq!(validate("x{/line}", &["line"]), Err(TemplateError::UnexpectedClose("line".into())));
        assert!(validate("{?a}{b}{/a}", &["a", "b"]).is_ok());
    }
}
