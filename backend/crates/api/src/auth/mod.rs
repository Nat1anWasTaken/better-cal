//! Authentication: Argon2id passwords, opaque session cookies, API tokens,
//! CSRF protection and the [`Principal`] extractor used by every handler.

pub mod password;
pub mod principal;
pub mod tokens;

pub use principal::{Principal, Role, Scope};

pub const SESSION_COOKIE: &str = "ccal_session";
