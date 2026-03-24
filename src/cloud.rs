//! Cloud push client — sends session data to a Convex cloud dashboard.
//!
//! Operates as a best-effort background pusher: data always goes to local SQLite
//! first, then gets pushed to the cloud endpoint when available. Failures are
//! logged but never block the local workflow.

use anyhow::Result;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::config::CloudConfig;
use crate::db::{ApiCallRecord, OtelEventRecord};

/// Messages sent to the cloud push worker.
#[derive(Debug)]
pub enum CloudMsg {
    /// Push or update a session record.
    Session(SessionPayload),
    /// Push a batch of API call records.
    Calls(CallsPayload),
    /// Push a batch of OTEL events.
    Events(EventsPayload),
    /// Graceful shutdown — flush remaining items.
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPayload {
    pub run_id: String,
    pub cc_session_id: Option<String>,
    pub project: String,
    pub project_path: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_secs: Option<i64>,
    pub api_call_count: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    pub cost_usd: Option<f64>,
    pub model: Option<String>,
    pub files_changed: Option<i64>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub changed_files: Option<String>,
    pub summary: Option<String>,
    pub tool_uses: Option<i64>,
    pub first_prompt: Option<String>,
    pub error_count: Option<i64>,
    pub machine_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallsPayload {
    pub session_id: String,
    pub calls: Vec<CallRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRecord {
    pub call_id: String,
    pub timestamp: String,
    pub method: String,
    pub path: String,
    pub status_code: Option<i64>,
    pub latency_ms: Option<i64>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    pub response_text: Option<String>,
    // Classification metadata (always sent, extracted from request body)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_query: Option<String>,
    // Full bodies (only when push_bodies=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_headers: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_headers: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsPayload {
    pub session_id: String,
    pub events: Vec<EventRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRecord {
    pub timestamp: String,
    pub name: String,
    pub body: Option<String>,
    pub attributes: Option<String>,
    pub severity: Option<String>,
}

/// Handle to the cloud push worker. Send messages via `tx`.
pub struct CloudPusher {
    pub tx: mpsc::UnboundedSender<CloudMsg>,
    worker_handle: tokio::task::JoinHandle<()>,
}

impl CloudPusher {
    /// Start the cloud push worker. Returns None if cloud is not configured.
    pub fn start(config: &CloudConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }
        let endpoint = config.endpoint.clone()?;
        let api_key = config.api_key.clone()?;

        let (tx, rx) = mpsc::unbounded_channel();
        let client = reqwest::Client::new();

        let worker_handle = tokio::spawn(cloud_worker(rx, client, endpoint, api_key));

        Some(Self { tx, worker_handle })
    }

    /// Send a session update (best-effort).
    pub fn push_session(&self, payload: SessionPayload) {
        let _ = self.tx.send(CloudMsg::Session(payload));
    }

    /// Send API calls (best-effort).
    pub fn push_calls(&self, payload: CallsPayload) {
        let _ = self.tx.send(CloudMsg::Calls(payload));
    }

    /// Send OTEL events (best-effort).
    pub fn push_events(&self, payload: EventsPayload) {
        let _ = self.tx.send(CloudMsg::Events(payload));
    }

    /// Gracefully shut down, flushing pending items.
    pub async fn shutdown(self) {
        let _ = self.tx.send(CloudMsg::Shutdown);
        let _ = self.worker_handle.await;
    }
}

/// Background worker that processes cloud push messages.
async fn cloud_worker(
    mut rx: mpsc::UnboundedReceiver<CloudMsg>,
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
) {
    // Track the cloud session ID returned by the server for calls/events pushes
    let mut cloud_session_id: Option<String> = None;

    while let Some(msg) = rx.recv().await {
        match msg {
            CloudMsg::Session(payload) => {
                match post_json(
                    &client,
                    &format!("{endpoint}/api/ingest/session"),
                    &api_key,
                    &payload,
                )
                .await
                {
                    Ok(resp) => {
                        if let Some(sid) = resp
                            .get("sessionId")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                        {
                            cloud_session_id = Some(sid);
                            tracing::debug!("cloud: session pushed (id: {})", cloud_session_id.as_deref().unwrap_or("?"));
                        }
                    }
                    Err(e) => tracing::warn!("cloud push session: {e}"),
                }
            }
            CloudMsg::Calls(mut payload) => {
                if let Some(ref sid) = cloud_session_id {
                    payload.session_id = sid.clone();
                }
                match post_json(
                    &client,
                    &format!("{endpoint}/api/ingest/calls"),
                    &api_key,
                    &payload,
                )
                .await
                {
                    Ok(_) => tracing::debug!("cloud: {} calls pushed", payload.calls.len()),
                    Err(e) => tracing::warn!("cloud push calls: {e}"),
                }
            }
            CloudMsg::Events(mut payload) => {
                if let Some(ref sid) = cloud_session_id {
                    payload.session_id = sid.clone();
                }
                match post_json(
                    &client,
                    &format!("{endpoint}/api/ingest/events"),
                    &api_key,
                    &payload,
                )
                .await
                {
                    Ok(_) => tracing::debug!("cloud: {} events pushed", payload.events.len()),
                    Err(e) => tracing::warn!("cloud push events: {e}"),
                }
            }
            CloudMsg::Shutdown => {
                tracing::debug!("cloud: worker shutting down");
                break;
            }
        }
    }
}

async fn post_json<T: Serialize>(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &T,
) -> Result<serde_json::Value> {
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await?;

    let status = resp.status();
    let text = resp.text().await?;

    if !status.is_success() {
        anyhow::bail!("HTTP {status}: {text}");
    }

    Ok(serde_json::from_str(&text).unwrap_or(serde_json::json!({})))
}

// ── Conversion helpers (from DB records) ─────────────────────────────

impl CallRecord {
    pub fn from_db(c: &ApiCallRecord, push_bodies: bool) -> Self {
        // Classify the call from request body (without sending the body itself)
        let (call_type, call_detail, tool_tags, user_query) =
            classify_call(c.request_body.as_deref(), &c.path);

        Self {
            call_id: format!("{}-{}", c.timestamp, c.path),
            timestamp: c.timestamp.clone(),
            method: c.method.clone(),
            path: c.path.clone(),
            status_code: c.status_code,
            latency_ms: c.latency_ms,
            model: c.model.clone(),
            input_tokens: c.input_tokens,
            output_tokens: c.output_tokens,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
            response_text: c.response_text.clone(),
            call_type: Some(call_type),
            call_detail,
            tool_tags,
            user_query,
            request_body: if push_bodies { c.request_body.clone() } else { None },
            response_body: if push_bodies { c.response_body.clone() } else { None },
            request_headers: if push_bodies { c.request_headers.clone() } else { None },
            response_headers: if push_bodies { c.response_headers.clone() } else { None },
        }
    }
}

/// Classify an API call from its request body, matching the logic in the
/// Svelte/React dashboard but running on the Rust side.
fn classify_call(request_body: Option<&str>, path: &str) -> (String, Option<String>, Option<String>, Option<String>) {
    let body: serde_json::Value = request_body
        .and_then(|b| serde_json::from_str(b).ok())
        .unwrap_or(serde_json::Value::Null);

    // Type classification
    if body.get("max_tokens").and_then(|v| v.as_i64()) == Some(1) {
        return ("QUOTA".into(), None, None, None);
    }
    if path.contains("count_tokens") {
        return ("TOKEN COUNT".into(), None, None, None);
    }

    let mut call_type = "UNKNOWN".to_string();
    let mut call_detail: Option<String> = None;

    if body.get("thinking").is_some() {
        call_type = "MAIN".into();
    } else if body.get("system").is_some() && body.get("tools").is_some() {
        call_type = "SUBAGENT".into();
        if let Some(sys) = body.get("system") {
            let sys_str = sys.to_string();
            if sys_str.contains("file search specialist") || sys_str.contains("READ-ONLY MODE") {
                call_detail = Some("EXPLORE".into());
            } else if sys_str.contains("web search tool use") {
                call_detail = Some("WEB SEARCH".into());
            } else if sys_str.contains("Claude Code Guide") {
                call_detail = Some("CC GUIDE".into());
            }
        }
    }

    // Extract tool usage tags from assistant messages
    let mut tool_counts: Vec<(String, usize)> = Vec::new();
    if let Some(msgs) = body.get("messages").and_then(|v| v.as_array()) {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for m in msgs {
            if m.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                if let Some(content) = m.get("content").and_then(|v| v.as_array()) {
                    for block in content {
                        if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            if let Some(name) = block.get("name").and_then(|v| v.as_str()) {
                                *counts.entry(name.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
        }
        tool_counts = counts.into_iter().collect();
        tool_counts.sort_by(|a, b| b.1.cmp(&a.1));
        tool_counts.truncate(4);
    }

    let tool_tags = if tool_counts.is_empty() {
        None
    } else {
        Some(
            tool_counts
                .iter()
                .map(|(name, count)| {
                    if *count > 1 {
                        format!("{} ×{}", name, count)
                    } else {
                        name.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" · "),
        )
    };

    // Extract the last user query (text only, strip system blocks)
    let user_query = extract_user_query(&body);

    (call_type, call_detail, tool_tags, user_query)
}

/// Extract the last user message text from a request body, stripping system-injected blocks.
fn extract_user_query(body: &serde_json::Value) -> Option<String> {
    let msgs = body.get("messages")?.as_array()?;
    for m in msgs.iter().rev() {
        if m.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let raw = if let Some(s) = m.get("content").and_then(|v| v.as_str()) {
            s.to_string()
        } else if let Some(arr) = m.get("content").and_then(|v| v.as_array()) {
            arr.iter()
                .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            continue;
        };

        // Strip system-injected XML blocks
        let re_pattern = "<(system-reminder|available-deferred-tools|tool-use-rules|functions)>[\\s\\S]*?</\\1>";
        let cleaned = regex_lite_strip(&raw, re_pattern);
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        // Truncate to 500 chars for the cloud
        let truncated = if cleaned.len() > 500 {
            format!("{}...", &cleaned[..cleaned.ceil_char_boundary(500)])
        } else {
            cleaned.to_string()
        };
        return Some(truncated);
    }
    None
}

/// Simple regex-like stripping for known XML system block patterns.
fn regex_lite_strip(input: &str, _pattern: &str) -> String {
    // Strip known system XML blocks without pulling in a regex crate
    let tags = [
        "system-reminder",
        "available-deferred-tools",
        "tool-use-rules",
        "functions",
    ];
    let mut result = input.to_string();
    for tag in &tags {
        loop {
            let open = format!("<{}>", tag);
            let close = format!("</{}>", tag);
            let Some(start) = result.find(&open) else { break };
            let Some(end) = result[start..].find(&close) else { break };
            result.replace_range(start..start + end + close.len(), "");
        }
    }
    result
}

impl From<&OtelEventRecord> for EventRecord {
    fn from(e: &OtelEventRecord) -> Self {
        Self {
            timestamp: e.timestamp.clone(),
            name: e.name.clone(),
            body: None,
            attributes: e.attributes.clone(),
            severity: None,
        }
    }
}
