use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rust_embed::Embed;
use serde::Deserialize;

use crate::db::Db;

#[derive(Embed)]
#[folder = "dashboard/dist"]
struct Assets;

// ── Public API ────────────────────────────────────────────────────────

pub fn router(db: Db) -> Router {
    Router::new()
        .nest("/api", api_router(db))
        .fallback(static_handler)
}

pub async fn start(db: Db, port: u16) -> anyhow::Result<()> {
    let app = router(db);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("dashboard at http://127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

// ── API routes ────────────────────────────────────────────────────────

fn api_router(db: Db) -> Router {
    Router::new()
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}/calls", get(get_session_calls))
        .route("/sessions/{id}/events", get(get_session_events))
        .route("/sessions/{id}/tools", get(get_session_tools))
        .with_state(db)
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<usize>,
}

async fn list_sessions(
    State(db): State<Db>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(100);
    match db.list_sessions(None, limit).await {
        Ok(mut sessions) => {
            // Merge in active OTEL sessions not yet recorded
            if let Ok(active) = db.list_active_otel_sessions().await {
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

async fn get_session(State(db): State<Db>, Path(id): Path<String>) -> impl IntoResponse {
    // Try to find in sessions table
    let session = db
        .list_sessions(None, 500)
        .await
        .ok()
        .and_then(|sessions| sessions.into_iter().find(|s| s.id == id));

    // Resolve the OTEL session ID via mapping table, with sessions table fallback
    let otel_id = resolve_otel_id(&db, &id).await;

    // Get OTEL data using the resolved ID
    let stats = match db.get_otel_session_stats(&otel_id).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("get_session stats: {e}");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let tools = db.get_otel_tool_usage(&otel_id).await.unwrap_or_default();
    let prompts = db.get_user_prompts(&otel_id).await.unwrap_or_default();

    axum::Json(SessionDetail {
        session,
        stats,
        tools,
        prompts,
    })
    .into_response()
}

async fn get_session_calls(
    State(db): State<Db>,
    Path(id): Path<String>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(200);
    let proxy_id = resolve_proxy_id(&db, &id).await;
    match db.list_api_calls(Some(&proxy_id), limit).await {
        Ok(calls) => axum::Json(calls).into_response(),
        Err(e) => {
            tracing::error!("get_session_calls: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn get_session_events(
    State(db): State<Db>,
    Path(id): Path<String>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(500);
    let otel_id = resolve_otel_id(&db, &id).await;
    match db.list_otel_events(Some(&otel_id), None, limit).await {
        Ok(events) => axum::Json(events).into_response(),
        Err(e) => {
            tracing::error!("get_session_events: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn get_session_tools(State(db): State<Db>, Path(id): Path<String>) -> impl IntoResponse {
    let otel_id = resolve_otel_id(&db, &id).await;
    match db.get_otel_tool_usage(&otel_id).await {
        Ok(tools) => axum::Json(tools).into_response(),
        Err(e) => {
            tracing::error!("get_session_tools: {e}");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Resolve a session ID to the CC OTEL session ID.
/// Primary: session_id_map table. Fallback: sessions table (historical data).
async fn resolve_otel_id(db: &Db, id: &str) -> String {
    // Primary: session_id_map
    if let Ok(Some(cc_id)) = db.resolve_cc_session_id(id).await {
        return cc_id;
    }
    // Fallback: sessions table
    db.list_sessions(None, 500)
        .await
        .ok()
        .and_then(|ss| ss.into_iter().find(|s| s.id == id).and_then(|s| s.cc_session_id))
        .unwrap_or_else(|| id.to_string())
}

/// Resolve a session ID to the proxy run_id.
/// Primary: session_id_map table. Fallback: sessions table (historical data).
async fn resolve_proxy_id(db: &Db, id: &str) -> String {
    // Primary: session_id_map (written at OTEL ingest time)
    if let Ok(Some(run_id)) = db.resolve_run_id(id).await {
        return run_id;
    }
    // Fallback: sessions table (historical data before mapping table existed)
    db.list_sessions(None, 500)
        .await
        .ok()
        .and_then(|ss| {
            ss.into_iter()
                .find(|s| s.cc_session_id.as_deref() == Some(id))
                .map(|s| s.id)
        })
        .unwrap_or_else(|| id.to_string())
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
