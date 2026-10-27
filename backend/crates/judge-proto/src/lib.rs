//! Wire types shared by the API server and the judge-runner.
//!
//! The runner only ever receives source code and stdin. Expected outputs never
//! leave the API server, so output comparison happens there.

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// Per-request limits. The runner clamps each value to its own configured caps.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Limits {
    /// CPU time limit per test, in milliseconds.
    pub time_ms: u32,
    /// Wall-clock limit per test, in milliseconds.
    pub wall_time_ms: u32,
    /// Memory limit per test, in KiB.
    pub memory_kb: u32,
    /// Maximum bytes the program may write to stdout (enforced as a file-size limit).
    pub output_kb: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self { time_ms: 1000, wall_time_ms: 3000, memory_kb: 65536, output_kb: 64 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestInput {
    pub id: String,
    pub stdin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub source: String,
    pub tests: Vec<TestInput>,
    pub limits: Limits,
    /// Stop executing remaining tests after the first non-OK run outcome.
    #[serde(default)]
    pub stop_on_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResult {
    /// True when gcc produced an executable.
    pub success: bool,
    pub exit_code: Option<i32>,
    /// Compiler ran out of its own time or memory budget.
    pub timed_out: bool,
    /// The JSON array emitted by `-fdiagnostics-format=json` (may be empty).
    pub diagnostics_json: serde_json::Value,
    /// Everything on stderr that is not the JSON diagnostics document
    /// (linker messages, driver errors).
    pub stderr_text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Exited with code 0.
    Ok,
    /// Exited normally with a non-zero exit code.
    NonZeroExit,
    /// Killed by a signal (not one caused by our limits).
    Signaled,
    /// CPU or wall time limit exceeded.
    TimeLimit,
    /// Memory limit exceeded (cgroup OOM).
    MemoryLimit,
    /// Output size limit exceeded.
    OutputLimit,
    /// The sandbox itself failed; not the student's fault.
    SandboxError,
    /// Skipped because an earlier test failed and stop_on_failure was set.
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunResult {
    pub id: String,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub time_ms: u32,
    pub wall_time_ms: u32,
    pub memory_kb: u32,
    /// True when the wall-clock limit fired while CPU time stayed low.
    pub wall_limit_with_low_cpu: bool,
    pub stdout: String,
    pub stdout_truncated: bool,
    pub stderr: String,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    pub protocol_version: u32,
    pub compile: CompileResult,
    pub tests: Vec<TestRunResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerInfo {
    pub protocol_version: u32,
    pub compiler: String,
    pub compiler_version: String,
    pub compile_flags: Vec<String>,
    pub isolate_version: String,
    pub max_limits: Limits,
    pub concurrency: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerError {
    pub code: String,
    pub message: String,
}

/// gcc prints the JSON diagnostics document as one line on stderr; linker and
/// driver messages are plain text around it.
pub fn split_gcc_stderr(stderr: &str) -> (serde_json::Value, String) {
    let mut json = serde_json::Value::Array(vec![]);
    let mut text = String::new();
    for line in stderr.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[')
            && let Ok(v @ serde_json::Value::Array(_)) = serde_json::from_str::<serde_json::Value>(trimmed)
        {
            if let (serde_json::Value::Array(acc), serde_json::Value::Array(new)) = (&mut json, v) {
                acc.extend(new);
            }
            continue;
        }
        text.push_str(line);
        text.push('\n');
    }
    (json, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_json_from_linker_text() {
        let stderr = "[{\"kind\": \"warning\", \"message\": \"x\"}]\n/usr/bin/ld: /tmp/cc.o: in function `main':\nmain.c:(.text+0x5): undefined reference to `prinf'\ncollect2: error: ld returned 1 exit status\n";
        let (json, text) = split_gcc_stderr(stderr);
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert!(text.contains("undefined reference to `prinf'"));
        assert!(!text.contains("\"kind\""));
    }
}
