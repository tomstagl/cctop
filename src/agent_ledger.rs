//! The agents ledger: one row per subagent with what it cost, what came
//! back and what of that was wasted. The agents view, `cctop query agents`,
//! dashboard row 6 and the coach's A48 all read these rows, so they cannot
//! disagree (`tasks/prd-cctop-agent-costs.md` §4.3 – §4.4).
//!
//! Waste is judged from structural evidence only — the task notification's
//! status and result length, the workflow journal, the agent's state, the
//! hook spool's pending calls, the usage — never from a prompt, a
//! description or a result's text.

use crate::agents::{Agent, State as AgentState, IDLE_MS};
use crate::metrics::cost::{Cost, Source};
use crate::transcript::TaskStatus;
use crate::ui::State;

/// Why an agent's money is counted as wasted. An agent has at most one
/// reason, tested in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WasteReason {
    /// The notification said `failed`, the workflow journal has a `failed`
    /// entry for it, or (no notification yet) the 60 s heuristic.
    Failed,
    /// The notification said `killed` (a `TaskStop`, or the session ended
    /// under it).
    Killed,
    /// `completed` with an absent or empty `<result>`; or a synchronous
    /// result with no content.
    #[serde(rename = "no_ret")]
    NoReturn,
    /// No notification, still running, no line for `IDLE_MS`, and no tool
    /// the hook spool saw start and not finish.
    Idle,
}

impl WasteReason {
    /// The word beside the dollars.
    pub fn label(self) -> &'static str {
        match self {
            WasteReason::Failed => "failed",
            WasteReason::Killed => "killed",
            WasteReason::NoReturn => "no ret",
            WasteReason::Idle => "idle",
        }
    }

    pub const ALL: [WasteReason; 4] = [
        WasteReason::Failed,
        WasteReason::Killed,
        WasteReason::NoReturn,
        WasteReason::Idle,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waste {
    /// The agent's priced cost (so far, for `Idle`); 0 on an unpriced model.
    pub usd: f64,
    pub reason: WasteReason,
    /// How long the agent has been idle, for the `idle <age>` label.
    pub idle_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentRow {
    pub id: String,
    pub agent_type: String,
    pub description: String,
    pub model: String,
    /// Claude Code's word when a notification arrived (`failed` and
    /// `killed` both read as failed), else the transcript heuristic.
    pub state: AgentState,
    pub status: Option<TaskStatus>,
    pub is_fork: bool,
    pub workflow: Option<String>,
    pub started_at: Option<i64>,
    pub elapsed_ms: Option<i64>,
    pub tokens: u64,
    pub output_tokens: u64,
    /// `Priced`, or `Unpriced` (usd 0) on a model the table does not know.
    pub cost: Cost,
    /// `<result>` length ÷ 4 (a synchronous result's content ÷ 4);
    /// `None` until a result exists, `Some(0)` for an empty one.
    pub returned_tokens: Option<u64>,
    pub waste: Option<Waste>,
    /// Not a fork, and its first call wrote more cache than it read.
    pub cold_start: bool,
    /// The cache-write dollars of that first call.
    pub cold_start_usd: f64,
    /// A fork's inherited context: its first own call's cache read.
    pub inherited_context: Option<u64>,
    pub launched_turn: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Spend,
    Waste,
    Elapsed,
    Started,
}

impl Sort {
    pub fn next(self) -> Sort {
        use Sort::*;
        match self {
            Spend => Waste,
            Waste => Elapsed,
            Elapsed => Started,
            Started => Spend,
        }
    }

    pub fn label(self) -> &'static str {
        use Sort::*;
        match self {
            Spend => "spend",
            Waste => "waste",
            Elapsed => "time",
            Started => "started",
        }
    }
}

/// A workflow run folded into one row: the journal's counts, the
/// notification's empty results when one arrived, and the subtotal of its
/// agents' rows.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowGroup {
    pub run: String,
    pub launched: usize,
    pub done: usize,
    pub failed: usize,
    /// `<agents_empty_result>` of the run's task notification.
    pub empty_result: Option<usize>,
    /// Agents of the run with a transcript.
    pub agents: usize,
    pub running: usize,
    pub cost: Cost,
    pub waste_usd: f64,
}

/// One group per workflow run seen in `rows` or in the journals, in run
/// order.
pub fn workflow_groups(state: &State, rows: &[AgentRow]) -> Vec<WorkflowGroup> {
    let mut runs: Vec<String> = state
        .workflow_journals
        .iter()
        .map(|j| j.run.clone())
        .collect();
    for r in rows {
        if let Some(run) = &r.workflow {
            if !runs.contains(run) {
                runs.push(run.clone());
            }
        }
    }
    runs.sort();
    // A notification whose `Workflow` launch was not seen (a cut prefix, a
    // resumed session) is keyed by its task id; it can only be the run's
    // when there is exactly one of each.
    let unkeyed: Vec<&crate::transcript::TaskNotification> = state
        .workflow_notifications
        .iter()
        .filter(|(k, _)| !runs.contains(k))
        .map(|(_, n)| n)
        .collect();
    let lone = (runs.len() == 1 && unkeyed.len() == 1).then(|| unkeyed[0]);
    runs.into_iter()
        .map(|run| {
            let journal = state.workflow_journals.iter().find(|j| j.run == run);
            let members: Vec<&AgentRow> = rows
                .iter()
                .filter(|r| r.workflow.as_deref() == Some(&run))
                .collect();
            let mut cost_usd = 0.0;
            let mut any_priced = false;
            let mut waste_usd = 0.0;
            for r in &members {
                if r.cost.source != Source::Unpriced {
                    cost_usd += r.cost.usd;
                    any_priced = true;
                }
                waste_usd += r.waste.map_or(0.0, |w| w.usd);
            }
            WorkflowGroup {
                empty_result: state
                    .workflow_notifications
                    .get(&run)
                    .or(lone)
                    .and_then(|n| n.workflow)
                    .map(|w| w.empty_result),
                launched: journal.map_or(members.len(), |j| j.launched),
                done: journal.map_or(0, |j| j.results),
                failed: journal.map_or(0, |j| j.failed),
                agents: members.len(),
                running: members
                    .iter()
                    .filter(|r| r.state == AgentState::Running)
                    .count(),
                cost: if any_priced {
                    Cost::priced(cost_usd)
                } else {
                    Cost {
                        usd: 0.0,
                        approx: true,
                        source: Source::Unpriced,
                    }
                },
                waste_usd,
                run,
            }
        })
        .collect()
}

/// The footer's figures over every row.
#[derive(Debug, Clone, PartialEq)]
pub struct Totals {
    pub agents: usize,
    pub running: usize,
    /// Σ priced cost; `Unpriced` when no agent could be priced.
    pub cost: Cost,
    pub waste_usd: f64,
    /// In `WasteReason::ALL` order.
    pub waste_by_reason: [f64; 4],
    /// Agents with a waste reason.
    pub classified: usize,
    pub cold_starts: usize,
    pub cold_start_usd: f64,
    /// Σ returned tokens ÷ Σ agent output tokens; `None` without output.
    pub returned_ratio: Option<f64>,
}

/// One agent's row.
pub fn row(state: &State, a: &Agent) -> AgentRow {
    let now = state.clock_ms();
    let pricing = state.cost.pricing();
    let cost = match pricing.estimate(&a.usage, &a.model) {
        Some(usd) => Cost::priced(usd),
        None => Cost {
            usd: 0.0,
            approx: true,
            source: Source::Unpriced,
        },
    };
    let status = a.notified.as_ref().map(|n| n.status.clone());
    // The journal's `failed` entry counts while Claude Code has not said
    // otherwise: a notification that says `completed` is the later word.
    let journal_failed = status.is_none()
        && state
            .workflow_journals
            .iter()
            .any(|j| j.failed_ids.contains(&a.id));
    let heuristic = a.state(now);
    let agent_state = match &status {
        Some(TaskStatus::Completed) => AgentState::Done,
        Some(TaskStatus::Failed) | Some(TaskStatus::Killed) => AgentState::Failed,
        _ if journal_failed => AgentState::Failed,
        _ => heuristic,
    };
    let result_chars = a
        .notified
        .as_ref()
        .and_then(|n| n.result_chars)
        .or(a.sync_result_chars);
    let returned_tokens = result_chars.map(|c| (c / 4) as u64);
    // A finished agent's time is what the notification measured, else the
    // span of its transcript; only a running one is timed to the clock.
    let elapsed_ms = match &a.notified {
        Some(n) => n.duration_ms.map(|d| d as i64).or_else(|| {
            a.started_at
                .zip(a.last_line_at)
                .map(|(s, l)| (l - s).max(0))
        }),
        None => a.elapsed_ms(now),
    };
    let idle_for = a
        .last_line_at
        .map(|t| now.saturating_sub(t))
        .filter(|age| *age > IDLE_MS);
    let reason = if matches!(status, Some(TaskStatus::Failed))
        || journal_failed
        || (status.is_none() && heuristic == AgentState::Failed)
    {
        Some(WasteReason::Failed)
    } else if matches!(status, Some(TaskStatus::Killed)) {
        Some(WasteReason::Killed)
    } else if matches!(status, Some(TaskStatus::Completed)) && result_chars.unwrap_or(0) == 0
        || a.sync_result_chars == Some(0)
    {
        Some(WasteReason::NoReturn)
    } else if status.is_none()
        && heuristic == AgentState::Running
        && idle_for.is_some()
        && a.hook_pending() == 0
        && a.transcript_pending() == 0
    {
        // Silent for IDLE_MS with nothing in flight on either side: no
        // tool the spool saw start, none the transcript shows unanswered
        // (a long build without hooks is not idle either).
        Some(WasteReason::Idle)
    } else {
        None
    };
    let waste = reason.map(|reason| Waste {
        usd: cost.usd,
        reason,
        idle_ms: (reason == WasteReason::Idle).then_some(idle_for).flatten(),
    });
    let first = a.first_own_call();
    let cold_start =
        !a.is_fork && first.is_some_and(|c| c.usage.cache_write() > c.usage.cache_read);
    let cold_start_usd = if cold_start {
        first
            .and_then(|c| pricing.price(&c.model).map(|p| (p, c)))
            .map(|(p, c)| {
                (c.usage.cache_write_5m as f64 * p.cache_write_5m()
                    + c.usage.cache_write_1h as f64 * p.cache_write_1h())
                    / 1e6
            })
            .unwrap_or(0.0)
    } else {
        0.0
    };
    AgentRow {
        id: a.id.clone(),
        agent_type: a.agent_type.clone(),
        description: a.description.clone(),
        model: a.model.clone(),
        state: agent_state,
        status,
        is_fork: a.is_fork,
        workflow: a.workflow.clone(),
        started_at: a.started_at,
        elapsed_ms,
        tokens: a.usage.total(),
        output_tokens: a.usage.output,
        cost,
        returned_tokens,
        waste,
        cold_start,
        cold_start_usd,
        inherited_context: first.filter(|_| a.is_fork).map(|c| c.usage.cache_read),
        launched_turn: a.launched_turn,
    }
}

/// Every agent's row, in `sort` order (descending unless `ascending`).
pub fn rows(state: &State, sort: Sort, ascending: bool) -> Vec<AgentRow> {
    let mut out: Vec<AgentRow> = state.agents.values().map(|a| row(state, a)).collect();
    let key = |r: &AgentRow| -> (f64, f64, i64) {
        match sort {
            Sort::Spend => (r.cost.usd, r.waste.map_or(0.0, |w| w.usd), 0),
            Sort::Waste => (r.waste.map_or(0.0, |w| w.usd), r.cost.usd, 0),
            Sort::Elapsed => (r.elapsed_ms.unwrap_or(0) as f64, r.cost.usd, 0),
            Sort::Started => (r.started_at.unwrap_or(0) as f64, r.cost.usd, 0),
        }
    };
    out.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        let ord =
            kb.0.partial_cmp(&ka.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(kb.1.partial_cmp(&ka.1).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| a.id.cmp(&b.id));
        if ascending {
            ord.reverse()
        } else {
            ord
        }
    });
    out
}

/// The footer over `rows`.
pub fn totals(rows: &[AgentRow]) -> Totals {
    let mut cost_usd = 0.0;
    let mut any_priced = false;
    let mut waste_by_reason = [0.0; 4];
    let mut classified = 0;
    let mut cold_starts = 0;
    let mut cold_start_usd = 0.0;
    let mut returned = 0u64;
    let mut output = 0u64;
    let mut running = 0;
    for r in rows {
        if r.cost.source != Source::Unpriced {
            cost_usd += r.cost.usd;
            any_priced = true;
        }
        if let Some(w) = r.waste {
            let i = WasteReason::ALL
                .iter()
                .position(|x| *x == w.reason)
                .expect("every reason is listed");
            waste_by_reason[i] += w.usd;
            classified += 1;
        }
        if r.cold_start {
            cold_starts += 1;
            cold_start_usd += r.cold_start_usd;
        }
        returned += r.returned_tokens.unwrap_or(0);
        output += r.output_tokens;
        if r.state == AgentState::Running {
            running += 1;
        }
    }
    Totals {
        agents: rows.len(),
        running,
        cost: if any_priced {
            Cost::priced(cost_usd)
        } else {
            Cost {
                usd: 0.0,
                approx: true,
                source: Source::Unpriced,
            }
        },
        waste_usd: waste_by_reason.iter().sum(),
        waste_by_reason,
        classified,
        cold_starts,
        cold_start_usd,
        returned_ratio: (output > 0).then(|| returned as f64 / output as f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Meta;
    use crate::metrics::cost::parse_ts_ms;
    use crate::transcript::Line;

    const T0: &str = "2026-01-01T00:00:00Z";

    /// An agent with `n` priced calls (opus, 1 000 output tokens each; the
    /// first call warm or cold) that ended with text, or with a pending
    /// tool call when `pending`.
    fn agent(id: &str, n: usize, cold: bool, pending: bool) -> Agent {
        let mut a = Agent::new(
            id,
            Meta {
                agent_type: "Explore".into(),
                ..Default::default()
            },
        );
        for i in 0..n {
            let (write, read) = if i == 0 && cold {
                (50_000, 100)
            } else {
                (100, 50_000)
            };
            let (block, stop) = if pending && i + 1 == n {
                (
                    r#"{"type":"tool_use","id":"t","name":"Bash","input":{}}"#,
                    "tool_use",
                )
            } else {
                (r#"{"type":"text","text":"done"}"#, "end_turn")
            };
            let l = Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:{i:02}Z","message":{{"id":"m{i}","model":"claude-opus-5","content":[{block}],"stop_reason":"{stop}","usage":{{"cache_creation_input_tokens":{write},"cache_read_input_tokens":{read},"output_tokens":1000}}}}}}"#
            ))
            .unwrap();
            a.push(&l);
        }
        a
    }

    fn state_with(agents: Vec<Agent>, now: &str) -> State {
        let mut s = State::new(crate::metrics::Pricing::bundled());
        s.now_ms = parse_ts_ms(now).unwrap();
        s.clock_override = true;
        for a in agents {
            s.agents.insert(a.id.clone(), a);
        }
        s
    }

    fn notify(text: &str) -> Line {
        Line::parse(&format!(
            r#"{{"type":"user","timestamp":"{T0}","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
            serde_json::to_string(text).unwrap()
        ))
        .unwrap()
    }

    fn notification(id: &str, status: &str, result: Option<&str>) -> String {
        let result = result
            .map(|r| format!("<result>{r}</result>"))
            .unwrap_or_default();
        format!(
            "<task-notification><task-id>{id}</task-id><status>{status}</status><summary>s</summary>{result}</task-notification>"
        )
    }

    #[test]
    fn failed_notification_wastes_the_whole_cost() {
        let mut s = state_with(vec![agent("0000000000000000a", 3, false, false)], T0);
        s.apply(&notify(&notification("0000000000000000a", "failed", None)));
        let r = &rows(&s, Sort::Spend, false)[0];
        assert_eq!(r.status, Some(TaskStatus::Failed));
        assert_eq!(r.state, AgentState::Failed);
        assert_eq!(r.returned_tokens, None);
        let w = r.waste.unwrap();
        assert_eq!(w.reason, WasteReason::Failed);
        assert!((w.usd - r.cost.usd).abs() < 1e-12 && w.usd > 0.0);
        assert_eq!(r.cost.source, Source::Priced);
    }

    #[test]
    fn journal_failed_entry_and_heuristic_failure() {
        let mut s = state_with(vec![agent("0000000000000000b", 2, false, false)], T0);
        s.workflow_journals.push(crate::agents::WorkflowJournal {
            run: "wf_1".into(),
            failed: 1,
            failed_ids: vec!["0000000000000000b".into()],
            ..Default::default()
        });
        let r = &rows(&s, Sort::Spend, false)[0];
        assert_eq!(r.waste.unwrap().reason, WasteReason::Failed);
        assert_eq!(r.state, AgentState::Failed);

        // No notification: an error result with nothing after it for 60 s.
        let mut a = agent("0000000000000000c", 1, false, true);
        a.push(
            &Line::parse(
                r#"{"type":"user","timestamp":"2026-01-01T00:00:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"x","is_error":true}]}}"#,
            )
            .unwrap(),
        );
        let s = state_with(vec![a], "2026-01-01T00:02:00Z");
        let r = &rows(&s, Sort::Spend, false)[0];
        assert_eq!(r.status, None);
        assert_eq!(r.waste.unwrap().reason, WasteReason::Failed);
    }

    #[test]
    fn killed_beats_an_empty_result() {
        let mut s = state_with(vec![agent("0000000000000000d", 2, false, false)], T0);
        s.apply(&notify(&notification(
            "0000000000000000d",
            "killed",
            Some(""),
        )));
        let r = &rows(&s, Sort::Spend, false)[0];
        assert_eq!(r.returned_tokens, Some(0));
        assert_eq!(r.waste.unwrap().reason, WasteReason::Killed);
        assert_eq!(r.state, AgentState::Failed);
    }

    #[test]
    fn completed_without_a_result_is_no_return() {
        let mut s = state_with(
            vec![
                agent("0000000000000000e", 2, false, false),
                agent("0000000000000000f", 2, false, false),
                agent("00000000000000010", 2, false, false),
            ],
            T0,
        );
        s.apply(&notify(&notification(
            "0000000000000000e",
            "completed",
            None,
        )));
        s.apply(&notify(&notification(
            "0000000000000000f",
            "completed",
            Some(""),
        )));
        s.apply(&notify(&notification(
            "00000000000000010",
            "completed",
            Some(&"x".repeat(2068)),
        )));
        let by_id = |id: &str| {
            rows(&s, Sort::Spend, false)
                .into_iter()
                .find(|r| r.id == id)
                .unwrap()
        };
        assert_eq!(
            by_id("0000000000000000e").waste.unwrap().reason,
            WasteReason::NoReturn
        );
        assert_eq!(
            by_id("0000000000000000f").waste.unwrap().reason,
            WasteReason::NoReturn
        );
        let ok = by_id("00000000000000010");
        assert_eq!(ok.returned_tokens, Some(517));
        assert_eq!(ok.waste, None);
        assert_eq!(ok.state, AgentState::Done);
    }

    #[test]
    fn idle_needs_silence_and_nothing_in_flight() {
        // A tool call the transcript shows unanswered: a long build is not
        // idle, hooks or no hooks.
        let building = agent("00000000000000011", 2, false, true);
        let s = state_with(vec![building], "2026-01-01T00:10:00Z");
        assert_eq!(rows(&s, Sort::Spend, false)[0].waste, None);

        // The tool answered, the next API call never came: running with
        // nothing in flight, silent since T0 + 10 s.
        let mut a = agent("00000000000000012", 2, false, true);
        let answered = Line::parse(
            r#"{"type":"user","timestamp":"2026-01-01T00:00:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"ok"}]}}"#,
        )
        .unwrap();
        a.push(&answered);
        assert_eq!(a.state(i64::MAX), AgentState::Running);
        assert_eq!(a.transcript_pending(), 0);
        let idle = state_with(vec![a.clone()], "2026-01-01T00:10:00Z");
        let w = rows(&idle, Sort::Spend, false)[0].waste.unwrap();
        assert_eq!(w.reason, WasteReason::Idle);
        assert!(w.idle_ms.unwrap() >= IDLE_MS);
        let fresh = state_with(vec![a.clone()], "2026-01-01T00:04:00Z");
        assert_eq!(
            rows(&fresh, Sort::Spend, false)[0].waste,
            None,
            "under IDLE_MS"
        );
        // A tool the spool saw start after the last line and not finish.
        a.note_hook(
            "PreToolUse",
            None,
            parse_ts_ms("2026-01-01T00:00:11Z").unwrap(),
        );
        let busy = state_with(vec![a.clone()], "2026-01-01T00:10:00Z");
        assert_eq!(
            rows(&busy, Sort::Spend, false)[0].waste,
            None,
            "{:?}",
            a.hook_pending()
        );
        a.note_hook(
            "PostToolUse",
            Some(5),
            parse_ts_ms("2026-01-01T00:00:12Z").unwrap(),
        );
        let idle = state_with(vec![a], "2026-01-01T00:10:00Z");
        assert_eq!(
            rows(&idle, Sort::Spend, false)[0].waste.unwrap().reason,
            WasteReason::Idle
        );
    }

    #[test]
    fn a_short_result_is_a_return_and_a_finished_agent_stops_its_clock() {
        let mut s = state_with(
            vec![
                agent("00000000000000016", 2, false, false),
                agent("00000000000000017", 2, false, true),
            ],
            "2026-01-01T00:00:00Z",
        );
        s.apply(&notify(&notification(
            "00000000000000016",
            "completed",
            Some("ok."),
        )));
        s.apply(&notify(&notification(
            "00000000000000017",
            "killed",
            Some(""),
        )));
        // The clock is far ahead: a notified agent's time does not follow it.
        s.now_ms = parse_ts_ms("2026-01-10T00:00:00Z").unwrap();
        let rs = rows(&s, Sort::Spend, false);
        let short = rs.iter().find(|r| r.id == "00000000000000016").unwrap();
        assert_eq!(short.returned_tokens, Some(0), "3 chars ÷ 4");
        assert_eq!(short.waste, None, "three characters came back");
        let killed = rs.iter().find(|r| r.id == "00000000000000017").unwrap();
        assert_eq!(
            killed.elapsed_ms,
            Some(1_000),
            "its transcript's span, not the clock"
        );
        // With `<duration_ms>` in the notification, that figure wins.
        let with_duration = Line::parse(&format!(
            r#"{{"type":"user","timestamp":"{T0}","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
            serde_json::to_string("<task-notification><task-id>00000000000000017</task-id><status>killed</status><summary>s</summary><usage><subagent_tokens>1</subagent_tokens><tool_uses>1</tool_uses><duration_ms>81000</duration_ms></usage></task-notification>").unwrap()
        ))
        .unwrap();
        s.apply(&with_duration);
        let rs = rows(&s, Sort::Spend, false);
        let killed = rs.iter().find(|r| r.id == "00000000000000017").unwrap();
        assert_eq!(killed.elapsed_ms, Some(81_000));
    }

    #[test]
    fn the_notification_outranks_the_journal() {
        let mut s = state_with(vec![agent("00000000000000018", 2, false, false)], T0);
        s.workflow_journals.push(crate::agents::WorkflowJournal {
            run: "wf_1".into(),
            failed: 1,
            failed_ids: vec!["00000000000000018".into()],
            ..Default::default()
        });
        assert_eq!(
            rows(&s, Sort::Spend, false)[0].waste.unwrap().reason,
            WasteReason::Failed
        );
        s.apply(&notify(&notification(
            "00000000000000018",
            "completed",
            Some("four"),
        )));
        let r = &rows(&s, Sort::Spend, false)[0];
        assert_eq!(r.state, AgentState::Done);
        assert_eq!(r.waste, None, "Claude Code's later word wins");
    }

    #[test]
    fn done_without_a_notification_is_not_waste() {
        let s = state_with(vec![agent("00000000000000012", 2, false, false)], T0);
        let r = &rows(&s, Sort::Spend, false)[0];
        assert_eq!(r.state, AgentState::Done);
        assert_eq!(r.waste, None);
        assert_eq!(r.returned_tokens, None);
    }

    #[test]
    fn cold_starts_unpriced_models_and_totals() {
        let mut unpriced = agent("00000000000000013", 1, false, false);
        unpriced.model = "claude-unknown-9".into();
        let mut s = state_with(
            vec![
                agent("00000000000000014", 2, true, false),
                agent("00000000000000015", 2, false, false),
                unpriced,
            ],
            T0,
        );
        s.apply(&notify(&notification(
            "00000000000000014",
            "completed",
            Some("four"),
        )));
        s.apply(&notify(&notification("00000000000000015", "failed", None)));
        s.apply(&notify(&notification("00000000000000013", "failed", None)));
        let rs = rows(&s, Sort::Waste, false);
        let cold = rs.iter().find(|r| r.id == "00000000000000014").unwrap();
        assert!(cold.cold_start);
        let p = crate::metrics::Pricing::bundled();
        let expected = 50_000.0 * p.price("claude-opus-5").unwrap().cache_write_5m() / 1e6;
        assert!((cold.cold_start_usd - expected).abs() < 1e-12);
        assert_eq!(cold.waste, None);
        let unp = rs.iter().find(|r| r.id == "00000000000000013").unwrap();
        assert_eq!(unp.cost.source, Source::Unpriced);
        assert_eq!(unp.waste.unwrap().usd, 0.0, "a reason, no dollars");
        // Sorted by waste: the priced failure first, the unpriced one last
        // among the classified.
        assert_eq!(rs[0].id, "00000000000000015");
        let t = totals(&rs);
        assert_eq!(t.agents, 3);
        assert_eq!(t.classified, 2);
        assert_eq!(t.cold_starts, 1);
        assert!((t.waste_usd - rs[0].cost.usd).abs() < 1e-12);
        assert!((t.waste_by_reason[0] - t.waste_usd).abs() < 1e-12);
        assert_eq!(t.cost.source, Source::Priced);
        assert!((t.cost.usd - (rs[0].cost.usd + cold.cost.usd)).abs() < 1e-12);
        // 1 returned token (4 chars) over 5 000 output tokens.
        assert!((t.returned_ratio.unwrap() - 1.0 / 5000.0).abs() < 1e-12);
    }

    #[test]
    fn fixture_c_has_the_full_path() {
        // The session fixture A's fork came from, whole: the `Agent` launch,
        // the fork's transcript and the notification that returned its
        // result (delivered twice: enqueued while the model was busy, then
        // as a user line); a spliced `killed` Explore agent with its
        // transcript, delivered as a queued command after a `TaskStop`; and
        // a background Bash command's `failed` notification, which is not
        // an agent's. No `failed` agent notification existed on the machine
        // the fixture was composed on (`fixtures/README.md`).
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-c.jsonl");
        let info = crate::ui::state::SessionInfo::from_fixture(&path);
        let s = crate::load::state_from(&path, info);
        assert_eq!(s.agents.len(), 2, "the shell task never became an agent");
        assert_eq!(s.agent_links.len(), 2);
        let rs = rows(&s, Sort::Waste, false);

        let killed = rs.iter().find(|r| r.id == "a2de915228d5f40fe").unwrap();
        assert_eq!(killed.status, Some(TaskStatus::Killed));
        assert_eq!(killed.state, AgentState::Failed);
        assert_eq!(killed.agent_type, "Explore");
        assert_eq!(
            killed.returned_tokens,
            Some(165 / 4),
            "a partial result came back"
        );
        let w = killed.waste.unwrap();
        assert_eq!(w.reason, WasteReason::Killed);
        assert!(w.usd > 0.1 && (w.usd - killed.cost.usd).abs() < 1e-12);
        assert_eq!(s.agents["a9a92645226d3a561"].launched_turn, Some(2));
        assert_eq!(
            killed.launched_turn,
            Some(7),
            "spliced after the spine's last turn"
        );
        assert!(!killed.is_fork);
        assert!(
            killed.cold_start,
            "a fresh Explore agent writes its cache first"
        );

        let fork = rs.iter().find(|r| r.id == "a9a92645226d3a561").unwrap();
        assert_eq!(fork.status, Some(TaskStatus::Completed));
        assert_eq!(fork.returned_tokens, Some(2069 / 4));
        assert_eq!(fork.waste, None);
        assert_eq!(fork.state, AgentState::Done);
        assert_eq!(
            s.agents["a9a92645226d3a561"]
                .notified
                .as_ref()
                .unwrap()
                .subagent_tokens,
            Some(75_218)
        );
        assert_eq!(rs[0].id, killed.id, "sorted by waste");

        let t = totals(&rs);
        assert_eq!(t.classified, 1);
        assert!((t.waste_usd - w.usd).abs() < 1e-12);
        assert!((t.waste_by_reason[1] - w.usd).abs() < 1e-12);
        assert_eq!(t.cold_starts, 1);
        assert!(t.returned_ratio.unwrap() > 0.0);
    }

    #[test]
    fn fixture_a_fork() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        let info = crate::ui::state::SessionInfo::from_fixture(&path);
        let s = crate::load::state_from(&path, info);
        let rs = rows(&s, Sort::Spend, false);
        assert_eq!(rs.len(), 1);
        let f = &rs[0];
        assert!(f.is_fork);
        assert_eq!(f.status, None, "its notification is in another session");
        assert_eq!(f.waste, None);
        assert!(!f.cold_start, "a fork is warm from its first own call");
        assert_eq!(f.returned_tokens, None);
        assert_eq!(f.inherited_context, Some(62_690));
        assert_eq!(f.state, AgentState::Done);
        assert!(f.cost.usd > 0.1);
        let t = totals(&rs);
        assert_eq!(t.waste_usd, 0.0);
        assert_eq!(t.returned_ratio, Some(0.0));
    }
}
