//! Thin wrapper over the `isolate` CLI: one sandbox ("box") per job.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use judge_proto::{CompileResult, Limits, RunStatus, TestInput, TestRunResult};
use tokio::process::Command;

use crate::meta::Meta;

/// Fixed compiler invocation. Requests cannot change any of this.
pub const COMPILER: &str = "/usr/bin/gcc-12";
pub const COMPILE_FLAGS: &[&str] =
    &["-std=c17", "-O2", "-pipe", "-Wall", "-Wextra", "-Wformat=2", "-fdiagnostics-format=json", "-fno-diagnostics-color", "-o", "main", "main.c", "-lm"];

const COMPILE_TIME_S: u32 = 10;
const COMPILE_WALL_S: u32 = 20;
const COMPILE_MEM_KB: u32 = 512 * 1024;
const COMPILE_PROCESSES: u32 = 64;
const STDERR_CAP: usize = 8 * 1024;
const COMPILER_OUTPUT_CAP: usize = 256 * 1024;

pub struct Sandbox {
    id: u32,
    root: PathBuf,
}

impl Sandbox {
    /// `isolate --init`; resets any leftover state from a crashed run first.
    pub async fn init(id: u32) -> Result<Sandbox> {
        let _ = isolate(id, &["--cleanup"]).await;
        let out = isolate(id, &["--init"]).await?;
        if !out.status.success() {
            bail!("isolate --init failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        let root = PathBuf::from(String::from_utf8(out.stdout)?.trim());
        Ok(Sandbox { id, root })
    }

    fn box_dir(&self) -> PathBuf {
        self.root.join("box")
    }

    fn meta_path(&self) -> PathBuf {
        PathBuf::from(format!("/run/isolate/meta-{}.txt", self.id))
    }

    pub async fn cleanup(self) {
        let _ = isolate(self.id, &["--cleanup"]).await;
    }

    async fn write_box_file(&self, name: &str, data: &[u8]) -> Result<()> {
        let path = self.box_dir().join(name);
        tokio::fs::write(&path, data).await.with_context(|| format!("write {}", path.display()))?;
        // Box files must be readable by the sandbox uid.
        set_mode(&path, 0o644).await
    }

    async fn run(&self, args: Vec<String>) -> Result<Meta> {
        let meta_path = self.meta_path();
        let _ = tokio::fs::remove_file(&meta_path).await;
        let mut full = vec!["--cg".to_string(), "-b".to_string(), self.id.to_string(), "-M".to_string(), meta_path.display().to_string()];
        full.extend(args);
        let out = Command::new("isolate").args(&full).output().await.context("spawn isolate")?;
        let meta_text = tokio::fs::read_to_string(&meta_path).await.unwrap_or_default();
        let meta = Meta::parse(&meta_text);
        // isolate exits 1 for "program failed" (normal), 2 for internal errors.
        if out.status.code() == Some(2) || meta.status.as_deref() == Some("XX") {
            bail!("isolate internal error: {} {}", String::from_utf8_lossy(&out.stderr).trim(), meta.message.unwrap_or_default());
        }
        Ok(meta)
    }

    pub async fn compile(&self, source: &str) -> Result<CompileResult> {
        self.write_box_file("main.c", source.as_bytes()).await?;
        let mut args: Vec<String> = [
            &format!("--processes={COMPILE_PROCESSES}"),
            &format!("--cg-mem={COMPILE_MEM_KB}"),
            &format!("--time={COMPILE_TIME_S}"),
            &format!("--wall-time={COMPILE_WALL_S}"),
            "--fsize=65536",
            "-E",
            "PATH=/usr/bin:/bin",
            "-E",
            "LANG=C",
            "-E",
            "LC_ALL=C",
            "-o",
            "compile.out",
            "-r",
            "compile.err",
            "--run",
            "--",
            COMPILER,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        args.extend(COMPILE_FLAGS.iter().map(|s| s.to_string()));
        let meta = self.run(args).await?;
        let stderr = read_capped(&self.box_dir().join("compile.err"), COMPILER_OUTPUT_CAP).await.0;
        let (diagnostics_json, stderr_text) = judge_proto::split_gcc_stderr(&stderr);
        let timed_out = meta.status.as_deref() == Some("TO") || meta.cg_oom_killed;
        let exe_exists = tokio::fs::metadata(self.box_dir().join("main")).await.is_ok();
        Ok(CompileResult { success: meta.exitcode == Some(0) && exe_exists, exit_code: meta.exitcode, timed_out, diagnostics_json, stderr_text })
    }

    pub async fn run_test(&self, test: &TestInput, limits: &Limits) -> Result<TestRunResult> {
        self.write_box_file("input.txt", test.stdin.as_bytes()).await?;
        for f in ["output.txt", "error.txt"] {
            let _ = tokio::fs::remove_file(self.box_dir().join(f)).await;
        }
        let time_s = format!("{:.3}", limits.time_ms as f64 / 1000.0);
        let wall_s = format!("{:.3}", limits.wall_time_ms as f64 / 1000.0);
        let args: Vec<String> = [
            "--processes=1",
            &format!("--cg-mem={}", limits.memory_kb),
            &format!("--stack={}", limits.memory_kb),
            &format!("--time={time_s}"),
            &format!("--wall-time={wall_s}"),
            "--extra-time=0.2",
            // stdout + stderr files share this per-file cap (KiB).
            &format!("--fsize={}", limits.output_kb.max(1)),
            "-E",
            "LANG=C",
            "-i",
            "input.txt",
            "-o",
            "output.txt",
            "-r",
            "error.txt",
            "--run",
            "--",
            "./main",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let meta = self.run(args).await?;
        let out_cap = limits.output_kb as usize * 1024;
        let (stdout, stdout_truncated) = read_capped(&self.box_dir().join("output.txt"), out_cap).await;
        let (stderr, stderr_truncated) = read_capped(&self.box_dir().join("error.txt"), STDERR_CAP).await;

        let status = classify(&meta);
        let wall_limit_with_low_cpu =
            status == RunStatus::TimeLimit && meta.message.as_deref().is_some_and(|m| m.contains("wall")) && meta.time_s * 1000.0 < limits.time_ms as f64 * 0.5;
        Ok(TestRunResult {
            id: test.id.clone(),
            status,
            exit_code: meta.exitcode,
            signal: meta.exitsig,
            time_ms: (meta.time_s * 1000.0).round() as u32,
            wall_time_ms: (meta.wall_time_s * 1000.0).round() as u32,
            memory_kb: meta.max_rss_kb,
            wall_limit_with_low_cpu,
            stdout,
            stdout_truncated,
            stderr,
            stderr_truncated,
        })
    }
}

/// Maps an isolate meta record to a run status.
pub fn classify(meta: &Meta) -> RunStatus {
    const SIGXFSZ: i32 = 25;
    if meta.cg_oom_killed {
        return RunStatus::MemoryLimit;
    }
    match meta.status.as_deref() {
        None => RunStatus::Ok,
        Some("TO") => RunStatus::TimeLimit,
        Some("SG") if meta.exitsig == Some(SIGXFSZ) => RunStatus::OutputLimit,
        Some("SG") => RunStatus::Signaled,
        Some("RE") => RunStatus::NonZeroExit,
        _ => RunStatus::SandboxError,
    }
}

async fn read_capped(path: &Path, cap: usize) -> (String, bool) {
    match tokio::fs::read(path).await {
        Ok(mut bytes) => {
            let truncated = bytes.len() > cap;
            bytes.truncate(cap);
            (String::from_utf8_lossy(&bytes).into_owned(), truncated)
        }
        Err(_) => (String::new(), false),
    }
}

async fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await?;
    Ok(())
}

async fn isolate(id: u32, args: &[&str]) -> Result<std::process::Output> {
    let fut = Command::new("isolate").arg("--cg").arg("-b").arg(id.to_string()).args(args).output();
    Ok(tokio::time::timeout(Duration::from_secs(30), fut).await.context("isolate timed out")??)
}

pub async fn command_version(cmd: &str, arg: &str) -> String {
    match Command::new(cmd).arg(arg).output().await {
        Ok(o) => String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string(),
        Err(_) => "unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_output_limit_and_oom() {
        let ole = Meta { status: Some("SG".into()), exitsig: Some(25), ..Default::default() };
        assert_eq!(classify(&ole), RunStatus::OutputLimit);
        let oom = Meta { status: Some("SG".into()), exitsig: Some(9), cg_oom_killed: true, ..Default::default() };
        assert_eq!(classify(&oom), RunStatus::MemoryLimit);
        let segv = Meta { status: Some("SG".into()), exitsig: Some(11), ..Default::default() };
        assert_eq!(classify(&segv), RunStatus::Signaled);
        assert_eq!(classify(&Meta { status: Some("RE".into()), exitcode: Some(3), ..Default::default() }), RunStatus::NonZeroExit);
        assert_eq!(classify(&Meta::default()), RunStatus::Ok);
    }
}
