//! End-of-session report and raw export.

use std::path::{Path, PathBuf};

use crate::baseline::Baseline;
use crate::ledger;
use crate::ui::fmt;
use crate::ui::State;

/// One-page Markdown summary of a session.
pub fn markdown(state: &State, baseline: Option<&Baseline>) -> String {
    let mut out = String::new();
    let s = &state.session;
    let ctx = state.context();
    let u = state.agg.total;
    let turns = state.agg.human_turns().max(1) as f64;
    out.push_str(&format!(
        "# cctop report — {}\n\n",
        if s.name.is_empty() {
            "session"
        } else {
            &s.name
        }
    ));
    out.push_str(&format!(
        "- session `{}` · {} · Claude Code {}\n- cwd `{}`\n- turns {} · API calls {} · tool calls {}\n",
        s.session_id,
        state.model().unwrap_or("—"),
        if s.version.is_empty() { "?" } else { &s.version },
        s.cwd.display(),
        state.agg.human_turns(),
        state.agg.api_calls(),
        state.tools.calls.len()
    ));
    if let (Some(start), Some(end)) = (
        state
            .agg
            .turns
            .first()
            .and_then(|t| t.started_at.as_deref())
            .and_then(crate::metrics::cost::parse_ts_ms),
        state.last_line_at_ms,
    ) {
        out.push_str(&format!("- duration {}\n", fmt::duration_ms(end - start)));
    }

    out.push_str("\n## Cost\n\n");
    // The session's whole spend: the ledger, the main responses after it
    // and the agents' calls after it (agent PRD §4.2).
    match state.cost.combined(state.agents.values()) {
        Some(c) => out.push_str(&format!(
            "- total {}{}\n",
            if c.approx { "≈ " } else { "" },
            fmt::usd(c.usd)
        )),
        None => out.push_str("- total — (no priced model)\n"),
    }
    if let Some((usd, share)) = state.agents_cost() {
        out.push_str(&format!(
            "- agents ≈{} ({:.0} % of the total; a fork's replayed parent message and the ledger's own copy of the agents' earlier calls are not counted twice)\n",
            fmt::usd(usd),
            share * 100.0
        ));
    }
    let by = state.cost.by_model();
    if !by.is_empty() {
        let total: f64 = by.values().sum();
        out.push_str("- by model:");
        for (m, c) in &by {
            out.push_str(&format!(
                " {} {} ({:.0} %)",
                m.trim_start_matches("claude-"),
                fmt::usd(*c),
                if total > 0.0 { c / total * 100.0 } else { 0.0 }
            ));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "- tokens: cache read {} · cache write {} · fresh {} · output {} (thinking {})\n- cache hit ratio {}\n",
        fmt::tokens(u.cache_read),
        fmt::tokens(u.cache_write()),
        fmt::tokens(u.input),
        fmt::tokens(u.output),
        fmt::tokens(u.thinking),
        u.cache_hit_ratio().map(|r| format!("{:.0} %", r * 100.0)).unwrap_or_else(|| "—".into())
    ));

    out.push_str("\n## Context\n\n");
    out.push_str(&format!(
        "- final size {} of {} ({:.0} %) · prefix {} · compactions {}\n",
        fmt::tokens(ctx.size),
        fmt::tokens(ctx.window),
        ctx.ratio() * 100.0,
        fmt::tokens(ctx.prefix),
        ctx.compactions.len()
    ));

    out.push_str("\n## Top context consumers\n\n");
    for c in state.tools.top_ctx(5) {
        out.push_str(&format!(
            "- {} `{}` — {} tokens (turn {})\n",
            c.name,
            c.input_summary,
            fmt::tokens(c.result_tokens_est),
            c.turn
        ));
    }

    out.push_str("\n## Most expensive turns\n\n");
    let mut rows = ledger::rows(state);
    rows.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for r in rows.iter().take(5) {
        out.push_str(&format!(
            "- turn {} — {} · {} API calls · {} tokens · {}\n",
            r.turn,
            r.cost_usd.map(fmt::usd).unwrap_or_else(|| "—".into()),
            r.api_calls,
            fmt::tokens(r.usage.total()),
            if r.tools.is_empty() {
                "no tools".to_string()
            } else {
                r.tools.clone()
            }
        ));
    }

    out.push_str("\n## Advisor\n\n");
    let engine = crate::advisor::Engine::for_state(state);
    let (fired, acted, snoozed) = engine.tally();
    out.push_str(&format!(
        "- nudges: {fired} fired, {acted} acted, {snoozed} snoozed · session mode {}\n",
        engine.session_mode.label()
    ));
    if engine.current.is_empty() {
        out.push_str("- nothing to fix — the session looked efficient\n");
    }
    for a in &engine.current {
        out.push_str(&format!(
            "- **{} {}** {} — {} _(saves {})_\n",
            a.urgency.label(),
            a.rule,
            a.headline,
            a.action,
            a.saving.label()
        ));
    }

    out.push_str("\n## Versus your last 7 days\n\n");
    match baseline.filter(|b| b.sessions > 0) {
        Some(b) => {
            let this_cost = state.cost.current().map(|c| c.usd / turns);
            let this_tokens = Some(u.total() as f64 / turns);
            let line = |label: &str,
                        this: Option<f64>,
                        med: Option<f64>,
                        f: &dyn Fn(f64) -> String| {
                match (this, med) {
                    (Some(t), Some(m)) if m > 0.0 => {
                        format!("- {label}: {} vs median {} (×{:.1})\n", f(t), f(m), t / m)
                    }
                    _ => format!("- {label}: —\n"),
                }
            };
            out.push_str(&line(
                "cost per turn",
                this_cost,
                b.cost_per_turn,
                &fmt::usd,
            ));
            out.push_str(&line(
                "tokens per turn",
                this_tokens,
                b.tokens_per_turn,
                &|v| fmt::tokens(v as u64),
            ));
            out.push_str(&line(
                "cache hit ratio",
                u.cache_hit_ratio(),
                b.cache_hit_ratio,
                &|v| format!("{:.0} %", v * 100.0),
            ));
            out.push_str(&format!("- baseline from {} sessions\n", b.sessions));
        }
        None => out.push_str("- no baseline yet\n"),
    }
    out
}

/// `<home>/reports/<YYYY-MM-DD>-<name>.md`.
pub fn report_path(home: &Path, state: &State) -> PathBuf {
    let day = state.clock_ms().div_euclid(86_400_000);
    // civil from days (Howard Hinnant)
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let name = if state.session.name.is_empty() {
        "session"
    } else {
        &state.session.name
    };
    home.join("reports")
        .join(format!("{y:04}-{m:02}-{d:02}-{name}.md"))
}

/// Ledger rows and events as JSON.
pub fn export_json(state: &State) -> serde_json::Value {
    serde_json::json!({
        "ledger": crate::query::ledger_json(state, None),
        "events": crate::query::events(state, None),
    })
}

/// Ledger rows as CSV (events with `events = true`).
pub fn export_csv(state: &State, events: bool) -> String {
    let mut out = String::new();
    if events {
        out.push_str("at_ms,kind,text\n");
        for e in state.events.iter() {
            out.push_str(&format!(
                "{},{},\"{}\"\n",
                e.at,
                e.kind.label(),
                e.text.replace('"', "\"\"")
            ));
        }
        return out;
    }
    out.push_str("turn,started_at_ms,duration_ms,api_calls,cache_read,cache_write,fresh_input,output,thinking,cost_usd,tools,compaction,effort,model\n");
    for r in ledger::rows(state) {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},\"{}\",{},{},{}\n",
            r.turn,
            r.started_at_ms.map(|v| v.to_string()).unwrap_or_default(),
            r.duration_ms.map(|v| v.to_string()).unwrap_or_default(),
            r.api_calls,
            r.usage.cache_read,
            r.usage.cache_write(),
            r.usage.input,
            r.usage.output,
            r.usage.thinking,
            r.cost_usd.map(|v| format!("{v:.4}")).unwrap_or_default(),
            r.tools,
            r.compaction,
            r.effort,
            r.model
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::tests_support::fixture_state;

    fn state() -> State {
        let mut s = fixture_state();
        s.session =
            crate::ui::state::SessionInfo::from_fixture(Path::new("fixtures/session-a.jsonl"));
        s.session.ended_at_ms = s.last_line_at_ms;
        s
    }

    #[test]
    fn report_has_every_section_and_baseline_multipliers() {
        let s = state();
        let b = Baseline {
            sessions: 4,
            cost_per_turn: Some(0.33),
            tokens_per_turn: Some(1_000_000.0),
            cache_hit_ratio: Some(0.9),
            ..Default::default()
        };
        let md = markdown(&s, Some(&b));
        for h in [
            "# cctop report — session-a",
            "## Cost",
            "## Context",
            "## Top context consumers",
            "## Most expensive turns",
            "## Advisor",
            "## Versus your last 7 days",
        ] {
            assert!(md.contains(h), "missing {h}\n{md}");
        }
        assert!(md.contains("total $9.90"), "{md}");
        assert!(
            md.contains("cost per turn: $0.71 vs median $0.33 (×2.1)"),
            "{md}"
        );
        assert!(
            md.contains("- nudges: 1 fired, 0 acted, 0 snoozed · session mode interactive"),
            "{md}"
        );
        assert!(md.contains("- **LATER A03**"), "{md}");
        let none = markdown(&s, None);
        assert!(none.contains("no baseline yet"));
        let p = report_path(Path::new("/h"), &s);
        assert_eq!(p, PathBuf::from("/h/reports/2026-08-27-session-a.md"));
    }

    #[test]
    fn export_shapes() {
        let s = state();
        let j = export_json(&s);
        assert_eq!(j["ledger"].as_array().unwrap().len(), 14);
        assert!(!j["events"].as_array().unwrap().is_empty());
        let csv = export_csv(&s, false);
        assert_eq!(csv.lines().count(), 15, "header + 14 turns");
        assert!(csv.starts_with("turn,started_at_ms"));
        let ev = export_csv(&s, true);
        assert!(ev.starts_with("at_ms,kind,text"));
        assert_eq!(ev.lines().count(), s.events.len() + 1);
    }
}
