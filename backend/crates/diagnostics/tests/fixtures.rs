//! Runs the analyzer over real GCC 12 output captured by
//! `scripts/gen-fixtures.sh` and checks each fixture's `expect.toml`.
//!
//! `expect.toml` keys:
//! - `primary` / `primary_line`: the card we recommend fixing first.
//! - `rules`: rules that must appear. `absent`: rules that must NOT appear
//!   (counter-examples guarding against wrong matches).
//! - `compile_success`: whether gcc produced an executable.
//! - `[vars]`: evidence variables expected on the primary finding.
//! - `cascade`: rules that must be marked as a possible knock-on effect.
//! - `findings`: exact number of findings (optional).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use diagnostics::{AnalysisInput, Catalog, RenderContext, analyze, render};
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Expect {
    primary: Option<String>,
    primary_line: Option<u32>,
    #[serde(default)]
    rules: Vec<String>,
    #[serde(default)]
    absent: Vec<String>,
    compile_success: bool,
    #[serde(default)]
    vars: BTreeMap<String, String>,
    #[serde(default)]
    cascade: Vec<String>,
    findings: Option<usize>,
}

fn load(dir: &Path) -> (String, judge_proto::CompileResult) {
    let source = std::fs::read_to_string(dir.join("main.c")).unwrap();
    let stderr = std::fs::read_to_string(dir.join("stderr.txt")).unwrap();
    let exit: i32 = std::fs::read_to_string(dir.join("exit_code.txt")).unwrap().trim().parse().unwrap();
    let (json, text) = judge_proto::split_gcc_stderr(&stderr);
    let compile = judge_proto::CompileResult { success: exit == 0, exit_code: Some(exit), timed_out: false, diagnostics_json: json, stderr_text: text };
    (source, compile)
}

#[test]
fn fixtures_match_expectations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let catalog = Catalog::builtin();
    let learned = BTreeSet::new();
    let dump = std::env::var("DUMP").is_ok();
    let mut failures = vec![];
    let mut count = 0;

    let mut dirs: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().filter(|e| e.path().is_dir()).collect();
    dirs.sort_by_key(|e| e.file_name());
    for entry in dirs {
        let dir = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let (source, compile) = load(&dir);
        let adj = BTreeMap::new();
        let a = analyze(&AnalysisInput {
            source: &source,
            compile: Some(&compile),
            runs: &[],
            fixed_regions: &[],
            system_error: false,
            custom_rules: &[],
            priority_adjust: &adj,
        });
        if dump {
            println!("=== {name} (compile ok: {})", a.compile_succeeded);
            for f in &a.findings {
                println!(
                    "  {}{} {:?}/{:?} line={:?} conf={:?} cascade={:?} vars={:?}",
                    if f.primary { "*" } else { " " },
                    f.rule,
                    f.category,
                    f.severity,
                    f.span.map(|s| s.line),
                    f.confidence,
                    f.maybe_cascade_of,
                    f.vars
                );
            }
        }
        let cards = render(&a.findings, &RenderContext { catalog: &catalog, builtin: &catalog, learned: &learned });
        for c in &cards {
            if c.used_fallback_text || c.title.is_empty() || c.what.is_empty() {
                failures.push(format!("{name}: card for {} did not render cleanly", c.rule));
            }
            for text in [&c.title, &c.what].into_iter().chain(c.cause.iter()).chain(c.hints.iter()) {
                if text.contains("{?") || text.contains("{/") {
                    failures.push(format!("{name}: unrendered template in {}: {text}", c.rule));
                }
            }
        }

        let Ok(expect_src) = std::fs::read_to_string(dir.join("expect.toml")) else {
            failures.push(format!("{name}: missing expect.toml"));
            continue;
        };
        let e: Expect = toml::from_str(&expect_src).unwrap_or_else(|err| panic!("{name}/expect.toml: {err}"));
        count += 1;
        let rules: Vec<&str> = a.findings.iter().map(|f| f.rule.as_str()).collect();
        if a.compile_succeeded != e.compile_success {
            failures.push(format!("{name}: compile_success {} != expected {}", a.compile_succeeded, e.compile_success));
        }
        let primary = a.findings.iter().find(|f| f.primary);
        if e.primary.as_deref() != primary.map(|p| p.rule.as_str()) {
            failures.push(format!("{name}: primary {:?} != expected {:?} (all: {rules:?})", primary.map(|p| &p.rule), e.primary));
        }
        if let (Some(line), Some(p)) = (e.primary_line, primary)
            && p.span.map(|s| s.line) != Some(line)
        {
            failures.push(format!("{name}: primary line {:?} != expected {line}", p.span.map(|s| s.line)));
        }
        for r in &e.rules {
            if !rules.contains(&r.as_str()) {
                failures.push(format!("{name}: expected rule {r} missing (got {rules:?})"));
            }
        }
        for r in &e.absent {
            if rules.contains(&r.as_str()) {
                failures.push(format!("{name}: rule {r} must not match (got {rules:?})"));
            }
        }
        for (k, v) in &e.vars {
            let got = primary.and_then(|p| p.vars.get(k));
            if got != Some(v) {
                failures.push(format!("{name}: primary var {k} = {got:?}, expected {v:?}"));
            }
        }
        for r in &e.cascade {
            if !a.findings.iter().any(|f| &f.rule == r && f.maybe_cascade_of.is_some()) {
                failures.push(format!("{name}: expected {r} to be marked as possible cascade"));
            }
        }
        if let Some(n) = e.findings
            && a.findings.len() != n
        {
            failures.push(format!("{name}: {} findings, expected {n} ({rules:?})", a.findings.len()));
        }
    }
    assert!(count >= 40, "expected at least 40 fixtures, found {count}");
    assert!(failures.is_empty(), "fixture failures:\n{}", failures.join("\n"));
}

#[test]
fn fixtures_were_generated_with_the_pinned_toolchain() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/TOOLCHAIN");
    let v = std::fs::read_to_string(root).unwrap();
    assert!(v.contains("12.2.0-14+deb12u1"), "fixtures were generated with an unexpected compiler: {v}");
}
