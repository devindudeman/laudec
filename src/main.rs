mod cloud;
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
    /// Show usage statistics
    Stats {
        /// Number of days to include (default: 7)
        #[arg(long, default_value = "7")]
        days: u64,
    },
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
        Some(Commands::Stats { days }) => cmd_stats(days).await,
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
    let cfg_with_source = config::Config::load_with_source(project_path)?;
    let port = cfg_with_source.config.dashboard.port;
    let database = open_db()?;
    println!("Dashboard: http://127.0.0.1:{port}");
    dashboard::start(database, port, Arc::new(cfg_with_source)).await?;
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
        let cfg_with_source = Arc::new(config::Config::load_with_source(project_path)?);
        tokio::spawn(async move {
            if let Err(e) = dashboard::start(db_clone, port, cfg_with_source).await {
                tracing::error!("dashboard: {e}");
            }
        });
    }

    // ── start cloud pusher ─────────────────────────────────────────
    let cloud_pusher = cloud::CloudPusher::start(&cfg.cloud);
    if cloud_pusher.is_some() {
        tracing::info!("cloud push enabled");
    }

    // Push initial "active" session to cloud
    if let Some(ref pusher) = cloud_pusher {
        pusher.push_session(cloud::SessionPayload {
            run_id: run_id.clone(),
            cc_session_id: None,
            project: project_name.clone(),
            project_path: Some(project_path.to_string_lossy().to_string()),
            started_at: started_at.clone(),
            ended_at: None,
            duration_secs: None,
            api_call_count: None,
            input_tokens: None,
            output_tokens: None,
            cache_read: None,
            cache_write: None,
            cost_usd: None,
            model: None,
            files_changed: None,
            lines_added: None,
            lines_removed: None,
            changed_files: None,
            summary: None,
            tool_uses: None,
            first_prompt: None,
            error_count: None,
            machine_id: None,
            status: "active".into(),
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

    // ── push completed session + data to cloud ───────────────────────
    if let Some(ref pusher) = cloud_pusher {
        // Push completed session
        pusher.push_session(cloud::SessionPayload {
            run_id: run_id.clone(),
            cc_session_id: cc_session_id.clone(),
            project: project_name.clone(),
            project_path: Some(project_path.to_string_lossy().to_string()),
            started_at: started_at.clone(),
            ended_at: Some(chrono::Utc::now().to_rfc3339()),
            duration_secs: Some(summary.duration_secs),
            api_call_count: Some(summary.api_calls),
            input_tokens: Some(summary.input_tokens),
            output_tokens: Some(summary.output_tokens),
            cache_read: Some(summary.cache_read),
            cache_write: None,
            cost_usd: Some(summary.cost_usd),
            model: summary.model.clone(),
            files_changed: Some(summary.files_changed),
            lines_added: Some(summary.lines_added),
            lines_removed: Some(summary.lines_removed),
            changed_files: None,
            summary: summary.summary_text.clone(),
            tool_uses: Some(summary.tool_uses),
            first_prompt: None,
            error_count: None,
            machine_id: None,
            status: "completed".into(),
        });

        // Push API calls
        let calls = database.list_api_calls(Some(&run_id), 1000).await?;
        if !calls.is_empty() {
            let call_records: Vec<cloud::CallRecord> =
                calls.iter().map(cloud::CallRecord::from).collect();
            // Batch in groups of 50 to stay within Convex mutation limits
            for chunk in call_records.chunks(50) {
                pusher.push_calls(cloud::CallsPayload {
                    session_id: String::new(), // Will be replaced by worker with cloud ID
                    calls: chunk.to_vec(),
                });
            }
        }

        // Push OTEL events
        let otel_id = cc_session_id.as_deref().unwrap_or(&run_id);
        let events = database.list_otel_events(Some(otel_id), None, 1000).await?;
        if !events.is_empty() {
            let event_records: Vec<cloud::EventRecord> =
                events.iter().map(cloud::EventRecord::from).collect();
            for chunk in event_records.chunks(50) {
                pusher.push_events(cloud::EventsPayload {
                    session_id: String::new(),
                    events: chunk.to_vec(),
                });
            }
        }

        // Give the worker time to flush
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Shut down cloud pusher
    if let Some(pusher) = cloud_pusher {
        pusher.shutdown().await;
    }

    // Show replay hint
    let short_id = &run_id[..8];
    println!("  replay:       laudec replay {short_id}");
    println!();

    Ok(())
}

// ── stats ─────────────────────────────────────────────────────────────

async fn cmd_stats(days: u64) -> Result<()> {
    let database = open_db()?;

    let since = (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
    let today = chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
    let today_since = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(today, chrono::Utc).to_rfc3339();

    let period_stats = database.get_aggregate_stats(Some(&since)).await?;
    let today_stats = database.get_aggregate_stats(Some(&today_since)).await?;
    let models = database.get_model_distribution(Some(&since)).await?;
    let tools = database.get_aggregate_tool_usage(Some(&since)).await?;

    let period_tokens = period_stats.total_input + period_stats.total_output;
    let today_tokens = today_stats.total_input + today_stats.total_output;

    println!(
        "Last {} days:  {} sessions · ${:.2} · {} tokens",
        days,
        period_stats.session_count,
        period_stats.total_cost,
        fmt_token_count(period_tokens),
    );
    println!(
        "Today:         {} sessions · ${:.2} · {} tokens",
        today_stats.session_count,
        today_stats.total_cost,
        fmt_token_count(today_tokens),
    );

    if let Some((model, count)) = models.first() {
        let total: i64 = models.iter().map(|(_, c)| c).sum();
        let pct = if total > 0 { count * 100 / total } else { 0 };
        println!("Top model:     {} ({}%)", model, pct);
    }

    if !tools.is_empty() {
        let tool_strs: Vec<String> = tools
            .iter()
            .map(|(name, count)| format!("{} ({})", name, count))
            .collect();
        println!("Top tools:     {}", tool_strs.join(" · "));
    }

    Ok(())
}

fn fmt_token_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        n.to_string()
    }
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

    if cfg.cloud.enabled {
        if let Some(ref endpoint) = cfg.cloud.endpoint {
            println!("  cloud:        {endpoint}");
        }
    }

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
