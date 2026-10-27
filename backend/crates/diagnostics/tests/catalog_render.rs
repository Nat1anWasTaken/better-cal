use std::collections::{BTreeMap, BTreeSet};

use diagnostics::catalog::{CustomMatchKind, validate_custom_rule};
use diagnostics::{AnalysisInput, Catalog, Category, Confidence, CustomRule, FixedRegion, RenderContext, RunObservation, SPECS, TestRef, analyze, render};
use judge_proto::{CompileResult, RunStatus};

fn compile_ok() -> CompileResult {
    CompileResult { success: true, exit_code: Some(0), timed_out: false, diagnostics_json: serde_json::json!([]), stderr_text: String::new() }
}

fn run(number: u32, hidden: bool, status: RunStatus) -> RunObservation {
    RunObservation {
        test: TestRef { number, hidden, name: None },
        status,
        exit_code: Some(0),
        signal: None,
        time_ms: 5,
        time_limit_ms: 1000,
        memory_limit_kb: 65536,
        wall_limit_with_low_cpu: false,
        answer_correct: Some(true),
        whitespace_only_difference: false,
        stderr_excerpt: String::new(),
    }
}

fn analyze_src(src: &str, compile: &CompileResult, runs: &[RunObservation]) -> diagnostics::Analysis {
    let adj = BTreeMap::new();
    analyze(&AnalysisInput { source: src, compile: Some(compile), runs, fixed_regions: &[], system_error: false, custom_rules: &[], priority_adjust: &adj })
}

fn learned(c: &[&str]) -> BTreeSet<String> {
    c.iter().map(|s| s.to_string()).collect()
}

#[test]
fn builtin_catalog_is_valid_and_complete() {
    let cat = Catalog::builtin();
    let errors = cat.validate();
    assert!(errors.is_empty(), "built-in catalog errors: {errors:#?}");
    for spec in SPECS {
        assert!(cat.rules.contains_key(spec.id), "rule {} has no built-in wording", spec.id);
        let e = &cat.rules[spec.id];
        assert!(!e.hints.is_empty() || spec.id == "judge.system_error" || spec.id == "compile.timeout", "rule {} has no hints", spec.id);
    }
    for id in cat.rules.keys() {
        assert!(SPECS.iter().any(|s| s.id == id), "catalog has wording for unknown rule {id}");
    }
}

#[test]
fn validation_reports_pointer_to_bad_field() {
    let mut cat = Catalog::builtin();
    cat.rules.get_mut("syntax.missing_semicolon").unwrap().hints[1] = "看第 {lien} 行".into();
    let errs = cat.validate();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].pointer, "/rules/syntax.missing_semicolon/hints/1");
    assert!(errs[0].message.contains("lien"));
}

#[test]
fn wording_changes_with_learned_concepts() {
    let json: serde_json::Value = serde_json::from_str(include_str!("../fixtures/scanf_missing_amp/stderr.txt").lines().next().unwrap()).unwrap();
    let compile = CompileResult { diagnostics_json: json, ..compile_ok() };
    let src = include_str!("../fixtures/scanf_missing_amp/main.c");
    let a = analyze_src(src, &compile, &[]);
    let cat = Catalog::builtin();
    let beginner = render(&a.findings, &RenderContext { catalog: &cat, builtin: &cat, learned: &learned(&[]) });
    let advanced = render(&a.findings, &RenderContext { catalog: &cat, builtin: &cat, learned: &learned(&["pointers"]) });
    let b = beginner[0].cause.clone().unwrap();
    let p = advanced[0].cause.clone().unwrap();
    assert!(b.contains("存在哪裡"), "{b}");
    assert!(!b.contains("指標"), "beginner wording must not assume pointers: {b}");
    assert!(p.contains("指標"), "{p}");
    // First hint gives a direction, it does not hand out the fix.
    assert!(!beginner[0].hints[0].contains('&'));
}

#[test]
fn broken_online_edit_falls_back_to_builtin_wording() {
    let src = "int main(void) {\n    int a = 1\n    return a;\n}\n";
    let json = serde_json::json!([{"kind": "error", "message": "expected ',' or ';' before 'return'", "locations": [{"caret": {"line": 3, "column": 5}}]}]);
    let compile = CompileResult { success: false, exit_code: Some(1), diagnostics_json: json, ..compile_ok() };
    let a = analyze_src(src, &compile, &[]);
    let builtin = Catalog::builtin();
    let mut edited = builtin.clone();
    // Bypass validation, as if a corrupted row were loaded.
    edited.rules.get_mut("syntax.missing_semicolon").unwrap().title = "{?line}未關閉".into();
    let cards = render(&a.findings, &RenderContext { catalog: &edited, builtin: &builtin, learned: &learned(&[]) });
    assert!(cards[0].used_fallback_text);
    assert_eq!(cards[0].title, "指令結尾可能少了分號");
}

#[test]
fn disabled_rule_uses_generic_card() {
    let src = "int main(void) {\n    int a = 1\n    return a;\n}\n";
    let json = serde_json::json!([{"kind": "error", "message": "expected ',' or ';' before 'return'", "locations": [{"caret": {"line": 3, "column": 5}}]}]);
    let compile = CompileResult { success: false, exit_code: Some(1), diagnostics_json: json, ..compile_ok() };
    let a = analyze_src(src, &compile, &[]);
    let mut cat = Catalog::builtin();
    cat.rules.get_mut("syntax.missing_semicolon").unwrap().enabled = false;
    let cards = render(&a.findings, &RenderContext { catalog: &cat, builtin: &cat, learned: &learned(&[]) });
    assert_eq!(cards[0].title, "編譯器回報了一個錯誤");
    assert!(cards[0].what.contains("第 3 行"));
}

#[test]
fn segfault_mentions_risky_warnings_but_stays_a_guess() {
    let json: serde_json::Value = serde_json::from_str(include_str!("../fixtures/scanf_missing_amp/stderr.txt").lines().next().unwrap()).unwrap();
    let compile = CompileResult { diagnostics_json: json, ..compile_ok() };
    let mut r = run(1, false, RunStatus::Signaled);
    r.signal = Some(11);
    r.answer_correct = None;
    let a = analyze_src(include_str!("../fixtures/scanf_missing_amp/main.c"), &compile, &[r]);
    let seg = a.findings.iter().find(|f| f.rule == "run.segfault").unwrap();
    assert!(seg.primary, "runtime crash should be fixed first");
    assert_eq!(seg.confidence, Confidence::Possible);
    assert_eq!(seg.vars.get("warning_lines").map(String::as_str), Some("5"));
}

#[test]
fn timeout_points_to_while_scanf_without_claiming_infinite_loop() {
    let src = include_str!("../fixtures/lint_while_scanf_eof/main.c");
    let mut r = run(2, true, RunStatus::TimeLimit);
    r.answer_correct = None;
    let a = analyze_src(src, &compile_ok(), &[r]);
    let tle = a.findings.iter().find(|f| f.rule == "run.time_limit").unwrap();
    assert_eq!(tle.vars.get("while_scanf_line").map(String::as_str), Some("5"));
    assert_eq!(tle.confidence, Confidence::Possible);
    let cat = Catalog::builtin();
    let cards = render(&a.findings, &RenderContext { catalog: &cat, builtin: &cat, learned: &learned(&[]) });
    let card = cards.iter().find(|c| c.rule == "run.time_limit").unwrap();
    assert!(card.why.as_deref().unwrap().contains("不一定代表無窮迴圈"));
    assert!(card.what.contains("隱藏測資"), "hidden test note expected: {}", card.what);
}

#[test]
fn wrong_answers_are_grouped_and_hidden_flagged() {
    let mut a1 = run(1, false, RunStatus::Ok);
    a1.answer_correct = Some(false);
    let mut a2 = run(3, true, RunStatus::Ok);
    a2.answer_correct = Some(false);
    let mut ws = run(2, false, RunStatus::Ok);
    ws.answer_correct = Some(false);
    ws.whitespace_only_difference = true;
    let a = analyze_src("int main(void){return 0;}\n", &compile_ok(), &[a1, ws, a2]);
    let wa = a.findings.iter().find(|f| f.rule == "judge.wrong_answer").unwrap();
    assert_eq!(wa.vars.get("count").map(String::as_str), Some("2"));
    assert_eq!(wa.test.as_ref().unwrap().number, 1);
    assert!(a.findings.iter().any(|f| f.rule == "judge.whitespace_only"));
    assert!(a.findings.iter().all(|f| f.category == Category::WrongAnswer));
}

#[test]
fn hidden_test_stderr_never_reaches_raw_output() {
    let mut r = run(4, true, RunStatus::NonZeroExit);
    r.exit_code = Some(3);
    r.answer_correct = None;
    r.stderr_excerpt = "SECRET-HIDDEN-INPUT".into();
    let a = analyze_src("int main(void){return 3;}\n", &compile_ok(), &[r]);
    let s = serde_json::to_string(&a).unwrap();
    assert!(!s.contains("SECRET-HIDDEN-INPUT"));
}

#[test]
fn system_error_is_primary_and_not_blamed_on_student() {
    let adj = BTreeMap::new();
    let a = analyze(&AnalysisInput { source: "", compile: None, runs: &[], fixed_regions: &[], system_error: true, custom_rules: &[], priority_adjust: &adj });
    assert_eq!(a.findings.len(), 1);
    assert!(a.findings[0].primary);
    let cat = Catalog::builtin();
    let cards = render(&a.findings, &RenderContext { catalog: &cat, builtin: &cat, learned: &learned(&[]) });
    assert!(cards[0].what.contains("不是你的程式有錯"));
}

#[test]
fn failed_compile_without_readable_diagnostics_still_gets_a_card() {
    let compile = CompileResult {
        success: false,
        exit_code: Some(1),
        timed_out: false,
        diagnostics_json: serde_json::json!({"not": "an array"}),
        stderr_text: String::new(),
    };
    let a = analyze_src("int main(void) { return 0; }\n", &compile, &[]);
    assert_eq!(a.findings.len(), 1);
    assert_eq!(a.findings[0].rule, "compile.unexplained");
    assert!(a.findings[0].primary);
}

#[test]
fn custom_rule_matches_unknown_diagnostic_as_guess() {
    let rule = CustomRule {
        id: "custom.break_outside".into(),
        kind: CustomMatchKind::Error,
        option: None,
        message_regex: r"^(?P<kw>break|continue) statement not within".into(),
        enabled: true,
        priority: 0,
        title: "`{kw}` 只能用在迴圈裡".into(),
        what: "第 {line} 行的 `{kw}` 不在任何迴圈裡。".into(),
        cause: String::new(),
        where_: String::new(),
        why: String::new(),
        next_steps: vec![],
        hints: vec!["檢查 {kw} 外面有沒有 for 或 while。".into()],
    };
    assert!(validate_custom_rule("/custom_rules/0", &rule).is_empty());
    let src = include_str!("../fixtures/unknown_break_outside_loop/main.c");
    let json: serde_json::Value = serde_json::from_str(include_str!("../fixtures/unknown_break_outside_loop/stderr.txt").lines().next().unwrap()).unwrap();
    let compile = CompileResult { success: false, exit_code: Some(1), diagnostics_json: json, ..compile_ok() };
    let adj = BTreeMap::new();
    let customs = vec![rule.clone()];
    let a = analyze(&AnalysisInput {
        source: src,
        compile: Some(&compile),
        runs: &[],
        fixed_regions: &[],
        system_error: false,
        custom_rules: &customs,
        priority_adjust: &adj,
    });
    assert_eq!(a.findings[0].rule, "custom.break_outside");
    assert_eq!(a.findings[0].confidence, Confidence::Possible);
    let mut cat = Catalog::builtin();
    cat.custom_rules.push(rule);
    let cards = render(&a.findings, &RenderContext { catalog: &cat, builtin: &cat, learned: &learned(&[]) });
    assert_eq!(cards[0].title, "`break` 只能用在迴圈裡");
}

#[test]
fn custom_rule_with_bad_regex_or_var_is_rejected() {
    let mut rule = CustomRule {
        id: "custom.x1".into(),
        kind: CustomMatchKind::Any,
        option: None,
        message_regex: "(unclosed".into(),
        enabled: true,
        priority: 0,
        title: "t".into(),
        what: "w".into(),
        cause: String::new(),
        where_: String::new(),
        why: String::new(),
        next_steps: vec![],
        hints: vec![],
    };
    assert!(validate_custom_rule("/c", &rule).iter().any(|e| e.pointer == "/c/message_regex"));
    rule.message_regex = "ok".into();
    rule.what = "{nope}".into();
    assert!(validate_custom_rule("/c", &rule).iter().any(|e| e.pointer == "/c/what"));
}

#[test]
fn fixed_region_is_flagged() {
    let src = "int main(void) {\n    int a = 1\n    return a;\n}\n";
    let json = serde_json::json!([{"kind": "error", "message": "expected ',' or ';' before 'return'", "locations": [{"caret": {"line": 3, "column": 5}}]}]);
    let compile = CompileResult { success: false, exit_code: Some(1), diagnostics_json: json, ..compile_ok() };
    let adj = BTreeMap::new();
    let fixed = vec![FixedRegion { start_line: 3, end_line: 4 }];
    let a = analyze(&AnalysisInput {
        source: src,
        compile: Some(&compile),
        runs: &[],
        fixed_regions: &fixed,
        system_error: false,
        custom_rules: &[],
        priority_adjust: &adj,
    });
    assert!(a.findings[0].in_fixed_region);
    let cat = Catalog::builtin();
    let cards = render(&a.findings, &RenderContext { catalog: &cat, builtin: &cat, learned: &learned(&[]) });
    assert!(cards[0].fixed_region_note.as_deref().unwrap().contains("你填寫的區塊"));
    // The semicolon hint still points to the previous (editable) line.
    assert_eq!(a.findings[0].vars.get("prev_line").map(String::as_str), Some("2"));
}

#[test]
fn priority_adjustment_changes_primary_warning() {
    let json = serde_json::json!([
        {"kind": "warning", "message": "unused variable 'x'", "option": "-Wunused-variable", "locations": [{"caret": {"line": 2, "column": 9}}]},
        {"kind": "warning", "message": "comparison of integer expressions of different signedness: 'int' and 'size_t'", "option": "-Wsign-compare", "locations": [{"caret": {"line": 3, "column": 9}}]}
    ]);
    let compile = CompileResult { diagnostics_json: json, ..compile_ok() };
    let src = "int main(void) {\n    int x;\n    return 0;\n}\n";
    let mut adj = BTreeMap::new();
    let a = analyze(&AnalysisInput {
        source: src,
        compile: Some(&compile),
        runs: &[],
        fixed_regions: &[],
        system_error: false,
        custom_rules: &[],
        priority_adjust: &adj,
    });
    assert_eq!(a.findings.iter().find(|f| f.primary).unwrap().rule, "warn.sign_compare");
    adj.insert("warn.unused_variable".to_string(), 50);
    let a = analyze(&AnalysisInput {
        source: src,
        compile: Some(&compile),
        runs: &[],
        fixed_regions: &[],
        system_error: false,
        custom_rules: &[],
        priority_adjust: &adj,
    });
    assert_eq!(a.findings.iter().find(|f| f.primary).unwrap().rule, "warn.unused_variable");
}

fn wrong(number: u32, hidden: bool) -> RunObservation {
    let mut r = run(number, hidden, RunStatus::Ok);
    r.answer_correct = Some(false);
    r
}

fn cards_for(runs: &[RunObservation]) -> Vec<diagnostics::Card> {
    let a = analyze_src("int main(void){return 0;}\n", &compile_ok(), runs);
    let cat = Catalog::builtin();
    render(&a.findings, &RenderContext { catalog: &cat, builtin: &cat, learned: &learned(&[]) })
}

#[test]
fn hidden_wrong_answer_explains_once_and_notes_public_passed() {
    let cards = cards_for(&[run(1, false, RunStatus::Ok), wrong(2, true)]);
    let card = cards.iter().find(|c| c.rule == "judge.wrong_answer").unwrap();
    let all = format!("{}{}", card.what, card.cause.as_deref().unwrap_or(""));
    assert_eq!(all.matches("隱藏測資").count(), 2, "hidden explanation repeated: {all}");
    assert!(all.contains("公開測資全部答對"), "{all}");
}

#[test]
fn hidden_wrong_answer_without_public_pass_uses_generic_cause() {
    let cards = cards_for(&[wrong(1, true), run(2, false, RunStatus::TimeLimit)]);
    let card = cards.iter().find(|c| c.rule == "judge.wrong_answer").unwrap();
    assert!(!card.cause.as_deref().unwrap().contains("公開測資全部答對"));
}

#[test]
fn public_wrong_answer_mentions_hidden_failures_too() {
    let cards = cards_for(&[wrong(1, false), wrong(2, true), wrong(3, true)]);
    let card = cards.iter().find(|c| c.rule == "judge.wrong_answer").unwrap();
    assert!(card.cause.as_deref().unwrap().contains("還有 2 筆隱藏測資"), "{:?}", card.cause);
}

#[test]
fn hidden_whitespace_only_does_not_point_at_comparison() {
    let mut ws = wrong(2, true);
    ws.whitespace_only_difference = true;
    let cards = cards_for(&[run(1, false, RunStatus::Ok), ws]);
    let card = cards.iter().find(|c| c.rule == "judge.whitespace_only").unwrap();
    assert!(card.next_steps.iter().all(|s| !s.contains("比較區")), "{:?}", card.next_steps);
    assert!(card.cause.as_deref().unwrap().contains("隱藏測資"));
}

#[test]
fn hidden_only_timeout_suggests_slow_method() {
    let mut tle = run(2, true, RunStatus::TimeLimit);
    tle.answer_correct = None;
    let cards = cards_for(&[run(1, false, RunStatus::Ok), tle]);
    let card = cards.iter().find(|c| c.rule == "run.time_limit").unwrap();
    assert!(card.cause.as_deref().unwrap().contains("只有隱藏測資超時"), "{:?}", card.cause);
}
