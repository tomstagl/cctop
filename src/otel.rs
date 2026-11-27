//! OpenTelemetry receiver (phase 2). Claude Code can export metrics and log
//! events over OTLP/HTTP; cctop hosts a tiny loopback receiver and uses the
//! data where it is more exact than the transcript (API latency including
//! time-to-first-token, tool durations, cost).
//!
//! Only OTLP/JSON is decoded (`OTEL_EXPORTER_OTLP_PROTOCOL=http/json`);
//! protobuf bodies get a 415 with that hint.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
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

/// Ids and times shared by every kept span.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpanIds {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub start_ns: u64,
    pub end_ns: u64,
}

/// A kept span: an ALLOWLIST of facts per kind. Nothing else from the wire
/// (prompts, tool inputs, hook definitions, account identifiers, `session.id`)
/// has a field here, so it cannot be stored.
#[derive(Debug, Clone, PartialEq)]
pub enum Span {
    Interaction {
        ids: SpanIds,
        sequence: Option<u64>,
    },
    LlmRequest {
        ids: SpanIds,
        model: Option<String>,
        query_source: Option<String>,
        response_id: Option<String>,
        attempts: u32,
        attempt_times_ns: Vec<u64>,
        ttft_ms: Option<u64>,
        first_content_ms: Option<u64>,
        status_code: Option<u64>,
        success: Option<bool>,
        duration_ms: Option<u64>,
    },
    Tool {
        ids: SpanIds,
        tool_name: Option<String>,
        mcp_server: Option<String>,
        tool_use_id: Option<String>,
    },
    Permission {
        ids: SpanIds,
        decision: Option<String>,
        /// Kept verbatim; displayed as `other` when not a known value.
        source: Option<String>,
        duration_ms: Option<u64>,
    },
    ToolExec {
        ids: SpanIds,
        success: Option<bool>,
        error_type: Option<String>,
        duration_ms: Option<u64>,
    },
    Hook {
        ids: SpanIds,
        hook_event: Option<String>,
        hook_name: Option<String>,
        num_hooks: Option<u64>,
        duration_ms: Option<u64>,
        num_blocking: Option<u64>,
    },
    Compaction {
        ids: SpanIds,
        trigger: Option<String>,
        message_count: Option<u64>,
    },
}

/// Spans kept per session (ring).
pub const SPAN_RING: usize = 2000;
/// Largest request body accepted; more is a 413.
pub const MAX_BODY: usize = 4 << 20;

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
    /// Allowlisted facts of the last [`SPAN_RING`] kept spans.
    pub spans: VecDeque<Span>,
    /// Spans whose name is not one of the kept kinds (counted, not stored).
    pub unknown_spans: u64,
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

fn str_attr(a: &HashMap<String, Value>, k: &str) -> Option<String> {
    a.get(k).and_then(Value::as_str).map(str::to_string)
}
fn u64_attr(a: &HashMap<String, Value>, k: &str) -> Option<u64> {
    num(a.get(k)).map(|f| f as u64)
}
fn bool_attr(a: &HashMap<String, Value>, k: &str) -> Option<bool> {
    a.get(k)
        .and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))
}
fn ns(v: Option<&Value>) -> u64 {
    num(v).map_or(0, |f| f as u64)
}

/// Decode `/v1/traces`: keep the seven span kinds' allowlisted facts in a
/// per-session ring; count other span names; drop everything else.
pub fn ingest_traces(store: &mut Store, v: &Value) {
    let Some(rs) = v.get("resourceSpans").and_then(Value::as_array) else {
        return;
    };
    for r in rs {
        let resource = attr_map(r.get("resource").and_then(|x| x.get("attributes")));
        for sc in r
            .get("scopeSpans")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for sp in sc
                .get("spans")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let a = attr_map(sp.get("attributes"));
                let sid = session_of(&a, &resource);
                let data = store.sessions.entry(sid).or_default();
                let id = |k: &str| sp.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let ids = SpanIds {
                    trace_id: id("traceId"),
                    span_id: id("spanId"),
                    parent_span_id: Some(id("parentSpanId")).filter(|p| !p.is_empty()),
                    start_ns: ns(sp.get("startTimeUnixNano")),
                    end_ns: ns(sp.get("endTimeUnixNano")),
                };
                let kept = match sp.get("name").and_then(Value::as_str).unwrap_or("") {
                    "claude_code.interaction" => Span::Interaction {
                        ids,
                        sequence: u64_attr(&a, "interaction.sequence"),
                    },
                    "claude_code.llm_request" => {
                        let attempt_times_ns: Vec<u64> = sp
                            .get("events")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter(|e| {
                                e.get("name").and_then(Value::as_str)
                                    == Some("gen_ai.request.attempt")
                            })
                            .map(|e| ns(e.get("timeUnixNano")))
                            .collect();
                        Span::LlmRequest {
                            ids,
                            model: str_attr(&a, "model"),
                            query_source: str_attr(&a, "query_source_safe")
                                .or_else(|| str_attr(&a, "query_source")),
                            response_id: str_attr(&a, "gen_ai.response.id"),
                            attempts: attempt_times_ns.len() as u32,
                            attempt_times_ns,
                            ttft_ms: u64_attr(&a, "ttft_ms"),
                            first_content_ms: u64_attr(&a, "first_content_ms"),
                            status_code: u64_attr(&a, "status_code"),
                            success: bool_attr(&a, "success"),
                            duration_ms: u64_attr(&a, "duration_ms"),
                        }
                    }
                    "claude_code.tool" => Span::Tool {
                        ids,
                        tool_name: str_attr(&a, "tool_name_safe")
                            .or_else(|| str_attr(&a, "tool_name"))
                            .or_else(|| str_attr(&a, "gen_ai.tool.name")),
                        mcp_server: str_attr(&a, "anthropic.mcp.server.name"),
                        tool_use_id: str_attr(&a, "tool_use_id"),
                    },
                    "claude_code.tool.blocked_on_user" => Span::Permission {
                        ids,
                        decision: str_attr(&a, "decision"),
                        source: str_attr(&a, "source"),
                        duration_ms: u64_attr(&a, "duration_ms"),
                    },
                    "claude_code.tool.execution" => Span::ToolExec {
                        ids,
                        success: bool_attr(&a, "success"),
                        error_type: str_attr(&a, "error.type"),
                        duration_ms: u64_attr(&a, "duration_ms"),
                    },
                    "claude_code.hook" => Span::Hook {
                        ids,
                        hook_event: str_attr(&a, "hook_event"),
                        hook_name: str_attr(&a, "hook_name"),
                        num_hooks: u64_attr(&a, "num_hooks"),
                        duration_ms: u64_attr(&a, "duration_ms"),
                        num_blocking: u64_attr(&a, "num_blocking"),
                    },
                    "claude_code.compaction" => Span::Compaction {
                        ids,
                        trigger: str_attr(&a, "trigger"),
                        message_count: u64_attr(&a, "message_count"),
                    },
                    _ => {
                        data.unknown_spans += 1;
                        continue;
                    }
                };
                data.spans.push_back(kept);
                if data.spans.len() > SPAN_RING {
                    data.spans.pop_front();
                }
            }
        }
    }
}

/// Write one raw request body to `<dir>/<unix-ms>-<path-tail>.json`
/// (`cctop run --dump-otel`). Best effort: a failed write never fails the
/// request. Bodies are kept as received, whatever the path or content type,
/// so they CONTAIN account identifiers (email, user and organization ids) and
/// prompt text: the directory is created 0700 and the files 0600 on Unix;
/// keep it private and delete it afterwards.
fn dump_body(dir: &Path, path: &str, body: &[u8]) {
    let tail: String = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(dir.join(format!("{ms}-{tail}.json")))
        {
            let _ = f.write_all(body);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(dir.join(format!("{ms}-{tail}.json")), body);
    }
}

/// Handle one HTTP/1.1 request on `stream`.
fn handle(mut stream: TcpStream, store: &Shared, dump: Option<&Path>) {
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
    let mut chunked = false;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h == "\r\n" || h == "\n" || h.is_empty() {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        } else if lower
            .strip_prefix("transfer-encoding:")
            .is_some_and(|v| v.contains("chunked"))
        {
            chunked = true;
        } else if let Some(v) = lower.strip_prefix("content-type:") {
            ctype = v.trim().to_string();
        }
    }
    if chunked && len == 0 {
        let b = r#"{"error":"send Content-Length; chunked bodies are not decoded"}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 411 Length Required\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
            b.len()
        );
        return;
    }
    if len > MAX_BODY {
        let b = r#"{"error":"body over 4 MB"}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 413 Payload Too Large\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
            b.len()
        );
        return;
    }
    let mut body = vec![0u8; len];
    let _ = reader.read_exact(&mut body);
    if let Some(dir) = dump {
        dump_body(dir, &path, &body);
    }
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
        "/v1/traces" => ingest_traces(&mut st, &v),
        _ => {
            drop(st);
            return respond(&mut stream, "404 Not Found", "{}");
        }
    }
    drop(st);
    respond(&mut stream, "200 OK", "{}");
}

/// Bind `addr` (loopback only) and serve on a background thread.
/// Returns the shared store and the bound address. With `dump` set, every
/// request body is also written there raw (see [`dump_body`]).
pub fn serve(addr: &str, dump: Option<PathBuf>) -> std::io::Result<(Shared, std::net::SocketAddr)> {
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
                handle(stream, &s2, dump.as_deref());
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
    "OTEL_TRACES_EXPORTER": "otlp",
    "CLAUDE_CODE_ENHANCED_TELEMETRY_BETA": "1",
    "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json",
    "OTEL_EXPORTER_OTLP_ENDPOINT": "http://{addr}",
    "OTEL_METRIC_EXPORT_INTERVAL": "10000",
    "OTEL_LOGS_EXPORT_INTERVAL": "5000"
  }}

Traces (OTEL_TRACES_EXPORTER, CLAUDE_CODE_ENHANCED_TELEMETRY_BETA) are a Claude
Code beta: leave both lines out for metrics and logs only. Every exported span
carries account identifiers (user.email, user.id, organization.id) and the
interaction span carries the prompt unless OTEL_LOG_USER_PROMPTS is unset. cctop
keeps none of them: its receiver drops identifiers, prompt and content
attributes before anything is stored. Keep OTEL_LOG_USER_PROMPTS unset.
Traces add one more POST per batch to cctop and nothing else; spans beyond the
last 2000 per session are dropped.

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

    /// One OTLP/JSON span: `attrs` are (key, JSON value) pairs.
    fn span(name: &str, id: u32, attrs: &[(&str, &str)], events: &str) -> String {
        let a: Vec<String> = attrs
            .iter()
            .map(|(k, v)| format!(r#"{{"key":"{k}","value":{v}}}"#))
            .collect();
        format!(
            r#"{{"traceId":"t1","spanId":"s{id}","parentSpanId":"p1","name":"{name}","startTimeUnixNano":"1000","endTimeUnixNano":"3000","attributes":[{}],"events":[{events}]}}"#,
            a.join(",")
        )
    }
    fn s(v: &str) -> String {
        format!(r#"{{"stringValue":"{v}"}}"#)
    }
    fn i(v: u64) -> String {
        format!(r#"{{"intValue":"{v}"}}"#)
    }
    fn traces(resource_sid: Option<&str>, spans: &[String]) -> Value {
        let res = resource_sid
            .map(|x| format!(r#"{{"key":"session.id","value":{}}}"#, s(x)))
            .unwrap_or_default();
        serde_json::from_str(&format!(
            r#"{{"resourceSpans":[{{"resource":{{"attributes":[{res}]}},"scopeSpans":[{{"spans":[{}]}}]}}]}}"#,
            spans.join(",")
        ))
        .unwrap()
    }

    #[test]
    fn traces_decode_each_kept_kind() {
        let ev = r#"{"name":"gen_ai.request.attempt","timeUnixNano":"1500","attributes":[{"key":"attempt","value":{"intValue":"1"}}]},{"name":"gen_ai.request.attempt","timeUnixNano":"2500"}"#;
        let spans = vec![
            span(
                "claude_code.interaction",
                1,
                &[("interaction.sequence", &i(3))],
                "",
            ),
            span(
                "claude_code.llm_request",
                2,
                &[
                    ("model", &s("m1")),
                    ("query_source_safe", &s("repl")),
                    ("gen_ai.response.id", &s("resp")),
                    ("ttft_ms", &i(120)),
                    ("first_content_ms", &i(150)),
                    ("duration_ms", &i(900)),
                    ("success", r#"{"boolValue":true}"#),
                ],
                ev,
            ),
            span(
                "claude_code.tool",
                3,
                &[
                    ("tool_name", &s("Bash")),
                    ("tool_use_id", &s("tu1")),
                    ("anthropic.mcp.server.name", &s("srv")),
                ],
                "",
            ),
            span(
                "claude_code.tool.blocked_on_user",
                4,
                &[
                    ("decision", &s("accept")),
                    ("source", &s("weird-new")),
                    ("duration_ms", &i(40)),
                ],
                "",
            ),
            span(
                "claude_code.tool.execution",
                5,
                &[
                    ("success", r#"{"boolValue":false}"#),
                    ("error.type", &s("E")),
                ],
                "",
            ),
            span(
                "claude_code.hook",
                6,
                &[
                    ("hook_event", &s("PreToolUse")),
                    ("hook_name", &s("h")),
                    ("num_hooks", &i(2)),
                    ("num_blocking", &i(1)),
                ],
                "",
            ),
            span(
                "claude_code.compaction",
                7,
                &[("trigger", &s("auto")), ("message_count", &i(88))],
                "",
            ),
        ];
        let mut st = Store::default();
        ingest_traces(&mut st, &traces(Some("sess-t"), &spans));
        let d = &st.sessions["sess-t"];
        assert_eq!(d.spans.len(), 7);
        assert_eq!(d.unknown_spans, 0);
        let Span::Interaction { ids, sequence } = &d.spans[0] else {
            panic!()
        };
        assert_eq!(ids.trace_id, "t1");
        assert_eq!(ids.span_id, "s1");
        assert_eq!(ids.parent_span_id.as_deref(), Some("p1"));
        assert_eq!((ids.start_ns, ids.end_ns), (1000, 3000));
        assert_eq!(*sequence, Some(3));
        let Span::LlmRequest {
            model,
            query_source,
            response_id,
            attempts,
            attempt_times_ns,
            ttft_ms,
            first_content_ms,
            status_code,
            success,
            duration_ms,
            ..
        } = &d.spans[1]
        else {
            panic!()
        };
        assert_eq!(model.as_deref(), Some("m1"));
        assert_eq!(query_source.as_deref(), Some("repl"));
        assert_eq!(response_id.as_deref(), Some("resp"));
        assert_eq!((*attempts, attempt_times_ns.clone()), (2, vec![1500, 2500]));
        assert_eq!((*ttft_ms, *first_content_ms), (Some(120), Some(150)));
        assert_eq!(
            (*status_code, *success, *duration_ms),
            (None, Some(true), Some(900))
        );
        assert!(
            matches!(&d.spans[2], Span::Tool { tool_name: Some(n), mcp_server: Some(m), tool_use_id: Some(u), .. } if n == "Bash" && m == "srv" && u == "tu1")
        );
        // Unknown source kept verbatim.
        assert!(
            matches!(&d.spans[3], Span::Permission { decision: Some(a), source: Some(b), duration_ms: Some(40), .. } if a == "accept" && b == "weird-new")
        );
        assert!(
            matches!(&d.spans[4], Span::ToolExec { success: Some(false), error_type: Some(e), .. } if e == "E")
        );
        assert!(
            matches!(&d.spans[5], Span::Hook { hook_event: Some(e), hook_name: Some(n), num_hooks: Some(2), num_blocking: Some(1), .. } if e == "PreToolUse" && n == "h")
        );
        assert!(
            matches!(&d.spans[6], Span::Compaction { trigger: Some(t), message_count: Some(88), .. } if t == "auto")
        );
    }

    #[test]
    fn tool_span_reads_tool_name_safe_first() {
        let mut st = Store::default();
        let only_safe = span("claude_code.tool", 1, &[("tool_name_safe", &s("Bash"))], "");
        let both = span(
            "claude_code.tool",
            2,
            &[("tool_name", &s("raw")), ("tool_name_safe", &s("safe"))],
            "",
        );
        let legacy = span("claude_code.tool", 3, &[("tool_name", &s("old"))], "");
        ingest_traces(&mut st, &traces(Some("t"), &[only_safe, both, legacy]));
        let names: Vec<_> = st.sessions["t"]
            .spans
            .iter()
            .map(|x| match x {
                Span::Tool { tool_name, .. } => tool_name.clone().unwrap(),
                _ => panic!(),
            })
            .collect();
        assert_eq!(names, ["Bash", "safe", "old"]);
    }

    #[test]
    fn chunked_without_length_is_411() {
        let (_st, addr) = serve("127.0.0.1:0", None).unwrap();
        let mut c = TcpStream::connect(addr).unwrap();
        write!(
            c,
            "POST /v1/traces HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
        )
        .unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 411"), "{out}");
    }

    #[test]
    fn llm_request_accepts_either_query_source_key() {
        let mut st = Store::default();
        let sp = span(
            "claude_code.llm_request",
            1,
            &[("query_source", &s("sdk"))],
            "",
        );
        ingest_traces(&mut st, &traces(Some("a"), &[sp]));
        assert!(
            matches!(&st.sessions["a"].spans[0], Span::LlmRequest { query_source: Some(q), attempts: 0, .. } if q == "sdk")
        );
    }

    #[test]
    fn traces_keep_allowlisted_facts_only() {
        let secrets = [
            ("hook_definitions", s("SECRET-HOOKDEF")),
            ("user_prompt", s("SECRET-PROMPT")),
            ("user_prompt_length", i(777_001)),
            ("tool_input", s("SECRET-TOOLIN")),
            ("tool_input.command", s("SECRET-TOOLCMD")),
            ("x.content", s("SECRET-CONTENT")),
            ("user.email", s("SECRET-EMAIL@x")),
            ("user.id", s("SECRET-USERID")),
            ("user.account_id", s("SECRET-ACCID")),
            ("user.account_uuid", s("SECRET-ACCUUID")),
            ("organization.id", s("SECRET-ORG")),
            ("gen_ai.conversation.id", s("SECRET-CONV")),
            ("bash_argv0", s("SECRET-ARGV0")),
        ];
        let kept: Vec<(&str, String)> = vec![("tool_name", s("Bash")), ("hook_event", s("Stop"))];
        let mut attrs: Vec<(&str, &str)> = secrets.iter().map(|(k, v)| (*k, v.as_str())).collect();
        attrs.extend(kept.iter().map(|(k, v)| (*k, v.as_str())));
        let names = [
            "claude_code.interaction",
            "claude_code.llm_request",
            "claude_code.tool",
            "claude_code.tool.blocked_on_user",
            "claude_code.tool.execution",
            "claude_code.hook",
            "claude_code.compaction",
        ];
        let spans: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(n, name)| span(name, n as u32, &attrs, ""))
            .collect();
        let mut st = Store::default();
        ingest_traces(&mut st, &traces(Some("SECRET-SESSION"), &spans));
        let dump = format!("{st:?}");
        // The session id keys the store (that is its one appearance).
        assert_eq!(dump.matches("SECRET-SESSION").count(), 1);
        assert!(dump.contains("Bash"));
        for needle in [
            "SECRET-HOOKDEF",
            "SECRET-PROMPT",
            "777001",
            "SECRET-TOOLIN",
            "SECRET-TOOLCMD",
            "SECRET-CONTENT",
            "SECRET-EMAIL",
            "SECRET-USERID",
            "SECRET-ACCID",
            "SECRET-ACCUUID",
            "SECRET-ORG",
            "SECRET-CONV",
            "SECRET-ARGV0",
        ] {
            assert!(!dump.contains(needle), "{needle} leaked: {dump}");
        }
    }

    #[test]
    fn unknown_span_names_are_counted_not_kept() {
        let mut st = Store::default();
        let spans = vec![
            span(
                "claude_code.mcp.rpc",
                1,
                &[("tool_name", &s("LEAK-NAME"))],
                "",
            ),
            span("other.thing", 2, &[], ""),
            span("claude_code.interaction", 3, &[], ""),
        ];
        ingest_traces(&mut st, &traces(Some("u"), &spans));
        let d = &st.sessions["u"];
        assert_eq!((d.spans.len(), d.unknown_spans), (1, 2));
        assert!(!format!("{st:?}").contains("LEAK-NAME"));
    }

    #[test]
    fn spans_are_keyed_by_their_own_session_id_over_the_resource() {
        let mut st = Store::default();
        let own = span(
            "claude_code.interaction",
            1,
            &[("session.id", &s("own"))],
            "",
        );
        let inherit = span("claude_code.interaction", 2, &[], "");
        ingest_traces(&mut st, &traces(Some("res"), &[own, inherit]));
        assert_eq!(st.sessions["own"].spans.len(), 1);
        assert_eq!(st.sessions["res"].spans.len(), 1);
        assert!(!format!("{:?}", st.sessions["own"].spans).contains("own"));
        let mut st = Store::default();
        ingest_traces(
            &mut st,
            &traces(None, &[span("claude_code.hook", 3, &[], "")]),
        );
        assert_eq!(st.sessions["unknown"].spans.len(), 1);
    }

    #[test]
    fn span_ring_keeps_the_last_2000() {
        let mut st = Store::default();
        let spans: Vec<String> = (0..2005)
            .map(|n| {
                span(
                    "claude_code.interaction",
                    n,
                    &[("interaction.sequence", &i(n as u64))],
                    "",
                )
            })
            .collect();
        ingest_traces(&mut st, &traces(Some("r"), &spans));
        let d = &st.sessions["r"];
        assert_eq!(d.spans.len(), 2000);
        assert!(matches!(
            d.spans.front(),
            Some(Span::Interaction {
                sequence: Some(5),
                ..
            })
        ));
        assert!(matches!(
            d.spans.back(),
            Some(Span::Interaction {
                sequence: Some(2004),
                ..
            })
        ));
    }

    #[test]
    fn traces_over_http_protobuf_415_and_body_cap_413() {
        let (store, addr) = serve("127.0.0.1:0", None).unwrap();
        let body = traces(Some("h"), &[span("claude_code.compaction", 1, &[], "")]).to_string();
        assert!(post(addr, "/v1/traces", "application/json", &body).starts_with("HTTP/1.1 200"));
        assert_eq!(store.lock().unwrap().sessions["h"].spans.len(), 1);
        let r = post(addr, "/v1/traces", "application/x-protobuf", "xx");
        assert!(
            r.starts_with("HTTP/1.1 415") && r.contains("http/json"),
            "{r}"
        );
        assert!(post(addr, "/v1/other", "application/json", "{}").starts_with("HTTP/1.1 404"));
        // Exactly 4 MB is accepted; one byte more is 413 (no body needed).
        let pad = " ".repeat((4 << 20) - body.len());
        let at_cap = format!("{body}{pad}");
        assert_eq!(at_cap.len(), 4 << 20);
        assert!(post(addr, "/v1/traces", "application/json", &at_cap).starts_with("HTTP/1.1 200"));
        let mut c = TcpStream::connect(addr).unwrap();
        write!(c, "POST /v1/traces HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", (4 << 20) + 1).unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 413"), "{out}");
    }

    #[test]
    fn dump_dir_receives_each_raw_body() {
        let dir = std::env::temp_dir().join(format!("cctop-dump-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (_store, addr) = serve("127.0.0.1:0", Some(dir.clone())).unwrap();
        let body = r#"{"resourceSpans":[]}"#;
        assert!(post(addr, "/v1/traces", "application/json", body).starts_with("HTTP/1.1"));
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with("-traces.json"), "{name}");
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), body);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir), 0o700);
            assert_eq!(mode(&files[0]), 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn receiver_ingests_metrics_and_logs_over_http() {
        let (store, addr) = serve("127.0.0.1:0", None).unwrap();
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
        assert!(serve("0.0.0.0:0", None).is_err(), "non-loopback refused");
        assert!(env_block(DEFAULT_ADDR).contains("http/json"));
    }

    #[test]
    fn env_block_turns_traces_on_and_says_what_cctop_drops() {
        let b = env_block(DEFAULT_ADDR);
        assert!(b.contains(r#""OTEL_TRACES_EXPORTER": "otlp""#), "{b}");
        assert!(
            b.contains(r#""CLAUDE_CODE_ENHANCED_TELEMETRY_BETA": "1""#),
            "{b}"
        );
        assert!(b.contains("a Claude\nCode beta"), "{b}");
        assert!(b.contains("cctop\nkeeps none of them"), "{b}");
        assert!(b.contains("Keep OTEL_LOG_USER_PROMPTS unset"), "{b}");
    }
}

#[cfg(test)]
mod state_tests {
    use super::tests::{LOGS, METRICS};
    use super::*;
    use crate::ui::state::tests_support::fixture_state;

    #[test]
    fn otel_data_reaches_summary_ttft_and_clears_approx() {
        let (store, addr) = serve("127.0.0.1:0", None).unwrap();
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
        app.state.open = Some(4);
        let out = crate::app::render_to_string(&app, 96, 70);
        assert!(out.contains("ttft 1.2s"), "{out}");
    }
}
