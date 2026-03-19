use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};

use crate::config::Config;

pub fn launch_claude(
    project_path: &Path,
    config: &Config,
    prompt: Option<&str>,
) -> Result<Child> {
    let mut env_vars: HashMap<String, String> = HashMap::new();

    // Route API traffic through our proxy
    if config.proxy.enabled && config.proxy.remote.is_none() {
        env_vars.insert(
            "ANTHROPIC_BASE_URL".into(),
            format!("http://127.0.0.1:{}", config.proxy.port),
        );
    } else if let Some(ref remote) = config.proxy.remote {
        env_vars.insert("ANTHROPIC_BASE_URL".into(), remote.clone());
    }

    // Forward API key from host environment
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        env_vars.insert("ANTHROPIC_API_KEY".into(), key);
    }

    // Override model if configured
    if let Some(ref model) = config.claude.model {
        env_vars.insert("CLAUDE_CODE_DEFAULT_MODEL".into(), model.clone());
    }

    let mut cmd = Command::new("claude");
    cmd.current_dir(project_path)
        .envs(&env_vars)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    // If a prompt is given, run in print mode (non-interactive)
    if let Some(p) = prompt {
        cmd.arg("-p").arg(p);
    }

    let child = cmd
        .spawn()
        .context("Failed to launch 'claude'. Is Claude Code installed?")?;

    Ok(child)
}
