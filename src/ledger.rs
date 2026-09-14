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

/// Build the rows from the state, in turn order.
pub fn rows(state: &State) -> Vec<Row> {
    let compacted: Vec<usize> = state.context().compactions.iter().map(|c| c.turn).collect();
    state
        .agg
        .turns
        .iter()
        .map(|t| {
            let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
            for c in state.tools.calls.iter().filter(|c| c.turn == t.number) {
                *counts.entry(c.name.as_str()).or_default() += 1;
            }
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
}
