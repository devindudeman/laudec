use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::{
            logs_service_server::{LogsService, LogsServiceServer},
            ExportLogsServiceRequest, ExportLogsServiceResponse,
        },
        metrics::v1::{
            metrics_service_server::{MetricsService, MetricsServiceServer},
            ExportMetricsServiceRequest, ExportMetricsServiceResponse,
        },
    },
    common::v1::{AnyValue, KeyValue},
    logs::v1::LogRecord,
    metrics::v1::{metric::Data, number_data_point, Metric},
};
use tonic::{Request, Response, Status};

use crate::db::Db;

#[derive(Clone)]
pub struct CollectorService {
    db: Db,
    run_id: String,
    mapping_written: Arc<AtomicBool>,
}

// ── MetricsService ────────────────────────────────────────────────────

#[tonic::async_trait]
impl MetricsService for CollectorService {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        let req = request.into_inner();
        for rm in req.resource_metrics {
            for sm in rm.scope_metrics {
                for metric in &sm.metrics {
                    self.process_metric(metric).await;
                }
            }
        }
        Ok(Response::new(ExportMetricsServiceResponse {
            partial_success: None,
        }))
    }
}

// ── LogsService ───────────────────────────────────────────────────────

#[tonic::async_trait]
impl LogsService for CollectorService {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        let req = request.into_inner();
        for rl in req.resource_logs {
            for sl in rl.scope_logs {
                for record in &sl.log_records {
                    self.process_log_record(record).await;
                }
            }
        }
        Ok(Response::new(ExportLogsServiceResponse {
            partial_success: None,
        }))
    }
}

// ── internals ─────────────────────────────────────────────────────────

impl CollectorService {
    async fn ensure_mapping(&self, cc_session_id: &str) {
        use std::sync::atomic::Ordering;
        if self.mapping_written.load(Ordering::Relaxed) {
            return;
        }
        if self
            .db
            .insert_session_id_mapping(&self.run_id, cc_session_id)
            .await
            .is_ok()
        {
            self.mapping_written.store(true, Ordering::Relaxed);
            tracing::debug!(run_id = %self.run_id, %cc_session_id, "session ID mapping written");
        }
    }

    async fn process_metric(&self, metric: &Metric) {
        let name = &metric.name;

        // Extract (value, time_nanos, attributes) from Gauge and Sum data
        let data_points: Vec<(f64, u64, &[KeyValue])> = match &metric.data {
            Some(Data::Gauge(g)) => g
                .data_points
                .iter()
                .map(|dp| (number_value(&dp.value), dp.time_unix_nano, dp.attributes.as_slice()))
                .collect(),
            Some(Data::Sum(s)) => s
                .data_points
                .iter()
                .map(|dp| (number_value(&dp.value), dp.time_unix_nano, dp.attributes.as_slice()))
                .collect(),
            _ => vec![],
        };

        for (value, time_nanos, attrs) in data_points {
            let timestamp = nanos_to_rfc3339(time_nanos);
            let attrs_json = kvs_to_json(attrs);
            let session_id = extract_session_id(attrs);

            if let Some(ref sid) = session_id {
                self.ensure_mapping(sid).await;
            }

            if let Err(e) = self
                .db
                .insert_otel_metric(
                    &timestamp,
                    name,
                    value,
                    Some(&attrs_json),
                    session_id.as_deref(),
                )
                .await
            {
                tracing::warn!("insert metric: {e}");
            }
        }
    }

    async fn process_log_record(&self, record: &LogRecord) {
        let timestamp = nanos_to_rfc3339(record.time_unix_nano);

        // Extract the real event name from attributes (e.g. "api_request",
        // "tool_result") since severity_text is always just "log".
        let event_name = extract_attr_string(&record.attributes, "event.name");
        let name = event_name.as_deref().unwrap_or(
            if record.severity_text.is_empty() {
                "log"
            } else {
                &record.severity_text
            },
        );
        let body_json = record.body.as_ref().map(|v| any_value_json_string(v));
        let attrs_json = kvs_to_json(&record.attributes);
        let session_id = extract_session_id(&record.attributes);

        if let Some(ref sid) = session_id {
            self.ensure_mapping(sid).await;
        }

        if let Err(e) = self
            .db
            .insert_otel_event(
                &timestamp,
                name,
                body_json.as_deref(),
                Some(&attrs_json),
                Some(&record.severity_text),
                session_id.as_deref(),
            )
            .await
        {
            tracing::warn!("insert event: {e}");
        }
    }
}

// ── server startup ────────────────────────────────────────────────────

pub async fn start(db: Db, port: u16, run_id: String) -> anyhow::Result<()> {
    let svc = CollectorService {
        db,
        run_id,
        mapping_written: Arc::new(AtomicBool::new(false)),
    };
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse()?;

    tracing::info!("OTEL collector listening on {addr}");

    tonic::transport::Server::builder()
        .add_service(MetricsServiceServer::new(svc.clone()))
        .add_service(LogsServiceServer::new(svc))
        .serve(addr)
        .await?;

    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────

fn number_value(v: &Option<number_data_point::Value>) -> f64 {
    match v {
        Some(number_data_point::Value::AsDouble(d)) => *d,
        Some(number_data_point::Value::AsInt(i)) => *i as f64,
        None => 0.0,
    }
}

fn nanos_to_rfc3339(nanos: u64) -> String {
    let secs = (nanos / 1_000_000_000) as i64;
    let nsecs = (nanos % 1_000_000_000) as u32;
    chrono::DateTime::from_timestamp(secs, nsecs)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339())
}

fn kvs_to_json(kvs: &[KeyValue]) -> String {
    let mut map = serde_json::Map::new();
    for kv in kvs {
        if let Some(ref value) = kv.value {
            map.insert(kv.key.clone(), any_value_to_json(value));
        }
    }
    serde_json::to_string(&map).unwrap_or_default()
}

fn any_value_json_string(value: &AnyValue) -> String {
    serde_json::to_string(&any_value_to_json(value)).unwrap_or_default()
}

fn any_value_to_json(value: &AnyValue) -> serde_json::Value {
    use opentelemetry_proto::tonic::common::v1::any_value::Value;
    match &value.value {
        Some(Value::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(Value::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(Value::IntValue(i)) => serde_json::json!(*i),
        Some(Value::DoubleValue(d)) => serde_json::json!(*d),
        Some(Value::ArrayValue(arr)) => {
            serde_json::Value::Array(arr.values.iter().map(any_value_to_json).collect())
        }
        Some(Value::KvlistValue(kvlist)) => {
            let mut map = serde_json::Map::new();
            for kv in &kvlist.values {
                if let Some(ref v) = kv.value {
                    map.insert(kv.key.clone(), any_value_to_json(v));
                }
            }
            serde_json::Value::Object(map)
        }
        Some(Value::BytesValue(bytes)) => {
            serde_json::Value::String(bytes.iter().map(|b| format!("{b:02x}")).collect())
        }
        None => serde_json::Value::Null,
    }
}

fn extract_session_id(attrs: &[KeyValue]) -> Option<String> {
    extract_attr_string(attrs, "session.id")
        .or_else(|| extract_attr_string(attrs, "session_id"))
        .or_else(|| extract_attr_string(attrs, "claude.session_id"))
}

fn extract_attr_string(attrs: &[KeyValue], key: &str) -> Option<String> {
    for kv in attrs {
        if kv.key == key {
            if let Some(ref value) = kv.value {
                use opentelemetry_proto::tonic::common::v1::any_value::Value;
                if let Some(Value::StringValue(s)) = &value.value {
                    return Some(s.clone());
                }
            }
        }
    }
    None
}
