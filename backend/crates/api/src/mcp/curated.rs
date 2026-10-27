//! Hand-written tools for multi-step workflows that agents get wrong when
//! chaining the generated ones. Each step still goes through the REST API.

use std::sync::LazyLock;

use axum::Router;
use axum::http::{Method, StatusCode};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::tools::{Reply, component_schema, dispatch, encode_segment, object_schema, spec_schemas};
use crate::auth::{Principal, Scope};

pub struct Curated {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub scopes: &'static [Scope],
    pub input_schema: Value,
}

impl Curated {
    pub fn listing(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": self.input_schema,
            "annotations": {"title": self.title, "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
        })
    }
}

fn props(pairs: Vec<(&str, Value)>) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub static CURATED: LazyLock<Vec<Curated>> = LazyLock::new(|| {
    let schemas = spec_schemas();
    vec![
        Curated {
            name: "save_and_validate_problem",
            title: "Save a problem and validate it",
            description: "Create the problem if the slug is new, otherwise save `content` as a new revision; then run the reference \
                solution against every test through the real judge and return the validation report. With `publish: true`, \
                publishes the revision when validation passes. Requires problems:write (and publish to publish).",
            scopes: &[Scope::ProblemsWrite],
            input_schema: object_schema(
                props(vec![
                    ("slug", json!({"type": "string", "description": "Immutable identifier, e.g. `even-odd`."})),
                    ("content", component_schema("ProblemContent")),
                    ("publish", json!({"type": "boolean", "default": false, "description": "Publish if validation passes."})),
                ]),
                vec!["slug".into(), "content".into()],
                &schemas,
            ),
        },
        Curated {
            name: "import_bundle",
            title: "Import problems and sets (dry run first)",
            description: "Validate a cycu-cal.import/v1 document (see resource ccal://docs/import-format) with a dry run. With \
                `apply: true` and a clean dry run, apply it in one transaction (idempotent: re-sending the same document changes \
                nothing). Imported content is always a draft. Requires problems:write and sets:write.",
            scopes: &[Scope::ProblemsWrite, Scope::SetsWrite],
            input_schema: object_schema(
                props(vec![
                    ("document", component_schema("ImportDocument")),
                    ("apply", json!({"type": "boolean", "default": false, "description": "Apply after a clean dry run."})),
                ]),
                vec!["document".into()],
                &schemas,
            ),
        },
        Curated {
            name: "publish_set_and_activate",
            title: "Publish a problem set and make it active",
            description: "Publish a problem set (all its problems must already be published) and make it the set students see \
                by default. Requires publish.",
            scopes: &[Scope::Publish],
            input_schema: object_schema(
                props(vec![("set", json!({"type": "string", "description": "Problem set slug or id."}))]),
                vec!["set".into()],
                &schemas,
            ),
        },
    ]
});

/// Result of a curated tool: the combined JSON, and whether a step failed.
pub async fn call(api: &Router, p: &Principal, name: &str, args: &Map<String, Value>) -> Option<(Value, bool)> {
    Some(match name {
        "save_and_validate_problem" => save_and_validate(api, p, args).await,
        "import_bundle" => import_bundle(api, p, args).await,
        "publish_set_and_activate" => publish_set_and_activate(api, p, args).await,
        _ => return None,
    })
}

fn arg_str<'a>(args: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn failed(step: &str, r: Reply) -> (Value, bool) {
    (json!({"failed_step": step, "status": r.status.as_u16(), "error": r.body}), true)
}

async fn save_and_validate(api: &Router, p: &Principal, args: &Map<String, Value>) -> (Value, bool) {
    let (Some(slug), Some(content)) = (arg_str(args, "slug"), args.get("content")) else {
        return (json!({"error": "`slug` and `content` are required"}), true);
    };
    let base = format!("/api/v1/admin/problems/{}", encode_segment(slug));
    let existing = dispatch(api, p, Method::GET, &base, &[], None).await;
    let saved = match existing.status {
        StatusCode::NOT_FOUND => dispatch(api, p, Method::POST, "/api/v1/admin/problems", &[], Some(&json!({"slug": slug, "content": content}))).await,
        s if s.is_success() => dispatch(api, p, Method::PUT, &base, &[], Some(&json!({"content": content}))).await,
        _ => return failed("load", existing),
    };
    if !saved.ok() {
        return failed("save", saved);
    }
    let created = existing.status == StatusCode::NOT_FOUND;
    let validation = dispatch(api, p, Method::POST, &format!("{base}/validate"), &[], Some(&json!({}))).await;
    if !validation.ok() {
        return failed("validate", validation);
    }
    let mut out = json!({
        "created": created,
        "problem": saved.body,
        "validation": validation.body,
    });
    let valid = validation.body.get("ok").and_then(Value::as_bool) == Some(true);
    if args.get("publish").and_then(Value::as_bool) == Some(true) {
        if !valid {
            out["published"] = json!(false);
            out["note"] = json!("not published: validation failed");
            return (out, true);
        }
        let r = dispatch(api, p, Method::POST, &format!("{base}/publish"), &[], Some(&json!({}))).await;
        if !r.ok() {
            out["publish_error"] = r.body;
            return (out, true);
        }
        out["published"] = json!(true);
    }
    (out, !valid)
}

async fn import_bundle(api: &Router, p: &Principal, args: &Map<String, Value>) -> (Value, bool) {
    let Some(doc) = args.get("document") else { return (json!({"error": "`document` is required"}), true) };
    let dry = dispatch(api, p, Method::POST, "/api/v1/admin/import?dry_run=true", &[], Some(doc)).await;
    if !dry.ok() {
        return failed("dry_run", dry);
    }
    let clean = dry.body.get("ok").and_then(Value::as_bool) == Some(true);
    if !clean || args.get("apply").and_then(Value::as_bool) != Some(true) {
        return (json!({"dry_run": dry.body, "applied": false}), !clean);
    }
    // The same document always maps to the same key, so retries are safe.
    let key = format!("mcp-import-{}", &hex::encode(Sha256::digest(doc.to_string().as_bytes()))[..32]);
    let applied = dispatch(api, p, Method::POST, "/api/v1/admin/import", &[("Idempotency-Key".into(), key)], Some(doc)).await;
    if !applied.ok() {
        return failed("apply", applied);
    }
    (json!({"dry_run": dry.body, "applied": true, "result": applied.body}), false)
}

async fn publish_set_and_activate(api: &Router, p: &Principal, args: &Map<String, Value>) -> (Value, bool) {
    let Some(set) = arg_str(args, "set") else { return (json!({"error": "`set` is required"}), true) };
    let published = dispatch(api, p, Method::POST, &format!("/api/v1/admin/problem-sets/{}/publish", encode_segment(set)), &[], None).await;
    if !published.ok() {
        return failed("publish", published);
    }
    let active = dispatch(api, p, Method::PUT, "/api/v1/admin/site/active-problem-set", &[], Some(&json!({"problem_set": set}))).await;
    if !active.ok() {
        return failed("activate", active);
    }
    (json!({"set": published.body, "active": active.body}), false)
}
