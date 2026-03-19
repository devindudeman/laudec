use std::path::Path;

use anyhow::Result;

use crate::db::Db;

pub struct SessionSummary {
    pub duration_secs: i64,
    pub api_calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cost_usd: f64,
    pub model: Option<String>,
    pub files_changed: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub tool_uses: i64,
    pub tools: Vec<crate::db::ToolUsage>,
    pub summary_text: Option<String>,
}

pub async fn post_session(
    db: &Db,
    run_id: &str,
    cc_session_id: Option<&str>,
    project_path: &Path,
    project_name: &str,
    started_at: &str,
    config_snapshot: Option<&str>,
) -> Result<SessionSummary> {
    let ended_at = chrono::Utc::now().to_rfc3339();

    // ── Stats from OTEL events (keyed by CC's session ID) ────────────
    // Write session ID mapping as guaranteed fallback
    // (covers remote collector mode and any missed early mappings)
    if let Some(cc_sid) = cc_session_id {
        let _ = db.insert_session_id_mapping(run_id, cc_sid).await;
    }

    let otel_id = cc_session_id.unwrap_or(run_id);
    let otel_stats = db.get_otel_session_stats(otel_id).await?;
    let tools = db.get_otel_tool_usage(otel_id).await?;
    let prompts = db.get_user_prompts(otel_id).await?;

    // ── Git diff ─────────────────────────────────────────────────────
    let (files_changed, lines_added, lines_removed, changed_files) =
        git_diff_stats(project_path).await;

    let duration_secs = {
        let s = chrono::DateTime::parse_from_rfc3339(started_at).ok();
        let e = chrono::DateTime::parse_from_rfc3339(&ended_at).ok();
        match (s, e) {
            (Some(s), Some(e)) => (e - s).num_seconds(),
            _ => 0,
        }
    };

    // ── Generate summary text ────────────────────────────────────────
    let summary_text = generate_summary(&prompts, &tools);

    let tool_uses: i64 = tools.iter().map(|t| t.uses).sum();
    let error_count: i64 = tools.iter().map(|t| t.failures).sum();
    let first_prompt = prompts.first().map(|p| {
        if p.len() > 200 {
            p[..p.ceil_char_boundary(200)].to_string()
        } else {
            p.clone()
        }
    });
    let changed_files_json = serde_json::to_string(&changed_files).ok();

    // ── Write session record ─────────────────────────────────────────
    db.insert_session(
        run_id,
        project_name,
        &project_path.to_string_lossy(),
        started_at,
        &ended_at,
        duration_secs,
        otel_stats.api_calls,
        otel_stats.input_tokens,
        otel_stats.output_tokens,
        otel_stats.cache_read,
        otel_stats.cache_creation,
        otel_stats.model.as_deref(),
        files_changed,
        lines_added,
        lines_removed,
        changed_files_json.as_deref(),
        summary_text.as_deref(),
        config_snapshot,
        otel_stats.cost_usd,
        tool_uses,
        cc_session_id,
        first_prompt.as_deref(),
        error_count,
    )
    .await?;

    Ok(SessionSummary {
        duration_secs,
        api_calls: otel_stats.api_calls,
        input_tokens: otel_stats.input_tokens,
        output_tokens: otel_stats.output_tokens,
        cache_read: otel_stats.cache_read,
        cost_usd: otel_stats.cost_usd,
        model: otel_stats.model,
        files_changed,
        lines_added,
        lines_removed,
        tool_uses,
        tools,
        summary_text,
    })
}

fn generate_summary(prompts: &[String], tools: &[crate::db::ToolUsage]) -> Option<String> {
    if prompts.is_empty() {
        return None;
    }

    let mut parts = Vec::new();

    // Summarize what was asked
    let prompt_summary: Vec<&str> = prompts
        .iter()
        .map(|p| {
            if p.len() > 60 {
                // Take first 60 chars and find word boundary
                let truncated = &p[..p.ceil_char_boundary(60)];
                truncated
            } else {
                p.as_str()
            }
        })
        .collect();

    if prompt_summary.len() == 1 {
        parts.push(format!("Asked: \"{}\"", prompt_summary[0]));
    } else {
        parts.push(format!(
            "{} prompts. Started with: \"{}\"",
            prompts.len(),
            prompt_summary[0]
        ));
    }

    // Summarize tools used
    if !tools.is_empty() {
        let tool_strs: Vec<String> = tools
            .iter()
            .map(|t| {
                if t.failures > 0 {
                    format!("{} ({}x, {} failed)", t.tool_name, t.uses, t.failures)
                } else {
                    format!("{} ({}x)", t.tool_name, t.uses)
                }
            })
            .collect();
        parts.push(format!("Tools: {}", tool_strs.join(", ")));
    }

    Some(parts.join(". "))
}

async fn git_diff_stats(project_path: &Path) -> (i64, i64, i64, Vec<String>) {
    let output = tokio::process::Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(project_path)
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => parse_git_stat(&String::from_utf8_lossy(&o.stdout)),
        _ => (0, 0, 0, vec![]),
    }
}

fn parse_git_stat(stat: &str) -> (i64, i64, i64, Vec<String>) {
    let mut files = Vec::new();
    let mut added: i64 = 0;
    let mut removed: i64 = 0;

    for line in stat.lines() {
        let line = line.trim();
        if line.contains('|') {
            if let Some(file) = line.split('|').next() {
                files.push(file.trim().to_string());
            }
        }
        if line.contains("insertion") || line.contains("deletion") {
            for part in line.split(',') {
                let part = part.trim();
                if part.contains("insertion") {
                    added = part
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                }
                if part.contains("deletion") {
                    removed = part
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                }
            }
        }
    }

    (files.len() as i64, added, removed, files)
}

pub fn print_summary(summary: &SessionSummary) {
    println!("────────────────────────────────────────────");
    println!("Session ended.\n");

    println!("  duration:     {}", format_duration(summary.duration_secs));

    if let Some(ref model) = summary.model {
        println!("  model:        {model}");
    }

    println!("  api calls:    {} requests", summary.api_calls);
    println!(
        "  tokens:       {:.1}k in / {:.1}k out / {:.1}k cached",
        summary.input_tokens as f64 / 1000.0,
        summary.output_tokens as f64 / 1000.0,
        summary.cache_read as f64 / 1000.0,
    );

    if summary.cost_usd > 0.0 {
        println!("  cost:         ${:.4}", summary.cost_usd);
    }

    if summary.tool_uses > 0 {
        let tool_strs: Vec<String> = summary
            .tools
            .iter()
            .map(|t| {
                if t.failures > 0 {
                    format!("{} {}x ({} failed)", t.tool_name, t.uses, t.failures)
                } else {
                    format!("{} {}x", t.tool_name, t.uses)
                }
            })
            .collect();
        println!("  tools:        {}", tool_strs.join(", "));
    }

    if summary.files_changed > 0 {
        println!(
            "  files:        {} changed (+{} / -{})",
            summary.files_changed, summary.lines_added, summary.lines_removed
        );
    }

    if let Some(ref text) = summary.summary_text {
        println!("  summary:      {text}");
    }

    println!();
}

pub fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
