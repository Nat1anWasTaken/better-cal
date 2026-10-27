//! MCP server and its OAuth authorization server, end to end: discovery,
//! dynamic registration, owner consent, PKCE, refresh rotation, revocation,
//! scope-filtered tools dispatched through the real API.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const VERIFIER: &str = "a-very-long-pkce-code-verifier-0123456789-abcdefghijklmnop";
const REDIRECT: &str = "https://client.example/cb";
const RESOURCE: &str = "http://app.test/mcp";

struct Raw {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
    text: String,
}

async fn raw(app: &TestApp, method: &str, uri: &str, headers: &[(&str, &str)], body: Option<String>) -> Raw {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let res = app.router.clone().oneshot(b.body(body.map(Body::from).unwrap_or_else(Body::empty)).unwrap()).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).to_string();
    Raw { status, headers, body: serde_json::from_slice(&bytes).unwrap_or(Value::Null), text }
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut u = reqwest::Url::parse("http://x/").unwrap();
    u.query_pairs_mut().extend_pairs(pairs);
    u.query().unwrap().to_string()
}

async fn token_request(app: &TestApp, pairs: &[(&str, &str)]) -> Raw {
    raw(app, "POST", "/oauth/token", &[("content-type", "application/x-www-form-urlencoded")], Some(form(pairs))).await
}

async fn mcp(app: &TestApp, bearer: &str, method: &str, params: Value) -> Raw {
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let auth = format!("Bearer {bearer}");
    raw(app, "POST", "/mcp", &[("content-type", "application/json"), ("authorization", &auth)], Some(msg.to_string())).await
}

async fn set_mcp_enabled(owner: &mut Client<'_>, on: bool) {
    let mut s = owner.get("/api/v1/admin/settings").await.body["settings"].clone();
    s["mcp_enabled"] = json!(on);
    let r = owner.put("/api/v1/admin/settings", s).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
}

async fn register(app: &TestApp) -> String {
    let r = raw(
        app,
        "POST",
        "/oauth/register",
        &[("content-type", "application/json")],
        Some(json!({"redirect_uris": [REDIRECT], "client_name": "Test Agent", "token_endpoint_auth_method": "none"}).to_string()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    assert!(r.body.get("client_secret").is_none(), "public clients get no secret");
    r.body["client_id"].as_str().unwrap().to_string()
}

/// Runs /oauth/authorize; returns the pending request id.
async fn authorize(app: &TestApp, client_id: &str, scope: &str) -> String {
    let q = form(&[
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", REDIRECT),
        ("code_challenge", &ccal_api::oauth::pkce_s256(VERIFIER)),
        ("code_challenge_method", "S256"),
        ("state", "st-123"),
        ("scope", scope),
        ("resource", RESOURCE),
    ]);
    let r = raw(app, "GET", &format!("/oauth/authorize?{q}"), &[], None).await;
    assert!(r.status.is_redirection(), "{} {}", r.status, r.text);
    let loc = r.headers["location"].to_str().unwrap();
    loc.strip_prefix("/oauth/consent?request=").unwrap_or_else(|| panic!("unexpected redirect {loc}")).to_string()
}

/// Owner approves with `scopes`; returns the authorization code.
async fn approve(owner: &mut Client<'_>, request: &str, scopes: &[&str]) -> String {
    let r = owner.post(&format!("/api/v1/oauth/requests/{request}"), json!({"approve": true, "scopes": scopes})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let to = reqwest::Url::parse(r.body["redirect_to"].as_str().unwrap()).unwrap();
    assert_eq!(format!("{}://{}{}", to.scheme(), to.host_str().unwrap(), to.path()), REDIRECT);
    let q: std::collections::HashMap<_, _> = to.query_pairs().into_owned().collect();
    assert_eq!(q["state"], "st-123");
    assert_eq!(q["iss"], "http://app.test");
    q["code"].clone()
}

async fn exchange(app: &TestApp, client_id: &str, code: &str, verifier: &str) -> Raw {
    token_request(
        app,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT),
            ("client_id", client_id),
            ("code_verifier", verifier),
            ("resource", RESOURCE),
        ],
    )
    .await
}

fn tool_names(r: &Raw) -> Vec<String> {
    r.body["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn oauth_flow_scoped_tools_and_revocation() {
    let app = app().await;
    let mut owner = app.owner().await;

    // Off by default: nothing is discoverable.
    assert_eq!(raw(&app, "GET", "/.well-known/oauth-protected-resource/mcp", &[], None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(mcp(&app, "x", "ping", json!({})).await.status, StatusCode::NOT_FOUND);
    set_mcp_enabled(&mut owner, true).await;

    // Discovery: 401 challenge → resource metadata → authorization server metadata.
    let r = raw(&app, "POST", "/mcp", &[("content-type", "application/json")], Some("{}".into())).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let www = r.headers["www-authenticate"].to_str().unwrap();
    assert!(www.contains("resource_metadata=\"http://app.test/.well-known/oauth-protected-resource/mcp\""), "{www}");
    let r = raw(&app, "GET", "/.well-known/oauth-protected-resource/mcp", &[], None).await;
    assert_eq!(r.body["resource"], RESOURCE);
    assert_eq!(r.body["authorization_servers"], json!(["http://app.test"]));
    let r = raw(&app, "GET", "/.well-known/oauth-authorization-server", &[], None).await;
    assert_eq!(r.body["token_endpoint"], "http://app.test/oauth/token");
    assert_eq!(r.body["code_challenge_methods_supported"], json!(["S256"]));
    for alias in ["/.well-known/openid-configuration", "/.well-known/oauth-authorization-server/mcp", "/.well-known/openid-configuration/mcp"] {
        assert_eq!(raw(&app, "GET", alias, &[], None).await.body["issuer"], "http://app.test", "{alias}");
    }
    assert_eq!(raw(&app, "GET", "/.well-known/nope", &[], None).await.status, StatusCode::NOT_FOUND);

    let client_id = register(&app).await;
    let request = authorize(&app, &client_id, "problems:read problems:write publish").await;

    // Only an owner's browser session may approve.
    let mut student = app.student("stu").await;
    assert_eq!(student.get(&format!("/api/v1/oauth/requests/{request}")).await.status, StatusCode::FORBIDDEN);
    let r = owner.get(&format!("/api/v1/oauth/requests/{request}")).await;
    assert_eq!(r.body["client_name"], "Test Agent");
    assert_eq!(r.body["redirect_host"], "client.example");
    assert_eq!(r.body["requested_scopes"], json!(["problems:read", "problems:write", "publish"]));

    // The owner narrows the scopes: no `publish`.
    let code = approve(&mut owner, &request, &["problems:read", "problems:write"]).await;
    let t = exchange(&app, &client_id, &code, VERIFIER).await;
    assert_eq!(t.status, StatusCode::OK, "{}", t.text);
    assert_eq!(t.headers["cache-control"], "no-store");
    assert_eq!(t.body["scope"], "problems:read problems:write");
    let access = t.body["access_token"].as_str().unwrap().to_string();
    let refresh = t.body["refresh_token"].as_str().unwrap().to_string();
    assert!(access.starts_with("ccalo_"));

    // MCP: initialize, scope-filtered tools, a call through the real API.
    let r = mcp(&app, &access, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}})).await;
    assert_eq!(r.body["result"]["protocolVersion"], "2025-06-18", "{}", r.text);
    let r = mcp(&app, &access, "notifications/initialized", json!({})).await;
    let _ = r;
    let r = mcp(&app, &access, "tools/list", json!({})).await;
    let names = tool_names(&r);
    for want in ["create_problem", "list_problems", "save_and_validate_problem", "concepts"] {
        assert!(names.contains(&want.to_string()), "missing {want}: {names:?}");
    }
    for hidden in ["publish_problem", "list_users", "publish_set_and_activate", "create_token"] {
        assert!(!names.contains(&hidden.to_string()), "{hidden} must be hidden without its scope");
    }
    let r =
        mcp(&app, &access, "tools/call", json!({"name": "create_problem", "arguments": {"body": ab_problem("mcp-ab", "h1"), "idempotency_key": "k1"}})).await;
    assert_eq!(r.body["result"]["isError"], false, "{}", r.text);
    assert_eq!(r.body["result"]["structuredContent"]["slug"], "mcp-ab");
    let r = mcp(&app, &access, "tools/call", json!({"name": "get_problem", "arguments": {"problem": "nope"}})).await;
    assert_eq!(r.body["result"]["isError"], true);
    assert!(r.body["result"]["content"][0]["text"].as_str().unwrap().starts_with("HTTP 404"));
    let r = mcp(&app, &access, "tools/call", json!({"name": "publish_problem", "arguments": {"problem": "mcp-ab"}})).await;
    assert_eq!(r.body["error"]["code"], -32602, "out-of-scope tools are unknown: {}", r.text);
    let r = mcp(&app, &access, "resources/read", json!({"uri": "ccal://docs/import-format"})).await;
    assert!(r.body["result"]["contents"][0]["text"].as_str().unwrap().len() > 100);

    // Audit attributes the change to the connection.
    let audit = owner.get("/api/v1/admin/audit").await;
    let e = audit.body["items"].as_array().unwrap().iter().find(|e| e["action"] == "problem.create").unwrap();
    assert_eq!(e["via_token"], "Test Agent");

    // OAuth tokens are bound to /mcp: the REST API refuses them.
    let mut rest = app.with_token(&access);
    assert_eq!(rest.get("/api/v1/admin/problems").await.status, StatusCode::UNAUTHORIZED);

    // Codes are single-use; a replay revokes the whole connection.
    let r = exchange(&app, &client_id, &code, VERIFIER).await;
    assert_eq!(r.body["error"], "invalid_grant");
    assert_eq!(mcp(&app, &access, "ping", json!({})).await.status, StatusCode::UNAUTHORIZED);
    let r = token_request(&app, &[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &client_id)]).await;
    assert_eq!(r.body["error"], "invalid_grant");

    // Reconnect; refresh tokens rotate, and reusing an old one revokes.
    let code = approve(&mut owner, &authorize(&app, &client_id, "").await, &["problems:read"]).await;
    let t = exchange(&app, &client_id, &code, VERIFIER).await;
    let refresh1 = t.body["refresh_token"].as_str().unwrap().to_string();
    let t2 = token_request(&app, &[("grant_type", "refresh_token"), ("refresh_token", &refresh1), ("client_id", &client_id)]).await;
    assert_eq!(t2.status, StatusCode::OK, "{}", t2.text);
    let access2 = t2.body["access_token"].as_str().unwrap().to_string();
    let r = mcp(&app, &access2, "tools/list", json!({})).await;
    assert!(!tool_names(&r).contains(&"create_problem".to_string()), "read-only connection");
    let r = token_request(&app, &[("grant_type", "refresh_token"), ("refresh_token", &refresh1), ("client_id", &client_id)]).await;
    assert_eq!(r.body["error"], "invalid_grant");
    let r = mcp(&app, &access2, "ping", json!({})).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(r.headers["www-authenticate"].to_str().unwrap().contains("invalid_token"));

    // The dashboard lists the connections (all revoked by now).
    let r = owner.get("/api/v1/admin/tokens").await;
    let conns: Vec<&Value> = r.body.as_array().unwrap().iter().filter(|t| t["kind"] == "oauth").collect();
    assert_eq!(conns.len(), 2);
    assert!(conns.iter().all(|c| !c["revoked_at"].is_null() && c["client"]["name"] == "Test Agent"));
    assert_eq!(conns[0]["client"]["redirect_hosts"], json!(["client.example"]));
}

#[tokio::test]
async fn bad_authorization_requests_are_rejected() {
    let app = app().await;
    let mut owner = app.owner().await;
    set_mcp_enabled(&mut owner, true).await;
    let client_id = register(&app).await;

    // Untrusted redirect: an error page, never a redirect.
    let q = form(&[
        ("response_type", "code"),
        ("client_id", &client_id),
        ("redirect_uri", "https://evil.example/cb"),
        ("code_challenge", &"a".repeat(43)),
        ("code_challenge_method", "S256"),
    ]);
    let r = raw(&app, "GET", &format!("/oauth/authorize?{q}"), &[], None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.headers.get("location").is_none());
    // Unknown client.
    let r = raw(&app, "GET", "/oauth/authorize?client_id=nope&response_type=code", &[], None).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // No PKCE: back to the client with an error.
    let q = form(&[("response_type", "code"), ("client_id", &client_id), ("redirect_uri", REDIRECT), ("state", "s")]);
    let r = raw(&app, "GET", &format!("/oauth/authorize?{q}"), &[], None).await;
    let loc = r.headers["location"].to_str().unwrap();
    assert!(loc.starts_with(REDIRECT) && loc.contains("error=invalid_request") && loc.contains("state=s"), "{loc}");
    // Wrong resource.
    let q = form(&[
        ("response_type", "code"),
        ("client_id", &client_id),
        ("redirect_uri", REDIRECT),
        ("code_challenge", &"a".repeat(43)),
        ("code_challenge_method", "S256"),
        ("resource", "https://other.example/mcp"),
    ]);
    let r = raw(&app, "GET", &format!("/oauth/authorize?{q}"), &[], None).await;
    assert!(r.headers["location"].to_str().unwrap().contains("error=invalid_target"));
    // Bad registrations.
    let r =
        raw(&app, "POST", "/oauth/register", &[("content-type", "application/json")], Some(json!({"redirect_uris": ["http://evil.example/cb"]}).to_string()))
            .await;
    assert_eq!(r.body["error"], "invalid_redirect_uri");

    // PKCE mismatch.
    let code = approve(&mut owner, &authorize(&app, &client_id, "problems:read").await, &["problems:read"]).await;
    let r = exchange(&app, &client_id, &code, "the-wrong-verifier-0123456789-0123456789-abcdef").await;
    assert_eq!(r.body["error"], "invalid_grant");

    // Denial goes back to the client as access_denied.
    let req = authorize(&app, &client_id, "problems:read").await;
    let r = owner.post(&format!("/api/v1/oauth/requests/{req}"), json!({"approve": false})).await;
    assert!(r.body["redirect_to"].as_str().unwrap().contains("error=access_denied"));
    assert_eq!(owner.get(&format!("/api/v1/oauth/requests/{req}")).await.status, StatusCode::NOT_FOUND);

    // Confidential clients must authenticate at the token endpoint.
    let r = raw(
        &app,
        "POST",
        "/oauth/register",
        &[("content-type", "application/json")],
        Some(json!({"redirect_uris": [REDIRECT], "client_name": "Conf"}).to_string()),
    )
    .await;
    let (cid, secret) = (r.body["client_id"].as_str().unwrap().to_string(), r.body["client_secret"].as_str().unwrap().to_string());
    let code = approve(&mut owner, &authorize(&app, &cid, "problems:read").await, &["problems:read"]).await;
    let r = exchange(&app, &cid, &code, VERIFIER).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.body["error"], "invalid_client");
    let code = approve(&mut owner, &authorize(&app, &cid, "problems:read").await, &["problems:read"]).await;
    let r = token_request(
        &app,
        &[("grant_type", "authorization_code"), ("code", &code), ("client_id", &cid), ("client_secret", &secret), ("code_verifier", VERIFIER)],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);

    // Turning MCP off closes everything.
    set_mcp_enabled(&mut owner, false).await;
    let access = r.body["access_token"].as_str().unwrap();
    assert_eq!(mcp(&app, access, "ping", json!({})).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_tokens_work_over_mcp_and_curated_tools_use_the_judge() {
    let app = app().await;
    let mut owner = app.owner().await;
    set_mcp_enabled(&mut owner, true).await;
    let token = owner.token(&["problems:read", "problems:write", "publish"]).await;

    let content = ab_problem("mcp-sv", "h")["content"].clone();
    let r = mcp(&app, &token, "tools/call", json!({"name": "save_and_validate_problem", "arguments": {"slug": "mcp-sv", "content": content, "publish": true}}))
        .await;
    let res = &r.body["result"];
    assert_eq!(res["isError"], false, "{}", r.text);
    assert_eq!(res["structuredContent"]["created"], true);
    assert_eq!(res["structuredContent"]["validation"]["ok"], true);
    assert_eq!(res["structuredContent"]["published"], true);

    // Calling it again revises the same problem.
    let r = mcp(&app, &token, "tools/call", json!({"name": "save_and_validate_problem", "arguments": {"slug": "mcp-sv", "content": content}})).await;
    assert_eq!(r.body["result"]["structuredContent"]["created"], false, "{}", r.text);

    // Batches (2025-03-26) and unknown methods.
    let auth = format!("Bearer {token}");
    let batch = json!([{"jsonrpc": "2.0", "id": 1, "method": "ping"}, {"jsonrpc": "2.0", "method": "notifications/initialized"}, {"jsonrpc": "2.0", "id": 2, "method": "nope"}]);
    let r = raw(&app, "POST", "/mcp", &[("content-type", "application/json"), ("authorization", &auth)], Some(batch.to_string())).await;
    let arr = r.body.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[1]["error"]["code"], -32601);
    let r = raw(&app, "GET", "/mcp", &[("authorization", &auth)], None).await;
    assert_eq!(r.status, StatusCode::METHOD_NOT_ALLOWED);

    let status = owner.get("/api/v1/admin/mcp").await;
    assert_eq!(status.body["server_url"], RESOURCE);
    assert!(status.body["tools"].as_array().unwrap().len() > 40);
}
