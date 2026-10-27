//! Sandbox behaviour of the real judge-runner (isolate + GCC 12).
//! Requires a running runner: RUNNER_URL / RUNNER_TOKEN.

use std::time::Duration;

use ccal_api::judge::client::RunnerClient;
use judge_proto::{ExecuteRequest, Limits, RunStatus, TestInput};

fn client() -> RunnerClient {
    let url = std::env::var("RUNNER_URL").unwrap_or_else(|_| "http://127.0.0.1:17070".into());
    let token = std::env::var("RUNNER_TOKEN").unwrap_or_else(|_| "dev-runner-token-0123456789abcdef".into());
    RunnerClient::new(&url, &token, Duration::from_secs(60))
}

async fn run(src: &str, stdin: &str, limits: Limits) -> judge_proto::ExecuteResponse {
    let req = ExecuteRequest { source: src.into(), tests: vec![TestInput { id: "t1".into(), stdin: stdin.into() }], limits, stop_on_failure: false };
    client().execute(&req).await.expect("judge-runner must be running for these tests")
}

fn limits() -> Limits {
    Limits { time_ms: 1000, wall_time_ms: 3000, memory_kb: 65536, output_kb: 64 }
}

#[tokio::test]
async fn reports_pinned_toolchain() {
    let info = client().info().await.unwrap();
    assert!(info.compiler_version.contains("12.2.0"), "{}", info.compiler_version);
    assert!(info.compile_flags.contains(&"-std=c17".to_string()));
    assert!(info.compile_flags.contains(&"-fdiagnostics-format=json".to_string()));
    assert!(info.isolate_version.contains("2.7"));
}

#[tokio::test]
async fn compiles_and_runs() {
    let r = run("#include <stdio.h>\nint main(void){int a,b;scanf(\"%d %d\",&a,&b);printf(\"%d\\n\",a+b);return 0;}\n", "2 40\n", limits()).await;
    assert!(r.compile.success);
    assert_eq!(r.tests[0].status, RunStatus::Ok);
    assert_eq!(r.tests[0].stdout, "42\n");
}

#[tokio::test]
async fn compile_errors_come_back_as_structured_json() {
    let r = run("int main(void) { return 0 }\n", "", limits()).await;
    assert!(!r.compile.success);
    assert!(r.tests.is_empty());
    let d = r.compile.diagnostics_json.as_array().unwrap();
    assert_eq!(d[0]["kind"], "error");
    assert!(d[0]["message"].as_str().unwrap().contains("expected ';'"));
}

#[tokio::test]
async fn linker_errors_are_reported_as_text() {
    let r = run("int mian(void) { return 0; }\n", "", limits()).await;
    assert!(!r.compile.success);
    assert!(r.compile.stderr_text.contains("undefined reference to `main'"), "{}", r.compile.stderr_text);
}

#[tokio::test]
async fn crashes_are_classified() {
    let segv = run("int main(void){ int *p = 0; return *p; }\n", "", limits()).await;
    assert_eq!(segv.tests[0].status, RunStatus::Signaled);
    assert_eq!(segv.tests[0].signal, Some(11));
    let fpe = run("int main(void){ volatile int z = 0; return 7 / z; }\n", "", limits()).await;
    assert_eq!(fpe.tests[0].signal, Some(8));
    let exit = run("int main(void){ return 3; }\n", "", limits()).await;
    assert_eq!(exit.tests[0].status, RunStatus::NonZeroExit);
    assert_eq!(exit.tests[0].exit_code, Some(3));
}

#[tokio::test]
async fn time_memory_and_output_limits_are_enforced() {
    let tle = run("int main(void){ volatile unsigned long x = 0; for(;;) x++; }\n", "", limits()).await;
    assert_eq!(tle.tests[0].status, RunStatus::TimeLimit);
    let sleepy = run("#include <unistd.h>\nint main(void){ sleep(10); return 0; }\n", "", limits()).await;
    assert_eq!(sleepy.tests[0].status, RunStatus::TimeLimit);
    assert!(sleepy.tests[0].wall_limit_with_low_cpu, "wall-clock timeout with idle CPU is distinguished");
    // `volatile` so -O2 cannot optimise the allocation away.
    let mle = run("#include <stdlib.h>\nint main(void){ size_t n = 200u << 20; volatile char *p = malloc(n); if (!p) return 5; for (size_t i = 0; i < n; i += 4096) p[i] = 1; return p[4096]; }\n", "", limits()).await;
    assert_eq!(mle.tests[0].status, RunStatus::MemoryLimit);
    let ole = run("#include <stdio.h>\nint main(void){ for(;;) puts(\"spam spam spam\"); }\n", "", limits()).await;
    assert_eq!(ole.tests[0].status, RunStatus::OutputLimit);
    assert!(ole.tests[0].stdout.len() <= 64 * 1024);
}

#[tokio::test]
async fn sandbox_has_no_network_and_cannot_fork() {
    let net = run(
        "#include <stdio.h>\n#include <sys/socket.h>\n#include <netinet/in.h>\n#include <arpa/inet.h>\nint main(void){int s=socket(AF_INET,SOCK_STREAM,0);struct sockaddr_in a={0};a.sin_family=AF_INET;a.sin_port=htons(80);a.sin_addr.s_addr=inet_addr(\"1.1.1.1\");printf(\"%d\\n\",connect(s,(void*)&a,sizeof a));return 0;}\n",
        "",
        limits(),
    )
    .await;
    assert_eq!(net.tests[0].stdout, "-1\n", "connect must fail inside the sandbox");
    let fork = run("#include <stdio.h>\n#include <unistd.h>\nint main(void){ int ok = 0; for (int i = 0; i < 50; i++) if (fork() >= 0) ok++; printf(\"%d\\n\", ok); return 0; }\n", "", limits()).await;
    // Only the main process may exist; fork() fails (or the child is killed).
    assert!(fork.tests[0].stdout.starts_with('0'), "fork must not succeed: {:?}", fork.tests[0].stdout);
}

#[tokio::test]
async fn requested_limits_are_capped_by_the_runner() {
    let huge = Limits { time_ms: 600_000, wall_time_ms: 600_000, memory_kb: 8 << 20, output_kb: 1 << 20 };
    let started = std::time::Instant::now();
    let r = run("int main(void){ volatile unsigned long x = 0; for(;;) x++; }\n", "", huge).await;
    assert_eq!(r.tests[0].status, RunStatus::TimeLimit);
    assert!(started.elapsed() < Duration::from_secs(30), "runner caps time limits");
}

#[tokio::test]
async fn wrong_token_is_rejected() {
    let url = std::env::var("RUNNER_URL").unwrap_or_else(|_| "http://127.0.0.1:17070".into());
    let c = RunnerClient::new(&url, "wrong-token-wrong-token-xx", Duration::from_secs(5));
    assert!(c.info().await.is_err());
}
