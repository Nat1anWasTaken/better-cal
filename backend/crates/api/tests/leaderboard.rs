//! Leaderboard rounds against a real PostgreSQL. Judged submissions are
//! seeded directly, so the judge-runner is not needed for scoring.

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};

fn row<'a>(board: &'a Value, name: &str) -> Option<&'a Value> {
    board["rows"].as_array().unwrap().iter().find(|r| r["name"] == name)
}

fn badges(row: &Value) -> Vec<(String, String)> {
    row["badges"].as_array().unwrap().iter().map(|b| (b["kind"].as_str().unwrap().into(), b["problem_id"].as_str().unwrap().into())).collect()
}

#[tokio::test]
async fn rounds_rank_freeze_and_respect_policy() {
    let app = app().await;
    let mut owner = app.owner().await;
    let mut ids = vec![];
    for slug in ["lb-a", "lb-b"] {
        let r = owner.post("/api/v1/admin/problems", ab_problem(slug, "SECRET")).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
        ids.push(r.body["id"].as_str().unwrap().to_string());
        assert_eq!(owner.post(&format!("/api/v1/admin/problems/{slug}/publish"), json!({})).await.status, StatusCode::OK);
    }
    let (pa, pb) = (ids[0].clone(), ids[1].clone());
    for (slug, problems) in [("lb-set", vec!["lb-a", "lb-b"]), ("lb-other", vec!["lb-a"])] {
        let r = owner.post("/api/v1/admin/problem-sets", json!({"slug": slug, "title": "排行測試", "problems": problems})).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
        assert_eq!(owner.post(&format!("/api/v1/admin/problem-sets/{slug}/publish"), json!({})).await.status, StatusCode::OK);
    }

    let mut s1 = app.student("s1").await;
    let mut s2 = app.student("s2").await;
    let mut hidden = app.student("hidden").await;
    let mut blind = app.student("blind").await;
    for (name, policy) in [("off", json!({"leaderboard": false})), ("other-only", json!({"visible_set_ids": []}))] {
        let c = owner.post("/api/v1/admin/cohorts", json!({"name": name, "policy": policy})).await;
        assert_eq!(c.status, StatusCode::CREATED, "{}", c.text);
        let user = if name == "off" { "hidden" } else { "blind" };
        let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1").bind(user).fetch_one(&app.state.db).await.unwrap();
        let r = owner.req("PATCH", &format!("/api/v1/admin/users/{uid}"), Some(json!({"cohort_id": c.body["id"]}))).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    }

    // No round yet.
    assert_eq!(s1.get("/api/v1/leaderboard").await.status, StatusCode::NOT_FOUND);
    let site = s1.get("/api/v1/site").await.body;
    assert_eq!((site["leaderboard"].as_bool(), site["open_round_id"].is_null()), (Some(true), true));
    assert_eq!(hidden.get("/api/v1/site").await.body["leaderboard"], false);

    // Activating without start_round doesn't start one; with it, it does.
    let r = owner.put("/api/v1/admin/site/active-problem-set", json!({"problem_set": "lb-set"})).await;
    assert!(r.body["round_id"].is_null(), "{}", r.text);
    let r = owner.put("/api/v1/admin/site/active-problem-set", json!({"problem_set": "lb-set", "start_round": {"title": "第一週"}})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let round1 = r.body["round_id"].as_str().unwrap().to_string();
    // Backdate the start so seeded submits (minutes after it) are in the past.
    sqlx::query("UPDATE rounds SET started_at = now() - interval '2 hours'").execute(&app.state.db).await.unwrap();
    assert_eq!(s1.get("/api/v1/site").await.body["open_round_id"], round1.as_str());

    // s1: A after a WA and a free CE at 10 min, B at 30 -> 2 solved, 10+20+30.
    app.seed_submission("s1", &pa, "submit", "wrong_answer", 2).await;
    app.seed_submission("s1", &pa, "submit", "compile_error", 3).await;
    app.seed_submission("s1", &pa, "submit", "accepted", 10).await;
    app.seed_submission("s1", &pb, "submit", "accepted", 30).await;
    // s2: A at 5 min; an accepted run on B doesn't count. It revealed a hint on A first.
    let sub = app.seed_submission("s2", &pa, "submit", "wrong_answer", 1).await;
    sqlx::query(
        "INSERT INTO hint_reveals (submission_id, finding_id, level, rule, revealed_at) SELECT id, 0, 1, 'x', created_at FROM submissions WHERE id = $1",
    )
    .bind(sub)
    .execute(&app.state.db)
    .await
    .unwrap();
    app.seed_submission("s2", &pa, "submit", "accepted", 5).await;
    app.seed_submission("s2", &pb, "run", "accepted", 1).await;
    // Students opted out by policy are never listed.
    app.seed_submission("hidden", &pa, "submit", "accepted", 1).await;

    let b = s1.get("/api/v1/leaderboard").await;
    assert_eq!(b.status, StatusCode::OK, "{}", b.text);
    let board = b.body;
    assert_eq!(board["round"]["title"], "第一週");
    assert_eq!(board["problems"].as_array().unwrap().len(), 2);
    assert_eq!(board["rows"].as_array().unwrap().len(), 2, "{board}");
    let (r1, r2) = (row(&board, "s1").unwrap(), row(&board, "s2").unwrap());
    assert_eq!((r1["rank"].as_u64(), r1["solved"].as_u64(), r1["penalty"].as_i64()), (Some(1), Some(2), Some(60)));
    assert_eq!((r2["rank"].as_u64(), r2["solved"].as_u64(), r2["penalty"].as_i64()), (Some(2), Some(1), Some(25)));
    assert_eq!(r1["cells"][0]["wrong_tries"], 1);
    assert!(badges(r2).contains(&("first_solve".into(), pa.clone())));
    assert!(!badges(r2).contains(&("no_hints".into(), pa.clone())), "s2 revealed a hint before solving");
    assert!(badges(r1).contains(&("first_solve".into(), pb.clone())));
    assert!(badges(r1).contains(&("no_hints".into(), pa.clone())));

    // Policy and visibility.
    assert_eq!(hidden.get("/api/v1/leaderboard").await.status, StatusCode::NOT_FOUND);
    assert_eq!(hidden.get("/api/v1/rounds").await.body, json!([]));
    assert_eq!(blind.get(&format!("/api/v1/rounds/{round1}/leaderboard")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(blind.get("/api/v1/rounds").await.body, json!([]));

    // Freeze: s2's later accept is pending for students, live for the owner.
    let r = owner.post(&format!("/api/v1/admin/rounds/{round1}/freeze"), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    app.seed_submission("s2", &pb, "submit", "accepted", 600).await;
    let frozen = s2.get("/api/v1/leaderboard").await.body;
    assert_eq!(frozen["frozen"], true);
    let r2 = row(&frozen, "s2").unwrap();
    assert_eq!((r2["solved"].as_u64(), r2["cells"][1]["pending"].as_u64()), (Some(1), Some(1)));
    let live = owner.get("/api/v1/leaderboard").await.body;
    assert_eq!((live["frozen"].as_bool(), row(&live, "s2").unwrap()["solved"].as_u64()), (Some(false), Some(2)));
    owner.post(&format!("/api/v1/admin/rounds/{round1}/reveal"), json!({})).await;
    assert_eq!(row(&s2.get("/api/v1/leaderboard").await.body, "s2").unwrap()["solved"], 2);

    // A new round ends the old one, whose board stays viewable.
    let r = owner.put("/api/v1/admin/site/active-problem-set", json!({"problem_set": "lb-other", "start_round": {}})).await;
    let round2 = r.body["round_id"].as_str().unwrap().to_string();
    let rounds = s1.get("/api/v1/rounds").await.body;
    assert_eq!(rounds.as_array().unwrap().len(), 2);
    assert_eq!(rounds[0]["title"], "排行測試", "title defaults to the set's title");
    assert!(!rounds[1]["ended_at"].is_null());
    assert_eq!(s1.get("/api/v1/leaderboard").await.body["round"]["id"], round2.as_str());
    assert_eq!(s1.get("/api/v1/leaderboard").await.body["rows"], json!([]));
    assert_eq!(row(&s1.get(&format!("/api/v1/rounds/{round1}/leaderboard")).await.body, "s1").unwrap()["rank"], 1);
    assert_eq!(owner.post(&format!("/api/v1/admin/rounds/{round1}/end"), json!({})).await.status, StatusCode::CONFLICT);
    assert_eq!(s1.post(&format!("/api/v1/admin/rounds/{round2}/freeze"), json!({})).await.status, StatusCode::FORBIDDEN);
    let r = owner.post(&format!("/api/v1/admin/rounds/{round2}/end"), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(s1.get("/api/v1/leaderboard").await.status, StatusCode::NOT_FOUND);
}
