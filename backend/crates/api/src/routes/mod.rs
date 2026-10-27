pub mod admin_catalog;
pub mod admin_problems;
pub mod admin_sets;
pub mod admin_students;
pub mod admin_system;
pub mod auth;
pub mod leaderboard;
pub mod student;
pub mod submissions;

pub const TAG_AUTH: &str = "auth";
pub const TAG_STUDENT: &str = "student";
pub const TAG_SUBMISSIONS: &str = "submissions";
pub const TAG_PROBLEMS: &str = "admin: problems";
pub const TAG_SETS: &str = "admin: problem sets";
pub const TAG_IMPORT: &str = "admin: import/export";
pub const TAG_CATALOG: &str = "admin: teaching text";
pub const TAG_STUDENTS: &str = "admin: students";
pub const TAG_SYSTEM: &str = "admin: system";
