use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rust_embed::Embed;
use serde::Deserialize;

use crate::config::ConfigWithSource;
use crate::db::Db;

#[derive(Embed)]
#[folder = "dashboard/dist"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
    db: Db,
    config: Arc<ConfigWithSource>,
}

// ── Public API ────────────────────────────────────────────────────────

pub fn router(db: Db, config: Arc<ConfigWithSource>) -> Router {
    let state = AppState { db, config };
    Router::new()
        .nest("/api", api_router())
        .with_state(state)
        .fallback(static_handler)
}

pub async fn start(db: Db, port: u16, config: Arc<ConfigWithSource>) -> anyhow::Result<()> {
    let app = router(db, config);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("dashboard at http://127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

// ── API routes ────────────────────────────────────────────────────────

fn api_router() -> Router<AppState> {
    Router::new()
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}/calls", get(get_session_calls))
        .route("/sessions/{id}/events", get(get_session_events))
        .route("/sessions/{id}/tools", get(get_session_tools))
        .route("/sessions/{id}/insights", get(get_session_insights))
        .route("/config", get(get_config))
}

const MAX_LIMIT: usize = 1000;

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<usize>,
}

impl ListQuery {
    fn limit(&self, default: usize) -> usize {
        self.limit.unwrap_or(default).min(MAX_LIMIT)
    }
}

async fn list_sessions(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = q.limit(100);
    match state.db.list_sessions(None, limit).await {
        Ok(mut sessions) => {
            // Merge in active OTEL sessions not yet recorded
            if let Ok(active) = state.db.list_active_otel_sessions().await {
                sessions.extend(active);
            }
            // Re-sort by started_at descending
            sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
            sessions.truncate(limit);
            axum::Json(sessions).into_response()
        }
        Err(e) => {
            tracing::error!("list_sessions: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(serde::Serialize)]
struct SessionDetail {
    session: Option<crate::db::SessionRecord>,
    stats: crate::db::OtelSessionStats,
    tools: Vec<crate::db::ToolUsage>,
    prompts: Vec<String>,
}

async fn get_session(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    // Try to find in sessions table by run_id, then by cc_session_id
    let session = match state.db.get_session_by_id(&id).await.ok().flatten() {
        Some(s) => Some(s),
        None => state.db.get_session_by_cc_id(&id).await.ok().flatten(),
    };

    // Resolve the OTEL session ID via mapping table, with sessions table fallback
    let otel_id = resolve_otel_id(&state.db, &id).await;

    // Get OTEL data using the resolved ID
    let stats = match state.db.get_otel_session_stats(&otel_id).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("get_session stats: {e}");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let tools = match state.db.get_otel_tool_usage(&otel_id).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("get_session tools: {e}");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let prompts = match state.db.get_user_prompts(&otel_id).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("get_session prompts: {e}");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    axum::Json(SessionDetail {
        session,
        stats,
        tools,
        prompts,
    })
    .into_response()
}

async fn get_session_calls(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = q.limit(200);
    let proxy_id = resolve_proxy_id(&state.db, &id).await;
    match state.db.list_api_calls(Some(&proxy_id), limit).await {
        Ok(calls) => axum::Json(calls).into_response(),
        Err(e) => {
            tracing::error!("get_session_calls: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn get_session_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = q.limit(500);
    let otel_id = resolve_otel_id(&state.db, &id).await;
    match state.db.list_otel_events(Some(&otel_id), None, limit).await {
        Ok(events) => axum::Json(events).into_response(),
        Err(e) => {
            tracing::error!("get_session_events: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn get_session_tools(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let otel_id = resolve_otel_id(&state.db, &id).await;
    match state.db.get_otel_tool_usage(&otel_id).await {
        Ok(tools) => axum::Json(tools).into_response(),
        Err(e) => {
            tracing::error!("get_session_tools: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ── Insights ──────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct SessionInsights {
    rate_limits: Vec<RateLimitSnapshot>,
    cache_analysis: CacheAnalysis,
    stop_reasons: HashMap<String, usize>,
    context_growth: Vec<ContextPoint>,
    system_prompt_tokens: Option<i64>,
}

#[derive(serde::Serialize)]
struct RateLimitSnapshot {
    timestamp: String,
    requests_remaining: Option<i64>,
    requests_limit: Option<i64>,
    tokens_remaining: Option<i64>,
    tokens_limit: Option<i64>,
}

#[derive(serde::Serialize)]
struct CacheAnalysis {
    total_cache_read: i64,
    total_cache_write: i64,
    total_input: i64,
    cache_hit_rate: f64,
    estimated_savings_usd: f64,
}

#[derive(serde::Serialize)]
struct ContextPoint {
    call_number: usize,
    timestamp: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read: i64,
    model: Option<String>,
    stop_reason: Option<String>,
}

async fn get_session_insights(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let proxy_id = resolve_proxy_id(&state.db, &id).await;
    let calls = match state.db.list_api_calls(Some(&proxy_id), 1000).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("get_session_insights: {e}");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut rate_limits = Vec::new();
    let mut stop_reasons: HashMap<String, usize> = HashMap::new();
    let mut context_growth = Vec::new();
    let mut total_cache_read: i64 = 0;
    let mut total_cache_write: i64 = 0;
    let mut total_input: i64 = 0;
    let mut system_prompt_tokens: Option<i64> = None;

    for (i, call) in calls.iter().enumerate() {
        // Skip non-messages endpoints
        if !call.path.contains("/messages") {
            continue;
        }

        let inp = call.input_tokens.unwrap_or(0);
        let out = call.output_tokens.unwrap_or(0);
        let cr = call.cache_read.unwrap_or(0);
        let cw = call.cache_write.unwrap_or(0);

        total_input += inp;
        total_cache_read += cr;
        total_cache_write += cw;

        // Rate limits from response headers
        if let Some(ref headers_str) = call.response_headers {
            if let Ok(headers) = serde_json::from_str::<serde_json::Value>(headers_str) {
                let get_i64 = |key: &str| -> Option<i64> {
                    headers.get(key).and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
                };
                let rr = get_i64("x-ratelimit-remaining-requests");
                let rl = get_i64("x-ratelimit-limit-requests");
                let tr = get_i64("x-ratelimit-remaining-tokens");
                let tl = get_i64("x-ratelimit-limit-tokens");
                if rr.is_some() || tr.is_some() {
                    rate_limits.push(RateLimitSnapshot {
                        timestamp: call.timestamp.clone(),
                        requests_remaining: rr,
                        requests_limit: rl,
                        tokens_remaining: tr,
                        tokens_limit: tl,
                    });
                }
            }
        }

        // Stop reason from response body
        let stop_reason = extract_stop_reason(call.response_body.as_deref());
        if let Some(ref reason) = stop_reason {
            *stop_reasons.entry(reason.clone()).or_insert(0) += 1;
        }

        // Context growth
        context_growth.push(ContextPoint {
            call_number: i + 1,
            timestamp: call.timestamp.clone(),
            input_tokens: inp,
            output_tokens: out,
            cache_read: cr,
            model: call.model.clone(),
            stop_reason,
        });

        // System prompt size from first call's request body
        if system_prompt_tokens.is_none() && inp > 0 {
            system_prompt_tokens = estimate_system_prompt_tokens(call.request_body.as_deref());
        }
    }

    // Cache savings: cache reads cost $0.30/MTok vs $3.00/MTok for regular input (Sonnet pricing)
    // Savings = cache_read * (regular_price - cache_price) per token
    let savings_per_token = (3.00 - 0.30) / 1_000_000.0;
    let estimated_savings = total_cache_read as f64 * savings_per_token;
    let total_context = total_input + total_cache_read;
    let cache_hit_rate = if total_context > 0 {
        total_cache_read as f64 / total_context as f64
    } else {
        0.0
    };

    axum::Json(SessionInsights {
        rate_limits,
        cache_analysis: CacheAnalysis {
            total_cache_read,
            total_cache_write,
            total_input,
            cache_hit_rate,
            estimated_savings_usd: estimated_savings,
        },
        stop_reasons,
        context_growth,
        system_prompt_tokens,
    })
    .into_response()
}

/// Extract stop_reason from SSE or JSON response body.
fn extract_stop_reason(body: Option<&str>) -> Option<String> {
    let body = body?;
    // SSE: look for message_delta with stop_reason
    if body.starts_with("event:") || body.contains("\ndata: ") {
        for line in body.lines() {
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            if json.get("type").and_then(|v| v.as_str()) == Some("message_delta") {
                if let Some(reason) = json
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|v| v.as_str())
                {
                    return Some(reason.to_string());
                }
            }
        }
        None
    } else {
        // JSON response
        let json: serde_json::Value = serde_json::from_str(body).ok()?;
        json.get("stop_reason")
            .and_then(|v| v.as_str())
            .map(String::from)
    }
}

/// Estimate system prompt token count from request body.
fn estimate_system_prompt_tokens(body: Option<&str>) -> Option<i64> {
    let body = body?;
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    // System prompt can be a string or array of blocks
    let system = json.get("system")?;
    let char_count = match system {
        serde_json::Value::String(s) => s.len(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
            .map(|s| s.len())
            .sum(),
        _ => return None,
    };
    // Rough estimate: ~4 chars per token for English
    Some((char_count as i64) / 4)
}

async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(state.config.as_ref().clone())
}

/// Resolve a session ID to the CC OTEL session ID.
/// Primary: session_id_map table. Fallback: sessions table (indexed lookup).
async fn resolve_otel_id(db: &Db, id: &str) -> String {
    // Primary: session_id_map
    if let Ok(Some(cc_id)) = db.resolve_cc_session_id(id).await {
        return cc_id;
    }
    // Fallback: sessions table by run_id
    if let Some(s) = db.get_session_by_id(id).await.ok().flatten() {
        if let Some(cc_id) = s.cc_session_id {
            return cc_id;
        }
    }
    id.to_string()
}

/// Resolve a session ID to the proxy run_id.
/// Primary: session_id_map table. Fallback: sessions table (indexed lookup).
async fn resolve_proxy_id(db: &Db, id: &str) -> String {
    // Primary: session_id_map
    if let Ok(Some(run_id)) = db.resolve_run_id(id).await {
        return run_id;
    }
    // Fallback: sessions table by cc_session_id
    if let Some(s) = db.get_session_by_cc_id(id).await.ok().flatten() {
        return s.id;
    }
    id.to_string()
}

// ── Static file serving ───────────────────────────────────────────────

async fn static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let ct = content_type(path);
            (
                [(axum::http::header::CONTENT_TYPE, ct)],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => {
            // SPA fallback — serve index.html for client-side routes
            match Assets::get("index.html") {
                Some(content) => Html(content.data.into_owned()).into_response(),
                None => axum::http::StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        Some("woff2") => "font/woff2",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}
