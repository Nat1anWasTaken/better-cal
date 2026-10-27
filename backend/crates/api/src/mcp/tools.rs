//! MCP tools generated from the OpenAPI document, so the MCP surface never
//! drifts from the REST API. Every admin operation a token may call becomes a
//! tool named after its `operationId`; arguments are the path, query and
//! header parameters plus `body` (the request schema, with `$ref`s moved into
//! `$defs`). Calls are dispatched in-process through the real router, so each
//! handler still enforces its own scope checks, validation and auditing.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Map, Value, json};
use tower::ServiceExt;

use crate::auth::{Principal, Scope};

/// The OpenAPI document as JSON (schemas are looked up by `$ref`).
pub static SPEC: LazyLock<Value> = LazyLock::new(|| serde_json::to_value(crate::app::api_router().split_for_parts().1).expect("openapi serializes"));

pub static CATALOG: LazyLock<Vec<ApiTool>> = LazyLock::new(|| build(&SPEC));

pub struct ApiTool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub method: Method,
    /// Full path template, e.g. `/api/v1/admin/problems/{problem}`.
    pub path: String,
    pub params: Vec<Param>,
    pub has_body: bool,
    pub scopes: Vec<Scope>,
    pub input_schema: Value,
}

pub struct Param {
    /// Argument name (header names are snake_cased: `Idempotency-Key` → `idempotency_key`).
    pub arg: String,
    pub name: String,
    pub location: Location,
}

#[derive(PartialEq, Eq)]
pub enum Location {
    Path,
    Query,
    Header,
}

/// Names for operations whose `operationId` is ambiguous on its own.
const RENAMES: &[(&str, &str, &str)] = &[
    ("GET", "/api/v1/admin/catalog", "catalog_overview"),
    ("GET", "/api/v1/admin/catalog/history", "catalog_history"),
    ("POST", "/api/v1/admin/catalog/preview", "catalog_preview"),
    ("POST", "/api/v1/admin/catalog/publish", "catalog_publish"),
    ("GET", "/api/v1/admin/overview", "dashboard_overview"),
    ("GET", "/api/v1/admin/progress", "progress_matrix"),
];

impl ApiTool {
    pub fn read_only(&self) -> bool {
        self.method == Method::GET
    }

    pub fn listing(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": self.input_schema,
            "annotations": {
                "title": self.title,
                "readOnlyHint": self.read_only(),
                "destructiveHint": self.method == Method::DELETE,
                "idempotentHint": matches!(self.method, Method::GET | Method::PUT | Method::DELETE),
                "openWorldHint": false,
            },
        })
    }

    /// Builds the request URI and headers from tool arguments.
    pub fn request(&self, args: &Map<String, Value>) -> Result<(String, Vec<(String, String)>, Option<Value>), String> {
        let mut path = self.path.clone();
        let mut query = vec![];
        let mut headers = vec![];
        for p in &self.params {
            let Some(v) = args.get(&p.arg).filter(|v| !v.is_null()) else {
                if p.location == Location::Path {
                    return Err(format!("missing required argument `{}`", p.arg));
                }
                continue;
            };
            let s = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            match p.location {
                Location::Path => path = path.replace(&format!("{{{}}}", p.name), &encode_segment(&s)),
                Location::Query => query.push((p.name.clone(), s)),
                Location::Header => headers.push((p.name.clone(), s)),
            }
        }
        let mut u = reqwest::Url::parse(&format!("http://local{path}")).map_err(|e| e.to_string())?;
        if !query.is_empty() {
            u.query_pairs_mut().extend_pairs(query);
        }
        let uri = match u.query() {
            Some(q) => format!("{}?{q}", u.path()),
            None => u.path().to_string(),
        };
        let body = if self.has_body { Some(args.get("body").cloned().unwrap_or_else(|| json!({}))) } else { None };
        Ok((uri, headers, body))
    }
}

pub fn encode_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn included(path: &str) -> bool {
    path.starts_with("/api/v1/admin/") || path == "/api/v1/concepts"
}

/// Scopes from the operation's `token` security requirement. `None` means
/// tokens can't call it at all (owner-session only, e.g. token management).
fn token_scopes(op: &Value) -> Option<Vec<Scope>> {
    let sec = op.get("security").and_then(Value::as_array);
    let Some(sec) = sec else { return Some(vec![]) };
    if sec.is_empty() {
        return Some(vec![]);
    }
    sec.iter().find_map(|req| req.get("token")).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).filter_map(Scope::parse).collect())
}

fn build(spec: &Value) -> Vec<ApiTool> {
    let schemas = spec.pointer("/components/schemas").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut out = vec![];
    let Some(paths) = spec.get("paths").and_then(Value::as_object) else { return out };
    for (path, item) in paths {
        if !included(path) {
            continue;
        }
        let Some(item) = item.as_object() else { continue };
        for (m, op) in item {
            let Ok(method) = m.to_uppercase().parse::<Method>() else { continue };
            let Some(scopes) = token_scopes(op) else { continue };
            let op_id = op.get("operationId").and_then(Value::as_str).unwrap_or_default();
            let name = RENAMES.iter().find(|(rm, rp, _)| *rm == method.as_str() && rp == path).map(|r| r.2).unwrap_or(op_id).to_string();

            let summary = op.get("summary").and_then(Value::as_str).unwrap_or_default().trim().to_string();
            let mut description = summary.clone();
            if let Some(d) = op.get("description").and_then(Value::as_str) {
                description.push_str("\n\n");
                description.push_str(d.trim());
            }
            description.push_str(&format!("\n\nREST: {} {}", method, path.trim_start_matches("/api/v1")));
            if !scopes.is_empty() {
                description.push_str(&format!(" (scope: {})", scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
            }

            let mut props = Map::new();
            let mut required = vec![];
            let mut params = vec![];
            for p in op.get("parameters").and_then(Value::as_array).into_iter().flatten() {
                let pname = p["name"].as_str().unwrap_or_default().to_string();
                let location = match p["in"].as_str() {
                    Some("path") => Location::Path,
                    Some("query") => Location::Query,
                    Some("header") => Location::Header,
                    _ => continue,
                };
                let arg = pname.to_lowercase().replace('-', "_");
                let mut schema = p.get("schema").cloned().unwrap_or_else(|| json!({"type": "string"}));
                if let Some(d) = p.get("description").and_then(Value::as_str)
                    && let Some(o) = schema.as_object_mut()
                {
                    o.insert("description".into(), json!(d));
                }
                if location == Location::Path || p.get("required").and_then(Value::as_bool) == Some(true) {
                    required.push(arg.clone());
                }
                props.insert(arg.clone(), schema);
                params.push(Param { arg, name: pname, location });
            }
            let body = op.pointer("/requestBody/content/application~1json/schema").cloned();
            let has_body = body.is_some();
            if let Some(b) = body {
                props.insert("body".into(), b);
                if op.pointer("/requestBody/required").and_then(Value::as_bool) == Some(true) {
                    required.push("body".into());
                }
            }
            let title = if summary.is_empty() { name.clone() } else { first_sentence(&summary) };
            out.push(ApiTool {
                input_schema: object_schema(props, required, &schemas),
                name,
                title,
                description,
                method,
                path: path.clone(),
                params,
                has_body,
                scopes,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn first_sentence(s: &str) -> String {
    let s = s.split(['\n', '。']).next().unwrap_or(s);
    let s = s.split(". ").next().unwrap_or(s).trim_end_matches('.');
    s.chars().take(80).collect()
}

/// An object schema whose `$ref`s point into an attached `$defs`.
pub fn object_schema(props: Map<String, Value>, required: Vec<String>, schemas: &Map<String, Value>) -> Value {
    let mut props = Value::Object(props);
    let mut wanted = BTreeSet::new();
    collect_refs(&props, &mut wanted);
    let mut defs = BTreeMap::new();
    let mut queue: Vec<String> = wanted.into_iter().collect();
    while let Some(n) = queue.pop() {
        if defs.contains_key(&n) {
            continue;
        }
        let Some(s) = schemas.get(&n) else { continue };
        let mut more = BTreeSet::new();
        collect_refs(s, &mut more);
        queue.extend(more.into_iter().filter(|m| !defs.contains_key(m)));
        defs.insert(n, s.clone());
    }
    rewrite_refs(&mut props);
    let mut out = json!({"type": "object", "properties": props, "additionalProperties": false});
    if !required.is_empty() {
        out["required"] = json!(required);
    }
    if !defs.is_empty() {
        let mut d = Value::Object(defs.into_iter().collect());
        rewrite_refs(&mut d);
        out["$defs"] = d;
    }
    out
}

/// Schemas reachable from a component, for tools written by hand.
pub fn component_schema(name: &str) -> Value {
    json!({"$ref": format!("#/components/schemas/{name}")})
}

pub fn spec_schemas() -> Map<String, Value> {
    SPEC.pointer("/components/schemas").and_then(Value::as_object).cloned().unwrap_or_default()
}

const REF_PREFIX: &str = "#/components/schemas/";

fn collect_refs(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(o) => {
            if let Some(r) = o.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix(REF_PREFIX)) {
                out.insert(r.to_string());
            }
            o.values().for_each(|x| collect_refs(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect_refs(x, out)),
        _ => {}
    }
}

fn rewrite_refs(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if let Some(Value::String(r)) = o.get_mut("$ref")
                && let Some(n) = r.strip_prefix(REF_PREFIX)
            {
                *r = format!("#/$defs/{n}");
            }
            o.values_mut().for_each(rewrite_refs);
        }
        Value::Array(a) => a.iter_mut().for_each(rewrite_refs),
        _ => {}
    }
}

/// A REST response seen from inside the server.
pub struct Reply {
    pub status: StatusCode,
    pub body: Value,
}

impl Reply {
    pub fn ok(&self) -> bool {
        self.status.is_success()
    }
}

/// Runs one request through the API router as `p`. The request carries no
/// cookies; the principal rides along as an extension, which the `Principal`
/// extractor picks up instead of authenticating again.
pub async fn dispatch(api: &Router, p: &Principal, method: Method, uri: &str, headers: &[(String, String)], body: Option<&Value>) -> Reply {
    let mut b = Request::builder().method(method).uri(uri);
    if body.is_some() {
        b = b.header("content-type", "application/json");
    }
    for (k, v) in headers {
        b = b.header(k.as_str(), v.as_str());
    }
    let mut req = match b.body(body.map(|v| Body::from(v.to_string())).unwrap_or_else(Body::empty)) {
        Ok(r) => r,
        Err(e) => return Reply { status: StatusCode::BAD_REQUEST, body: json!({"detail": format!("invalid request: {e}")}) },
    };
    req.extensions_mut().insert(p.clone());
    let res = match api.clone().oneshot(req).await {
        Ok(r) => r,
        Err(never) => match never {},
    };
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024 * 1024).await.unwrap_or_default();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    Reply { status, body }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_unique_and_self_contained() {
        let mut names = BTreeSet::new();
        assert!(CATALOG.len() > 40, "expected the admin API, got {} tools", CATALOG.len());
        for t in CATALOG.iter() {
            assert!(names.insert(t.name.clone()), "duplicate tool name {}", t.name);
            assert!(t.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "bad tool name {}", t.name);
            let mut refs = BTreeSet::new();
            collect_refs(&t.input_schema, &mut refs);
            assert!(refs.is_empty(), "{} still points at components: {refs:?}", t.name);
            let s = t.input_schema.to_string();
            for r in s.split("\"#/$defs/").skip(1) {
                let n = r.split('"').next().unwrap();
                assert!(t.input_schema["$defs"].get(n).is_some(), "{}: dangling $ref {n}", t.name);
            }
        }
        for excluded in ["create_token", "list_tokens", "revoke_token", "reset_password", "mcp_status", "get_request", "decide"] {
            assert!(!names.contains(excluded), "{excluded} must not be exposed over MCP");
        }
        for expected in ["create_problem", "update_problem", "publish_problem", "import", "catalog_overview", "dashboard_overview", "concepts"] {
            assert!(names.contains(expected), "missing tool {expected}");
        }
    }

    #[test]
    fn arguments_become_a_request() {
        let t = CATALOG.iter().find(|t| t.name == "update_problem").unwrap();
        let args = json!({"problem": "a b/c", "if_match": "3", "body": {"content": {}}});
        let (uri, headers, body) = t.request(args.as_object().unwrap()).unwrap();
        assert_eq!(uri, "/api/v1/admin/problems/a%20b%2Fc");
        assert_eq!(headers, vec![("If-Match".to_string(), "3".to_string())]);
        assert_eq!(body.unwrap(), json!({"content": {}}));
        let t = CATALOG.iter().find(|t| t.name == "list_problems").unwrap();
        let (uri, _, body) = t.request(json!({"q": "加法", "limit": 5}).as_object().unwrap()).unwrap();
        assert!(uri.starts_with("/api/v1/admin/problems?") && uri.contains("limit=5"), "{uri}");
        assert!(body.is_none());
    }
}
