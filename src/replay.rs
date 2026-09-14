//! `cctop coach-replay <transcript.jsonl>…`: run the Advisor engine over
//! whole transcripts, line by line as a live session would have seen them,
//! and count what the coach would have said — fires per rule, sessions
//! touched, the acted-within-TTL share — so a rule's rate can be read
//! before it ships to the slot. Nothing but counts and rule ids leaves this
//! module: no prompt text, no paths of the transcripts' files.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::advisor::Engine;
use crate::metrics::Pricing;
use crate::transcript::parse_file;
use crate::ui::state::{SessionInfo, State};

/// One rule's tally over a replay.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RuleTally {
    pub fires: usize,
    /// Distinct sessions in which it fired.
    pub sessions: usize,
    pub acted: usize,
    pub expired: usize,
    pub snoozed: usize,
    /// Retired because its predicate went false (the light cleared).
    pub retired: usize,
    /// Sessions in which the rule sat in the `next` row without ever
    /// taking the slot.
    pub next_row_only: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Replay {
    pub sessions: usize,
    pub human_turns: usize,
    pub by_rule: BTreeMap<String, RuleTally>,
    /// Sessions whose mode was never interactive (loops, machine turns).
    pub silent_sessions: usize,
}

/// Replay one transcript: the engine is evaluated at every user line (a
/// prompt, a tool result), every turn end and at least every 20 lines —
/// the moments a live session would have redrawn on — the clock following
/// the transcript's own timestamps.
pub fn replay_one(path: &Path, tally: &mut Replay) {
    let Ok(lines) = parse_file(path) else {
        return;
    };
    let mut state = State::new(Pricing::bundled());
    state.session = SessionInfo::from_fixture(path);
    state.clock_override = true;
    let mut engine = Engine::default();
    let mut fired_here: BTreeMap<String, usize> = BTreeMap::new();
    let mut next_row: BTreeMap<String, usize> = BTreeMap::new();
    let mut interactive = false;
    let mut since_eval = 0usize;
    for line in &lines {
        state.apply(line);
        if let Some(at) = state.last_line_at_ms {
            state.now_ms = at;
        }
        since_eval += 1;
        let moment = matches!(
            line,
            crate::transcript::Line::User(_) | crate::transcript::Line::System(_)
        );
        if !moment && since_eval < 20 {
            continue;
        }
        since_eval = 0;
        engine.evaluate(&state);
        if engine.session_mode == crate::advisor::SessionMode::Interactive {
            interactive = true;
        }
        for a in engine.current.iter().filter(|a| a.next_row_only) {
            *next_row.entry(a.rule.to_string()).or_default() += 1;
        }
    }
    for r in &engine.records {
        *fired_here.entry(r.rule.clone()).or_default() += 1;
        let t = tally.by_rule.entry(r.rule.clone()).or_default();
        t.fires += 1;
        match r.retired.as_ref().map(|(w, _)| w.as_str()) {
            Some("acted") => t.acted += 1,
            Some("expired") => t.expired += 1,
            Some("snoozed") => t.snoozed += 1,
            Some("retired") => t.retired += 1,
            _ => {}
        }
    }
    for (rule, _) in fired_here {
        tally.by_rule.entry(rule).or_default().sessions += 1;
    }
    for (rule, _) in next_row {
        tally.by_rule.entry(rule).or_default().next_row_only += 1;
    }
    tally.sessions += 1;
    tally.human_turns += state.agg.human_turns();
    if !interactive {
        tally.silent_sessions += 1;
    }
}

/// Replay every transcript; unreadable files are skipped.
pub fn replay(paths: &[std::path::PathBuf]) -> Replay {
    let mut out = Replay::default();
    for p in paths {
        replay_one(p, &mut out);
    }
    out
}

/// The table as text: one row per rule.
pub fn table(r: &Replay) -> String {
    let mut s = format!(
        "{} sessions · {} human turns · {} never interactive\n{:<6} {:>6} {:>8} {:>6} {:>7} {:>7} {:>7} {:>9}\n",
        r.sessions, r.human_turns, r.silent_sessions, "rule", "fires", "sessions", "acted", "expired", "snoozed", "cleared", "next-row"
    );
    for (rule, t) in &r.by_rule {
        s.push_str(&format!(
            "{:<6} {:>6} {:>8} {:>6} {:>7} {:>7} {:>7} {:>9}\n",
            rule, t.fires, t.sessions, t.acted, t.expired, t.snoozed, t.retired, t.next_row_only
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_both_fixtures_and_counts_fires_per_rule() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let r = replay(&[
            root.join("fixtures/session-a.jsonl"),
            root.join("fixtures/session-b.jsonl"),
        ]);
        assert_eq!(r.sessions, 2);
        assert_eq!(r.human_turns, 14 + 6);
        assert!(r.by_rule.contains_key("A17"), "{:?}", r.by_rule.keys());
        let a17 = &r.by_rule["A17"];
        assert!(a17.fires >= 1 && a17.sessions >= 1);
        let text = table(&r);
        assert!(text.starts_with("2 sessions · 20 human turns"), "{text}");
        assert!(text.contains("A17"));
        // A missing file is skipped, never a panic.
        let r2 = replay(&[root.join("fixtures/nope.jsonl")]);
        assert_eq!(r2.sessions, 0);
    }
}
