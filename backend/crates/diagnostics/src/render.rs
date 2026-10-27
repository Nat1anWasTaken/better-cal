//! Turns findings into teaching cards using the (editable) catalog.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, RuleText, select_text};
use crate::template;
use crate::{Category, Confidence, Finding, Precision, RawDiagnostic, Severity, Span, TestRef};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Card {
    pub finding_id: u32,
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    pub confidence: Confidence,
    /// e.g. 「編譯器確認」「很可能」「推測」.
    pub confidence_label: String,
    pub title: String,
    /// 發生什麼事 (fact).
    pub what: String,
    /// 可能原因 (interpretation; render with `confidence_label`).
    pub cause: Option<String>,
    /// 從哪裡看起.
    pub where_text: Option<String>,
    /// 為什麼有問題.
    pub why: Option<String>,
    /// 下一步可以做什麼.
    pub next_steps: Vec<String>,
    /// All hint levels; the API decides how many to reveal.
    pub hints: Vec<String>,
    pub span: Option<Span>,
    pub precision: Precision,
    pub precision_note: Option<String>,
    pub in_fixed_region: bool,
    pub fixed_region_note: Option<String>,
    pub primary: bool,
    pub maybe_cascade_of: Option<u32>,
    pub cascade_note: Option<String>,
    pub test: Option<TestRef>,
    pub raw: Vec<RawDiagnostic>,
    /// True when the owner's edited wording failed to render and the built-in
    /// wording (or a minimal message) was used instead.
    pub used_fallback_text: bool,
}

pub struct RenderContext<'a> {
    /// Merged catalog (built-in + published online edits).
    pub catalog: &'a Catalog,
    /// Pristine built-in catalog, used if an edited entry fails to render.
    pub builtin: &'a Catalog,
    /// Concepts the student has learned (effective policy).
    pub learned: &'a BTreeSet<String>,
}

fn fallback_rule(f: &Finding) -> &'static str {
    match (f.category, f.severity) {
        (Category::LinkError, _) => "fallback.link_error",
        (_, Severity::Error) => "fallback.compile_error",
        _ => "fallback.warning",
    }
}

fn render_text(t: &RuleText, vars: &BTreeMap<String, String>) -> Result<RuleText, template::TemplateError> {
    let r = |s: &str| template::render(s, vars);
    let list = |v: &[String]| -> Result<Vec<String>, template::TemplateError> {
        v.iter().map(|s| r(s)).filter(|x| x.as_ref().map_or(true, |s| !s.is_empty())).collect()
    };
    Ok(RuleText {
        title: r(&t.title)?,
        what: r(&t.what)?,
        cause: r(&t.cause)?,
        where_: r(&t.where_)?,
        why: r(&t.why)?,
        next_steps: list(&t.next_steps)?,
        hints: list(&t.hints)?,
    })
}

fn non_empty(s: String) -> Option<String> {
    (!s.trim().is_empty()).then_some(s)
}

pub fn render(findings: &[Finding], ctx: &RenderContext<'_>) -> Vec<Card> {
    findings.iter().map(|f| render_one(f, ctx)).collect()
}

fn render_one(f: &Finding, ctx: &RenderContext<'_>) -> Card {
    let mut vars = f.vars.clone();
    if !vars.contains_key("message")
        && let Some(r) = f.raw.first()
    {
        vars.insert("message".into(), r.message.clone());
    }

    // Disabled rules are shown with the honest generic wording.
    let entry = ctx.catalog.entry(&f.rule).filter(|e| e.enabled).or_else(|| ctx.catalog.entry(fallback_rule(f)));
    let mut used_fallback_text = false;
    let text = entry
        .as_ref()
        .and_then(|e| render_text(&select_text(e, ctx.learned), &vars).ok())
        .or_else(|| {
            used_fallback_text = true;
            ctx.builtin.entry(&f.rule).and_then(|e| render_text(&select_text(&e, ctx.learned), &vars).ok())
        })
        .unwrap_or_else(|| {
            used_fallback_text = true;
            RuleText { title: "程式有一個問題".into(), what: vars.get("message").cloned().unwrap_or_default(), ..Default::default() }
        });

    let texts = &ctx.catalog;
    let tvar = |key: &str| -> String { template::render(&texts.text(key), &vars).unwrap_or_else(|_| ctx.builtin.text(key)) };
    let confidence_label = tvar(match f.confidence {
        Confidence::Confirmed => "confidence.confirmed",
        Confidence::Likely => "confidence.likely",
        Confidence::Possible => "confidence.possible",
    });
    let precision_note = match f.precision {
        Precision::Exact => None,
        Precision::Near => non_empty(tvar("precision.near")),
        Precision::Approximate => non_empty(tvar("precision.approximate")),
    };
    let hidden_note = f.test.as_ref().filter(|t| t.hidden).map(|_| tvar("hidden_test"));
    let what = match hidden_note {
        Some(n) if !n.is_empty() => format!("{}\n\n{}", text.what, n),
        _ => text.what,
    };

    Card {
        finding_id: f.id,
        rule: f.rule.clone(),
        category: f.category,
        severity: f.severity,
        confidence: f.confidence,
        confidence_label,
        title: text.title,
        what,
        cause: non_empty(text.cause),
        where_text: non_empty(text.where_),
        why: non_empty(text.why),
        next_steps: text.next_steps,
        hints: text.hints,
        span: f.span,
        precision: f.precision,
        precision_note,
        in_fixed_region: f.in_fixed_region,
        fixed_region_note: f.in_fixed_region.then(|| tvar("fixed_region")).and_then(non_empty),
        primary: f.primary,
        maybe_cascade_of: f.maybe_cascade_of,
        cascade_note: f.maybe_cascade_of.map(|_| tvar("cascade")).and_then(non_empty),
        test: f.test.clone(),
        raw: f.raw.clone(),
        used_fallback_text,
    }
}
