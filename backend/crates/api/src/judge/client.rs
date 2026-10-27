//! HTTP client for the judge-runner. Behind a narrow interface so another
//! backend (e.g. Judge0) could be plugged in later.

use std::time::Duration;

use judge_proto::{ExecuteRequest, ExecuteResponse, RunnerInfo};

#[derive(Debug, thiserror::Error)]
pub enum RunnerFailure {
    #[error("judge runner unreachable: {0}")]
    Unavailable(String),
    #[error("judge runner busy")]
    Busy,
    #[error("judge runner error: {0}")]
    Failed(String),
}

#[derive(Clone)]
pub struct RunnerClient {
    http: reqwest::Client,
    url: String,
    token: String,
}

impl RunnerClient {
    pub fn new(url: &str, token: &str, timeout: Duration) -> RunnerClient {
        let http = reqwest::Client::builder().timeout(timeout).connect_timeout(Duration::from_secs(3)).build().expect("http client");
        RunnerClient { http, url: url.trim_end_matches('/').to_string(), token: token.to_string() }
    }

    pub async fn execute(&self, req: &ExecuteRequest) -> Result<ExecuteResponse, RunnerFailure> {
        let res = self
            .http
            .post(format!("{}/v1/execute", self.url))
            .bearer_auth(&self.token)
            .json(req)
            .send()
            .await
            .map_err(|e| RunnerFailure::Unavailable(e.to_string()))?;
        match res.status().as_u16() {
            200 => res.json().await.map_err(|e| RunnerFailure::Failed(format!("bad response: {e}"))),
            503 => Err(RunnerFailure::Busy),
            code => Err(RunnerFailure::Failed(format!("HTTP {code}: {}", res.text().await.unwrap_or_default()))),
        }
    }

    pub async fn info(&self) -> Result<RunnerInfo, RunnerFailure> {
        let res = self
            .http
            .get(format!("{}/v1/info", self.url))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| RunnerFailure::Unavailable(e.to_string()))?;
        if !res.status().is_success() {
            return Err(RunnerFailure::Failed(format!("HTTP {}", res.status())));
        }
        res.json().await.map_err(|e| RunnerFailure::Failed(e.to_string()))
    }
}
