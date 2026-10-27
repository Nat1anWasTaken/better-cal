use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cyculcal", about = "CYCU-CAL C teaching & judging platform")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP API, judge workers and the web UI (default).
    Serve,
    /// Create an owner account. Refuses if an owner exists unless --force.
    CreateOwner {
        #[arg(long)]
        username: String,
        #[arg(long, default_value = "")]
        display_name: String,
        /// Read the password from this environment variable instead of prompting.
        #[arg(long)]
        password_env: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Apply database migrations and exit.
    Migrate,
    /// Print the OpenAPI document.
    Openapi,
    /// Exit 0 if the local server answers /healthz (for container health checks).
    Healthcheck,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn,tower_http=info".into()))
        .init();
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve) {
        Command::Openapi => {
            println!("{}", ccal_api::app::openapi_json());
            Ok(())
        }
        Command::Migrate => {
            let cfg = ccal_api::config::Config::from_env()?;
            let db = ccal_api::connect(&cfg).await?;
            ccal_api::MIGRATOR.run(&db).await?;
            println!("migrations applied");
            Ok(())
        }
        Command::CreateOwner { username, display_name, password_env, force } => create_owner(username, display_name, password_env, force).await,
        Command::Serve => serve().await,
        Command::Healthcheck => healthcheck(),
    }
}

async fn create_owner(username: String, display_name: String, password_env: Option<String>, force: bool) -> anyhow::Result<()> {
    use ccal_api::auth::password;
    let cfg = ccal_api::config::Config::from_env()?;
    let db = ccal_api::connect(&cfg).await?;
    ccal_api::MIGRATOR.run(&db).await?;
    let owners: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE role = 'owner'").fetch_one(&db).await?;
    if owners > 0 && !force {
        anyhow::bail!("an owner account already exists; use --force to add another");
    }
    let pw = match password_env {
        Some(var) => std::env::var(&var).map_err(|_| anyhow::anyhow!("environment variable {var} is not set"))?,
        None => {
            let a = rpassword::prompt_password("New owner password: ")?;
            let b = rpassword::prompt_password("Repeat password: ")?;
            if a != b {
                anyhow::bail!("passwords do not match");
            }
            a
        }
    };
    password::check_strength(&pw).map_err(|m| anyhow::anyhow!("{m}"))?;
    sqlx::query("INSERT INTO users (id, username, display_name, role, password_hash) VALUES ($1, $2, $3, 'owner', $4)")
        .bind(uuid::Uuid::now_v7())
        .bind(&username)
        .bind(&display_name)
        .bind(password::hash(&pw)?)
        .execute(&db)
        .await?;
    println!("owner `{username}` created");
    Ok(())
}

/// SIGINT or SIGTERM (docker stop). In-flight judge jobs are simply dropped:
/// their lease expires and the next process picks them up again.
async fn shutdown_signal() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    tracing::info!("shutting down");
}

fn healthcheck() -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let port = std::env::var("BIND").ok().and_then(|b| b.rsplit(':').next().map(str::to_string)).unwrap_or_else(|| "8080".into());
    let mut s = std::net::TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse()?, std::time::Duration::from_secs(2))?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    s.write_all(b"GET /healthz HTTP/1.0\r\n\r\n")?;
    let mut buf = String::new();
    s.read_to_string(&mut buf)?;
    if buf.contains(" 200 ") { Ok(()) } else { anyhow::bail!("unhealthy") }
}

async fn serve() -> anyhow::Result<()> {
    let cfg = ccal_api::config::Config::from_env()?;
    if cfg.runner_token.len() < 24 {
        anyhow::bail!("RUNNER_TOKEN must be set (at least 24 random characters) and match the runner's");
    }
    let bind = cfg.bind.clone();
    let state = ccal_api::init_state(cfg).await?;
    ccal_api::judge::worker::spawn_workers(&state);
    tokio::spawn(ccal_api::catalog_store::watch(state.clone()));
    let app = ccal_api::app::build(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "CYCU-CAL API listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}
