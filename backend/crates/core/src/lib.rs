//! Pure domain logic shared by the API server: fill-in templates, output
//! comparison, the JSON import format, problem validation, the effective
//! policy merge and leaderboard scoring. Nothing here performs I/O.

pub mod compare;
pub mod fillin;
pub mod import;
pub mod leaderboard;
pub mod policy;
pub mod problem;

pub use diagnostics::CONCEPTS;

/// Lower-case letters, digits and single hyphens; 3–64 characters.
pub fn valid_slug(s: &str) -> bool {
    let len_ok = (3..=64).contains(&s.len());
    let chars_ok = s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    len_ok && chars_ok && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--")
}

pub fn valid_concept(c: &str) -> bool {
    CONCEPTS.iter().any(|(id, _)| *id == c)
}

#[cfg(test)]
mod tests {
    #[test]
    fn slugs() {
        assert!(super::valid_slug("a-plus-b"));
        assert!(super::valid_slug("week1-01"));
        assert!(!super::valid_slug("A+B"));
        assert!(!super::valid_slug("-x-"));
        assert!(!super::valid_slug("ab"));
    }
}
