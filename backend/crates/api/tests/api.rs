//! API integration tests against a real PostgreSQL and the real judge-runner.

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn permissions_are_enforced_for_every_kind_of_caller() {
    let app = app().await;
    let mut owner = app.owner().await;
    let (pid, _) = published_problem_in_set(&mut owner, "perm-ab", "perm-set").await;

    // Anonymous.
    let mut anon = app.anon();
    for path in ["/api/v1/problem-sets", "/api/v1/me/progress", "/api/v1/admin/problems", "/api/v1/admin/users"] {
        assert_eq!(anon.get(path).await.status, StatusCode::UNAUTHORIZED, "{path}");
    }

    // Students cannot use any admin endpoint.
    let mut s1 = app.student("s1").await;
    for path in [
        "/api/v1/admin/problems",
        "/api/v1/admin/problems/perm-ab",
        "/api/v1/admin/users",
        "/api/v1/admin/catalog",
        "/api/v1/admin/settings",
        "/api/v1/admin/submissions",
        "/api/v1/admin/export",
    ] {
        assert_eq!(s1.get(path).await.status, StatusCode::FORBIDDEN, "{path}");
    }
    assert_eq!(s1.post("/api/v1/admin/problems", ab_problem("evil", "x")).await.status, StatusCode::FORBIDDEN);
    assert_eq!(s1.put("/api/v1/admin/site/active-problem-set", json!({"problem_set": "perm-set"})).await.status, StatusCode::FORBIDDEN);

    // Cookie writes without the CSRF token are rejected, even for owners.
    let mut no_csrf = app.login("owner", "ownerpass123").await.without_csrf();
    assert_eq!(no_csrf.post("/api/v1/admin/problems", ab_problem("csrf-x", "x")).await.status, StatusCode::FORBIDDEN);
    // Foreign Origin is rejected even with the token.
    let r = owner.req_h("POST", "/api/v1/admin/problems", Some(ab_problem("origin-x", "x")), &[("origin", "https://evil.example")]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Least-privilege tokens.
    let read_only = owner.token(&["problems:read"]).await;
    let mut ro = app.with_token(&read_only);
    assert_eq!(ro.get("/api/v1/admin/problems").await.status, StatusCode::OK);
    let r = ro.post("/api/v1/admin/problems", ab_problem("ro-x", "x")).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.body["detail"].as_str().unwrap().contains("problems:write"));
    assert_eq!(ro.post("/api/v1/admin/problems/perm-ab/publish", json!({})).await.status, StatusCode::FORBIDDEN);
    // Tokens can never mint tokens or read student data without the scope.
    assert_eq!(ro.post("/api/v1/admin/tokens", json!({"name": "x", "scopes": ["publish"]})).await.status, StatusCode::FORBIDDEN);
    assert_eq!(ro.get("/api/v1/admin/users").await.status, StatusCode::FORBIDDEN);

    // Revoked tokens stop working immediately.
    let tokens = owner.get("/api/v1/admin/tokens").await;
    let tid = tokens.body[0]["id"].as_str().unwrap().to_string();
    assert_eq!(owner.req("DELETE", &format!("/api/v1/admin/tokens/{tid}"), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(ro.get("/api/v1/admin/problems").await.status, StatusCode::UNAUTHORIZED);
    // Token secrets are not stored in plain text.
    let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT token_hash FROM api_tokens").fetch_all(&app.state.db).await.unwrap();
    assert!(stored.iter().all(|h| h.len() == 32 && !String::from_utf8_lossy(h).contains("ccal_")));

    // Students cannot read other students' submissions (indistinguishable from missing).
    let sub = s1.submit(&pid, "run", AB_REF).await;
    let id = sub["id"].as_str().unwrap();
    let mut s2 = app.student("s2").await;
    assert_eq!(s2.get(&format!("/api/v1/submissions/{id}")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s2.post(&format!("/api/v1/submissions/{id}/rejudge"), json!({})).await.status, StatusCode::NOT_FOUND);
    // Owners can, and see full detail.
    let r = owner.get(&format!("/api/v1/submissions/{id}")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body["assembled_source"].is_string());
}

#[tokio::test]
async fn hidden_tests_never_reach_students() {
    let app = app().await;
    let mut owner = app.owner().await;
    let (pid, _) = published_problem_in_set(&mut owner, "leak-ab", "leak-set").await;
    let mut s = app.student("s1").await;

    let view = s.get("/api/v1/problems/leak-ab").await;
    assert_eq!(view.status, StatusCode::OK);
    for secret in [HIDDEN_IN_A, HIDDEN_IN_B, HIDDEN_OUT, "SECRET-NAME-77", "reference_solution", "scanf(\\\"%d %d\\\", &a, &b)"] {
        assert!(!view.text.contains(secret), "problem view leaked {secret}");
    }
    assert_eq!(view.body["hidden_test_count"], 1);

    // A program that echoes its input: wrong on the hidden test.
    let echo = "#include <stdio.h>\nint main(void) {\n    int a, b;\n    scanf(\"%d %d\", &a, &b);\n    printf(\"%d %d\\n\", a, b);\n    fprintf(stderr, \"debug %d\\n\", a);\n    return 0;\n}\n";
    let r = s.submit(&pid, "submit", echo).await;
    assert_eq!(r["verdict"], "wrong_answer");
    let text = r.to_string();
    for secret in [HIDDEN_IN_A, HIDDEN_IN_B, HIDDEN_OUT, "SECRET-NAME-77"] {
        assert!(!text.contains(secret), "submission view leaked {secret}: {text}");
    }
    let hidden = r["tests"].as_array().unwrap().iter().find(|t| t["hidden"] == true).unwrap();
    assert!(
        hidden["input"].is_null()
            && hidden["expected_output"].is_null()
            && hidden["actual_output"].is_null()
            && hidden["stderr"].is_null()
            && hidden["diff"].is_null()
    );
    // History and progress endpoints don't leak either.
    let hist = s.get("/api/v1/me/submissions").await;
    assert!(!hist.text.contains(HIDDEN_OUT));
    // A run (public tests only) never executes hidden tests.
    let run = s.submit(&pid, "run", echo).await;
    assert!(run["tests"].as_array().unwrap().iter().all(|t| t["hidden"] == false));

    // The owner sees everything for the same submission.
    let id = r["id"].as_str().unwrap();
    let full = owner.get(&format!("/api/v1/admin/submissions/{id}")).await;
    assert!(full.text.contains(HIDDEN_OUT));
}

#[tokio::test]
async fn real_judge_verdicts_and_teaching_cards() {
    let app = app().await;
    let mut owner = app.owner().await;
    let (pid, _) = published_problem_in_set(&mut owner, "verdict-ab", "verdict-set").await;
    let mut s = app.student("s1").await;

    let ac = s.submit(&pid, "submit", AB_REF).await;
    assert_eq!(ac["verdict"], "accepted", "{ac}");
    assert_eq!(ac["passed"], 2);

    let wa = s.submit(&pid, "run", "#include <stdio.h>\nint main(void){int a,b;scanf(\"%d %d\",&a,&b);printf(\"%d\\n\",a-b);return 0;}\n").await;
    assert_eq!(wa["verdict"], "wrong_answer");
    let t = &wa["tests"][0];
    assert_eq!(t["expected_output"], "3\n");
    assert_eq!(t["actual_output"], "-1\n");
    assert!(t["diff"].as_array().unwrap().iter().any(|d| d["kind"] == "missing" && d["text"] == "3↵"));
    assert!(wa["compare_description"].as_str().unwrap().contains("比對方式"));

    let ce = s.submit(&pid, "run", "#include <stdio.h>\nint main(void) {\n    int a = 1\n    printf(\"%d\\n\", a);\n    return 0;\n}\n").await;
    assert_eq!(ce["verdict"], "compile_error");
    let card = &ce["cards"][0];
    assert_eq!(card["rule"], "syntax.missing_semicolon");
    assert_eq!(card["primary"], true);
    assert!(card["where_text"].as_str().unwrap().contains("第 3 行"));
    assert!(card["raw"][0]["message"].as_str().unwrap().contains("expected"));
    // Hints are progressive: none shown until requested (site default).
    assert_eq!(card["hints"].as_array().unwrap().len(), 0);
    let ce_id = ce["id"].as_str().unwrap();
    let fid = card["finding_id"].as_u64().unwrap();
    let h = s.post(&format!("/api/v1/submissions/{ce_id}/findings/{fid}/hints"), json!({})).await;
    assert_eq!(h.status, StatusCode::OK, "{}", h.text);
    assert_eq!(h.body["hints"].as_array().unwrap().len(), 1);
    let again = s.get(&format!("/api/v1/submissions/{ce_id}")).await;
    assert_eq!(again.body["cards"][0]["hints"].as_array().unwrap().len(), 1, "revealed hints persist");

    let re = s.submit(&pid, "run", "int main(void){int *p = 0; return *p;}\n").await;
    assert_eq!(re["verdict"], "runtime_error");
    assert!(re["cards"].as_array().unwrap().iter().any(|c| c["rule"] == "run.segfault" && c["confidence"] == "possible"));

    let tle = s.submit(&pid, "run", "int main(void){ volatile unsigned long x = 0; for(;;) x++; }\n").await;
    assert_eq!(tle["verdict"], "time_limit");
    assert!(tle["cards"].as_array().unwrap().iter().any(|c| c["rule"] == "run.time_limit"));

    let warn = s.submit(&pid, "run", "#include <stdio.h>\nint main(void){int a,b;scanf(\"%d %d\",&a,b);printf(\"%d\\n\",a+b);return 0;}\n").await;
    assert!(warn["compile_success"] == true, "warnings must not be a compile failure");
    assert!(warn["cards"].as_array().unwrap().iter().any(|c| c["rule"] == "format.scanf_missing_address"));

    // Custom stdin run: output only.
    let r = s.post("/api/v1/submissions", json!({"problem_id": pid, "kind": "run", "code": AB_REF, "stdin": "20 22\n"})).await;
    let custom = s.wait(r.body["id"].as_str().unwrap()).await;
    assert_eq!(custom["verdict"], "completed");
    assert_eq!(custom["tests"][0]["actual_output"], "42\n");
}

#[tokio::test]
async fn editing_a_published_problem_keeps_old_submissions_traceable() {
    let app = app().await;
    let mut owner = app.owner().await;
    let (pid, _) = published_problem_in_set(&mut owner, "trace-ab", "trace-set").await;
    let mut s = app.student("s1").await;
    let first = s.submit(&pid, "submit", AB_REF).await;
    assert_eq!(first["revision_number"], 1);

    // Owner changes the expected output of the public test (now "sum = 3").
    let mut p = ab_problem("trace-ab", "SECRET-NAME-77");
    p["content"]["tests"][0]["expected_output"] = json!("sum = 3\n");
    p["content"]["tests"][1]["expected_output"] = json!(format!("sum = {HIDDEN_OUT}\n"));
    p["content"]["reference_solution"] = json!(AB_REF.replace("printf(\"%d\\n\"", "printf(\"sum = %d\\n\""));
    // Stale If-Match is refused.
    let r = owner.req_h("PUT", "/api/v1/admin/problems/trace-ab", Some(json!({"content": p["content"]})), &[("if-match", "7")]).await;
    assert_eq!(r.status, StatusCode::PRECONDITION_FAILED);
    let r = owner.req_h("PUT", "/api/v1/admin/problems/trace-ab", Some(json!({"content": p["content"]})), &[("if-match", "1")]).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.body["head_revision"]["number"], 2);
    assert_eq!(r.body["has_unpublished_changes"], true);
    // Same content again: no new revision.
    let r = owner.put("/api/v1/admin/problems/trace-ab", json!({"content": p["content"]})).await;
    assert_eq!(r.body["revision_created"], false);

    // Students still get revision 1 until it is published.
    assert_eq!(s.get("/api/v1/problems/trace-ab").await.body["revision_number"], 1);
    assert_eq!(owner.post("/api/v1/admin/problems/trace-ab/publish", json!({})).await.status, StatusCode::OK);
    assert_eq!(s.get("/api/v1/problems/trace-ab").await.body["revision_number"], 2);

    // The old submission still shows revision 1 and its original expected output.
    let id = first["id"].as_str().unwrap();
    let old = s.get(&format!("/api/v1/submissions/{id}")).await;
    assert_eq!(old.body["revision_number"], 1);
    assert_eq!(old.body["verdict"], "accepted");
    assert_eq!(old.body["tests"][0]["expected_output"], "3\n");
    let rev1 = owner.get("/api/v1/admin/problems/trace-ab/revisions/1").await;
    assert_eq!(rev1.body["content"]["tests"][0]["expected_output"], "3\n");
    let revs = owner.get("/api/v1/admin/problems/trace-ab/revisions").await;
    assert_eq!(revs.body.as_array().unwrap().len(), 2);
    assert_eq!(revs.body[1]["submission_count"], 1);

    // New submissions use revision 2 (old program is now wrong).
    let second = s.submit(&pid, "submit", AB_REF).await;
    assert_eq!(second["revision_number"], 2);
    assert_eq!(second["verdict"], "wrong_answer");
}

#[tokio::test]
async fn problems_are_shared_between_sets_and_removal_is_local() {
    let app = app().await;
    let mut owner = app.owner().await;
    let (_pid, _) = published_problem_in_set(&mut owner, "shared-ab", "set-a").await;
    let r = owner.post("/api/v1/admin/problem-sets", json!({"slug": "set-b", "title": "B", "problems": ["shared-ab"]})).await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(owner.post("/api/v1/admin/problem-sets/set-b/publish", json!({})).await.status, StatusCode::OK);
    // Archiving a problem used by sets is refused.
    assert_eq!(owner.req("DELETE", "/api/v1/admin/problems/shared-ab", None).await.status, StatusCode::CONFLICT);

    let r = owner.put("/api/v1/admin/problem-sets/set-a/items", json!({"problems": []})).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body["items"].as_array().unwrap().is_empty());
    let b = owner.get("/api/v1/admin/problem-sets/set-b").await;
    assert_eq!(b.body["items"][0]["slug"], "shared-ab");
    let p = owner.get("/api/v1/admin/problems/shared-ab").await;
    assert_eq!(p.body["status"], "published");
    assert_eq!(p.body["sets"].as_array().unwrap().len(), 1);
    // Students still reach it through set-b.
    let mut s = app.student("s1").await;
    assert_eq!(s.get("/api/v1/problems/shared-ab").await.status, StatusCode::OK);
    // Unknown problem in an item list is reported with a pointer.
    let r = owner.put("/api/v1/admin/problem-sets/set-b/items", json!({"problems": ["shared-ab", "nope-nope"]})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["errors"][0]["pointer"], "/problems/1");
}

#[tokio::test]
async fn drafts_survive_set_switches_and_in_flight_work_keeps_its_revision() {
    let app = app().await;
    let mut owner = app.owner().await;
    let (pid, set_a) = published_problem_in_set(&mut owner, "switch-ab", "switch-a").await;
    let r = owner.post("/api/v1/admin/problem-sets", json!({"slug": "switch-b", "title": "B", "problems": ["switch-ab"]})).await;
    let set_b = r.body["id"].as_str().unwrap().to_string();
    owner.post("/api/v1/admin/problem-sets/switch-b/publish", json!({})).await;
    owner.put("/api/v1/admin/site/active-problem-set", json!({"problem_set": "switch-a"})).await;

    let mut s = app.student("s1").await;
    assert_eq!(s.get("/api/v1/site").await.body["default_set_id"], set_a);
    let d = s.put(&format!("/api/v1/me/drafts/{pid}"), json!({"code": "// my work in progress\n"})).await;
    assert_eq!(d.status, StatusCode::OK);
    let v = d.body["version"].as_i64().unwrap();
    // Stale writes are refused with the stored draft attached.
    let stale = s.put(&format!("/api/v1/me/drafts/{pid}"), json!({"code": "other tab", "base_version": v - 1})).await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(stale.body["data"]["code"], "// my work in progress\n");

    // Queue a submission, then switch the active set while it is pending.
    let r = s.post("/api/v1/submissions", json!({"problem_id": pid, "set_id": set_a, "kind": "submit", "code": AB_REF})).await;
    let sub_id = r.body["id"].as_str().unwrap().to_string();
    let before = s.get("/api/v1/site").await.body["config_version"].as_i64().unwrap();
    let r = owner.put("/api/v1/admin/site/active-problem-set", json!({"problem_set": "switch-b"})).await;
    assert_eq!(r.status, StatusCode::OK);
    let site = s.get("/api/v1/site").await.body;
    assert_eq!(site["default_set_id"], set_b, "switch applies without restart");
    assert!(site["config_version"].as_i64().unwrap() > before);

    // Draft is intact, and the submission is judged against its own revision/set.
    assert_eq!(s.get(&format!("/api/v1/me/drafts/{pid}")).await.body["code"], "// my work in progress\n");
    let done = s.wait(&sub_id).await;
    assert_eq!(done["verdict"], "accepted");
    assert_eq!(done["set_id"], set_a);
    assert_eq!(done["revision_number"], 1);
}

#[tokio::test]
async fn fill_in_fixed_code_cannot_be_bypassed() {
    let app = app().await;
    let mut owner = app.owner().await;
    let template = "#include <stdio.h>\n\nint twice(int x) {\n/*@blank body@*/    return 0;\n/*@end@*/}\n\nint main(void) {\n    int n;\n    scanf(\"%d\", &n);\n    printf(\"%d\\n\", twice(n));\n    return 0;\n}\n";
    let r = owner
        .post(
            "/api/v1/admin/problems",
            json!({"slug": "fill-twice", "content": {
                "title": "Twice", "statement_md": "回傳兩倍", "mode": "fill_in", "template": template,
                "reference_blanks": {"body": "    return x * 2;\n"},
                "tests": [{"visibility": "public", "input": "2\n", "expected_output": "4\n"}, {"visibility": "hidden", "input": "21\n", "expected_output": "42\n"}]
            }}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    let pid = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(owner.post("/api/v1/admin/problems/fill-twice/publish", json!({})).await.status, StatusCode::OK);
    owner.post("/api/v1/admin/problem-sets", json!({"slug": "fill-set", "title": "F", "problems": ["fill-twice"]})).await;
    owner.post("/api/v1/admin/problem-sets/fill-set/publish", json!({})).await;

    let mut s = app.student("s1").await;
    let view = s.get("/api/v1/problems/fill-twice").await;
    assert_eq!(view.body["segments"][1]["kind"], "blank");

    // Sending a whole program (with a rewritten main) is refused.
    let cheat = "#include <stdio.h>\nint main(void){ printf(\"42\\n\"); return 0; }\n";
    let r = s.post("/api/v1/submissions", json!({"problem_id": pid, "kind": "submit", "code": cheat})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["errors"][0]["pointer"], "/code");
    // Unknown blanks and marker injection are refused.
    let r = s.post("/api/v1/submissions", json!({"problem_id": pid, "kind": "submit", "blanks": {"body": "return 1;", "main": "x"}})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = s
        .post(
            "/api/v1/submissions",
            json!({"problem_id": pid, "kind": "submit", "blanks": {"body": "return 1;\n/*@end@*/}\nint main(void){return 0;}\n/*@blank x@*/"}}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    // Closing the function early inside the blank still compiles against the
    // server's fixed code, so the fixed main() always runs.
    let r = s.post("/api/v1/submissions", json!({"problem_id": pid, "kind": "submit", "blanks": {"body": "    return x * 2;\n"}})).await;
    let done = s.wait(r.body["id"].as_str().unwrap()).await;
    assert_eq!(done["verdict"], "accepted");
    let row: (String,) = sqlx::query_as("SELECT assembled_source FROM submissions WHERE id = $1::uuid")
        .bind(done["id"].as_str().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(row.0.contains("printf(\"%d\\n\", twice(n));"), "fixed code comes from the server template");

    // Diagnostics reported inside fixed code are flagged as such, and line
    // numbers match the editor (the student sees the full template).
    let r = s.post("/api/v1/submissions", json!({"problem_id": pid, "kind": "run", "blanks": {"body": "    int y = x * 2;\n"}})).await;
    let done = s.wait(r.body["id"].as_str().unwrap()).await;
    let cards = done["cards"].as_array().unwrap();
    let missing_return = cards.iter().find(|c| c["rule"] == "func.missing_return").expect("missing return card");
    assert_eq!(missing_return["span"]["line"], 5, "GCC reports it at the fixed closing brace");
    assert_eq!(missing_return["in_fixed_region"], true);
    assert!(missing_return["fixed_region_note"].as_str().unwrap().contains("你填寫的區塊"));
    let unused = cards.iter().find(|c| c["rule"] == "warn.unused_variable").expect("unused variable card");
    assert_eq!(unused["span"]["line"], 4);
    assert_eq!(unused["in_fixed_region"], false);
}

#[tokio::test]
async fn import_is_validated_idempotent_and_all_or_nothing() {
    let app = app().await;
    let mut owner = app.owner().await;
    let token = owner.token(&["problems:read", "problems:write", "sets:write"]).await;
    let mut agent = app.with_token(&token);
    let doc = json!({
        "format_version": "cycu-cal.import/v1",
        "problems": [ab_problem("imp-ab", "h1"), ab_problem("imp-ab2", "h2")],
        "problem_sets": [{"slug": "imp-set", "title": "Imported", "problems": ["imp-ab", "imp-ab2"]}]
    });

    // Dry run writes nothing and reports actions.
    let r = agent.post("/api/v1/admin/import?dry_run=true", doc.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.body["problems"][0]["action"], "create");
    assert!(agent.get("/api/v1/admin/problems/imp-ab").await.status == StatusCode::NOT_FOUND);

    // Errors carry precise pointers; nothing is applied.
    let mut bad = doc.clone();
    bad["problems"][1]["content"]["limits"] = json!({"time_ms": 1, "memory_kb": 65536, "output_kb": 64});
    bad["problem_sets"][0]["problems"] = json!(["imp-ab", "missing-one"]);
    let r = agent.post("/api/v1/admin/import", bad).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["code"], "validation_failed", "import errors are problem+json");
    assert_eq!(r.body["data"]["ok"], false, "full report in `data`");
    let ptrs: Vec<&str> = r.body["errors"].as_array().unwrap().iter().map(|e| e["pointer"].as_str().unwrap()).collect();
    assert!(ptrs.contains(&"/problems/1/content/limits/time_ms"), "{ptrs:?}");
    assert!(ptrs.contains(&"/problem_sets/0/problems/1"), "{ptrs:?}");
    assert_eq!(agent.get("/api/v1/admin/problems/imp-ab").await.status, StatusCode::NOT_FOUND, "all-or-nothing");

    // Real import, then a retry (with and without Idempotency-Key): no duplicates.
    let r = agent.req_h("POST", "/api/v1/admin/import", Some(doc.clone()), &[("idempotency-key", "imp-1")]).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let replay = agent.req_h("POST", "/api/v1/admin/import", Some(doc.clone()), &[("idempotency-key", "imp-1")]).await;
    assert_eq!(replay.body, r.body, "same key replays the original response");
    let again = agent.post("/api/v1/admin/import", doc.clone()).await;
    assert!(again.body["problems"].as_array().unwrap().iter().all(|p| p["action"] == "unchanged"));
    assert_eq!(again.body["problem_sets"][0]["action"], "unchanged");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM problems").fetch_one(&app.state.db).await.unwrap();
    assert_eq!(count, 2);
    let revs: i64 = sqlx::query_scalar("SELECT count(*) FROM problem_revisions").fetch_one(&app.state.db).await.unwrap();
    assert_eq!(revs, 2);
    // Reusing the key with a different body is refused.
    let mut changed = doc.clone();
    changed["problems"][0]["content"]["title"] = json!("A+B v2");
    let r = agent.req_h("POST", "/api/v1/admin/import", Some(changed.clone()), &[("idempotency-key", "imp-1")]).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    // Changed content → new revision, still a draft (import never publishes).
    let r = agent.post("/api/v1/admin/import", changed).await;
    assert_eq!(r.body["problems"][0]["action"], "update");
    let p = agent.get("/api/v1/admin/problems/imp-ab").await;
    assert_eq!(p.body["head_revision"]["number"], 2);
    assert_eq!(p.body["status"], "draft");
    let set = agent.get("/api/v1/admin/problem-sets/imp-set").await;
    assert_eq!(set.body["status"], "draft");
    // Draft content is invisible to students.
    let mut s = app.student("s1").await;
    assert_eq!(s.get("/api/v1/problems/imp-ab").await.status, StatusCode::NOT_FOUND);
    assert!(s.get("/api/v1/problem-sets").await.body.as_array().unwrap().is_empty());

    // Export round-trips through import as a no-op.
    let exported = agent.get("/api/v1/admin/export").await.body;
    let r = agent.post("/api/v1/admin/import?dry_run=true", exported).await;
    assert!(r.body["problems"].as_array().unwrap().iter().all(|p| p["action"] == "unchanged"), "{}", r.text);

    // Retried problem creation with Idempotency-Key does not duplicate.
    let body = ab_problem("idem-ab", "x");
    let a = agent.req_h("POST", "/api/v1/admin/problems", Some(body.clone()), &[("idempotency-key", "k-1")]).await;
    let b = agent.req_h("POST", "/api/v1/admin/problems", Some(body), &[("idempotency-key", "k-1")]).await;
    assert_eq!(a.status, StatusCode::CREATED);
    assert_eq!(b.status, StatusCode::CREATED);
    assert_eq!(a.body["id"], b.body["id"]);
}

#[tokio::test]
async fn publishing_requires_a_passing_reference_solution() {
    let app = app().await;
    let mut owner = app.owner().await;
    let mut p = ab_problem("bad-ref", "x");
    p["content"]["tests"][1]["expected_output"] = json!("999\n");
    owner.post("/api/v1/admin/problems", p).await;
    let r = owner.post("/api/v1/admin/problems/bad-ref/publish", json!({})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.text);
    assert_eq!(r.body["errors"][0]["code"], "reference_failed");
    // Sets cannot be published with unpublished problems.
    owner.post("/api/v1/admin/problem-sets", json!({"slug": "bad-set", "title": "x", "problems": ["bad-ref"]})).await;
    let r = owner.post("/api/v1/admin/problem-sets/bad-set/publish", json!({})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["errors"][0]["code"], "problem_not_published");
    // Only published sets can be activated.
    let r = owner.put("/api/v1/admin/site/active-problem-set", json!({"problem_set": "bad-set"})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn submission_retries_are_idempotent() {
    let app = app().await;
    let mut owner = app.owner().await;
    let (pid, _) = published_problem_in_set(&mut owner, "idem-sub", "idem-set").await;
    let mut s = app.student("s1").await;
    let body = json!({"problem_id": pid, "kind": "submit", "code": AB_REF});
    let a = s.req_h("POST", "/api/v1/submissions", Some(body.clone()), &[("idempotency-key", "same-key")]).await;
    let b = s.req_h("POST", "/api/v1/submissions", Some(body), &[("idempotency-key", "same-key")]).await;
    assert_eq!(a.status, StatusCode::ACCEPTED);
    assert_eq!(b.status, StatusCode::OK);
    assert_eq!(a.body["id"], b.body["id"]);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM submissions WHERE kind = 'submit'").fetch_one(&app.state.db).await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn online_teaching_text_and_policies_apply_without_restart() {
    let app = app().await;
    let mut owner = app.owner().await;
    let (pid, set_id) = published_problem_in_set(&mut owner, "text-ab", "text-set").await;
    let mut s = app.student("s1").await;
    let ce = s.submit(&pid, "run", "#include <stdio.h>\nint main(void) {\n    int a = 1\n    printf(\"%d\\n\", a);\n    return 0;\n}\n").await;
    let id = ce["id"].as_str().unwrap().to_string();
    assert_eq!(ce["cards"][0]["title"], "指令結尾可能少了分號");

    // Invalid template variables are rejected with a pointer.
    let mut entry = owner.get("/api/v1/admin/catalog/rules/syntax.missing_semicolon").await.body["published"].clone();
    entry["hints"][0] = json!("看第 {lien} 行");
    let r = owner.put("/api/v1/admin/catalog/rules/syntax.missing_semicolon", entry.clone()).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.body["errors"][0]["pointer"], "/hints/0");

    entry["hints"][0] = json!("看第 {line} 行附近");
    entry["title"] = json!("（新版）這裡可能少了分號");
    let r = owner.put("/api/v1/admin/catalog/rules/syntax.missing_semicolon", entry).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    // Draft: students still see the old text; preview shows the new one.
    assert_eq!(s.get(&format!("/api/v1/submissions/{id}")).await.body["cards"][0]["title"], "指令結尾可能少了分號");
    let pv = owner.post("/api/v1/admin/catalog/preview", json!({"submission_id": id})).await;
    assert_eq!(pv.body["cards"][0]["title"], "（新版）這裡可能少了分號");
    let r = owner.post("/api/v1/admin/catalog/publish", json!({})).await;
    assert_eq!(r.status, StatusCode::OK);
    // Published: the same old submission now shows the new wording.
    assert_eq!(s.get(&format!("/api/v1/submissions/{id}")).await.body["cards"][0]["title"], "（新版）這裡可能少了分號");
    let hist = owner.get("/api/v1/admin/catalog/history").await;
    assert_eq!(hist.body[0]["key"], "rule:syntax.missing_semicolon");

    // Cohort concepts change the wording of the scanf card.
    let warn = s.submit(&pid, "run", "#include <stdio.h>\nint main(void){int a,b;scanf(\"%d %d\",&a,b);printf(\"%d\\n\",a+b);return 0;}\n").await;
    let wid = warn["id"].as_str().unwrap().to_string();
    let cause = |v: &serde_json::Value| {
        v["cards"].as_array().unwrap().iter().find(|c| c["rule"] == "format.scanf_missing_address").unwrap()["cause"].as_str().unwrap().to_string()
    };
    assert!(!cause(&warn).contains("指標"));
    let cohort = owner.post("/api/v1/admin/cohorts", json!({"name": "進階班", "policy": {"learned_concepts": ["pointers"], "auto_reveal_hints": 1}})).await;
    assert_eq!(cohort.status, StatusCode::CREATED, "{}", cohort.text);
    let s1_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = 's1'").fetch_one(&app.state.db).await.unwrap();
    let r = owner.req("PATCH", &format!("/api/v1/admin/users/{s1_id}"), Some(json!({"cohort_id": cohort.body["id"]}))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let again = s.get(&format!("/api/v1/submissions/{wid}")).await.body;
    assert!(cause(&again).contains("指標"), "{}", cause(&again));
    assert_eq!(again["cards"][0]["hints"].as_array().unwrap().len(), 1, "cohort auto-reveals one hint");
    let pol = owner.get(&format!("/api/v1/admin/users/{s1_id}/policy")).await;
    assert_eq!(pol.body["effective"]["auto_reveal_hints"]["source"], "cohort");

    // Visibility: a cohort restricted to another set, plus a personal extra set.
    let r = owner.post("/api/v1/admin/problem-sets", json!({"slug": "secret-set", "title": "S", "listed": false, "problems": ["text-ab"]})).await;
    let secret_id = r.body["id"].as_str().unwrap().to_string();
    owner.post("/api/v1/admin/problem-sets/secret-set/publish", json!({})).await;
    let mut s2 = app.student("s2").await;
    let slugs = |v: serde_json::Value| v.as_array().unwrap().iter().map(|x| x["slug"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(slugs(s2.get("/api/v1/problem-sets").await.body), vec!["text-set"], "unlisted sets are hidden");
    let s2_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = 's2'").fetch_one(&app.state.db).await.unwrap();
    owner.put(&format!("/api/v1/admin/users/{s2_id}/policy"), json!({"extra_set_ids": [secret_id]})).await;
    assert_eq!(slugs(s2.get("/api/v1/problem-sets").await.body), vec!["text-set", "secret-set"]);
    assert_eq!(slugs(s.get("/api/v1/problem-sets").await.body), vec!["text-set"], "only s2 got the extra set");
    let _ = set_id;
}

#[tokio::test]
async fn auth_flows_password_change_and_session_revocation() {
    let app = app().await;
    let _owner = app.owner().await;
    let mut a = app.student("s1").await;
    let mut b = app.login("s1", "studentpass1").await;
    // Wrong password and unknown user give the same answer.
    let mut x = app.anon();
    let r1 = x.post("/api/v1/auth/login", json!({"username": "s1", "password": "nope-nope"})).await;
    let r2 = x.post("/api/v1/auth/login", json!({"username": "ghost", "password": "nope-nope"})).await;
    assert_eq!(r1.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r1.body["detail"], r2.body["detail"]);
    // Weak new password rejected with pointer.
    let r = a.post("/api/v1/auth/password", json!({"current_password": "studentpass1", "new_password": "short"})).await;
    assert_eq!(r.body["errors"][0]["pointer"], "/new_password");
    let r = a.post("/api/v1/auth/password", json!({"current_password": "studentpass1", "new_password": "a-much-better-one"})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    // Other sessions are signed out; the current one stays.
    assert_eq!(b.get("/api/v1/auth/me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(a.get("/api/v1/auth/me").await.status, StatusCode::OK);
    let r = a.post("/api/v1/auth/logout", json!({})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(a.get("/api/v1/auth/me").await.status, StatusCode::UNAUTHORIZED);
    app.login("s1", "a-much-better-one").await;
    // Passwords are stored as Argon2id hashes.
    let h: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 's1'").fetch_one(&app.state.db).await.unwrap();
    assert!(h.starts_with("$argon2id$"));
}

#[tokio::test]
async fn openapi_document_matches_committed_copy() {
    let generated = ccal_api::app::openapi_json();
    let committed = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/openapi.json")).expect("docs/openapi.json exists");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&generated).unwrap(),
        serde_json::from_str::<serde_json::Value>(&committed).unwrap(),
        "docs/openapi.json is stale: run `cyculcal openapi > docs/openapi.json`"
    );
}
