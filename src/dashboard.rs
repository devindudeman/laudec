use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rust_embed::Embed;
use serde::Deserialize;
use std::sync::Arc;

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
