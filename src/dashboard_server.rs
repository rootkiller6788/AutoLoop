use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::broadcast,
    time::{Duration, timeout},
};

use crate::AutoLoopApp;

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardEvent {
    kind: String,
    tool: Option<String>,
    action: Option<String>,
    created_at_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OperatorSettings {
    language: String,
    provider_vendor: String,
    api_base_url: String,
    default_model: String,
    api_key: String,
}

impl Default for OperatorSettings {
    fn default() -> Self {
        Self {
            language: "zh-CN".into(),
            provider_vendor: "alibaba".into(),
            api_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            default_model: "qwen-plus-latest".into(),
            api_key: String::new(),
        }
    }
}

pub async fn run_dashboard_server(app: Arc<AutoLoopApp>, host: &str, port: u16) -> Result<()> {
    let listener = TcpListener::bind((host, port)).await?;
    let (events_tx, _) = broadcast::channel::<String>(128);
    tracing::info!("dashboard snapshot server listening on http://{host}:{port}");

    loop {
        let (stream, _) = listener.accept().await?;
        let app = Arc::clone(&app);
        let events_tx = events_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, app, events_tx).await {
                tracing::warn!("dashboard snapshot request failed: {error}");
            }
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    app: Arc<AutoLoopApp>,
    events_tx: broadcast::Sender<String>,
) -> Result<()> {
    let mut buffer = [0_u8; 8192];
    let bytes_read = stream.read(&mut buffer).await?;
    if bytes_read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let mut lines = request.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or("/");
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, tail)| tail.to_string())
        .unwrap_or_default();
    let path = target.split('?').next().unwrap_or(target).trim_end_matches('/');

    if method == "GET" && path == "/api/events" {
        return handle_event_stream(stream, events_tx.subscribe()).await;
    }

    let response = match method {
        "GET" => route_request(&app, target).await?,
        "POST" => route_mutation(&app, target, &body, &events_tx).await?,
        _ => build_json_response(
            405,
            serde_json::json!({"error":"method_not_allowed","allowed":["GET","POST"]}).to_string(),
        ),
    };

    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn handle_event_stream(
    mut stream: TcpStream,
    mut events_rx: broadcast::Receiver<String>,
) -> Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        )
        .await?;
    stream
        .write_all(sse_frame("connected", "{\"status\":\"ready\"}").as_bytes())
        .await?;

    loop {
        match timeout(Duration::from_secs(15), events_rx.recv()).await {
            Ok(Ok(message)) => {
                stream
                    .write_all(sse_frame("dashboard", &message).as_bytes())
                    .await?;
            }
            Ok(Err(_)) => break,
            Err(_) => {
                stream.write_all(b": heartbeat\n\n").await?;
            }
        }
    }

    Ok(())
}

async fn route_request(app: &AutoLoopApp, target: &str) -> Result<String> {
    let path = target.split('?').next().unwrap_or(target).trim_end_matches('/');

    if path.is_empty() || path == "/" || path == "/health" {
        return Ok(build_json_response(
            200,
            serde_json::json!({
                "status": "ok",
                "service": "autoloop-dashboard-snapshot",
            })
            .to_string(),
        ));
    }

    if let Some(session) = path.strip_prefix("/api/dashboard/") {
        if let Some(body) = read_runtime_artifact("dashboard", session) {
            return Ok(build_json_response(200, body));
        }
        return Ok(build_json_response(
            200,
            app.export_dashboard_snapshot(session).await?,
        ));
    }

    if let Some(session) = path.strip_prefix("/api/replay/") {
        if let Some(body) = read_runtime_artifact("replay", session) {
            return Ok(build_json_response(200, body));
        }
        return Ok(build_json_response(
            200,
            app.export_session_replay(session).await?,
        ));
    }

    if let Some(session) = path.strip_prefix("/api/catalog/") {
        let body = read_runtime_artifact("dashboard", session)
            .unwrap_or(app.export_dashboard_snapshot(session).await?);
        let value = serde_json::from_str::<serde_json::Value>(&body)?;
        let catalog = value
            .get("capabilityCatalog")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        return Ok(build_json_response(
            200,
            serde_json::to_string_pretty(&catalog)?,
        ));
    }

    if let Some(session) = path.strip_prefix("/api/business/") {
        let body = read_runtime_artifact("dashboard", session)
            .unwrap_or(app.export_dashboard_snapshot(session).await?);
        let value = serde_json::from_str::<serde_json::Value>(&body)?;
        let payload = serde_json::json!({
            "session_id": session,
            "business": value.get("business").cloned().unwrap_or_else(|| serde_json::json!({})),
            "work_orders": value.get("workOrders")
                .cloned()
                .or_else(|| value.get("work_orders").cloned())
                .unwrap_or_else(|| serde_json::json!([])),
            "revenue_events": value.get("revenueEvents")
                .cloned()
                .or_else(|| value.get("revenue_events").cloned())
                .unwrap_or_else(|| serde_json::json!([])),
        });
        return Ok(build_json_response(200, serde_json::to_string_pretty(&payload)?));
    }

    if path == "/api/operator/settings" {
        let settings = read_operator_settings().unwrap_or_default();
        return Ok(build_json_response(
            200,
            serde_json::to_string_pretty(&settings)?,
        ));
    }

    if let Some(category) = path.strip_prefix("/runtime/") {
        let file = resolve_runtime_file(category);
        if let Ok(body) = std::fs::read_to_string(&file) {
            return Ok(build_json_response(200, body));
        }
    }

    Ok(build_json_response(
        404,
        serde_json::json!({"error":"not_found","path":path}).to_string(),
    ))
}

async fn route_mutation(
    app: &AutoLoopApp,
    target: &str,
    body: &str,
    events_tx: &broadcast::Sender<String>,
) -> Result<String> {
    let path = target.split('?').next().unwrap_or(target).trim_end_matches('/');

    if path == "/api/capabilities/govern" {
        let request = serde_json::from_str::<CapabilityGovernanceRequest>(body)?;
        let result = app
            .govern_mcp_capability(&request.action, &request.tool)
            .await?;

        let _ = events_tx.send(serde_json::to_string(&DashboardEvent {
            kind: "capability_governed".into(),
            tool: Some(request.tool),
            action: Some(request.action),
            created_at_ms: crate::orchestration::current_time_ms(),
        })?);

        return Ok(build_json_response(200, result));
    }

    if path == "/api/operator/settings" {
        let settings = serde_json::from_str::<OperatorSettings>(body)?;
        write_operator_settings(&settings)?;
        let _ = events_tx.send(serde_json::to_string(&DashboardEvent {
            kind: "operator_settings_saved".into(),
            tool: None,
            action: None,
            created_at_ms: crate::orchestration::current_time_ms(),
        })?);
        return Ok(build_json_response(
            200,
            serde_json::json!({
                "status": "saved",
                "path": resolve_runtime_file("operator-settings.json"),
            })
            .to_string(),
        ));
    }

    Ok(build_json_response(
        404,
        serde_json::json!({"error":"not_found","path":path}).to_string(),
    ))
}

#[derive(Debug, serde::Deserialize)]
struct CapabilityGovernanceRequest {
    action: String,
    tool: String,
}

fn resolve_runtime_file(category_and_name: &str) -> PathBuf {
    PathBuf::from("D:\\AutoLoop\\autoloop-app\\deploy\\runtime").join(category_and_name)
}

fn read_runtime_artifact(category: &str, session: &str) -> Option<String> {
    let file = resolve_runtime_file(&format!("{category}\\{session}.json"));
    std::fs::read_to_string(file).ok()
}

fn read_operator_settings() -> Option<OperatorSettings> {
    std::fs::read_to_string(resolve_runtime_file("operator-settings.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

fn write_operator_settings(settings: &OperatorSettings) -> Result<()> {
    let file = resolve_runtime_file("operator-settings.json");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}

fn build_json_response(status: u16, body: String) -> String {
    let status_text = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    };

    format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn sse_frame(event: &str, data: &str) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_builder_includes_cors_and_json_type() {
        let response = build_json_response(200, "{\"ok\":true}".into());
        assert!(response.contains("Access-Control-Allow-Origin: *"));
        assert!(response.contains("Content-Type: application/json"));
    }

    #[test]
    fn sse_frame_formats_event_and_data() {
        let frame = sse_frame("dashboard", "{\"ok\":true}");
        assert!(frame.contains("event: dashboard"));
        assert!(frame.contains("data: {\"ok\":true}"));
    }

    #[test]
    fn operator_settings_default_is_valid() {
        let settings = OperatorSettings::default();
        assert!(!settings.language.is_empty());
        assert!(!settings.provider_vendor.is_empty());
        assert!(!settings.api_base_url.is_empty());
        assert!(!settings.default_model.is_empty());
    }
}
