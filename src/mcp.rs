//! `cctop mcp`: the query interface as MCP tools over stdio (JSON-RPC 2.0,
//! newline-delimited, protocol 2024-11-05). Each tool returns exactly what
//! the matching `cctop query` subcommand prints.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use crate::load::Target;
use crate::query;

pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Tool name, description (≤ 200 chars), input schema.
pub fn tools() -> Vec<Value> {
    let session = json!({"type":"string","description":"Session id, name, pid or fixture path; default: the session cctop discovers"});
    let obj = |props: Value, required: Vec<&str>| json!({"type":"object","properties":props,"required":required});
    vec![
        json!({"name":"cctop_summary","description":"Session at a glance: context fill, tokens by class, cache hit ratio, cost and burn rate, rate limits, tool calls, advice count.","inputSchema":obj(json!({"session":session}), vec![])}),
        json!({"name":"cctop_ledger","description":"One row per turn: duration, API calls, cache read/write, fresh input, output, thinking, cost, tools, compaction, effort, model.","inputSchema":obj(json!({"session":session,"last":{"type":"integer","description":"Only the last N turns"}}), vec![])}),
        json!({"name":"cctop_tools","description":"Per-tool statistics (calls, errors, p50/p95, tokens pushed into context) and the five largest single results.","inputSchema":obj(json!({"session":session}), vec![])}),
        json!({"name":"cctop_advice","description":"Ranked Advisor recommendations with evidence, action, estimated saving and an explanation of the rule.","inputSchema":obj(json!({"session":session}), vec![])}),
        json!({"name":"cctop_prefix","description":"What rides on every request: CLAUDE.md files, tool schemas per MCP server, skills listing, memory index, reconciled against the first call.","inputSchema":obj(json!({"session":session}), vec![])}),
        json!({"name":"cctop_events","description":"Recent events (tool start/end, hooks, permissions, compactions, alerts). `since` like 10m, 2h.","inputSchema":obj(json!({"session":session,"since":{"type":"string"}}), vec![])}),
        json!({"name":"cctop_explain_metric","description":"Definition, formula, sources and caveats of a metric id from docs/metrics.md.","inputSchema":obj(json!({"metric_id":{"type":"string"}}), vec!["metric_id"])}),
    ]
}

fn state_for(args: &Value) -> Result<crate::ui::State, String> {
    let t = Target {
        session: args
            .get("session")
            .and_then(Value::as_str)
            .map(str::to_string),
        cwd: None,
        wait: false,
    };
    let mut state = crate::load::state(&t).map_err(|e| e.to_string())?;
    let mut engine = crate::advisor::Engine::default();
    engine.evaluate(&state);
    state.advice = engine.current.clone();
    Ok(state)
}

/// Run one tool. Errors become MCP `isError` results, not protocol errors.
pub fn call(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "cctop_explain_metric" => {
            let id = args
                .get("metric_id")
                .and_then(Value::as_str)
                .ok_or("metric_id is required")?;
            Ok(query::explain(id))
        }
        "cctop_summary" => Ok(query::summary(&state_for(args)?)),
        "cctop_ledger" => {
            let last = args.get("last").and_then(Value::as_u64).map(|n| n as usize);
            Ok(query::ledger_json(&state_for(args)?, last))
        }
        "cctop_tools" => Ok(query::tools(&state_for(args)?)),
        "cctop_advice" => Ok(query::advice(&state_for(args)?)),
        "cctop_prefix" => Ok(query::prefix(&state_for(args)?)),
        "cctop_events" => {
            let since = args
                .get("since")
                .and_then(Value::as_str)
                .and_then(query::parse_since);
            Ok(query::events(&state_for(args)?, since))
        }
        other => Err(format!("unknown tool {other}")),
    }
}

/// Handle one JSON-RPC request; `None` for notifications.
pub fn handle(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!({}));
    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "cctop", "version": env!("CARGO_PKG_VERSION")},
        }),
        "notifications/initialized" | "notifications/cancelled" => return None,
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(name, &args) {
                Ok(v) => {
                    json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&v).unwrap_or_default()}],"isError":false})
                }
                Err(e) => json!({"content":[{"type":"text","text":e}],"isError":true}),
            }
        }
        _ => {
            return id.map(|id| json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("method not found: {method}")}}));
        }
    };
    id.map(|id| json!({"jsonrpc":"2.0","id":id,"result":result}))
}

/// Serve newline-delimited JSON-RPC on the given reader/writer until EOF.
pub fn serve<R: BufRead, W: Write>(input: R, mut output: W) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            let err =
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}});
            writeln!(output, "{err}")?;
            output.flush()?;
            continue;
        };
        if let Some(resp) = handle(&req) {
            writeln!(output, "{resp}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// `cctop mcp`: stdio transport.
pub fn run_stdio() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(stdin.lock(), stdout.lock())
}

/// Rough token size of the tool schemas as a client would see them.
pub fn schema_tokens() -> usize {
    serde_json::to_string(&tools())
        .map(|s| s.len() / 4)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn descriptions_are_short_and_schema_is_small() {
        for t in tools() {
            let d = t["description"].as_str().unwrap();
            assert!(
                d.chars().count() <= 200,
                "{}: {} chars",
                t["name"],
                d.chars().count()
            );
            assert!(t["inputSchema"]["type"] == "object");
        }
        assert_eq!(tools().len(), 7);
        let tokens = schema_tokens();
        assert!(tokens < 600, "schemas ≈ {tokens} tokens");
    }

    #[test]
    fn stdio_initialize_list_and_call() {
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/session-a.jsonl");
        let input = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"cctop_summary","arguments":{"session":fixture}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"cctop_explain_metric","arguments":{"metric_id":"cache_hit_ratio"}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"cctop_ledger","arguments":{"session":fixture,"last":2}}}),
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"nope","arguments":{}}}),
            json!({"jsonrpc":"2.0","id":7,"method":"resources/list"}),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n")
            + "\n{not json}\n";
        let mut out = Vec::new();
        serve(Cursor::new(input), &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            lines.len(),
            8,
            "7 responses + 1 parse error; the notification is silent"
        );
        assert_eq!(lines[0]["result"]["serverInfo"]["name"], "cctop");
        assert_eq!(lines[0]["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(lines[1]["result"]["tools"].as_array().unwrap().len(), 7);
        let summary: Value =
            serde_json::from_str(lines[2]["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(summary["turns"]["value"], 15);
        assert_eq!(lines[2]["result"]["isError"], false);
        let explain: Value =
            serde_json::from_str(lines[3]["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(explain["panel"], "Tokens & Cost");
        let ledger: Value =
            serde_json::from_str(lines[4]["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(ledger.as_array().unwrap().len(), 2);
        assert_eq!(lines[5]["result"]["isError"], true);
        assert_eq!(lines[6]["error"]["code"], -32601);
        assert_eq!(lines[7]["error"]["code"], -32700);
        // Same JSON as `cctop query`.
        let via_query = crate::query::explain("cache_hit_ratio");
        assert_eq!(explain, via_query);
    }
}
