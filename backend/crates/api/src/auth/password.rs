//! Password hashing with Argon2id (RustCrypto `argon2`, default OWASP-recommended parameters).

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

pub const MIN_PASSWORD_LEN: usize = 8;

pub fn hash(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default().hash_password(password.as_bytes(), &salt).map_err(|e| anyhow::anyhow!("hash failed: {e}"))?.to_string())
}

pub fn verify(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// A hash to verify against when the user does not exist, so login timing
/// does not reveal which usernames exist.
pub fn dummy_hash() -> &'static str {
    use std::sync::OnceLock;
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| hash("not-a-real-password-0").unwrap())
}

pub fn check_strength(password: &str) -> Result<(), &'static str> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err("密碼至少需要 8 個字元");
    }
    if password.len() > 256 {
        return Err("密碼太長");
    }
    Ok(())
}

/// Readable random password for accounts created by an owner.
pub fn generate() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rngs::OsRng;
    (0..12).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn hash_and_verify() {
        let h = super::hash("correct horse").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(super::verify("correct horse", &h));
        assert!(!super::verify("wrong", &h));
        assert!(!super::verify("x", "garbage"));
    }
}
