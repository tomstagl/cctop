//! The turn ledger: one row per turn with everything that turn cost.
//! The panels are summaries of this table.

use crate::metrics::Usage;
use crate::tools::Call;
use crate::ui::State;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub turn: usize,
    pub started_at_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub api_calls: usize,
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    /// `Read×3 Bash×1`, most used first.
    pub tools: String,
    pub compaction: bool,
    pub effort: String,
    pub model: String,
    // -- the coach PRD's columns (§4)
    pub tool_calls: usize,
    pub tool_errors: usize,
    pub prompt_chars: usize,
    /// Harness (attachment) tokens injected during the turn.
    pub harness_tokens: u64,
    /// The named cache-miss cause of a call in this turn, if any.
    pub miss_cause: Option<String>,
    /// `explore 6 · implement 3 · verify 1`: the turn's phase mix.
    pub phase_mix: String,
    pub human: bool,
    /// What the turn would have cost at 100 k of context.
    pub cost_at_100k_usd: Option<f64>,
    pub api_errors: usize,
    pub steers: usize,
    pub interrupted: bool,
    /// Thinking blocks Claude Code dropped from this turn's requests.
    pub thinking_dropped: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Turn,
    Duration,
    Calls,
    CacheRead,
    CacheWrite,
    Fresh,
    Output,
    Cost,
}

impl Sort {
    pub fn next(self) -> Sort {
        use Sort::*;
        match self {
            Turn => Duration,
            Duration => Calls,
            Calls => CacheRead,
            CacheRead => CacheWrite,
            CacheWrite => Fresh,
            Fresh => Output,
            Output => Cost,
            Cost => Turn,
        }
    }
    pub fn label(self) -> &'static str {
        use Sort::*;
        match self {
            Turn => "#",
            Duration => "DUR",
            Calls => "API",
            CacheRead => "READ",
            CacheWrite => "WRITE",
            Fresh => "FRESH",
            Output => "OUT",
            Cost => "COST",
        }
    }
}

/// Does the API's own reason name the same thing as the located cause?
fn reason_agrees(api: &str, kind: &str) -> bool {
    match api {
        "tools_changed" => kind == "tools_changed",
        "model_changed" => kind == "params_changed",
        "system_changed" => kind == "system_changed",
        "messages_changed" | "previous_message_not_found" => {
            matches!(kind, "prefix_rewritten" | "expired" | "thinking_drop")
        }
        r if r.starts_with("ttl_expired") => kind == "expired",
        _ => false,
    }
}

/// The compact located cause; when the API named a different reason, that
/// reason follows in parentheses (`unavailable` names nothing).
fn miss_label(m: &crate::requests::LocatedMiss) -> String {
    match m.api_reason.as_deref() {
        Some(r) if r != "unavailable" && !reason_agrees(r, m.cause.kind()) => {
            format!("{} (API: {r})", m.cause.compact())
        }
        _ => m.cause.compact(),
    }
}

/// Build the rows from the state, in turn order.
pub fn rows(state: &State) -> Vec<Row> {
    let compacted: Vec<usize> = state.context().compactions.iter().map(|c| c.turn).collect();
    // The first named miss of each turn.
    let mut misses: std::collections::HashMap<usize, String> = Default::default();
    for (turn, cause) in &state.agg.miss_causes_by_turn {
        misses.entry(*turn).or_insert_with(|| cause.clone());
    }
    // A located cause (api-request lines, 2.1.302+) names what changed;
    // the first one of a turn replaces the API's own label.
    let mut located: std::collections::HashSet<usize> = Default::default();
    for m in state.requests.misses() {
        let Some(turn) = m.turn else { continue };
        if m.cause.kind() != "unlocated" && located.insert(turn) {
            misses.insert(turn, miss_label(m));
        }
    }
    let price = state
        .model()
        .and_then(|m| state.cost.pricing().price(m).copied());
    state
        .agg
        .turns
        .iter()
        .map(|t| {
            let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
            let mut phases: std::collections::BTreeMap<&'static str, usize> = Default::default();
            for c in state.tools.calls.iter().filter(|c| c.turn == t.number) {
                *counts.entry(c.name.as_str()).or_default() += 1;
                let word = match c.class {
                    crate::phase::ToolClass::Explore | crate::phase::ToolClass::GitRead => {
                        "explore"
                    }
                    crate::phase::ToolClass::Implement => "implement",
                    crate::phase::ToolClass::Test | crate::phase::ToolClass::BuildLint => "verify",
                    crate::phase::ToolClass::Commit => "commit",
                    _ => "other",
                };
                *phases.entry(word).or_default() += 1;
            }
            let mut phase_v: Vec<(&str, usize)> = phases.into_iter().collect();
            phase_v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let phase_mix = phase_v
                .iter()
                .map(|(p, n)| format!("{p} {n}"))
                .collect::<Vec<_>>()
                .join(" · ");
            // The same turn priced as if every call read 100 k of context.
            let cost_at_100k_usd = price.map(|p| {
                let calls = t.api_calls as f64;
                (calls * 100_000.0 * p.cache_read() + t.usage.output as f64 * p.output) / 1e6
            });
            let mut v: Vec<(&str, usize)> = counts.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let tools = v
                .iter()
                .map(|(n, c)| format!("{n}×{c}"))
                .collect::<Vec<_>>()
                .join(" ");
            let cost_usd = t
                .models
                .first()
                .and_then(|m| state.cost.pricing().estimate(&t.usage, m));
            Row {
                turn: t.number,
                started_at_ms: t
                    .started_at
                    .as_deref()
                    .and_then(crate::metrics::cost::parse_ts_ms),
                duration_ms: t.elapsed_ms(state.clock_ms()),
                api_calls: t.api_calls,
                usage: t.usage,
                cost_usd,
                tools,
                compaction: compacted.contains(&t.number),
                effort: t.effort.clone().unwrap_or_default(),
                model: t.models.last().cloned().unwrap_or_default(),
                tool_calls: t.tool_calls,
                tool_errors: t.tool_errors,
                prompt_chars: t.prompt_chars,
                harness_tokens: t.harness_tokens,
                miss_cause: misses.get(&t.number).cloned(),
                phase_mix,
                human: t.human,
                cost_at_100k_usd,
                api_errors: t.api_errors,
                steers: t.steers,
                interrupted: t.interrupted_after_calls.is_some(),
                thinking_dropped: t.thinking_dropped_blocks,
            }
        })
        .collect()
}

pub fn sorted(state: &State, sort: Sort, ascending: bool) -> Vec<Row> {
    let mut v = rows(state);
    v.sort_by(|a, b| {
        let ord = match sort {
            Sort::Turn => a.turn.cmp(&b.turn),
            Sort::Duration => a.duration_ms.cmp(&b.duration_ms),
            Sort::Calls => a.api_calls.cmp(&b.api_calls),
            Sort::CacheRead => a.usage.cache_read.cmp(&b.usage.cache_read),
            Sort::CacheWrite => a.usage.cache_write().cmp(&b.usage.cache_write()),
            Sort::Fresh => a.usage.input.cmp(&b.usage.input),
            Sort::Output => a.usage.output.cmp(&b.usage.output),
            Sort::Cost => a
                .cost_usd
                .partial_cmp(&b.cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal),
        };
        if ascending {
            ord
        } else {
            ord.reverse()
        }
    });
    v
}

/// The tool calls of one turn.
pub fn calls_of(state: &State, turn: usize) -> Vec<&Call> {
    state
        .tools
        .calls
        .iter()
        .filter(|c| c.turn == turn)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use std::path::Path;

    fn state() -> State {
        let mut s = State::new(Pricing::bundled());
        for l in parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl"))
            .unwrap()
        {
            s.apply(&l);
        }
        s
    }

    #[test]
    fn miss_turn_carries_the_located_short_form() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-r.jsonl");
        let info = crate::ui::state::SessionInfo::from_fixture(&path);
        let s = crate::load::state_from(&path, info);
        let r = rows(&s);
        let first = &s.requests.misses()[0];
        let row = r.iter().find(|x| Some(x.turn) == first.turn).unwrap();
        assert_eq!(
            row.miss_cause.as_deref(),
            Some(first.cause.compact().as_str())
        );
        assert!(row.miss_cause.as_deref().unwrap().starts_with('+'));
    }

    fn located(
        cause: crate::requests::LocatedCause,
        api: Option<&str>,
    ) -> crate::requests::LocatedMiss {
        crate::requests::LocatedMiss {
            message_id: "m".into(),
            request_id: None,
            turn: Some(1),
            at: None,
            tokens: 30_000,
            cause,
            api_reason: api.map(String::from),
        }
    }

    #[test]
    fn miss_label_adds_the_api_reason_only_when_it_disagrees() {
        use crate::requests::LocatedCause::*;
        let tools = || ToolsChanged {
            added: vec!["x".into()],
            removed: vec![],
            resized: vec![],
        };
        let expired = || Expired {
            gap_ms: 400_000,
            ttl_ms: 300_000,
        };
        // Agree: the compact cause alone.
        assert_eq!(miss_label(&located(tools(), Some("tools_changed"))), "+x");
        assert_eq!(
            miss_label(&located(expired(), Some("ttl_expired_5m"))),
            "cache expired"
        );
        assert_eq!(miss_label(&located(tools(), None)), "+x");
        assert_eq!(miss_label(&located(tools(), Some("unavailable"))), "+x");
        // Disagree: the API's reason follows.
        assert_eq!(
            miss_label(&located(expired(), Some("tools_changed"))),
            "cache expired (API: tools_changed)"
        );
        assert_eq!(
            miss_label(&located(tools(), Some("messages_changed"))),
            "+x (API: messages_changed)"
        );
    }

    #[test]
    fn fixture_rows() {
        let s = state();
        let r = rows(&s);
        assert_eq!(r.len(), 14);
        assert_eq!(r.iter().map(|x| x.api_calls).sum::<usize>(), 143);
        let t3 = &r[2];
        assert_eq!(t3.turn, 3);
        assert_eq!(t3.duration_ms, Some(95_830));
        assert_eq!(t3.api_calls, 8);
        assert!(t3.tools.contains("×"), "{}", t3.tools);
        assert!(t3.cost_usd.unwrap() > 0.0);
        assert_eq!(t3.effort, "medium");
        assert_eq!(t3.model, "claude-sonnet-5");
        assert!(!r.iter().any(|x| x.compaction));
        let by_cost = sorted(&s, Sort::Cost, false);
        assert!(by_cost[0].cost_usd >= by_cost[1].cost_usd);
        let by_turn_asc = sorted(&s, Sort::Turn, true);
        assert_eq!(by_turn_asc[0].turn, 1);
        assert_eq!(calls_of(&s, 3).len(), 7);
    }

    #[test]
    fn thinking_drop_lands_on_its_turn_only() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-t.jsonl");
        let info = crate::ui::state::SessionInfo::from_fixture(&path);
        let s = crate::load::state_from(&path, info);
        let dropped: Vec<(usize, u64)> = rows(&s)
            .iter()
            .filter(|r| r.thinking_dropped > 0)
            .map(|r| (r.turn, r.thinking_dropped))
            .collect();
        assert_eq!(dropped.len(), 1, "{dropped:?}");
        assert_eq!(dropped[0].1, 10);
        assert_eq!(dropped[0].0, s.agg.turns.last().unwrap().number);
        let j = crate::query::ledger_json(&s, None);
        let last = j.as_array().unwrap().last().unwrap();
        assert_eq!(last["thinking_dropped"]["value"], 10);
        assert_eq!(last["thinking_dropped"]["metric_id"], "thinking_dropped");
        assert!(j[0]["thinking_dropped"].is_null());
    }
}
