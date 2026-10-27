//! `private_key_jwt` client authentication (RFC 7523 §2.2): the client signs
//! a short-lived JWT with a key published in its metadata (`jwks` or
//! `jwks_uri`), and sends it as `client_assertion` at the token endpoint.
//! ChatGPT authenticates this way.

use std::time::Duration;

use base64::Engine;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde_json::Value;

use super::ClientRow;
use crate::state::AppState;

pub const ASSERTION_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

pub const SIGNING_ALGS: &[Algorithm] =
    &[Algorithm::RS256, Algorithm::RS384, Algorithm::RS512, Algorithm::PS256, Algorithm::PS384, Algorithm::PS512, Algorithm::ES256, Algorithm::ES384];

/// Assertions are short-lived; remember their `jti`s at least this long.
const REPLAY_WINDOW: Duration = Duration::from_secs(15 * 60);

pub fn requires_assertion(client: &ClientRow) -> bool {
    client.metadata.get("token_endpoint_auth_method").and_then(Value::as_str) == Some("private_key_jwt")
}

/// The `iss` claim, read *without* verification, to find the client when
/// the request carries no `client_id`.
pub fn unverified_issuer(assertion: &str) -> Option<String> {
    let payload = assertion.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice::<Value>(&bytes).ok()?.get("iss")?.as_str().map(str::to_string)
}

/// Verifies signature, `iss` = `sub` = client id, `aud` (one of `audiences`),
/// expiry, and that the `jti` has not been seen before.
pub async fn verify(st: &AppState, client: &ClientRow, assertion: &str, audiences: &[String]) -> Result<(), String> {
    if !requires_assertion(client) {
        return Err("client is not registered for private_key_jwt".into());
    }
    let header = decode_header(assertion).map_err(|e| format!("malformed client assertion: {e}"))?;
    if !SIGNING_ALGS.contains(&header.alg) {
        return Err(format!("unsupported signing algorithm {:?}", header.alg));
    }
    let jwks: JwkSet = match (client.metadata.get("jwks").filter(|v| !v.is_null()), client.metadata.get("jwks_uri").and_then(Value::as_str)) {
        (Some(inline), _) => serde_json::from_value(inline.clone()).map_err(|e| format!("invalid jwks: {e}"))?,
        (None, Some(uri)) => serde_json::from_value(super::cimd::fetch(uri).await?).map_err(|e| format!("invalid jwks: {e}"))?,
        (None, None) => return Err("client publishes no keys".into()),
    };
    let jwk = match &header.kid {
        Some(kid) => jwks.find(kid),
        None if jwks.keys.len() == 1 => jwks.keys.first(),
        None => None,
    }
    .ok_or("no key matches the assertion's kid")?;
    let key = DecodingKey::from_jwk(jwk).map_err(|e| format!("unusable key: {e}"))?;
    let mut v = Validation::new(header.alg);
    v.set_audience(audiences);
    v.set_issuer(&[&client.id]);
    v.sub = Some(client.id.clone());
    v.set_required_spec_claims(&["exp", "iss", "sub", "aud"]);
    v.leeway = 60;
    let claims = decode::<Value>(assertion, &key, &v).map_err(|e| format!("invalid client assertion: {e}"))?.claims;
    if let Some(jti) = claims.get("jti").and_then(Value::as_str)
        && !st.limiter.check(&format!("jti:{}:{jti}", client.id), 1, REPLAY_WINDOW).await
    {
        return Err("client assertion was already used".into());
    }
    Ok(())
}
