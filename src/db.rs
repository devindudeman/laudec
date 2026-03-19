use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusqlite::Connection;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating dir {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;

        Self::create_tables(&conn)?;
        Self::migrate(&conn);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn create_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS api_calls (
                id               TEXT PRIMARY KEY,
                session_id       TEXT,
                timestamp        TEXT NOT NULL,
                method           TEXT NOT NULL,
                path             TEXT NOT NULL,
                request_body     TEXT,
                response_body    TEXT,
                status_code      INTEGER,
                latency_ms       INTEGER,
                model            TEXT,
                input_tokens     INTEGER,
                output_tokens    INTEGER,
                cache_read       INTEGER,
                cache_write      INTEGER,
                request_headers  TEXT,
                response_headers TEXT
            );

            CREATE TABLE IF NOT EXISTS otel_metrics (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp   TEXT NOT NULL,
                name        TEXT NOT NULL,
                value       REAL NOT NULL,
                attributes  TEXT,
                session_id  TEXT
            );

            CREATE TABLE IF NOT EXISTS otel_events (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp   TEXT NOT NULL,
                name        TEXT NOT NULL,
                body        TEXT,
                attributes  TEXT,
                severity    TEXT,
                session_id  TEXT
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id              TEXT PRIMARY KEY,
                project         TEXT NOT NULL,
                project_path    TEXT NOT NULL,
                started_at      TEXT NOT NULL,
                ended_at        TEXT NOT NULL,
                duration_secs   INTEGER,
                api_call_count  INTEGER,
                input_tokens    INTEGER,
                output_tokens   INTEGER,
                cache_read      INTEGER,
                cache_write     INTEGER,
                model           TEXT,
                files_changed   INTEGER,
                lines_added     INTEGER,
                lines_removed   INTEGER,
                changed_files   TEXT,
                summary         TEXT,
                config_snapshot TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_api_calls_session   ON api_calls(session_id);
            CREATE INDEX IF NOT EXISTS idx_api_calls_timestamp  ON api_calls(timestamp);
            CREATE INDEX IF NOT EXISTS idx_otel_metrics_session ON otel_metrics(session_id);
            CREATE INDEX IF NOT EXISTS idx_otel_metrics_name    ON otel_metrics(name);
            CREATE INDEX IF NOT EXISTS idx_otel_events_session  ON otel_events(session_id);
            CREATE INDEX IF NOT EXISTS idx_otel_events_name     ON otel_events(name);
            CREATE INDEX IF NOT EXISTS idx_sessions_project     ON sessions(project);

            CREATE TABLE IF NOT EXISTS session_id_map (
                run_id         TEXT NOT NULL PRIMARY KEY,
                cc_session_id  TEXT NOT NULL,
                created_at     TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_session_id_map_cc_session_id
                ON session_id_map(cc_session_id);",
        )?;
        Ok(())
    }

    fn migrate(conn: &Connection) {
        // Phase 2+ columns — ignore errors if they already exist
        for stmt in &[
            "ALTER TABLE sessions ADD COLUMN cost_usd REAL",
            "ALTER TABLE sessions ADD COLUMN tool_uses INTEGER",
            "ALTER TABLE sessions ADD COLUMN cc_session_id TEXT",
            "ALTER TABLE api_calls ADD COLUMN response_text TEXT",
        ] {
            let _ = conn.execute(stmt, []);
        }
    }

    // ── API call logging ──────────────────────────────────────────────

    pub async fn insert_api_request(
        &self,
        id: &str,
        session_id: Option<&str>,
        method: &str,
        path: &str,
        request_body: Option<&str>,
        request_headers: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.clone();
        let id = id.to_string();
        let session_id = session_id.map(String::from);
        let method = method.to_string();
        let path = path.to_string();
        let request_body = request_body.map(String::from);
        let request_headers = request_headers.map(String::from);
        let timestamp = chrono::Utc::now().to_rfc3339();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            conn.execute(
                "INSERT INTO api_calls (id, session_id, timestamp, method, path, request_body, request_headers)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, session_id, timestamp, method, path, request_body, request_headers],
            )?;
            Ok(())
        })
        .await?
    }

    pub async fn update_api_response(
        &self,
        id: &str,
        status_code: u16,
        response_body: Option<&str>,
        response_headers: Option<&str>,
        latency_ms: i64,
        model: Option<&str>,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cache_read: Option<i64>,
        cache_write: Option<i64>,
        response_text: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.clone();
        let id = id.to_string();
        let response_body = response_body.map(String::from);
        let response_headers = response_headers.map(String::from);
        let model = model.map(String::from);
        let response_text = response_text.map(String::from);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            conn.execute(
                "UPDATE api_calls
                 SET status_code = ?1, response_body = ?2, response_headers = ?3,
                     latency_ms = ?4, model = ?5, input_tokens = ?6, output_tokens = ?7,
                     cache_read = ?8, cache_write = ?9, response_text = ?10
                 WHERE id = ?11",
                rusqlite::params![
                    status_code as i64, response_body, response_headers,
                    latency_ms, model, input_tokens, output_tokens,
                    cache_read, cache_write, response_text, id
                ],
            )?;
            Ok(())
        })
        .await?
    }

    // ── OTEL data ─────────────────────────────────────────────────────

    pub async fn insert_otel_metric(
        &self,
        timestamp: &str,
        name: &str,
        value: f64,
        attributes: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.clone();
        let timestamp = timestamp.to_string();
        let name = name.to_string();
        let attributes = attributes.map(String::from);
        let session_id = session_id.map(String::from);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            conn.execute(
                "INSERT INTO otel_metrics (timestamp, name, value, attributes, session_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![timestamp, name, value, attributes, session_id],
            )?;
            Ok(())
        })
        .await?
    }

    pub async fn insert_otel_event(
        &self,
        timestamp: &str,
        name: &str,
        body: Option<&str>,
        attributes: Option<&str>,
        severity: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.clone();
        let timestamp = timestamp.to_string();
        let name = name.to_string();
        let body = body.map(String::from);
        let attributes = attributes.map(String::from);
        let severity = severity.map(String::from);
        let session_id = session_id.map(String::from);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            conn.execute(
                "INSERT INTO otel_events (timestamp, name, body, attributes, severity, session_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![timestamp, name, body, attributes, severity, session_id],
            )?;
            Ok(())
        })
        .await?
    }

    // ── Session ID mapping ──────────────────────────────────────────────

    pub async fn insert_session_id_mapping(
        &self,
        run_id: &str,
        cc_session_id: &str,
    ) -> Result<()> {
        let conn = self.conn.clone();
        let run_id = run_id.to_string();
        let cc_session_id = cc_session_id.to_string();
        let created_at = chrono::Utc::now().to_rfc3339();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            conn.execute(
                "INSERT OR REPLACE INTO session_id_map (run_id, cc_session_id, created_at)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![run_id, cc_session_id, created_at],
            )?;
            Ok(())
        })
        .await?
    }

    /// Resolve any session ID (run_id or cc_session_id) to the proxy run_id.
    pub async fn resolve_run_id(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.clone();
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            let result: Option<String> = conn
                .query_row(
                    "SELECT run_id FROM session_id_map WHERE cc_session_id = ?1 OR run_id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .ok();
            Ok(result)
        })
        .await?
    }

    /// Resolve any session ID (run_id or cc_session_id) to the OTEL cc_session_id.
    pub async fn resolve_cc_session_id(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.clone();
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            let result: Option<String> = conn
                .query_row(
                    "SELECT cc_session_id FROM session_id_map WHERE run_id = ?1 OR cc_session_id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .ok();
            Ok(result)
        })
        .await?
    }

    /// Find Claude Code's native session ID from OTEL events since a given timestamp.
    pub async fn find_cc_session_id(&self, since: &str) -> Result<Option<String>> {
        let conn = self.conn.clone();
        let since = since.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            let result: Option<String> = conn
                .query_row(
                    "SELECT session_id FROM otel_events
                     WHERE session_id IS NOT NULL AND timestamp >= ?1
                     ORDER BY timestamp DESC LIMIT 1",
                    rusqlite::params![since],
                    |row| row.get(0),
                )
                .ok();
            Ok(result)
        })
        .await?
    }

    // ── Sessions ──────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_session(
        &self,
        id: &str,
        project: &str,
        project_path: &str,
        started_at: &str,
        ended_at: &str,
        duration_secs: i64,
        api_call_count: i64,
        input_tokens: i64,
        output_tokens: i64,
        cache_read: i64,
        cache_write: i64,
        model: Option<&str>,
        files_changed: i64,
        lines_added: i64,
        lines_removed: i64,
        changed_files: Option<&str>,
        summary: Option<&str>,
        config_snapshot: Option<&str>,
        cost_usd: f64,
        tool_uses: i64,
        cc_session_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.clone();
        let id = id.to_string();
        let project = project.to_string();
        let project_path = project_path.to_string();
        let started_at = started_at.to_string();
        let ended_at = ended_at.to_string();
        let model = model.map(String::from);
        let changed_files = changed_files.map(String::from);
        let summary = summary.map(String::from);
        let config_snapshot = config_snapshot.map(String::from);
        let cc_session_id = cc_session_id.map(String::from);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            conn.execute(
                "INSERT INTO sessions
                 (id, project, project_path, started_at, ended_at, duration_secs,
                  api_call_count, input_tokens, output_tokens, cache_read, cache_write,
                  model, files_changed, lines_added, lines_removed,
                  changed_files, summary, config_snapshot, cost_usd, tool_uses, cc_session_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
                rusqlite::params![
                    id, project, project_path, started_at, ended_at, duration_secs,
                    api_call_count, input_tokens, output_tokens, cache_read, cache_write,
                    model, files_changed, lines_added, lines_removed,
                    changed_files, summary, config_snapshot, cost_usd, tool_uses, cc_session_id
                ],
            )?;
            Ok(())
        })
        .await?
    }

    // ── OTEL-based queries (Phase 2) ──────────────────────────────────

    /// Get session stats from OTEL api_request events (richer than proxy log).
    pub async fn get_otel_session_stats(&self, session_id: &str) -> Result<OtelSessionStats> {
        let conn = self.conn.clone();
        let session_id = session_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            // Handle both old events (name='log') and new (name='api_request')
            let mut stmt = conn.prepare(
                "SELECT
                    COUNT(*) as calls,
                    COALESCE(SUM(CAST(json_extract(attributes, '$.input_tokens') AS INTEGER)), 0),
                    COALESCE(SUM(CAST(json_extract(attributes, '$.output_tokens') AS INTEGER)), 0),
                    COALESCE(SUM(CAST(json_extract(attributes, '$.cache_creation_tokens') AS INTEGER)), 0),
                    COALESCE(SUM(CAST(json_extract(attributes, '$.cache_read_tokens') AS INTEGER)), 0),
                    COALESCE(SUM(CAST(json_extract(attributes, '$.cost_usd') AS REAL)), 0.0)
                 FROM otel_events
                 WHERE session_id = ?1
                   AND (name = 'api_request' OR json_extract(attributes, '$.event.name') = 'api_request')",
            )?;
            let stats = stmt.query_row(rusqlite::params![session_id], |row| {
                Ok(OtelSessionStats {
                    api_calls: row.get(0)?,
                    input_tokens: row.get(1)?,
                    output_tokens: row.get(2)?,
                    cache_creation: row.get(3)?,
                    cache_read: row.get(4)?,
                    cost_usd: row.get(5)?,
                    model: None,
                })
            })?;

            // Get primary model
            let model: Option<String> = conn
                .query_row(
                    "SELECT json_extract(attributes, '$.model')
                     FROM otel_events
                     WHERE session_id = ?1
                       AND (name = 'api_request' OR json_extract(attributes, '$.event.name') = 'api_request')
                     GROUP BY json_extract(attributes, '$.model')
                     ORDER BY COUNT(*) DESC LIMIT 1",
                    rusqlite::params![session_id],
                    |row| row.get(0),
                )
                .ok();

            Ok(OtelSessionStats { model, ..stats })
        })
        .await?
    }

    /// Get tool usage breakdown from OTEL tool_result events.
    pub async fn get_otel_tool_usage(&self, session_id: &str) -> Result<Vec<ToolUsage>> {
        let conn = self.conn.clone();
        let session_id = session_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            let mut stmt = conn.prepare(
                "SELECT
                    json_extract(attributes, '$.tool_name') as tool,
                    COUNT(*) as uses,
                    SUM(CASE WHEN json_extract(attributes, '$.success') = 'true' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN json_extract(attributes, '$.success') = 'false' THEN 1 ELSE 0 END)
                 FROM otel_events
                 WHERE session_id = ?1
                   AND (name = 'tool_result' OR json_extract(attributes, '$.event.name') = 'tool_result')
                   AND json_extract(attributes, '$.tool_name') IS NOT NULL
                 GROUP BY tool
                 ORDER BY uses DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![session_id], |row| {
                    Ok(ToolUsage {
                        tool_name: row.get::<_, String>(0).unwrap_or_default(),
                        uses: row.get(1)?,
                        successes: row.get(2)?,
                        failures: row.get(3)?,
                    })
                })?
                .filter_map(Result::ok)
                .collect();
            Ok(rows)
        })
        .await?
    }

    /// Get user prompts from OTEL events for a session.
    pub async fn get_user_prompts(&self, session_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.clone();
        let session_id = session_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            let mut stmt = conn.prepare(
                "SELECT json_extract(attributes, '$.prompt')
                 FROM otel_events
                 WHERE session_id = ?1
                   AND (name = 'user_prompt' OR json_extract(attributes, '$.event.name') = 'user_prompt')
                 ORDER BY timestamp",
            )?;
            let rows: Vec<String> = stmt
                .query_map(rusqlite::params![session_id], |row| {
                    row.get::<_, String>(0)
                })?
                .filter_map(Result::ok)
                .collect();
            Ok(rows)
        })
        .await?
    }

    // ── CLI query methods ─────────────────────────────────────────────

    /// List API calls, optionally filtered by session.
    pub async fn list_api_calls(
        &self,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ApiCallRecord>> {
        let conn = self.conn.clone();
        let session_id = session_id.map(String::from);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            let select = "SELECT timestamp, method, path, status_code, latency_ms, model,
                            input_tokens, output_tokens, cache_read, cache_write,
                            response_text, request_body, response_body,
                            request_headers, response_headers
                     FROM api_calls";
            if let Some(ref sid) = session_id {
                let mut stmt = conn.prepare(
                    &format!("{select} WHERE session_id = ?1 ORDER BY timestamp ASC LIMIT ?2"),
                )?;
                let rows: Vec<_> = stmt
                    .query_map(rusqlite::params![sid, limit as i64], row_to_api_call)?
                    .filter_map(Result::ok)
                    .collect();
                Ok(rows)
            } else {
                let mut stmt = conn.prepare(
                    &format!("{select} ORDER BY timestamp DESC LIMIT ?1"),
                )?;
                let rows: Vec<_> = stmt
                    .query_map(rusqlite::params![limit as i64], row_to_api_call)?
                    .filter_map(Result::ok)
                    .collect();
                Ok(rows)
            }
        })
        .await?
    }

    /// List OTEL events, optionally filtered by session and event name.
    pub async fn list_otel_events(
        &self,
        session_id: Option<&str>,
        name_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<OtelEventRecord>> {
        let conn = self.conn.clone();
        let session_id = session_id.map(String::from);
        let name_filter = name_filter.map(String::from);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;

            let (where_clause, params) = build_event_filter(&session_id, &name_filter);
            let sql = format!(
                "SELECT timestamp, name, session_id, attributes
                 FROM otel_events {where_clause}
                 ORDER BY timestamp DESC LIMIT ?{}",
                params.len() + 1
            );

            let mut stmt = conn.prepare(&sql)?;
            let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = params
                .into_iter()
                .map(|s| Box::new(s) as Box<dyn rusqlite::types::ToSql>)
                .collect();
            all_params.push(Box::new(limit as i64));

            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                all_params.iter().map(|p| p.as_ref()).collect();

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(OtelEventRecord {
                        timestamp: row.get(0)?,
                        name: row.get(1)?,
                        session_id: row.get(2)?,
                        attributes: row.get(3)?,
                    })
                })?
                .filter_map(Result::ok)
                .collect();
            Ok(rows)
        })
        .await?
    }

    /// Get the full timeline for a session (for replay).
    pub async fn get_session_timeline(&self, session_id: &str) -> Result<Vec<OtelEventRecord>> {
        let conn = self.conn.clone();
        let session_id = session_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            let mut stmt = conn.prepare(
                "SELECT timestamp, name, session_id, attributes
                 FROM otel_events WHERE session_id = ?1
                 ORDER BY timestamp ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![session_id], |row| {
                    Ok(OtelEventRecord {
                        timestamp: row.get(0)?,
                        name: row.get(1)?,
                        session_id: row.get(2)?,
                        attributes: row.get(3)?,
                    })
                })?
                .filter_map(Result::ok)
                .collect();
            Ok(rows)
        })
        .await?
    }

    // ── Existing queries ──────────────────────────────────────────────

    pub async fn get_session_token_stats(&self, since: Option<&str>) -> Result<SessionStats> {
        let conn = self.conn.clone();
        let since = since.map(String::from);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            if let Some(ref since) = since {
                let mut stmt = conn.prepare(
                    "SELECT COUNT(*), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                            COALESCE(SUM(cache_read),0), COALESCE(SUM(cache_write),0)
                     FROM api_calls WHERE timestamp >= ?1",
                )?;
                Ok(stmt.query_row(rusqlite::params![since], row_to_stats)?)
            } else {
                let mut stmt = conn.prepare(
                    "SELECT COUNT(*), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                            COALESCE(SUM(cache_read),0), COALESCE(SUM(cache_write),0)
                     FROM api_calls",
                )?;
                Ok(stmt.query_row([], row_to_stats)?)
            }
        })
        .await?
    }

    pub async fn list_sessions(
        &self,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SessionRecord>> {
        let conn = self.conn.clone();
        let project = project.map(String::from);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            if let Some(ref project) = project {
                let mut stmt = conn.prepare(
                    "SELECT id, project, started_at, ended_at, duration_secs,
                            api_call_count, input_tokens, output_tokens,
                            files_changed, lines_added, lines_removed, summary, cost_usd, cc_session_id
                     FROM sessions WHERE project = ?1
                     ORDER BY started_at DESC LIMIT ?2",
                )?;
                let rows: Vec<_> = stmt.query_map(rusqlite::params![project, limit as i64], row_to_session)?
                    .filter_map(Result::ok).collect();
                Ok(rows)
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, project, started_at, ended_at, duration_secs,
                            api_call_count, input_tokens, output_tokens,
                            files_changed, lines_added, lines_removed, summary, cost_usd, cc_session_id
                     FROM sessions ORDER BY started_at DESC LIMIT ?1",
                )?;
                let rows: Vec<_> = stmt.query_map(rusqlite::params![limit as i64], row_to_session)?
                    .filter_map(Result::ok).collect();
                Ok(rows)
            }
        })
        .await?
    }

    /// Find active OTEL sessions not yet recorded in the sessions table.
    /// Returns synthetic SessionRecords for live sessions.
    /// Find active OTEL sessions not yet recorded in the sessions table.
    /// Only returns sessions whose last event is within the last 5 minutes
    /// (older orphans are just unmapped legacy data, not live).
    pub async fn list_active_otel_sessions(&self) -> Result<Vec<SessionRecord>> {
        let conn = self.conn.clone();
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("db lock: {e}"))?;
            let mut stmt = conn.prepare(
                "SELECT
                    e.session_id,
                    MIN(e.timestamp) as first_seen,
                    MAX(e.timestamp) as last_seen,
                    COUNT(CASE WHEN e.name = 'api_request' OR json_extract(e.attributes, '$.event.name') = 'api_request' THEN 1 END) as api_calls,
                    COALESCE(SUM(CASE WHEN e.name = 'api_request' OR json_extract(e.attributes, '$.event.name') = 'api_request'
                        THEN CAST(json_extract(e.attributes, '$.input_tokens') AS INTEGER) END), 0) as input_tokens,
                    COALESCE(SUM(CASE WHEN e.name = 'api_request' OR json_extract(e.attributes, '$.event.name') = 'api_request'
                        THEN CAST(json_extract(e.attributes, '$.output_tokens') AS INTEGER) END), 0) as output_tokens,
                    COALESCE(SUM(CASE WHEN e.name = 'api_request' OR json_extract(e.attributes, '$.event.name') = 'api_request'
                        THEN CAST(json_extract(e.attributes, '$.cost_usd') AS REAL) END), 0.0) as cost_usd
                 FROM otel_events e
                 WHERE e.session_id IS NOT NULL
                   AND e.session_id NOT IN (SELECT cc_session_id FROM sessions WHERE cc_session_id IS NOT NULL)
                   AND e.session_id NOT IN (SELECT id FROM sessions)
                 GROUP BY e.session_id
                 HAVING MAX(e.timestamp) >= ?1
                 ORDER BY first_seen DESC",
            )?;
            let rows: Vec<_> = stmt
                .query_map(rusqlite::params![cutoff], |row| {
                    let sid: String = row.get(0)?;
                    let first: String = row.get(1)?;
                    let last: String = row.get(2)?;
                    Ok(SessionRecord {
                        id: sid.clone(),
                        project: "active".to_string(),
                        started_at: first,
                        ended_at: last,
                        duration_secs: None,
                        api_call_count: row.get(3)?,
                        input_tokens: row.get(4)?,
                        output_tokens: row.get(5)?,
                        files_changed: None,
                        lines_added: None,
                        lines_removed: None,
                        summary: Some("(live session)".to_string()),
                        cost_usd: row.get(6)?,
                        cc_session_id: Some(sid),
                    })
                })?
                .filter_map(Result::ok)
                .collect();
            Ok(rows)
        })
        .await?
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

fn build_event_filter(
    session_id: &Option<String>,
    name_filter: &Option<String>,
) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut params = Vec::new();

    if let Some(ref sid) = session_id {
        params.push(sid.clone());
        clauses.push(format!("session_id = ?{}", params.len()));
    }
    if let Some(ref name) = name_filter {
        params.push(name.clone());
        clauses.push(format!("name = ?{}", params.len()));
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (where_clause, params)
}

// ── Row mappers ───────────────────────────────────────────────────────

fn row_to_stats(row: &rusqlite::Row) -> rusqlite::Result<SessionStats> {
    Ok(SessionStats {
        api_call_count: row.get(0)?,
        input_tokens: row.get(1)?,
        output_tokens: row.get(2)?,
        cache_read: row.get(3)?,
        cache_write: row.get(4)?,
    })
}

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: row.get(0)?,
        project: row.get(1)?,
        started_at: row.get(2)?,
        ended_at: row.get(3)?,
        duration_secs: row.get(4)?,
        api_call_count: row.get(5)?,
        input_tokens: row.get(6)?,
        output_tokens: row.get(7)?,
        files_changed: row.get(8)?,
        lines_added: row.get(9)?,
        lines_removed: row.get(10)?,
        summary: row.get(11)?,
        cost_usd: row.get(12)?,
        cc_session_id: row.get(13)?,
    })
}

fn row_to_api_call(row: &rusqlite::Row) -> rusqlite::Result<ApiCallRecord> {
    Ok(ApiCallRecord {
        timestamp: row.get(0)?,
        method: row.get(1)?,
        path: row.get(2)?,
        status_code: row.get(3)?,
        latency_ms: row.get(4)?,
        model: row.get(5)?,
        input_tokens: row.get(6)?,
        output_tokens: row.get(7)?,
        cache_read: row.get(8)?,
        cache_write: row.get(9)?,
        response_text: row.get(10)?,
        request_body: row.get(11)?,
        response_body: row.get(12)?,
        request_headers: row.get(13)?,
        response_headers: row.get(14)?,
    })
}

// ── Types ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SessionStats {
    pub api_call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct OtelSessionStats {
    pub api_calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation: i64,
    pub cache_read: i64,
    pub cost_usd: f64,
    pub model: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ToolUsage {
    pub tool_name: String,
    pub uses: i64,
    pub successes: i64,
    pub failures: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct SessionRecord {
    pub id: String,
    pub project: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_secs: Option<i64>,
    pub api_call_count: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub files_changed: Option<i64>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub summary: Option<String>,
    pub cost_usd: Option<f64>,
    pub cc_session_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ApiCallRecord {
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
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub request_headers: Option<String>,
    pub response_headers: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct OtelEventRecord {
    pub timestamp: String,
    pub name: String,
    pub session_id: Option<String>,
    pub attributes: Option<String>,
}

