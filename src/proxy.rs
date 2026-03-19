use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::response::Response;
use axum::Router;
use bytes::Bytes;
use futures::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::config::ProxyConfig;
use crate::db::Db;

pub struct ProxyState {
    pub db: Db,
    pub client: reqwest::Client,
    pub config: ProxyConfig,
    /// Static run ID — always set, no race condition.
    pub run_id: String,
}

pub fn router(state: Arc<ProxyState>) -> Router {
    Router::new().fallback(proxy_handler).with_state(state)
}

pub async fn start(state: Arc<ProxyState>, port: u16) -> anyhow::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("proxy listening on 127.0.0.1:{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn proxy_handler(
    State(state): State<Arc<ProxyState>>,
    req: axum::extract::Request,
) -> Result<Response, axum::http::StatusCode> {
    let start = Instant::now();
    let call_id = uuid::Uuid::new_v4().to_string();

    // ── extract request ──────────────────────────────────────────────
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());

    let body_bytes = axum::body::to_bytes(body, 50 * 1024 * 1024)
        .await
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;

    // ── log request (always tagged with run_id) ──────────────────────
    if state.config.log_requests {
        let headers_json = redacted_headers(&parts.headers, state.config.redact_keys);
        let body_str = std::str::from_utf8(&body_bytes).ok();

        let _ = state
            .db
            .insert_api_request(
                &call_id,
                Some(&state.run_id),
                method.as_str(),
                &path,
                body_str,
                Some(&headers_json),
            )
            .await;
    }

    // ── forward to Anthropic ─────────────────────────────────────────
    let url = format!("https://api.anthropic.com{path}");
    let mut forward_headers = reqwest::header::HeaderMap::new();
    for (key, value) in &parts.headers {
        if key != "host" && key != "accept-encoding" {
            forward_headers.insert(key.clone(), value.clone());
        }
    }

    let response = state
        .client
        .request(method, &url)
        .headers(forward_headers)
        .body(body_bytes)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("proxy forward error: {e}");
            axum::http::StatusCode::BAD_GATEWAY
        })?;

    let status = response.status();
    let resp_headers = response.headers().clone();

    let is_sse = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));

    if is_sse {
        stream_response(state, response, status, resp_headers, call_id, start).await
    } else {
        buffered_response(state, response, status, resp_headers, call_id, start).await
    }
}

// ── streaming SSE passthrough ─────────────────────────────────────────

async fn stream_response(
    state: Arc<ProxyState>,
    response: reqwest::Response,
    status: reqwest::StatusCode,
    resp_headers: reqwest::header::HeaderMap,
    call_id: String,
    start: Instant,
) -> Result<Response, axum::http::StatusCode> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(64);
    let db = state.db.clone();
    let log_responses = state.config.log_responses;
    let redact = state.config.redact_keys;
    let resp_headers_for_log = resp_headers.clone();

    tokio::spawn(async move {
        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    buffer.extend_from_slice(&bytes);
                    if tx.send(Ok(bytes)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
                        .await;
                    break;
                }
            }
        }
        drop(tx);

        if log_responses {
            let body_str = String::from_utf8_lossy(&buffer);
            let (model, inp, out, cr, cw) = parse_sse_usage(&body_str);
            // Parse response text at capture time — no more query-time SSE parsing
            let response_text = parse_sse_text(&body_str);
            let headers_json = redacted_headers(&resp_headers_for_log, redact);

            let _ = db
                .update_api_response(
                    &call_id,
                    status.as_u16(),
                    Some(&body_str),
                    Some(&headers_json),
                    start.elapsed().as_millis() as i64,
                    model.as_deref(),
                    inp,
                    out,
                    cr,
                    cw,
                    if response_text.is_empty() {
                        None
                    } else {
                        Some(&response_text)
                    },
                )
                .await;
        }
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    *resp.headers_mut() = resp_headers;
    Ok(resp)
}

// ── non-streaming passthrough ─────────────────────────────────────────

async fn buffered_response(
    state: Arc<ProxyState>,
    response: reqwest::Response,
    status: reqwest::StatusCode,
    resp_headers: reqwest::header::HeaderMap,
    call_id: String,
    start: Instant,
) -> Result<Response, axum::http::StatusCode> {
    let resp_bytes = response.bytes().await.map_err(|e| {
        tracing::error!("proxy read error: {e}");
        axum::http::StatusCode::BAD_GATEWAY
    })?;

    if state.config.log_responses {
        let body_str = std::str::from_utf8(&resp_bytes).ok();
        let (model, inp, out, cr, cw) = body_str
            .map(parse_json_usage)
            .unwrap_or((None, None, None, None, None));
        let headers_json = redacted_headers(&resp_headers, state.config.redact_keys);

        let _ = state
            .db
            .update_api_response(
                &call_id,
                status.as_u16(),
                body_str,
                Some(&headers_json),
                start.elapsed().as_millis() as i64,
                model.as_deref(),
                inp,
                out,
                cr,
                cw,
                None, // non-SSE responses don't have parseable text
            )
            .await;
    }

    let mut resp = Response::new(Body::from(resp_bytes));
    *resp.status_mut() = status;
    *resp.headers_mut() = resp_headers;
    Ok(resp)
}

// ── header helpers ────────────────────────────────────────────────────

fn redacted_headers(headers: &reqwest::header::HeaderMap, redact: bool) -> String {
    let mut map = serde_json::Map::new();
    for (key, value) in headers {
        let key_str = key.as_str();
        let val = if redact
            && (key_str == "x-api-key"
                || key_str == "authorization"
                || key_str == "cookie"
                || key_str == "x-session-token")
        {
            "[REDACTED]".to_string()
        } else {
            value.to_str().unwrap_or("[binary]").to_string()
        };
        map.insert(key_str.to_string(), serde_json::Value::String(val));
    }
    serde_json::to_string(&map).unwrap_or_default()
}

// ── SSE parsing ───────────────────────────────────────────────────────

type Usage = (
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

fn parse_sse_usage(body: &str) -> Usage {
    let mut model = None;
    let mut input_tokens = None;
    let mut output_tokens = None;
    let mut cache_read = None;
    let mut cache_write = None;

    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };

        match json.get("type").and_then(|v| v.as_str()) {
            Some("message_start") => {
                if let Some(msg) = json.get("message") {
                    model = msg.get("model").and_then(|v| v.as_str()).map(String::from);
                    if let Some(u) = msg.get("usage") {
                        input_tokens = u.get("input_tokens").and_then(|v| v.as_i64());
                        cache_read =
                            u.get("cache_read_input_tokens").and_then(|v| v.as_i64());
                        cache_write =
                            u.get("cache_creation_input_tokens").and_then(|v| v.as_i64());
                    }
                }
            }
            Some("message_delta") => {
                if let Some(u) = json.get("usage") {
                    output_tokens = u.get("output_tokens").and_then(|v| v.as_i64());
                }
            }
            _ => {}
        }
    }

    (model, input_tokens, output_tokens, cache_read, cache_write)
}

/// Extract text content from SSE response at capture time.
pub fn parse_sse_text(body: &str) -> String {
    let mut text = String::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if json.get("type").and_then(|v| v.as_str()) == Some("content_block_delta") {
            if let Some(t) = json
                .get("delta")
                .and_then(|d| d.get("text"))
                .and_then(|v| v.as_str())
            {
                text.push_str(t);
            }
        }
    }
    text
}

fn parse_json_usage(body: &str) -> Usage {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return (None, None, None, None, None);
    };
    let model = json
        .get("model")
        .and_then(|v| v.as_str())
        .map(String::from);
    let u = json.get("usage");
    let input_tokens = u.and_then(|u| u.get("input_tokens")).and_then(|v| v.as_i64());
    let output_tokens = u
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_i64());
    let cache_read = u
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(|v| v.as_i64());
    let cache_write = u
        .and_then(|u| u.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_i64());
    (model, input_tokens, output_tokens, cache_read, cache_write)
}
