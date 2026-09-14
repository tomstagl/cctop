//! `cctop query`: every number the TUI shows, as JSON. Values carry their
//! unit and metric id so a caller (or the `cctop-insights` skill) can look
//! them up in `docs/metrics.md`; absent optional sources say so explicitly.

use serde_json::{json, Value};

use crate::advisor;
use crate::ledger;
use crate::metrics::{cost, registry};
use crate::ui::fmt;
use crate::ui::State;

/// A measured value with provenance.
pub fn m(value: impl Into<Value>, unit: &str, metric_id: &str, approx: bool) -> Value {
    json!({"value": value.into(), "unit": unit, "metric_id": metric_id, "approx": approx})
}

/// An optional source that is not on disk.
pub fn missing(hint: &str) -> Value {
    json!({"source": "missing", "hint": hint})
}

const INSTALL_HINT: &str = "run cctop install";

pub fn summary(state: &State) -> Value {
    let ctx = state.context();
    let u = state.agg.total;
    let rates = cost::rates(&state.agg, state.cost.pricing(), state.clock_ms());
    let cost_v = match state.cost.current() {
        Some(c) => m(c.usd, "USD", "cost", c.approx),
        None => missing("no priced model or cost-state yet"),
    };
    let limits = match &state.limits {
        Some(l) => json!({
            "five_hour": m(l.five_hour_pct, "%", "limit_5h", false),
            "seven_day": m(l.seven_day_pct, "%", "limit_7d", false),
            "five_hour_resets_at_ms": l.five_hour_resets_at_ms,
            "exhaustion_ms": l.exhaustion_ms.map(|e| m(e, "epoch_ms", "limit_exhaustion", true)),
            "other_live_sessions": state.other_live_sessions,
        }),
        None => missing(INSTALL_HINT),
    };
    let current = state.agg.current_turn();
    json!({
        "session": {
            "name": state.session.name,
            "session_id": state.session.session_id,
            "pid": state.session.pid,
            "cwd": state.session.cwd,
            "version": state.session.version,
            "model": state.model(),
            "alive": state.session.alive,
            "permission_mode": state.session.permission_mode,
            "effort": state.agg.turns.iter().rev().find_map(|t| t.effort.clone()),
            "plan": state.session.tier.clone().map(|t| m(t, "enum", "plan_tier", false)).unwrap_or_else(|| missing("no ~/.claude.json tier")),
            "session_name": state.status_facts.session_name,
            "thinking": state.status_facts.thinking_enabled,
            "fast_mode": state.status_facts.fast_mode,
            "pr": state.status_facts.pr_number.or(state.agg.pr_number),
            "title": state.agg.title,
        },
        "cache": cache_object(state),
        "turns": m(state.agg.human_turns(), "count", "turn_number", false),
        "machine_turns": m(state.agg.turns.len() - state.agg.human_turns(), "count", "turn_number", false),
        "api_calls": m(state.agg.api_calls(), "count", "api_calls", false),
        "current_turn_elapsed": current.and_then(|t| t.elapsed_ms(state.clock_ms())).map(|e| m(e, "ms", "turn_elapsed", false)),
        "context": {
            "size": m(ctx.size, "tokens", "context_size", !ctx.window_exact),
            "window": m(ctx.window, "tokens", "context_window", !ctx.window_exact),
            "ratio": m(ctx.ratio(), "ratio", "context_size", !ctx.window_exact),
            "prefix": m(ctx.prefix, "tokens", "context_prefix", false),
            "velocity": m(ctx.velocity, "tokens/turn", "context_velocity", false),
            "turns_until_compaction": ctx.turns_until_compaction().map(|n| m(n, "turns", "turns_until_compaction", !ctx.threshold_learned)),
            "compactions": m(ctx.compactions.len(), "count", "compactions", false),
        },
        "tokens": {
            "cache_read": m(u.cache_read, "tokens", "cache_read", false),
            "cache_write": m(u.cache_write(), "tokens", "cache_write", false),
            "fresh_input": m(u.input, "tokens", "fresh_input", false),
            "output": m(u.output, "tokens", "output", false),
            "thinking": m(u.thinking, "tokens", "thinking", false),
            "cache_hit_ratio": u.cache_hit_ratio().map(|r| m(r, "ratio", "cache_hit_ratio", false)),
            "cache_ttl": state.agg.observed_ttl.map(|t| format!("{t:?}")),
        },
        "cost": cost_v,
        "cost_by_model": state.cost.by_model(),
        "cost_gradient": state.gradient().map(|g| json!({
            "per_call": m(g.per_call, "USD", "cost_per_call", true),
            "per_turn": m(g.per_turn, "USD", "cost_per_turn", true),
            "per_turn_at_100k": m(g.per_turn_at_100k, "USD", "cost_per_turn", true),
            "next_30_calls": m(g.next_30_calls, "USD", "cost_per_turn", true),
            "calls_per_turn": g.calls_per_turn,
            "cold": g.cold,
        })),
        "attribution": state.attribution_top(5).iter().map(|(k, s)| json!({"owner": k, "share": m(*s, "ratio", "attribution", false)})).collect::<Vec<_>>(),
        "agents_cost": state.agents_cost().map(|(usd, share)| json!({"usd": m(usd, "USD", "agents_cost", true), "share": m(share, "ratio", "agents_cost", true)})),
        "limit_weight_tier": state.model().map(crate::harness_facts::usage_weight::tier),
        "behaviour_flags": behaviour_flags(state),
        "burn_rate": rates.usd_per_hour.map(|h| m(h, "USD/h", "burn_rate", true)),
        "input_rate": m(rates.input_tokens_per_min, "tokens/min", "input_rate", false),
        "limits": limits,
        "tool_calls": m(state.tools.calls.len(), "count", "tool_calls", false),
        "tokens_to_ctx": m(state.tools.tokens_to_ctx(), "tokens", "tokens_to_ctx", true),
        "agents": m(state.agents.len(), "count", "agent_state", false),
        "hooks_installed": state.session.hooks_installed,
        "permission_waits": if state.session.hooks_installed {
            json!({"count": state.session.permission_waits, "total": m(state.session.permission_wait_ms, "ms", "permission_wait", true)})
        } else {
            missing(INSTALL_HINT)
        },
        "queued_prompts": m(state.agg.queued_prompts, "count", "queued_prompts", false),
        "advice_count": state.advice.len(),
        "otel": match &state.otel {
            Some(o) => json!({
                "tokens": o.tokens,
                "cost": m(o.cost_usd, "USD", "cost", false),
                "lines_added": o.lines_added,
                "lines_removed": o.lines_removed,
                "active_time_s": o.active_time_s,
                "api_requests": o.api_requests.len(),
                "api_errors": o.api_errors,
                "ttft_ms": o.last_ttft_ms().map(|t| m(t, "ms", "api_time", false)),
                "tool_results": o.tool_results.len(),
            }),
            None => missing("run cctop with --otlp and export telemetry (cctop install --otel)"),
        },
    })
}

/// `/usage`'s behaviour flags for the session.
fn behaviour_flags(state: &State) -> Value {
    let f = state.behaviour_flags();
    json!({
        "cache_miss_pct": m(f.cache_miss_pct, "%", "behaviour_flags", false),
        "long_context_pct": m(f.long_context_pct, "%", "behaviour_flags", false),
        "subagent_pct": m(f.subagent_pct, "%", "behaviour_flags", false),
        "high_parallel": f.high_parallel,
        "cron": f.cron,
        "tips": f.tips(),
    })
}

/// The cache block of `summary`: Claude Code's own diagnosis when the shim
/// is installed, the transcript's observation otherwise.
pub fn cache_object(state: &State) -> Value {
    let c = &state.cache;
    let clock = state.cache_clock();
    let approx = clock.is_none_or(|k| k.approx);
    let ttl = match state.cache_ttl_ms() {
        3_600_000 => "1h",
        _ => "5m",
    };
    json!({
        "source": if c.from_shim { "status_line" } else { "transcript" },
        "warm": state.cache_warm().map(|(w, a)| m(w, "bool", "cache_warm", a)),
        "ttl": m(ttl, "enum", "cache_ttl", !c.from_shim),
        "expires_in": clock.map(|k| m(k.remaining_ms, "ms", "cache_expires_in", k.approx)),
        "recache_tokens_if_cold": if c.from_shim { m(c.recache_tokens_if_cold, "tokens", "cache_recache_if_cold", false) } else { m(state.context().size, "tokens", "cache_recache_if_cold", true) },
        "misses": if c.from_shim { m(c.misses, "count", "cache_misses", false) } else { missing(INSTALL_HINT) },
        "expected_rebuilds": c.from_shim.then_some(c.expected_rebuilds),
        "last_miss_cause": c.last_miss_cause,
        "miss_causes": c.miss_causes,
        "hit_ratio": c.hit_ratio.or_else(|| state.agg.total.cache_hit_ratio()).map(|r| m(r, "ratio", "cache_hit_ratio", approx)),
    })
}

pub fn ledger_json(state: &State, last: Option<usize>) -> Value {
    let rows = ledger::rows(state);
    let skip = last.map(|n| rows.len().saturating_sub(n)).unwrap_or(0);
    Value::Array(
        rows.iter()
            .skip(skip)
            .map(|r| {
                json!({
                    "turn": r.turn,
                    "started_at_ms": r.started_at_ms,
                    "duration": r.duration_ms.map(|d| m(d, "ms", "turn_duration", false)),
                    "api_calls": m(r.api_calls, "count", "api_calls", false),
                    "cache_read": m(r.usage.cache_read, "tokens", "cache_read", false),
                    "cache_write": m(r.usage.cache_write(), "tokens", "cache_write", false),
                    "fresh_input": m(r.usage.input, "tokens", "fresh_input", false),
                    "output": m(r.usage.output, "tokens", "output", false),
                    "thinking": m(r.usage.thinking, "tokens", "thinking", false),
                    "cost": r.cost_usd.map(|c| m(c, "USD", "cost", true)),
                    "tools": r.tools,
                    "compaction": r.compaction,
                    "effort": r.effort,
                    "model": r.model,
                    "human": r.human,
                    "tool_calls": m(r.tool_calls, "count", "tool_calls", false),
                    "tool_errors": m(r.tool_errors, "count", "tool_errors", false),
                    "prompt_chars": r.prompt_chars,
                    "harness_tokens": m(r.harness_tokens, "tokens", "harness_tokens", true),
                    "miss_cause": r.miss_cause,
                    "phase_mix": r.phase_mix,
                    "cost_at_100k": r.cost_at_100k_usd.map(|c| m(c, "USD", "cost_per_turn", true)),
                    "api_errors": r.api_errors,
                    "steers": r.steers,
                    "interrupted": r.interrupted,
                })
            })
            .collect(),
    )
}

pub fn tools(state: &State) -> Value {
    let by = state.tools.by_name();
    let mut rows: Vec<Value> = by
        .values()
        .map(|t| {
            json!({
                "tool": t.name,
                "calls": m(t.calls, "count", "tool_calls", false),
                "errors": m(t.errors, "count", "tool_errors", false),
                "running": t.running,
                "p50": t.p50_ms.map(|v| m(v, "ms", "tool_p50", t.approx)),
                "p95": t.p95_ms.map(|v| m(v, "ms", "tool_p95", t.approx)),
                "last_call_at_ms": t.last_call_at,
                "tokens_to_ctx": m(t.tokens_to_ctx, "tokens", "tokens_to_ctx", true),
            })
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r["calls"]["value"].as_u64().unwrap_or(0)));
    let top: Vec<Value> = state
        .tools
        .top_ctx(5)
        .iter()
        .map(|c| json!({"tool": c.name, "input": c.input_summary, "turn": c.turn, "tokens": m(c.result_tokens_est, "tokens", "top_ctx", true), "truncated": c.truncated, "persisted_bytes": c.persisted_output_size, "reread_tax": state.reread_tax(c).map(|(n, usd)| json!({"rereads": n, "usd": m(usd, "USD", "reread_tax", true)}))}))
        .collect();
    let bash: Vec<Value> = state
        .tools
        .bash_by_class()
        .iter()
        .map(|(k, n, e)| json!({"class": k.label(), "calls": n, "errors": e}))
        .collect();
    let errors: Vec<Value> = state
        .tools
        .errors_by_class()
        .iter()
        .map(|(k, n)| json!({"class": k.label(), "count": n}))
        .collect();
    let input: Vec<Value> = state
        .tools
        .input_chars_by_name()
        .iter()
        .map(
            |(n, c)| json!({"tool": n, "tokens": m(*c as u64 / 4, "tokens", "input_tokens", true)}),
        )
        .collect();
    json!({"tools": rows, "top_ctx": top, "bash_by_class": bash, "errors_by_class": errors, "input_to_ctx": input, "tool_search_loads": state.tools.tool_search_loads})
}

pub fn files(state: &State) -> Value {
    let mut rows: Vec<&crate::files::FileStats> = state.files.files.values().collect();
    rows.sort_by_key(|f| std::cmp::Reverse(f.last_touch_ms));
    Value::Array(
        rows.iter()
            .map(|f| {
                json!({
                    "path": f.path,
                    "reads": f.reads,
                    "edits": f.edits,
                    "writes": f.writes,
                    "touches": m(f.touches(), "count", "file_touches", false),
                    "lines_added": f.lines_added.map(|v| m(v, "lines", "file_lines", false)).unwrap_or_else(|| missing("not a git repo or no diff")),
                    "lines_removed": f.lines_removed.map(|v| m(v, "lines", "file_lines", false)).unwrap_or_else(|| missing("not a git repo or no diff")),
                    "reread_warning": f.reread_warning(),
                })
            })
            .collect(),
    )
}

pub fn agents(state: &State) -> Value {
    let now = state.clock_ms();
    let agents: Vec<Value> = state
        .agents
        .values()
        .map(|a| {
            json!({
                "id": a.id,
                "type": a.agent_type,
                "description": a.description,
                "model": a.model,
                "state": format!("{:?}", a.state(now)).to_lowercase(),
                "elapsed": a.elapsed_ms(now).map(|e| m(e, "ms", "agent_state", false)),
                "tokens": m(a.usage.total(), "tokens", "agent_tokens", false),
                "workflow": a.workflow,
                "inherited_context": a.inherited_context_len,
                "depth": a.spawn_depth,
            })
        })
        .collect();
    let mcp: Vec<Value> = state
        .procs
        .mcp
        .iter()
        .map(|s| {
            json!({"name": s.name, "pid": s.pid, "rss": m(s.rss_bytes, "bytes", "mcp_rss", false), "restarts": s.restarts,
                   "calls": m(state.tools.by_name().get(&format!("mcp:{}", s.name)).map(|t| t.calls).unwrap_or(0), "count", "mcp_calls", false)})
        })
        .collect();
    let tasks: Vec<Value> = state
        .tasks
        .iter()
        .map(|t| json!({"id": t.id, "kind": t.kind, "description": t.description, "started_at_ms": t.started_at_ms, "status": t.status}))
        .collect();
    let workflows: Vec<Value> = state
        .workflow_journals
        .iter()
        .map(|j| json!({"run": j.run, "launched": j.launched, "done": j.results, "failed": j.failed}))
        .collect();
    let teammates: Vec<Value> = state
        .teammates
        .iter()
        .map(|t| json!({"name": t.name, "type": t.agent_type}))
        .collect();
    json!({
        "agents": agents,
        "depth": state.agent_depth(),
        "workflows": workflows,
        "teammates": teammates,
        "mcp": if state.session.pid.is_some() { Value::Array(mcp) } else { missing("no live process (fixture)") },
        "mcp_needs_auth": state.mcp_needs_auth,
        "mcp_failed": state.mcp_failed,
        "tool_search_loads": state.tools.tool_search_loads,
        "tasks": tasks,
    })
}

pub fn advice(state: &State) -> Value {
    let mut engine = advisor::Engine::default();
    engine.evaluate(state);
    Value::Array(
        engine
            .current
            .iter()
            .map(|x| json!({"rule": x.rule, "headline": x.headline, "evidence": x.evidence, "action": x.action, "saving": x.saving.label(), "doc_key": x.doc_key, "explain": advisor::rules::explain(x.doc_key)}))
            .collect(),
    )
}

pub fn prefix(state: &State) -> Value {
    let ctx = state.context();
    let rows: Vec<Value> = state
        .prefix
        .rows(ctx.prefix)
        .iter()
        .map(|r| json!({"kind": format!("{:?}", r.kind).to_lowercase(), "name": r.name, "bytes": r.bytes, "tokens": m(r.tokens_est, "tokens", "context_prefix", true), "count": r.count}))
        .collect();
    json!({"total": m(ctx.prefix, "tokens", "context_prefix", false), "rows": rows})
}

pub fn events(state: &State, since_ms: Option<i64>) -> Value {
    let cutoff = since_ms.map(|s| state.clock_ms() - s).unwrap_or(i64::MIN);
    Value::Array(
        state
            .events
            .iter()
            .filter(|e| e.at >= cutoff)
            .map(|e| json!({"at_ms": e.at, "kind": e.kind.label(), "text": e.text}))
            .collect(),
    )
}

pub fn baseline(b: Option<&crate::baseline::Baseline>) -> Value {
    match b {
        Some(b) => serde_json::to_value(b).unwrap_or(Value::Null),
        None => missing("no transcripts in the last 7 days"),
    }
}

pub fn explain(metric_id: &str) -> Value {
    match registry::get(metric_id) {
        Some(mt) => {
            json!({"id": mt.id, "panel": mt.panel, "name": mt.name, "unit": mt.unit, "formula": mt.formula, "sources": mt.sources, "caveats": mt.caveats, "estimate_when": mt.estimate_when})
        }
        None => {
            json!({"error": format!("unknown metric {metric_id}"), "known": registry::METRICS.iter().map(|m| m.id).collect::<Vec<_>>()})
        }
    }
}

/// `10m`, `2h`, `90s` → milliseconds.
pub fn parse_since(s: &str) -> Option<i64> {
    let (num, unit) = s.trim().split_at(s.trim().len().checked_sub(1)?);
    let n: i64 = num.parse().ok()?;
    Some(match unit {
        "s" => n * 1000,
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        "d" => n * 86_400_000,
        _ => return None,
    })
}

/// Human-friendly label for a saving/rate in text output.
pub fn tokens_label(v: u64) -> String {
    fmt::tokens(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::tests_support::fixture_state;

    fn state() -> State {
        let mut s = fixture_state();
        s.session = crate::ui::state::SessionInfo::from_fixture(std::path::Path::new(
            "fixtures/session-a.jsonl",
        ));
        s.session.ended_at_ms = s.last_line_at_ms;
        s.agents = crate::agents::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/session-a")
                .as_path(),
        );
        s
    }

    #[test]
    fn summary_and_ledger_snapshots() {
        let s = state();
        insta::assert_snapshot!(
            "query_summary",
            serde_json::to_string_pretty(&summary(&s)).unwrap()
        );
        insta::assert_snapshot!(
            "query_ledger_last3",
            serde_json::to_string_pretty(&ledger_json(&s, Some(3))).unwrap()
        );
    }

    /// Fixture B folded into a state, as `cctop query --session` would.
    fn state_b() -> State {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
        let mut s = State::new(crate::metrics::Pricing::bundled());
        s.session = crate::ui::state::SessionInfo::from_fixture(&path);
        for l in crate::transcript::parse_file(&path).unwrap() {
            s.apply(&l);
        }
        s.session.ended_at_ms = s.last_line_at_ms;
        s
    }

    #[test]
    fn fixture_b_summary_snapshot() {
        let s = state_b();
        let sum = summary(&s);
        assert_eq!(sum["turns"]["value"], 6);
        assert_eq!(sum["context"]["compactions"]["value"], 1);
        assert_eq!(sum["cache"]["source"], "transcript");
        insta::assert_snapshot!(
            "query_summary_b",
            serde_json::to_string_pretty(&sum).unwrap()
        );
        insta::assert_snapshot!(
            "query_events_b_tail",
            serde_json::to_string_pretty(&events(&s, Some(5 * 60_000))).unwrap()
        );
    }

    #[test]
    fn shapes_and_missing_sources() {
        let s = state();
        let sum = summary(&s);
        assert_eq!(sum["limits"]["source"], "missing");
        assert_eq!(sum["limits"]["hint"], "run cctop install");
        assert_eq!(sum["context"]["size"]["metric_id"], "context_size");
        assert_eq!(sum["context"]["size"]["approx"], true, "no shim → est");
        assert_eq!(sum["cost"]["approx"], false, "cost-state present");
        let t = tools(&s);
        assert_eq!(t["tools"][0]["tool"], "mcp:claude-in-chrome");
        assert_eq!(t["top_ctx"].as_array().unwrap().len(), 5);
        let f = files(&s);
        assert_eq!(f.as_array().unwrap().len(), 4);
        assert_eq!(f[0]["lines_added"]["source"], "missing");
        let a = agents(&s);
        assert_eq!(a["agents"][0]["type"], "fork");
        assert_eq!(a["mcp"]["source"], "missing");
        let adv = advice(&s);
        assert!(adv
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["explain"].is_string()));
        let p = prefix(&s);
        assert_eq!(p["total"]["value"], 60_582);
        // The fixture's last prompt came 9 min after its last event.
        let ev = events(&s, Some(15 * 60_000));
        assert!(!ev.as_array().unwrap().is_empty());
        // Only events on the clock's last millisecond (the interrupt note
        // and the cost-state written with it) are "since 1 ms".
        let last = s.clock_ms() - 1;
        assert!(events(&s, Some(1))
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["at_ms"].as_i64().unwrap() >= last));
        assert_eq!(events(&s, None).as_array().unwrap().len(), s.events.len());
        assert_eq!(explain("cache_hit_ratio")["panel"], "Tokens & Cost");
        assert!(explain("nope")["error"].is_string());
        assert_eq!(parse_since("10m"), Some(600_000));
        assert_eq!(parse_since("2h"), Some(7_200_000));
        assert_eq!(parse_since("x"), None);
    }
}
