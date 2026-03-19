use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::Config;

/// RAII guard that writes Claude Code settings on creation and restores the
/// original file (or removes ours) when dropped.
pub struct SettingsGuard {
    settings_path: PathBuf,
    backup_path: Option<PathBuf>,
    restored: bool,
}

impl SettingsGuard {
    pub fn apply(project_path: &Path, config: &Config) -> Result<Self> {
        let claude_dir = project_path.join(".claude");
        std::fs::create_dir_all(&claude_dir)?;

        let settings_path = claude_dir.join("settings.local.json");
        let backup_path = claude_dir.join("settings.local.json.laudec-backup");

        let had_backup = if settings_path.exists() {
            std::fs::copy(&settings_path, &backup_path)
                .context("backing up existing settings.local.json")?;
            Some(backup_path)
        } else {
            None
        };

        let settings = generate_settings(config);
        let json = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&settings_path, json)?;

        Ok(Self {
            settings_path,
            backup_path: had_backup,
            restored: false,
        })
    }

    fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;

        if let Some(ref backup) = self.backup_path {
            let _ = std::fs::rename(backup, &self.settings_path);
        } else {
            let _ = std::fs::remove_file(&self.settings_path);
        }
    }
}

impl Drop for SettingsGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

fn generate_settings(config: &Config) -> serde_json::Value {
    let mut settings = serde_json::Map::new();

    // Sandbox
    if config.sandbox.enabled {
        let mut sandbox = serde_json::Map::new();
        sandbox.insert("enabled".into(), true.into());
        if !config.sandbox.allowed_domains.is_empty() {
            sandbox.insert(
                "allowedDomains".into(),
                config
                    .sandbox
                    .allowed_domains
                    .iter()
                    .map(|d| serde_json::Value::String(d.clone()))
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        settings.insert("sandbox".into(), sandbox.into());
    }

    // Permissions
    let mut permissions = serde_json::Map::new();
    if !config.permissions.allow.is_empty() {
        permissions.insert(
            "allow".into(),
            config
                .permissions
                .allow
                .iter()
                .map(|t| serde_json::Value::String(t.clone()))
                .collect::<Vec<_>>()
                .into(),
        );
    }
    if !config.permissions.deny.is_empty() {
        permissions.insert(
            "deny".into(),
            config
                .permissions
                .deny
                .iter()
                .map(|t| serde_json::Value::String(t.clone()))
                .collect::<Vec<_>>()
                .into(),
        );
    }
    if !permissions.is_empty() {
        settings.insert("permissions".into(), permissions.into());
    }

    // Env (OTEL configuration)
    if config.telemetry.enabled {
        let mut env = serde_json::Map::new();
        env.insert("CLAUDE_CODE_ENABLE_TELEMETRY".into(), "1".into());
        env.insert("OTEL_METRICS_EXPORTER".into(), "otlp".into());
        env.insert("OTEL_LOGS_EXPORTER".into(), "otlp".into());
        env.insert("OTEL_EXPORTER_OTLP_PROTOCOL".into(), "grpc".into());

        let endpoint = config
            .telemetry
            .remote
            .clone()
            .unwrap_or_else(|| format!("http://127.0.0.1:{}", config.telemetry.collector_port));
        env.insert("OTEL_EXPORTER_OTLP_ENDPOINT".into(), endpoint.into());

        if config.telemetry.log_prompts {
            env.insert("OTEL_LOG_USER_PROMPTS".into(), "1".into());
        }
        if config.telemetry.log_tool_details {
            env.insert("OTEL_LOG_TOOL_DETAILS".into(), "1".into());
        }

        settings.insert("env".into(), env.into());
    }

    serde_json::Value::Object(settings)
}
