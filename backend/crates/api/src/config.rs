use std::time::Duration;

/// Runtime configuration, read from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: String,
    pub runner_url: String,
    pub runner_token: String,
    /// Allowed browser origins for cookie-authenticated writes (CSRF defence in depth).
    pub public_origins: Vec<String>,
    /// Send cookies with `Secure` (enable behind HTTPS).
    pub cookie_secure: bool,
    /// Built frontend (SPA) directory; served at `/`.
    pub static_dir: Option<String>,
    pub judge_workers: usize,
    pub session_ttl: Duration,
    /// Runner request timeout for one phase.
    pub runner_timeout: Duration,
    pub max_attempts: i32,
    /// Canonical external origin (e.g. `https://cal.example.edu`). Required for
    /// MCP: it is the OAuth issuer and `{public_url}/mcp` is the protected resource.
    pub public_url: Option<String>,
    /// Header carrying the client address, set by the reverse proxy in front of
    /// the app (e.g. `cf-connecting-ip`). Unset: the last `X-Forwarded-For` hop.
    pub client_ip_header: Option<String>,
    /// Playground runs allowed at once (they share the runner with judging).
    pub playground_concurrency: usize,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Config> {
        let database_url = env("DATABASE_URL").ok_or_else(|| anyhow::anyhow!("DATABASE_URL is required"))?;
        let runner_token = env("RUNNER_TOKEN").unwrap_or_default();
        Ok(Config {
            database_url,
            bind: env("BIND").unwrap_or_else(|| "0.0.0.0:8080".into()),
            runner_url: env("RUNNER_URL").unwrap_or_else(|| "http://runner:7070".into()),
            runner_token,
            public_origins: env("PUBLIC_ORIGINS").map(|v| v.split(',').map(|s| s.trim().trim_end_matches('/').to_string()).collect()).unwrap_or_default(),
            cookie_secure: env("COOKIE_SECURE").map(|v| v == "true" || v == "1").unwrap_or(false),
            static_dir: env("STATIC_DIR"),
            judge_workers: env("JUDGE_WORKERS").and_then(|v| v.parse().ok()).unwrap_or(2).clamp(1, 32),
            session_ttl: Duration::from_secs(env("SESSION_TTL_HOURS").and_then(|v| v.parse::<u64>().ok()).unwrap_or(24 * 14) * 3600),
            runner_timeout: Duration::from_secs(env("RUNNER_TIMEOUT_S").and_then(|v| v.parse().ok()).unwrap_or(90)),
            max_attempts: env("JUDGE_MAX_ATTEMPTS").and_then(|v| v.parse().ok()).unwrap_or(3),
            public_url: env("PUBLIC_URL").map(|v| v.trim().trim_end_matches('/').to_string()),
            client_ip_header: env("CLIENT_IP_HEADER").map(|v| v.trim().to_ascii_lowercase()),
            playground_concurrency: env("PLAYGROUND_CONCURRENCY").and_then(|v| v.parse().ok()).unwrap_or(2).clamp(1, 32),
        })
    }
}

impl Config {
    /// The MCP resource URL, when `PUBLIC_URL` is configured.
    pub fn mcp_url(&self) -> Option<String> {
        self.public_url.as_ref().map(|u| format!("{u}/mcp"))
    }
}
