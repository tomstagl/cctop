//! OpenTelemetry receiver (phase 2). Claude Code can export metrics and log
//! events over OTLP/HTTP; cctop hosts a tiny loopback receiver and uses the
//! data where it is more exact than the transcript (API latency including
//! time-to-first-token, tool durations, cost).
//!
//! Only OTLP/JSON is decoded (`OTEL_EXPORTER_OTLP_PROTOCOL=http/json`);
//! protobuf bodies get a 415 with that hint.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use serde_json::Value;

pub const DEFAULT_ADDR: &str = "127.0.0.1:4318";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApiRequest {
    pub at_ms: i64,
    pub duration_ms: Option<u64>,
    pub ttft_ms: Option<u64>,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolResult {
    pub at_ms: i64,
    pub tool_name: String,
    pub tool_use_id: Option<String>,
    pub duration_ms: Option<u64>,
    pub success: Option<bool>,
}

/// Everything received for one session id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionData {
    /// `claude_code.token.usage` by `type` attribute (input, output, cacheRead, cacheCreation).
    pub tokens: BTreeMap<String, f64>,
    pub cost_usd: f64,
    pub lines_added: f64,
    pub lines_removed: f64,
    pub active_time_s: f64,
    pub api_requests: Vec<ApiRequest>,
    pub api_errors: usize,
    pub tool_results: Vec<ToolResult>,
}

impl SessionData {
    pub fn last_ttft_ms(&self) -> Option<u64> {
        self.api_requests.iter().rev().find_map(|r| r.ttft_ms)
    }
    /// Durations per tool name (for exact p50/p95 without hooks).
    pub fn durations_by_tool(&self) -> HashMap<String, Vec<u64>> {
        let mut m: HashMap<String, Vec<u64>> = HashMap::new();
        for t in &self.tool_results {
            if let Some(d) = t.duration_ms {
                m.entry(t.tool_name.clone()).or_default().push(d);
            }
        }
        m
    }
}

#[derive(Debug, Default)]
pub struct Store {
    pub sessions: BTreeMap<String, SessionData>,
    pub requests: usize,
}

pub type Shared = Arc<Mutex<Store>>;

fn attr_map(attrs: Option<&Value>) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    if let Some(a) = attrs.and_then(Value::as_array) {
        for kv in a {
            if let (Some(k), Some(v)) = (kv.get("key").and_then(Value::as_str), kv.get("value")) {
                let val = v
                    .get("stringValue")
                    .cloned()
                    .or_else(|| v.get("intValue").cloned())
                    .or_else(|| v.get("doubleValue").cloned())
                    .or_else(|| v.get("boolValue").cloned())
                    .unwrap_or(Value::Null);
                m.insert(k.to_string(), val);
            }
        }
    }
    m
}

fn num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

fn session_of(attrs: &HashMap<String, Value>, resource: &HashMap<String, Value>) -> String {
    for k in ["session.id", "session_id", "claude.session_id"] {
        if let Some(Value::String(s)) = attrs.get(k).or_else(|| resource.get(k)) {
            return s.clone();
        }
    }
    "unknown".into()
}

fn point_value(dp: &Value) -> f64 {
    num(dp.get("asDouble"))
        .or_else(|| num(dp.get("asInt")))
        .unwrap_or(0.0)
}

/// Fold an OTLP/JSON metrics export into the store.
pub fn ingest_metrics(store: &mut Store, v: &Value) {
    for rm in v
        .get("resourceMetrics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let resource = attr_map(rm.get("resource").and_then(|r| r.get("attributes")));
        for sm in rm
            .get("scopeMetrics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for metric in sm
                .get("metrics")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = metric.get("name").and_then(Value::as_str).unwrap_or("");
                let points = ["sum", "gauge"]
                    .iter()
                    .filter_map(|k| {
                        metric
                            .get(k)
                            .and_then(|s| s.get("dataPoints"))
                            .and_then(Value::as_array)
                    })
                    .flatten();
                for dp in points {
                    let attrs = attr_map(dp.get("attributes"));
                    let sid = session_of(&attrs, &resource);
                    let s = store.sessions.entry(sid).or_default();
                    let val = point_value(dp);
                    match name {
                        "claude_code.token.usage" => {
                            let ty = attrs
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                                .to_string();
                            *s.tokens.entry(ty).or_insert(0.0) += val;
                        }
                        "claude_code.cost.usage" => s.cost_usd += val,
                        "claude_code.lines_of_code.count" => {
                            match attrs.get("type").and_then(Value::as_str) {
                                Some("removed") => s.lines_removed += val,
                                _ => s.lines_added += val,
                            }
                        }
                        "claude_code.active_time.total" => s.active_time_s += val,
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Fold an OTLP/JSON logs export into the store.
pub fn ingest_logs(store: &mut Store, v: &Value) {
    for rl in v
        .get("resourceLogs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let resource = attr_map(rl.get("resource").and_then(|r| r.get("attributes")));
        for sl in rl
            .get("scopeLogs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for rec in sl
                .get("logRecords")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let attrs = attr_map(rec.get("attributes"));
                let event = attrs
                    .get("event.name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        rec.get("body")
                            .and_then(|b| b.get("stringValue"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
                let at_ms = num(rec.get("timeUnixNano"))
                    .map(|n| (n / 1e6) as i64)
                    .unwrap_or(0);
                let sid = session_of(&attrs, &resource);
                let s = store.sessions.entry(sid).or_default();
                let ms = |k: &str| num(attrs.get(k)).map(|x| x as u64);
                match event.as_str() {
                    "api_request" => s.api_requests.push(ApiRequest {
                        at_ms,
                        duration_ms: ms("duration_ms"),
                        ttft_ms: ms("time_to_first_token_ms").or_else(|| ms("ttft_ms")),
                        model: attrs
                            .get("model")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        cost_usd: num(attrs.get("cost_usd")),
                    }),
                    "api_error" => s.api_errors += 1,
                    "tool_result" => s.tool_results.push(ToolResult {
                        at_ms,
                        tool_name: attrs
                            .get("tool_name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_string(),
                        tool_use_id: attrs
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        duration_ms: ms("duration_ms"),
                        success: attrs
                            .get("success")
                            .and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true"))),
                    }),
                    _ => {}
                }
            }
        }
    }
}

/// Handle one HTTP/1.1 request on `stream`.
fn handle(mut stream: TcpStream, store: &Shared) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut len = 0usize;
    let mut ctype = String::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h == "\r\n" || h == "\n" || h.is_empty() {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = lower.strip_prefix("content-type:") {
            ctype = v.trim().to_string();
        }
    }
    let mut body = vec![0u8; len.min(64 << 20)];
    let _ = reader.read_exact(&mut body);
    let respond = |stream: &mut TcpStream, status: &str, body: &str| {
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    };
    if method != "POST" {
        return respond(&mut stream, "405 Method Not Allowed", "{}");
    }
    if ctype.contains("protobuf") {
        return respond(
            &mut stream,
            "415 Unsupported Media Type",
            r#"{"error":"cctop decodes OTLP/JSON only; set OTEL_EXPORTER_OTLP_PROTOCOL=http/json"}"#,
        );
    }
    let Ok(v) = serde_json::from_slice::<Value>(&body) else {
        return respond(
            &mut stream,
            "400 Bad Request",
            r#"{"error":"invalid JSON"}"#,
        );
    };
    let mut st = store.lock().unwrap();
    st.requests += 1;
    match path.as_str() {
        "/v1/metrics" => ingest_metrics(&mut st, &v),
        "/v1/logs" => ingest_logs(&mut st, &v),
        _ => {
            drop(st);
            return respond(&mut stream, "404 Not Found", "{}");
        }
    }
    drop(st);
    respond(&mut stream, "200 OK", "{}");
}

/// Bind `addr` (loopback only) and serve on a background thread.
/// Returns the shared store and the bound address.
pub fn serve(addr: &str) -> std::io::Result<(Shared, std::net::SocketAddr)> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    if !local.ip().is_loopback() {
        return Err(std::io::Error::other(
            "cctop --otlp binds loopback addresses only",
        ));
    }
    let store: Shared = Arc::new(Mutex::new(Store::default()));
    let s2 = store.clone();
    std::thread::Builder::new()
        .name("otlp".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                handle(stream, &s2);
            }
        })?;
    Ok((store, local))
}

/// The `env` block a user adds to `~/.claude/settings.json` to export here.
pub fn env_block(addr: &str) -> String {
    format!(
        r#"Add to ~/.claude/settings.json (cctop never writes this for you):

  "env": {{
    "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
    "OTEL_METRICS_EXPORTER": "otlp",
    "OTEL_LOGS_EXPORTER": "otlp",
    "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json",
    "OTEL_EXPORTER_OTLP_ENDPOINT": "http://{addr}",
    "OTEL_METRIC_EXPORT_INTERVAL": "10000",
    "OTEL_LOGS_EXPORT_INTERVAL": "5000"
  }}

Then run cctop with --otlp (default {DEFAULT_ADDR}).
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const METRICS: &str = r#"{"resourceMetrics":[{"resource":{"attributes":[{"key":"session.id","value":{"stringValue":"sess-1"}}]},"scopeMetrics":[{"metrics":[
      {"name":"claude_code.token.usage","sum":{"dataPoints":[
        {"asInt":"1200","attributes":[{"key":"type","value":{"stringValue":"input"}},{"key":"session.id","value":{"stringValue":"sess-1"}}]},
        {"asInt":"300","attributes":[{"key":"type","value":{"stringValue":"output"}},{"key":"session.id","value":{"stringValue":"sess-1"}}]},
        {"asInt":"9000","attributes":[{"key":"type","value":{"stringValue":"cacheRead"}},{"key":"session.id","value":{"stringValue":"sess-1"}}]}]}},
      {"name":"claude_code.cost.usage","sum":{"dataPoints":[{"asDouble":0.42,"attributes":[{"key":"session.id","value":{"stringValue":"sess-1"}}]}]}},
      {"name":"claude_code.lines_of_code.count","sum":{"dataPoints":[{"asInt":"31","attributes":[{"key":"type","value":{"stringValue":"added"}}]},{"asInt":"4","attributes":[{"key":"type","value":{"stringValue":"removed"}}]}]}},
      {"name":"claude_code.active_time.total","sum":{"dataPoints":[{"asDouble":95.5,"attributes":[]}]}}
    ]}]}]}"#;

    pub const LOGS: &str = r#"{"resourceLogs":[{"resource":{"attributes":[{"key":"session.id","value":{"stringValue":"sess-1"}}]},"scopeLogs":[{"logRecords":[
      {"timeUnixNano":"1787824000000000000","attributes":[{"key":"event.name","value":{"stringValue":"api_request"}},{"key":"duration_ms","value":{"intValue":"2400"}},{"key":"time_to_first_token_ms","value":{"intValue":"1200"}},{"key":"model","value":{"stringValue":"claude-opus-5"}},{"key":"cost_usd","value":{"doubleValue":0.12}}]},
      {"timeUnixNano":"1787824001000000000","attributes":[{"key":"event.name","value":{"stringValue":"api_error"}}]},
      {"timeUnixNano":"1787824002000000000","attributes":[{"key":"event.name","value":{"stringValue":"tool_result"}},{"key":"tool_name","value":{"stringValue":"Bash"}},{"key":"tool_use_id","value":{"stringValue":"toolu_01RFWxUzic1XtD2xrUwwVYhe"}},{"key":"duration_ms","value":{"intValue":"640"}},{"key":"success","value":{"boolValue":false}}]},
      {"timeUnixNano":"1787824003000000000","attributes":[{"key":"event.name","value":{"stringValue":"tool_result"}},{"key":"tool_name","value":{"stringValue":"Read"}},{"key":"duration_ms","value":{"intValue":"20"}},{"key":"success","value":{"boolValue":true}}]}
    ]}]}]}"#;

    fn post(addr: std::net::SocketAddr, path: &str, ctype: &str, body: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        write!(s, "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn receiver_ingests_metrics_and_logs_over_http() {
        let (store, addr) = serve("127.0.0.1:0").unwrap();
        assert!(post(addr, "/v1/metrics", "application/json", METRICS).starts_with("HTTP/1.1 200"));
        assert!(post(addr, "/v1/logs", "application/json", LOGS).starts_with("HTTP/1.1 200"));
        assert!(
            post(addr, "/v1/metrics", "application/x-protobuf", "xx").starts_with("HTTP/1.1 415")
        );
        assert!(post(addr, "/v1/other", "application/json", "{}").starts_with("HTTP/1.1 404"));
        assert!(post(addr, "/v1/logs", "application/json", "{nope").starts_with("HTTP/1.1 400"));
        let st = store.lock().unwrap();
        let s = &st.sessions["sess-1"];
        assert_eq!(s.tokens["input"], 1200.0);
        assert_eq!(s.tokens["cacheRead"], 9000.0);
        assert_eq!(s.cost_usd, 0.42);
        assert_eq!((s.lines_added, s.lines_removed), (31.0, 4.0));
        assert_eq!(s.active_time_s, 95.5);
        assert_eq!(s.api_requests.len(), 1);
        assert_eq!(s.last_ttft_ms(), Some(1200));
        assert_eq!(s.api_errors, 1);
        assert_eq!(s.tool_results.len(), 2);
        assert_eq!(s.durations_by_tool()["Bash"], vec![640]);
        assert!(serve("0.0.0.0:0").is_err(), "non-loopback refused");
        assert!(env_block(DEFAULT_ADDR).contains("http/json"));
    }
}

#[cfg(test)]
mod state_tests {
    use super::tests::{LOGS, METRICS};
    use super::*;
    use crate::ui::state::tests_support::fixture_state;

    #[test]
    fn otel_data_reaches_summary_ttft_and_clears_approx() {
        let (store, addr) = serve("127.0.0.1:0").unwrap();
        for (path, body) in [("/v1/metrics", METRICS), ("/v1/logs", LOGS)] {
            let mut s = TcpStream::connect(addr).unwrap();
            write!(s, "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
        }
        let data = store.lock().unwrap().sessions["sess-1"].clone();
        let mut state = fixture_state();
        assert!(
            state.tools.by_name()["Bash"].approx,
            "transcript timings are approximate"
        );
        state.apply_otel(&data);
        let bash = &state.tools.by_name()["Bash"];
        assert!(!bash.approx, "OTel durations replace the estimate");
        assert_eq!(bash.p50_ms, Some(640));
        assert_eq!(
            state
                .tools
                .get("toolu_01RFWxUzic1XtD2xrUwwVYhe")
                .unwrap()
                .duration_ms,
            Some(640)
        );
        let sum = crate::query::summary(&state);
        assert_eq!(sum["otel"]["tokens"]["input"], 1200.0);
        assert_eq!(sum["otel"]["ttft_ms"]["value"], 1200);
        assert_eq!(sum["otel"]["api_errors"], 1);
        let mut app = crate::app::App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut crate::ui::State| s.apply(l)),
        );
        app.state = state;
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        let out = crate::app::render_to_string(&app, 70, 60);
        assert!(out.contains("ttft 1.2s"), "{out}");
    }
}
