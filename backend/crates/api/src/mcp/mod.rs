//! Remote MCP server at `/mcp` (Streamable HTTP, stateless, JSON responses).
//!
//! Authenticated with an OAuth access token issued by `crate::oauth` (cloud
//! agents such as Claude and ChatGPT) or a plain API token (local agents that
//! can send a header). Tools are the admin REST API (`tools`) plus a few
//! workflow tools (`curated`); resources are the agent-facing docs.

pub mod curated;
pub mod tools;

use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Map, Value, json};

use crate::auth::principal::{api_token_principal, oauth_access_principal};
use crate::auth::{Principal, tokens};
use crate::oauth;
use crate::state::AppState;

/// Protocol revisions we speak; the first is preferred.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// Tool output above this is truncated (agents pay for every token).
const MAX_RESULT_BYTES: usize = 256 * 1024;

const INSTRUCTIONS: &str = "CYCU-CAL is a C programming course judge. These tools manage its problem bank, problem sets, \
students and teaching text through the same REST API the dashboard uses.\n\
Typical authoring flow: read ccal://docs/api-guide, then save_and_validate_problem (creates or revises a draft and runs the \
reference solution through the real judge), publish_problem, create_set / update_set, publish_set_and_activate.\n\
Bulk changes: import_bundle with a cycu-cal.import/v1 document (ccal://docs/import-format), dry run first.\n\
Errors are RFC 9457 problem+json; `errors[].pointer` points at the offending field of `body`.";

/// The `/api/v1` router (with state) that tool calls are dispatched through.
#[derive(Clone)]
pub struct Api(pub Router);

pub fn router(api: Router) -> Router<AppState> {
    Router::new().route("/mcp", post(handle).get(not_allowed).delete(not_allowed)).layer(Extension(Api(api)))
}

/// No server-initiated stream and no sessions: GET and DELETE are not offered.
async fn not_allowed() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

fn challenge(issuer: &str, error: Option<&str>) -> HeaderValue {
    let mut v = format!("Bearer resource_metadata=\"{}\"", oauth::resource_metadata_url(issuer));
    if let Some(e) = error {
        v.push_str(&format!(", error=\"{e}\""));
    }
    HeaderValue::from_str(&v).expect("ascii header")
}

async fn authenticate(st: &AppState, headers: &HeaderMap) -> Result<Principal, Box<Response>> {
    let Some(e) = oauth::endpoints(st) else {
        return Err(Box::new((StatusCode::NOT_FOUND, "MCP is not enabled on this server").into_response()));
    };
    let unauthorized = |error: Option<&str>| {
        let mut r =
            (StatusCode::UNAUTHORIZED, Json(json!({"error": error.unwrap_or("unauthorized"), "error_description": "Bearer token required"}))).into_response();
        r.headers_mut().insert(header::WWW_AUTHENTICATE, challenge(&e.issuer, error));
        Box::new(r)
    };
    let Some(token) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(str::trim) else {
        return Err(unauthorized(None));
    };
    let p = if token.starts_with(tokens::OAUTH_ACCESS_PREFIX) { oauth_access_principal(st, token).await } else { api_token_principal(st, token).await };
    p.map_err(|_| unauthorized(Some("invalid_token")))
}

async fn handle(State(st): State<AppState>, Extension(api): Extension<Api>, headers: HeaderMap, body: Bytes) -> Response {
    let p = match authenticate(&st, &headers).await {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(rpc_error(Value::Null, -32700, &format!("parse error: {e}")))).into_response(),
    };
    let ctx = Ctx { st: &st, api: &api.0, p: &p };
    match msg {
        // JSON-RPC batches (protocol 2025-03-26).
        Value::Array(items) => {
            let mut out = vec![];
            for m in &items {
                if let Some(r) = ctx.process(m).await {
                    out.push(r);
                }
            }
            if out.is_empty() { StatusCode::ACCEPTED.into_response() } else { Json(Value::Array(out)).into_response() }
        }
        m => match ctx.process(&m).await {
            Some(r) => Json(r).into_response(),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

struct Ctx<'a> {
    st: &'a AppState,
    api: &'a Router,
    p: &'a Principal,
}

type RpcResult = Result<Value, (i64, String)>;

impl Ctx<'_> {
    /// Handles one message; `None` for notifications and responses.
    async fn process(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            // A response to a request we never send, or garbage.
            return id.filter(|_| msg.get("result").is_none() && msg.get("error").is_none()).map(|id| rpc_error(id, -32600, "invalid request"));
        };
        let id = id?;
        let empty = Map::new();
        let params = msg.get("params").and_then(Value::as_object).unwrap_or(&empty);
        let r: RpcResult = match method {
            "initialize" => Ok(self.initialize(params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(self.list_tools()),
            "tools/call" => self.call_tool(params).await,
            "resources/list" => Ok(resources_list()),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "resources/read" => self.read_resource(params).await,
            "prompts/list" => Ok(json!({"prompts": []})),
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        Some(match r {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => rpc_error(id, code, &message),
        })
    }

    fn initialize(&self, params: &Map<String, Value>) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
        let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false}},
            "serverInfo": {"name": "cycu-cal", "title": self.st.snapshot().settings.site_name, "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS,
        })
    }

    /// Only the tools this connection's scopes allow.
    fn list_tools(&self) -> Value {
        let mut list: Vec<Value> = curated::CURATED.iter().filter(|t| self.p.has_scopes(t.scopes)).map(|t| t.listing()).collect();
        list.extend(tools::CATALOG.iter().filter(|t| self.p.has_scopes(&t.scopes)).map(|t| t.listing()));
        json!({"tools": list})
    }

    async fn call_tool(&self, params: &Map<String, Value>) -> RpcResult {
        let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "missing tool name".to_string()))?;
        let empty = Map::new();
        let args = params.get("arguments").and_then(Value::as_object).unwrap_or(&empty);
        if let Some(t) = curated::CURATED.iter().find(|t| t.name == name) {
            if !self.p.has_scopes(t.scopes) {
                return Err((-32602, format!("unknown tool: {name}")));
            }
            let (out, is_error) = curated::call(self.api, self.p, name, args).await.expect("curated tool exists");
            return Ok(tool_result(StatusCode::OK, out, is_error));
        }
        let t = tools::CATALOG.iter().find(|t| t.name == name && self.p.has_scopes(&t.scopes)).ok_or((-32602, format!("unknown tool: {name}")))?;
        let (uri, headers, body) = match t.request(args) {
            Ok(r) => r,
            Err(e) => return Ok(tool_result(StatusCode::BAD_REQUEST, json!({"error": e}), true)),
        };
        let r = tools::dispatch(self.api, self.p, t.method.clone(), &uri, &headers, body.as_ref()).await;
        let is_error = !r.ok();
        Ok(tool_result(r.status, r.body, is_error))
    }

    async fn read_resource(&self, params: &Map<String, Value>) -> RpcResult {
        let uri = params.get("uri").and_then(Value::as_str).ok_or((-32602, "missing uri".to_string()))?;
        let (mime, text) = match uri {
            "ccal://docs/api-guide" => ("text/markdown", API_GUIDE.to_string()),
            "ccal://docs/import-format" => ("text/markdown", IMPORT_FORMAT.to_string()),
            "ccal://concepts" => {
                let r = tools::dispatch(self.api, self.p, axum::http::Method::GET, "/api/v1/concepts", &[], None).await;
                ("application/json", r.body.to_string())
            }
            _ => return Err((-32002, format!("resource not found: {uri}"))),
        };
        Ok(json!({"contents": [{"uri": uri, "mimeType": mime, "text": text}]}))
    }
}

const API_GUIDE: &str = include_str!("../../../../../docs/api-guide.md");
const IMPORT_FORMAT: &str = include_str!("../../../../../docs/import-format.md");

fn resources_list() -> Value {
    json!({"resources": [
        {"uri": "ccal://docs/api-guide", "name": "api-guide", "title": "API guide for agents", "mimeType": "text/markdown",
         "description": "Problem lifecycle, scopes, conventions, safe retries."},
        {"uri": "ccal://docs/import-format", "name": "import-format", "title": "Import format (cycu-cal.import/v1)", "mimeType": "text/markdown",
         "description": "Schema of the bulk import/export document."},
        {"uri": "ccal://concepts", "name": "concepts", "title": "Teaching concepts", "mimeType": "application/json",
         "description": "Valid values for a problem's `concepts`."},
    ]})
}

/// A `CallToolResult`: the JSON as text (truncated if huge), plus
/// `structuredContent` for object results that fit.
fn tool_result(status: StatusCode, body: Value, is_error: bool) -> Value {
    let mut text = if body.is_null() { format!("HTTP {}", status.as_u16()) } else { body.to_string() };
    let truncated = text.len() > MAX_RESULT_BYTES;
    if truncated {
        let mut cut = MAX_RESULT_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n…[truncated: narrow the request, e.g. with limit/filters]");
    }
    if is_error && status.as_u16() >= 400 {
        text = format!("HTTP {} {}", status.as_u16(), text);
    }
    let mut out = json!({"content": [{"type": "text", "text": text}], "isError": is_error});
    if body.is_object() && !truncated {
        out["structuredContent"] = body;
    }
    out
}
