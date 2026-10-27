//! Random opaque tokens. Only SHA-256 digests are stored in the database;
//! the tokens carry 256 bits of OS randomness, so a fast hash is appropriate.

use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

pub fn digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// API token format: `ccal_<random>`; the prefix makes leaked tokens easy to spot.
pub fn new_api_token() -> String {
    format!("ccal_{}", random_token(32))
}

/// OAuth access token (`ccalo_`), accepted only at `/mcp`.
pub const OAUTH_ACCESS_PREFIX: &str = "ccalo_";

pub fn new_oauth_access() -> String {
    format!("{OAUTH_ACCESS_PREFIX}{}", random_token(32))
}

/// OAuth refresh token (`ccalr_`), accepted only at the token endpoint.
pub fn new_oauth_refresh() -> String {
    format!("ccalr_{}", random_token(32))
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
