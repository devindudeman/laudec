use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct Config {
    pub sandbox: SandboxConfig,
    pub permissions: PermissionsConfig,
    pub proxy: ProxyConfig,
    pub telemetry: TelemetryConfig,
    pub dashboard: DashboardConfig,
    pub session: SessionConfig,
    pub claude: ClaudeConfig,
    pub cloud: CloudConfig,
}

#[derive(Debug, Serialize, Clone)]
pub struct ConfigWithSource {
    pub source: String,
    #[serde(flatten)]
    pub config: Config,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub allowed_domains: Vec<String>,
    pub allow_write: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct PermissionsConfig {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub mode: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct ProxyConfig {
    pub enabled: bool,
    pub port: u16,
    pub log_requests: bool,
    pub log_responses: bool,
    pub redact_keys: bool,
    pub remote: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub collector_port: u16,
    pub log_prompts: bool,
    pub log_tool_details: bool,
    pub forward: Option<String>,
    pub forward_headers: Option<HashMap<String, String>>,
    pub remote: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct DashboardConfig {
    pub enabled: bool,
    pub port: u16,
    pub open_browser: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct SessionConfig {
    pub summary: bool,
    pub summary_model: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct ClaudeConfig {
    pub model: Option<String>,
    pub claude_md: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct CloudConfig {
    pub enabled: bool,
    /// The Convex HTTP actions URL (e.g. https://your-project.convex.site)
    pub endpoint: Option<String>,
    /// API key for authentication (created in the cloud dashboard)
    pub api_key: Option<String>,
    /// Push full request/response bodies (large, disabled by default)
    pub push_bodies: bool,
}

// --- Defaults ---

impl Default for Config {
    fn default() -> Self {
        Self {
            sandbox: SandboxConfig::default(),
            permissions: PermissionsConfig::default(),
            proxy: ProxyConfig::default(),
            telemetry: TelemetryConfig::default(),
            dashboard: DashboardConfig::default(),
            session: SessionConfig::default(),
            claude: ClaudeConfig::default(),
            cloud: CloudConfig::default(),
        }
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_domains: vec![],
            allow_write: vec![],
        }
    }
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            allow: vec![
                "Read".into(),
                "Write".into(),
                "Edit".into(),
                "Glob".into(),
                "Grep".into(),
                "Bash(git *)".into(),
            ],
            deny: vec![
                "Bash(rm -rf *)".into(),
                "Bash(sudo *)".into(),
                "Read(.env*)".into(),
            ],
            mode: "plan".into(),
        }
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 18080,
            log_requests: true,
            log_responses: true,
            redact_keys: true,
            remote: None,
        }
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collector_port: 14317,
            log_prompts: true,
            log_tool_details: true,
            forward: None,
            forward_headers: None,
            remote: None,
        }
    }
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 18384,
            open_browser: false,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            summary: true,
            summary_model: "claude-haiku-4-5-20251001".into(),
        }
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            model: None,
            claude_md: None,
        }
    }
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            api_key: None,
            push_bodies: true,
        }
    }
}

// --- Loading ---

impl Config {
    pub fn load(project_path: &Path) -> Result<Self> {
        Ok(Self::load_with_source(project_path)?.config)
    }

    pub fn load_with_source(project_path: &Path) -> Result<ConfigWithSource> {
        // 1. Project-level laudec.toml
        let project_config = project_path.join("laudec.toml");
        if project_config.exists() {
            let config = Self::load_file(&project_config)?;
            return Ok(ConfigWithSource {
                source: project_config.display().to_string(),
                config,
            });
        }

        // 2. User-level ~/.config/laudec/config.toml
        if let Some(config_dir) = dirs::config_dir() {
            let user_config = config_dir.join("laudec").join("config.toml");
            if user_config.exists() {
                let config = Self::load_file(&user_config)?;
                return Ok(ConfigWithSource {
                    source: user_config.display().to_string(),
                    config,
                });
            }
        }

        // 3. Defaults
        Ok(ConfigWithSource {
            source: "defaults".into(),
            config: Config::default(),
        })
    }

    fn load_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))
    }

    pub fn starter_toml() -> &'static str {
        r#"# laudec.toml — all fields optional, shown with defaults

[sandbox]
enabled = true
# allowed_domains = ["github.com", "registry.npmjs.org"]
# allow_write = ["/tmp"]

[permissions]
allow = [
  "Read",
  "Write",
  "Edit",
  "Glob",
  "Grep",
  "Bash(git *)",
]

deny = [
  "Bash(rm -rf *)",
  "Bash(sudo *)",
  "Read(.env*)",
]

# "plan" = ask before most actions (default)
# "auto" = auto-approve file edits
# "bypassPermissions" = approve everything (only if sandbox is on)
mode = "plan"

[proxy]
enabled = true
port = 18080
log_requests = true
log_responses = true
redact_keys = true

[telemetry]
enabled = true
collector_port = 14317
log_prompts = true
log_tool_details = true
# forward = "http://otel.corp:4317"

[dashboard]
enabled = true
port = 18384
# open_browser = true

[session]
summary = true
# summary_model = "claude-haiku-4-5-20251001"

[claude]
# model = "claude-sonnet-4-20250514"
# claude_md = "./CLAUDE.md"

[cloud]
# Push session data to a centralized Convex dashboard
# enabled = true
# endpoint = "https://your-project.convex.site"
# api_key = "ldc_..."
# push_bodies = true  # Set to false to skip request/response bodies (saves bandwidth)
"#
    }
}
