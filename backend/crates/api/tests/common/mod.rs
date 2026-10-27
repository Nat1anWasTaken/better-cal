//! Test harness: a fresh PostgreSQL database per test, the real router
//! in-process, judge workers talking to the real judge-runner.
//!
//! Requires:
//!   TEST_DATABASE_URL  admin URL of a PostgreSQL server (a database per test is created)
//!   RUNNER_URL / RUNNER_TOKEN  a running judge-runner (docker compose ... up runner)
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use ccal_api::config::Config;
use ccal_api::state::AppState;

pub struct TestApp {
    pub router: Router,
    pub state: AppState,
}

pub struct Client<'a> {
    app: &'a TestApp,
    cookie: Option<String>,
    csrf: Option<String>,
    bearer: Option<String>,
}

pub struct Resp {
    pub status: StatusCode,
    pub body: Value,
    pub text: String,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

pub async fn app() -> TestApp {
    let admin_url = env_or("TEST_DATABASE_URL", "postgres://cyculcal:devpassword123@127.0.0.1:55432/cyculcal");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let db_name = format!("ccal_test_{now}_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let admin = sqlx::PgPool::connect(&admin_url).await.expect("TEST_DATABASE_URL must point to a reachable PostgreSQL server");
    // Drop databases left by earlier runs (older than an hour).
    let old: Vec<String> = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'ccal_test_%'").fetch_all(&admin).await.unwrap();
    for name in old {
        let ts: u64 = name.split('_').nth(2).and_then(|t| t.parse().ok()).unwrap_or(0);
        if ts + 3600 < now {
            let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).execute(&admin).await;
        }
    }
    sqlx::query(&format!("CREATE DATABASE {db_name}")).execute(&admin).await.unwrap();
    let base = admin_url.rsplit_once('/').unwrap().0;
    let cfg = Config {
        database_url: format!("{base}/{db_name}"),
        bind: "127.0.0.1:0".into(),
        runner_url: env_or("RUNNER_URL", "http://127.0.0.1:17070"),
        runner_token: env_or("RUNNER_TOKEN", "dev-runner-token-0123456789abcdef"),
        public_origins: vec!["http://app.test".into()],
        cookie_secure: false,
        static_dir: None,
        judge_workers: 2,
        session_ttl: std::time::Duration::from_secs(3600),
        runner_timeout: std::time::Duration::from_secs(60),
        max_attempts: 2,
        public_url: Some("http://app.test".into()),
        client_ip_header: None,
    };
    let state = ccal_api::init_state(cfg).await.unwrap();
    ccal_api::judge::worker::spawn_workers(&state);
    let router = ccal_api::app::build(state.clone());
    TestApp { router, state }
}

impl TestApp {
    pub fn anon(&self) -> Client<'_> {
        Client { app: self, cookie: None, csrf: None, bearer: None }
    }

    pub fn with_token(&self, token: &str) -> Client<'_> {
        Client { app: self, cookie: None, csrf: None, bearer: Some(token.to_string()) }
    }

    pub async fn create_user(&self, username: &str, role: &str, password: &str) -> uuid::Uuid {
        let id = uuid::Uuid::now_v7();
        sqlx::query("INSERT INTO users (id, username, role, password_hash) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(username)
            .bind(role)
            .bind(ccal_api::auth::password::hash(password).unwrap())
            .execute(&self.state.db)
            .await
            .unwrap();
        id
    }

    pub async fn login(&self, username: &str, password: &str) -> Client<'_> {
        let mut c = self.anon();
        let r = c.req("POST", "/api/v1/auth/login", Some(json!({"username": username, "password": password}))).await;
        assert_eq!(r.status, StatusCode::OK, "login failed: {}", r.text);
        c.csrf = r.body["csrf_token"].as_str().map(str::to_string);
        c
    }

    /// Insert an already-judged `kind` submission `minutes` after the open
    /// round started, without going through the runner.
    pub async fn seed_submission(&self, username: &str, problem_id: &str, kind: &str, verdict: &str, minutes: i32) -> uuid::Uuid {
        let id = uuid::Uuid::now_v7();
        let r = sqlx::query(
            "INSERT INTO submissions (id, user_id, problem_id, revision_id, kind, code, assembled_source, status, verdict, created_at, finished_at)
             SELECT $1, u.id, p.id, p.published_revision_id, $3, '', '', 'done', $4, r.started_at + make_interval(mins => $5), r.started_at + make_interval(mins => $5)
             FROM users u, problems p, rounds r WHERE u.username = $2 AND p.id = $6::uuid AND r.ended_at IS NULL",
        )
        .bind(id)
        .bind(username)
        .bind(kind)
        .bind(verdict)
        .bind(minutes)
        .bind(problem_id)
        .execute(&self.state.db)
        .await
        .unwrap();
        assert_eq!(r.rows_affected(), 1, "seed needs an open round, user {username} and problem {problem_id}");
        id
    }

    pub async fn owner(&self) -> Client<'_> {
        self.create_user("owner", "owner", "ownerpass123").await;
        self.login("owner", "ownerpass123").await
    }

    pub async fn student(&self, name: &str) -> Client<'_> {
        self.create_user(name, "student", "studentpass1").await;
        self.login(name, "studentpass1").await
    }
}

impl Client<'_> {
    pub fn without_csrf(mut self) -> Self {
        self.csrf = None;
        self
    }

    pub async fn req(&mut self, method: &str, path: &str, body: Option<Value>) -> Resp {
        self.req_h(method, path, body, &[]).await
    }

    pub async fn req_h(&mut self, method: &str, path: &str, body: Option<Value>, headers: &[(&str, &str)]) -> Resp {
        let mut b = Request::builder().method(method).uri(path).header("content-type", "application/json");
        if let Some(c) = &self.cookie {
            b = b.header("cookie", c);
        }
        if let Some(c) = &self.csrf {
            b = b.header("x-csrf-token", c);
        }
        if let Some(t) = &self.bearer {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let req = b.body(body.map(|v| Body::from(v.to_string())).unwrap_or_else(Body::empty)).unwrap();
        let res = self.app.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        if let Some(sc) = res.headers().get("set-cookie").and_then(|v| v.to_str().ok()) {
            let kv = sc.split(';').next().unwrap().to_string();
            self.cookie = if kv.ends_with('=') { None } else { Some(kv) };
        }
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).to_string();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Resp { status, body, text }
    }

    pub async fn get(&mut self, path: &str) -> Resp {
        self.req("GET", path, None).await
    }
    pub async fn post(&mut self, path: &str, body: Value) -> Resp {
        self.req("POST", path, Some(body)).await
    }
    pub async fn put(&mut self, path: &str, body: Value) -> Resp {
        self.req("PUT", path, Some(body)).await
    }

    /// Creates an API token (owner session only) and returns the secret.
    pub async fn token(&mut self, scopes: &[&str]) -> String {
        let r = self.post("/api/v1/admin/tokens", json!({"name": "test", "scopes": scopes})).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
        r.body["token"].as_str().unwrap().to_string()
    }

    /// Polls a submission until judged.
    pub async fn wait(&mut self, id: &str) -> Value {
        for _ in 0..240 {
            let r = self.get(&format!("/api/v1/submissions/{id}")).await;
            let st = r.body["status"].as_str().unwrap_or("");
            if st == "done" || st == "system_error" {
                return r.body;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        panic!("submission {id} was not judged in time (is the judge-runner up?)");
    }

    pub async fn submit(&mut self, problem_id: &str, kind: &str, code: &str) -> Value {
        let r = self.post("/api/v1/submissions", json!({"problem_id": problem_id, "kind": kind, "code": code})).await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.text);
        let id = r.body["id"].as_str().unwrap().to_string();
        self.wait(&id).await
    }
}

pub const AB_REF: &str =
    "#include <stdio.h>\nint main(void) {\n    int a, b;\n    scanf(\"%d %d\", &a, &b);\n    printf(\"%d\\n\", a + b);\n    return 0;\n}\n";

/// Distinctive values only present in the hidden test (for leak checks).
pub const HIDDEN_IN_A: &str = "1234567";
pub const HIDDEN_IN_B: &str = "7654321";
pub const HIDDEN_OUT: &str = "8888888";

pub fn ab_problem(slug: &str, hidden_name: &str) -> Value {
    json!({
        "slug": slug,
        "content": {
            "title": "A+B",
            "statement_md": "輸入兩個整數，輸出和。",
            "mode": "full",
            "concepts": ["io"],
            "reference_solution": AB_REF,
            "tests": [
                {"visibility": "public", "input": "1 2\n", "expected_output": "3\n"},
                {"visibility": "hidden", "input": format!("{HIDDEN_IN_A} {HIDDEN_IN_B}\n"), "expected_output": format!("{HIDDEN_OUT}\n"), "name": hidden_name}
            ]
        }
    })
}

/// Creates + publishes a problem and a published set containing it.
pub async fn published_problem_in_set(owner: &mut Client<'_>, slug: &str, set_slug: &str) -> (String, String) {
    let r = owner.post("/api/v1/admin/problems", ab_problem(slug, "SECRET-NAME-77")).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    let pid = r.body["id"].as_str().unwrap().to_string();
    let r = owner.post(&format!("/api/v1/admin/problems/{slug}/publish"), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "publish failed: {}", r.text);
    let r = owner.post("/api/v1/admin/problem-sets", json!({"slug": set_slug, "title": "Set", "problems": [slug]})).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    let sid = r.body["id"].as_str().unwrap().to_string();
    let r = owner.post(&format!("/api/v1/admin/problem-sets/{set_slug}/publish"), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    (pid, sid)
}
