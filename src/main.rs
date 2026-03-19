mod collector;
mod config;
mod dashboard;
mod db;
mod launcher;
mod proxy;
mod session;
mod settings;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "laudec",
    version,
    about = "See everything Claude Code does. One binary. One command."
)]
struct Cli {
    /// Project path (default: current directory)
    path: Option<PathBuf>,

    /// Run a single prompt (non-interactive, for testing)
    #[arg(short, long)]
    prompt: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// View session history
    Log {
        /// Show all projects
        #[arg(long)]
        all: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Limit results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// View API call log
    Calls {
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Limit results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// View OTEL events
    Events {
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Filter by event name (api_request, tool_result, user_prompt, tool_decision)
        #[arg(long, short = 'n')]
        name: Option<String>,
        /// Limit results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Replay a session timeline
    Replay {
        /// Session ID (prefix match supported)
        session_id: String,
    },
    /// Start the dashboard without launching Claude Code
    Dashboard,
    /// Print resolved configuration
    Config,
    /// Generate a starter laudec.toml
    Init,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("laudec=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let project_path = cli
        .path
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine cwd"));
    let project_path = std::fs::canonicalize(&project_path)
        .with_context(|| format!("resolving path: {}", project_path.display()))?;

    match cli.command {
        Some(Commands::Init) => cmd_init(&project_path),
        Some(Commands::Config) => cmd_config(&project_path),
        Some(Commands::Log { all, json: _, limit }) => cmd_log(&project_path, all, limit).await,
        Some(Commands::Calls { session, limit }) => cmd_calls(session.as_deref(), limit).await,
        Some(Commands::Events { session, name, limit }) => {
            cmd_events(session.as_deref(), name.as_deref(), limit).await
        }
        Some(Commands::Replay { session_id }) => cmd_replay(&session_id).await,
        Some(Commands::Dashboard) => cmd_dashboard(&project_path).await,
        None => cmd_run(&project_path, cli.prompt.as_deref()).await,
    }
}

// ── init ──────────────────────────────────────────────────────────────

fn cmd_init(project_path: &std::path::Path) -> Result<()> {
    let path = project_path.join("laudec.toml");
    if path.exists() {
        anyhow::bail!("laudec.toml already exists at {}", path.display());
    }
    std::fs::write(&path, config::Config::starter_toml())?;
    println!("Created {}", path.display());
    Ok(())
}

// ── config ────────────────────────────────────────────────────────────

fn cmd_config(project_path: &std::path::Path) -> Result<()> {
    let cfg = config::Config::load(project_path)?;
    println!("{cfg:#?}");
    Ok(())
}

// ── log ───────────────────────────────────────────────────────────────

async fn cmd_log(project_path: &std::path::Path, all: bool, limit: usize) -> Result<()> {
    let database = open_db()?;

    let project = if all {
        None
    } else {
        Some(
            project_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        )
    };

    let sessions = database.list_sessions(project.as_deref(), limit).await?;
    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    for s in &sessions {
        let dur = s.duration_secs.map(session::format_duration).unwrap_or_default();
        let calls = s.api_call_count.unwrap_or(0);
        let inp = s.input_tokens.unwrap_or(0);
        let out = s.output_tokens.unwrap_or(0);
        let cost = s.cost_usd.unwrap_or(0.0);
        let started = if s.started_at.len() >= 16 { &s.started_at[..16] } else { &s.started_at };

        print!(
            "  {started}  {dur:<8} {:>3} calls  {:.1}k in/{:.1}k out",
            calls,
            inp as f64 / 1000.0,
            out as f64 / 1000.0,
        );
        if cost > 0.0 {
            print!("  ${cost:.4}");
        }
        if s.files_changed.unwrap_or(0) > 0 {
            print!(
                "  +{}/-{} {}f",
                s.lines_added.unwrap_or(0),
                s.lines_removed.unwrap_or(0),
                s.files_changed.unwrap_or(0),
            );
        }
        println!();

        if let Some(ref summary) = s.summary {
            println!("    {summary}");
        }
        println!("    id: {}", s.id);
        println!();
    }
    Ok(())
}

// ── calls ─────────────────────────────────────────────────────────────

async fn cmd_calls(session: Option<&str>, limit: usize) -> Result<()> {
    let database = open_db()?;
    let calls = database.list_api_calls(session, limit).await?;

    if calls.is_empty() {
        println!("No API calls found.");
        return Ok(());
    }

    println!(
        "{:<20} {:<24} {:>6} {:>7} {:<20} {:>8} {:>8} {:>8}",
        "TIMESTAMP", "PATH", "STATUS", "LATENCY", "MODEL", "IN", "OUT", "CACHED"
    );
    println!("{}", "─".repeat(105));

    for c in &calls {
        let ts = if c.timestamp.len() >= 19 { &c.timestamp[..19] } else { &c.timestamp };
        let path = if c.path.len() > 22 { &c.path[..22] } else { &c.path };
        println!(
            "{:<20} {:<24} {:>6} {:>5}ms {:<20} {:>8} {:>8} {:>8}",
            ts,
            path,
            c.status_code.map(|s| s.to_string()).unwrap_or_else(|| "---".into()),
            c.latency_ms.unwrap_or(0),
            c.model.as_deref().unwrap_or(""),
            c.input_tokens.unwrap_or(0),
            c.output_tokens.unwrap_or(0),
            c.cache_read.unwrap_or(0),
        );
    }
    Ok(())
}

// ── events ────────────────────────────────────────────────────────────

async fn cmd_events(session: Option<&str>, name: Option<&str>, limit: usize) -> Result<()> {
    let database = open_db()?;
    let events = database.list_otel_events(session, name, limit).await?;

    if events.is_empty() {
        println!("No events found.");
        return Ok(());
    }

    for e in &events {
        let ts = if e.timestamp.len() >= 19 { &e.timestamp[..19] } else { &e.timestamp };

        // Extract key fields from attributes for display
        let attrs: serde_json::Value = e
            .attributes
            .as_deref()
            .and_then(|a| serde_json::from_str(a).ok())
            .unwrap_or(serde_json::Value::Null);

        print!("  {ts}  {:<16}", e.name);

        match e.name.as_str() {
            "api_request" => {
                let model = attrs.get("model").and_then(|v| v.as_str()).unwrap_or("");
                let cost = attrs.get("cost_usd").and_then(|v| v.as_str()).unwrap_or("0");
                let inp = attrs.get("input_tokens").and_then(|v| v.as_str()).unwrap_or("0");
                let out = attrs.get("output_tokens").and_then(|v| v.as_str()).unwrap_or("0");
                let dur = attrs.get("duration_ms").and_then(|v| v.as_str()).unwrap_or("0");
                print!("{model:<24} {inp:>6} in {out:>6} out  {dur:>5}ms  ${cost}");
            }
            "user_prompt" => {
                let prompt = attrs.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                let display = if prompt.len() > 80 {
                    format!("{}...", &prompt[..prompt.ceil_char_boundary(80)])
                } else {
                    prompt.to_string()
                };
                print!("\"{display}\"");
            }
            "tool_result" => {
                let tool = attrs.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let success = attrs.get("success").and_then(|v| v.as_str()).unwrap_or("");
                let dur = attrs.get("duration_ms").and_then(|v| v.as_str()).unwrap_or("0");
                let err = attrs.get("error").and_then(|v| v.as_str());
                print!("{tool:<16} success={success:<6} {dur:>5}ms");
                if let Some(err) = err {
                    let short = if err.len() > 50 { &err[..50] } else { err };
                    print!("  err: {short}");
                }
            }
            "tool_decision" => {
                let tool = attrs.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let decision = attrs.get("decision").and_then(|v| v.as_str()).unwrap_or("");
                let source = attrs.get("source").and_then(|v| v.as_str()).unwrap_or("");
                print!("{tool:<16} {decision:<8} ({source})");
            }
            _ => {
                // Generic: show first few attributes
                if let Some(obj) = attrs.as_object() {
                    let preview: Vec<String> = obj
                        .iter()
                        .take(3)
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect();
                    print!("{}", preview.join(" "));
                }
            }
        }
        println!();
    }
    Ok(())
}

// ── replay ────────────────────────────────────────────────────────────

async fn cmd_replay(session_id: &str) -> Result<()> {
    let database = open_db()?;

    // Support prefix matching — check sessions table first, then OTEL events
    let full_id = if session_id.len() < 36 {
        // Try sessions table
        let sessions = database.list_sessions(None, 100).await?;
        if let Some(s) = sessions.iter().find(|s| s.id.starts_with(session_id)) {
            s.id.clone()
        } else {
            // Fall back to OTEL event session IDs
            let events = database.list_otel_events(None, None, 500).await?;
            events
                .iter()
                .filter_map(|e| e.session_id.as_ref())
                .find(|sid| sid.starts_with(session_id))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no session matching prefix '{session_id}'"))?
        }
    } else {
        session_id.to_string()
    };

    let timeline = database.get_session_timeline(&full_id).await?;
    if timeline.is_empty() {
        println!("No events found for session {full_id}");
        return Ok(());
    }

    println!("Session {full_id}");
    println!("════════════════════════════════════════════════════════════\n");

    for e in &timeline {
        let ts = if e.timestamp.len() >= 19 { &e.timestamp[11..19] } else { &e.timestamp };
        let attrs: serde_json::Value = e
            .attributes
            .as_deref()
            .and_then(|a| serde_json::from_str(a).ok())
            .unwrap_or(serde_json::Value::Null);

        match e.name.as_str() {
            "user_prompt" => {
                let prompt = attrs.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                println!("  [{ts}] YOU:");
                for line in prompt.lines() {
                    println!("    {line}");
                }
                println!();
            }
            "api_request" => {
                let model = attrs.get("model").and_then(|v| v.as_str()).unwrap_or("?");
                let out = attrs.get("output_tokens").and_then(|v| v.as_str()).unwrap_or("0");
                let cost = attrs.get("cost_usd").and_then(|v| v.as_str()).unwrap_or("0");
                let dur = attrs.get("duration_ms").and_then(|v| v.as_str()).unwrap_or("0");
                println!("  [{ts}] API  {model}  {out} tokens  {dur}ms  ${cost}");
                println!();
            }
            "tool_decision" => {
                let tool = attrs.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let decision = attrs.get("decision").and_then(|v| v.as_str()).unwrap_or("");
                let source = attrs.get("source").and_then(|v| v.as_str()).unwrap_or("");
                println!("  [{ts}] TOOL {tool}  {decision} ({source})");
            }
            "tool_result" => {
                let tool = attrs.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let success = attrs.get("success").and_then(|v| v.as_str()) == Some("true");
                let dur = attrs.get("duration_ms").and_then(|v| v.as_str()).unwrap_or("0");
                let icon = if success { "OK" } else { "FAIL" };
                print!("  [{ts}]      {tool}  {icon}  {dur}ms");
                if let Some(err) = attrs.get("error").and_then(|v| v.as_str()) {
                    print!("  ({err})");
                }
                println!("\n");
            }
            _ => {
                // Show other events generically
                println!("  [{ts}] {:<14}", e.name);
            }
        }
    }

    // Print session totals
    let stats = database.get_otel_session_stats(&full_id).await?;
    let tools = database.get_otel_tool_usage(&full_id).await?;

    println!("════════════════════════════════════════════════════════════");
    println!(
        "  {} API calls  |  {} in / {} out tokens  |  ${:.4}",
        stats.api_calls, stats.input_tokens, stats.output_tokens, stats.cost_usd
    );
    if !tools.is_empty() {
        let tool_strs: Vec<String> = tools
            .iter()
            .map(|t| format!("{} {}x", t.tool_name, t.uses))
            .collect();
        println!("  Tools: {}", tool_strs.join(", "));
    }
    println!();
    Ok(())
}

// ── dashboard ─────────────────────────────────────────────────────────

async fn cmd_dashboard(project_path: &std::path::Path) -> Result<()> {
    let cfg = config::Config::load(project_path)?;
    let database = open_db()?;
    let port = cfg.dashboard.port;
    println!("Dashboard: http://127.0.0.1:{port}");
    dashboard::start(database, port).await?;
    Ok(())
}

// ── run (main flow) ───────────────────────────────────────────────────

async fn cmd_run(project_path: &std::path::Path, prompt: Option<&str>) -> Result<()> {
    let cfg = config::Config::load(project_path)?;
    let project_name = project_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());

    // Database
    let db_path = data_dir()?.join("laudec.db");
    let database = db::Db::open(&db_path)?;

    // Settings (restored on drop)
    let _settings_guard = settings::SettingsGuard::apply(project_path, &cfg)?;

    let run_id = uuid::Uuid::new_v4().to_string();
    let started_at = chrono::Utc::now().to_rfc3339();

    // Banner
    print_banner(&cfg, &project_name);

    // ── start proxy ──────────────────────────────────────────────────
    if cfg.proxy.enabled && cfg.proxy.remote.is_none() {
        let state = Arc::new(proxy::ProxyState {
            db: database.clone(),
            client: reqwest::Client::new(),
            config: cfg.proxy.clone(),
            run_id: run_id.clone(),
        });
        let port = cfg.proxy.port;
        tokio::spawn(async move {
            if let Err(e) = proxy::start(state, port).await {
                tracing::error!("proxy: {e}");
            }
        });
    }

    // ── start OTEL collector ─────────────────────────────────────────
    if cfg.telemetry.enabled && cfg.telemetry.remote.is_none() {
        let db_clone = database.clone();
        let port = cfg.telemetry.collector_port;
        let run_id_clone = run_id.clone();
        tokio::spawn(async move {
            if let Err(e) = collector::start(db_clone, port, run_id_clone).await {
                tracing::error!("collector: {e}");
            }
        });
    }

    // ── start dashboard ────────────────────────────────────────────
    if cfg.dashboard.enabled {
        let db_clone = database.clone();
        let port = cfg.dashboard.port;
        tokio::spawn(async move {
            if let Err(e) = dashboard::start(db_clone, port).await {
                tracing::error!("dashboard: {e}");
            }
        });
    }

    // Let servers bind
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // ── launch Claude Code ───────────────────────────────────────────
    let mut child = launcher::launch_claude(project_path, &cfg, prompt)?;
    let status = child.wait().context("waiting for claude")?;
    tracing::info!("claude exited: {status}");

    // ── post-session bookkeeping ─────────────────────────────────────
    // Discover CC's native session.id from OTEL events
    let cc_session_id = database.find_cc_session_id(&started_at).await?;

    let summary = session::post_session(
        &database,
        &run_id,
        cc_session_id.as_deref(),
        project_path,
        &project_name,
        &started_at,
        None,
    )
    .await?;

    session::print_summary(&summary);

    // Show replay hint
    let short_id = &run_id[..8];
    println!("  replay:       laudec replay {short_id}");
    println!();

    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────

fn open_db() -> Result<db::Db> {
    let db_path = data_dir()?.join("laudec.db");
    if !db_path.exists() {
        anyhow::bail!("No data yet. Run `laudec .` first.");
    }
    db::Db::open(&db_path)
}

fn print_banner(cfg: &config::Config, project_name: &str) {
    println!("laudec v{}\n", env!("CARGO_PKG_VERSION"));

    let sandbox = if cfg.sandbox.enabled { "on" } else { "off" };
    let domains = if cfg.sandbox.allowed_domains.is_empty() {
        "api.anthropic.com".into()
    } else {
        format!(
            "api.anthropic.com, {}",
            cfg.sandbox.allowed_domains.join(", ")
        )
    };

    println!("  project:      {project_name}");
    println!("  sandbox:      {sandbox} (domains: {domains})");

    if cfg.proxy.enabled {
        if let Some(ref remote) = cfg.proxy.remote {
            println!("  proxy:        {remote} (remote)");
        } else {
            println!(
                "  proxy:        http://127.0.0.1:{} -> api.anthropic.com",
                cfg.proxy.port
            );
        }
    }

    if cfg.telemetry.enabled {
        if let Some(ref remote) = cfg.telemetry.remote {
            println!("  telemetry:    {remote} (remote)");
        } else {
            println!(
                "  telemetry:    http://127.0.0.1:{} (local)",
                cfg.telemetry.collector_port
            );
        }
    }

    if cfg.dashboard.enabled {
        println!(
            "  dashboard:    http://127.0.0.1:{}",
            cfg.dashboard.port
        );
    }

    println!(
        "  permissions:  {} allow, {} deny, mode={}",
        cfg.permissions.allow.len(),
        cfg.permissions.deny.len(),
        cfg.permissions.mode,
    );

    println!("\nLaunching Claude Code...");
    println!("────────────────────────────────────────────");
}

fn data_dir() -> Result<PathBuf> {
    let dir = if let Ok(d) = std::env::var("LAUDEC_DATA_DIR") {
        PathBuf::from(d)
    } else {
        dirs::data_local_dir()
            .context("cannot determine data directory")?
            .join("laudec")
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
