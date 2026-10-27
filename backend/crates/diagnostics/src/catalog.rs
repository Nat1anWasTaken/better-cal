//! Editable teaching text for every rule.
//!
//! The built-in catalog ships as TOML (`catalog/*.toml`) and seeds the
//! database. Owners edit entries online; the API passes the merged catalog to
//! [`crate::render`]. Every entry is validated against the variables its rule
//! actually produces before it can be saved.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::rules::{COMMON_VARS, spec};
use crate::template::{self, TemplateError};

/// The wording of one card. Every field is a template.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuleText {
    /// Short heading.
    pub title: String,
    /// 發生什麼事: what the tool definitely observed, in plain words.
    pub what: String,
    /// 可能原因: the system's interpretation; shown with the confidence label.
    #[serde(default)]
    pub cause: String,
    /// 從哪裡看起.
    #[serde(default, rename = "where")]
    pub where_: String,
    /// 為什麼有問題: the minimum concept needed right now.
    #[serde(default)]
    pub why: String,
    /// 下一步可以做什麼.
    #[serde(default)]
    pub next_steps: Vec<String>,
    /// Progressive hints, from a direction to check to a generic example.
    #[serde(default)]
    pub hints: Vec<String>,
}

/// Partial override applied when the student has learned all `when_concepts`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub when_concepts: Vec<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub what: Option<String>,
    #[serde(default)]
    pub cause: Option<String>,
    #[serde(default, rename = "where")]
    pub where_: Option<String>,
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub next_steps: Option<Vec<String>>,
    #[serde(default)]
    pub hints: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuleEntry {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Added to the finding's ordering weight (−100..=100).
    #[serde(default)]
    pub priority: i32,
    pub title: String,
    pub what: String,
    #[serde(default)]
    pub cause: String,
    #[serde(default, rename = "where")]
    pub where_: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub next_steps: Vec<String>,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default)]
    pub variants: Vec<Variant>,
}

impl RuleEntry {
    pub fn text(&self) -> RuleText {
        RuleText {
            title: self.title.clone(),
            what: self.what.clone(),
            cause: self.cause.clone(),
            where_: self.where_.clone(),
            why: self.why.clone(),
            next_steps: self.next_steps.clone(),
            hints: self.hints.clone(),
        }
    }

    pub fn from_text(enabled: bool, priority: i32, t: RuleText, variants: Vec<Variant>) -> RuleEntry {
        RuleEntry {
            enabled,
            priority,
            title: t.title,
            what: t.what,
            cause: t.cause,
            where_: t.where_,
            why: t.why,
            next_steps: t.next_steps,
            hints: t.hints,
            variants,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustomMatchKind {
    Error,
    Warning,
    Any,
}

/// An owner-defined rule. It can only match compiler diagnostics by kind,
/// warning option and a (linear-time) regular expression on the message, and
/// it is always presented as a *guess* (`Confidence::Possible`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CustomRule {
    /// Must start with `custom.`.
    pub id: String,
    pub kind: CustomMatchKind,
    #[serde(default)]
    pub option: Option<String>,
    /// Named capture groups become template variables.
    pub message_regex: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
    pub title: String,
    pub what: String,
    #[serde(default)]
    pub cause: String,
    #[serde(default, rename = "where")]
    pub where_: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub next_steps: Vec<String>,
    #[serde(default)]
    pub hints: Vec<String>,
}

impl CustomRule {
    pub fn text(&self) -> RuleText {
        RuleText {
            title: self.title.clone(),
            what: self.what.clone(),
            cause: self.cause.clone(),
            where_: self.where_.clone(),
            why: self.why.clone(),
            next_steps: self.next_steps.clone(),
            hints: self.hints.clone(),
        }
    }

    pub fn regex(&self) -> Result<regex::Regex, regex::Error> {
        regex::RegexBuilder::new(&self.message_regex).size_limit(1 << 20).build()
    }
    pub fn capture_names(&self) -> Vec<String> {
        self.regex().map(|r| r.capture_names().flatten().map(str::to_string).collect()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Catalog {
    pub rules: BTreeMap<String, RuleEntry>,
    /// Shared UI strings (confidence labels, notes, compare-mode descriptions).
    #[serde(default)]
    pub texts: BTreeMap<String, String>,
    #[serde(default)]
    pub custom_rules: Vec<CustomRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{pointer}: {message}")]
pub struct CatalogError {
    /// JSON-pointer-like path to the offending field, e.g. `/rules/x/hints/1`.
    pub pointer: String,
    pub message: String,
}

const BUILTIN_FILES: &[&str] = &[
    include_str!("../catalog/general.toml"),
    include_str!("../catalog/syntax.toml"),
    include_str!("../catalog/identifiers.toml"),
    include_str!("../catalog/functions.toml"),
    include_str!("../catalog/format.toml"),
    include_str!("../catalog/linker.toml"),
    include_str!("../catalog/warnings.toml"),
    include_str!("../catalog/runtime.toml"),
];

#[derive(Deserialize)]
struct CatalogFile {
    #[serde(default)]
    rules: BTreeMap<String, RuleEntry>,
    #[serde(default)]
    texts: BTreeMap<String, String>,
}

/// Keys every `texts` table must provide (the UI relies on them).
pub const REQUIRED_TEXTS: &[&str] = &[
    "confidence.confirmed",
    "confidence.likely",
    "confidence.possible",
    "precision.near",
    "precision.approximate",
    "fixed_region",
    "cascade",
    "hidden_test",
    "compare.trim_trailing_whitespace",
    "compare.exact",
];

impl Catalog {
    /// The catalog compiled into the binary. Panics only if the shipped TOML
    /// is broken, which the test-suite guarantees it is not.
    pub fn builtin() -> Catalog {
        let mut cat = Catalog::default();
        for (i, src) in BUILTIN_FILES.iter().enumerate() {
            let file: CatalogFile = toml::from_str(src).unwrap_or_else(|e| panic!("built-in catalog file #{i} is invalid: {e}"));
            cat.rules.extend(file.rules);
            cat.texts.extend(file.texts);
        }
        cat
    }

    /// Variables a rule's templates may reference.
    pub fn allowed_vars(&self, rule_id: &str) -> Option<Vec<String>> {
        if let Some(s) = spec(rule_id) {
            return Some(s.vars.iter().chain(COMMON_VARS).map(|v| v.to_string()).collect());
        }
        self.custom_rules.iter().find(|c| c.id == rule_id).map(|c| c.capture_names().into_iter().chain(COMMON_VARS.iter().map(|v| v.to_string())).collect())
    }

    /// Full validation; returns every problem found.
    pub fn validate(&self) -> Vec<CatalogError> {
        let mut errs = Vec::new();
        for (id, entry) in &self.rules {
            let Some(allowed) = self.allowed_vars(id) else {
                errs.push(CatalogError { pointer: format!("/rules/{id}"), message: "沒有這個規則 id".into() });
                continue;
            };
            errs.extend(validate_entry(&format!("/rules/{id}"), entry, &allowed));
        }
        for key in REQUIRED_TEXTS {
            if !self.texts.contains_key(*key) {
                errs.push(CatalogError { pointer: format!("/texts/{key}"), message: "缺少必要的文字".into() });
            }
        }
        let mut seen = BTreeSet::new();
        for (i, c) in self.custom_rules.iter().enumerate() {
            let p = format!("/custom_rules/{i}");
            errs.extend(validate_custom_rule(&p, c));
            if !seen.insert(&c.id) {
                errs.push(CatalogError { pointer: format!("{p}/id"), message: "自訂規則 id 重複".into() });
            }
        }
        errs
    }

    /// Wording for `rule_id`, if it exists and is enabled.
    pub fn entry(&self, rule_id: &str) -> Option<RuleEntry> {
        if let Some(e) = self.rules.get(rule_id) {
            return Some(e.clone());
        }
        self.custom_rules.iter().find(|c| c.id == rule_id).map(|c| RuleEntry::from_text(c.enabled, c.priority, c.text(), vec![]))
    }

    pub fn text(&self, key: &str) -> String {
        self.texts.get(key).cloned().unwrap_or_default()
    }
}

fn check(pointer: String, src: &str, allowed: &[String], errs: &mut Vec<CatalogError>) {
    let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
    if let Err(e) = template::validate(src, &allowed) {
        errs.push(CatalogError { pointer, message: describe(&e, &allowed) });
    }
}

fn describe(e: &TemplateError, allowed: &[&str]) -> String {
    match e {
        TemplateError::UnknownVariable(_) => format!("{e}；可用的變數：{}", allowed.join("、")),
        _ => e.to_string(),
    }
}

fn validate_text(p: &str, t: &RuleText, allowed: &[String], errs: &mut Vec<CatalogError>) {
    if t.title.trim().is_empty() {
        errs.push(CatalogError { pointer: format!("{p}/title"), message: "標題不可空白".into() });
    }
    if t.what.trim().is_empty() {
        errs.push(CatalogError { pointer: format!("{p}/what"), message: "「發生什麼事」不可空白".into() });
    }
    for (f, v) in [("title", &t.title), ("what", &t.what), ("cause", &t.cause), ("where", &t.where_), ("why", &t.why)] {
        check(format!("{p}/{f}"), v, allowed, errs);
    }
    for (i, s) in t.next_steps.iter().enumerate() {
        check(format!("{p}/next_steps/{i}"), s, allowed, errs);
    }
    for (i, s) in t.hints.iter().enumerate() {
        check(format!("{p}/hints/{i}"), s, allowed, errs);
    }
}

pub fn validate_entry(p: &str, e: &RuleEntry, allowed: &[String]) -> Vec<CatalogError> {
    let mut errs = Vec::new();
    validate_text(p, &e.text(), allowed, &mut errs);
    if !(-100..=100).contains(&e.priority) {
        errs.push(CatalogError { pointer: format!("{p}/priority"), message: "優先度必須介於 -100 與 100 之間".into() });
    }
    for (i, v) in e.variants.iter().enumerate() {
        let vp = format!("{p}/variants/{i}");
        if v.when_concepts.is_empty() {
            errs.push(CatalogError { pointer: format!("{vp}/when_concepts"), message: "至少要指定一個概念".into() });
        }
        for c in &v.when_concepts {
            if !crate::CONCEPTS.iter().any(|(id, _)| id == c) {
                errs.push(CatalogError { pointer: format!("{vp}/when_concepts"), message: format!("未知的概念 `{c}`") });
            }
        }
        for (f, val) in [("title", &v.title), ("what", &v.what), ("cause", &v.cause), ("where", &v.where_), ("why", &v.why)] {
            if let Some(val) = val {
                check(format!("{vp}/{f}"), val, allowed, &mut errs);
            }
        }
        for (f, list) in [("next_steps", &v.next_steps), ("hints", &v.hints)] {
            for (j, s) in list.iter().flatten().enumerate() {
                check(format!("{vp}/{f}/{j}"), s, allowed, &mut errs);
            }
        }
    }
    errs
}

pub fn validate_custom_rule(p: &str, c: &CustomRule) -> Vec<CatalogError> {
    let mut errs = Vec::new();
    if !c.id.starts_with("custom.") || c.id.len() < 8 || !c.id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_') {
        errs.push(CatalogError {
            pointer: format!("{p}/id"), message: "自訂規則 id 必須以 `custom.` 開頭，只能使用英數字、`.` 與 `_`".into()
        });
    }
    if let Err(e) = c.regex() {
        errs.push(CatalogError { pointer: format!("{p}/message_regex"), message: format!("正規表達式無效：{e}") });
        return errs;
    }
    let allowed: Vec<String> = c.capture_names().into_iter().chain(COMMON_VARS.iter().map(|v| v.to_string())).collect();
    validate_text(p, &c.text(), &allowed, &mut errs);
    errs
}

/// Applies matching concept variants (least specific first, so the most
/// specific variant wins).
pub fn select_text(entry: &RuleEntry, learned: &BTreeSet<String>) -> RuleText {
    let mut t = entry.text();
    let mut vs: Vec<&Variant> = entry.variants.iter().filter(|v| v.when_concepts.iter().all(|c| learned.contains(c))).collect();
    vs.sort_by_key(|v| v.when_concepts.len());
    for v in vs {
        if let Some(x) = &v.title {
            t.title = x.clone();
        }
        if let Some(x) = &v.what {
            t.what = x.clone();
        }
        if let Some(x) = &v.cause {
            t.cause = x.clone();
        }
        if let Some(x) = &v.where_ {
            t.where_ = x.clone();
        }
        if let Some(x) = &v.why {
            t.why = x.clone();
        }
        if let Some(x) = &v.next_steps {
            t.next_steps = x.clone();
        }
        if let Some(x) = &v.hints {
            t.hints = x.clone();
        }
    }
    t
}
