//! A `Workflow` run's record (`<session dir>/workflows/wf_*.json`), read for
//! its identifiers and, per phase, where the script launches its agents.
//!
//! The record half reads it; the derivation half folds the record, the
//! journal, the member agents and the main transcript's launches and
//! notification into a run's verdict: its state, window, per-phase figures,
//! a cause per failed agent and the fix lines (spec §4.1, §4.3).

use std::collections::BTreeMap;
use std::path::Path;

use crate::agents::WorkflowJournal;
use crate::workflow_script::{pointer, Pointer};

/// One run record, as far as cctop keeps it: identifiers, times and one
/// pointer per phase — never the script, nor any prose the run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowRecord {
    pub run: String,
    pub name: Option<String>,
    /// `"completed"` | `"killed"` | …
    pub status: Option<String>,
    pub start_ms: Option<i64>,
    pub duration_ms: Option<u64>,
    pub script_path: Option<String>,
    /// Phase title → where its agents are launched in the script.
    pub pointers: BTreeMap<String, Pointer>,
    /// The record file's modification time, epoch ms: a cache key only,
    /// never rendered or serialised.
    pub mtime_ms: Option<i64>,
    /// The (phase, label prefixes) the pointers were computed from: a cache
    /// key only, never rendered or serialised.
    pub scanned_with: Vec<(String, Vec<String>)>,
}

/// The fields of a run record cctop reads. Deliberately not read, so never
/// materialised: `detail` (per phase), `args`, `result`, `logs`, `summary`,
/// `error` and every other field; `script` is held only for the pointer scan
/// and dropped before the record is kept (spec §4.6).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRecord {
    run_id: Option<String>,
    workflow_name: Option<String>,
    status: Option<String>,
    start_time: Option<i64>,
    duration_ms: Option<u64>,
    script_path: Option<String>,
    script: Option<String>,
}

/// `<session dir>/workflows/wf_*.json`; `journals` supplies each phase's label
/// prefixes; `prev` is the last read, reused per file while its mtime and the
/// prefixes are unchanged, so a script is tokenised once per change rather
/// than once per tick. Unreadable or invalid files are skipped, as is anything
/// that is not `wf_*.json` (the `scripts/` directory lives beside the records).
/// A record whose run has no journal gets no pointers.
pub fn read_records(
    session_dir: &Path,
    journals: &[WorkflowJournal],
    prev: &[WorkflowRecord],
) -> Vec<WorkflowRecord> {
    let Ok(entries) = std::fs::read_dir(session_dir.join("workflows")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        let Some(stem) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|s| s.starts_with("wf_"))
            .map(str::to_owned)
        else {
            continue;
        };
        let Ok(meta) = e.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        if let Some(p) = prev.iter().find(|p| {
            p.run == stem
                && mtime_ms.is_some()
                && p.mtime_ms == mtime_ms
                && p.scanned_with == scan_key(journals, &p.run)
        }) {
            out.push(p.clone());
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(raw) = serde_json::from_str::<RawRecord>(&text) else {
            continue;
        };
        drop(text);
        let run = raw.run_id.unwrap_or(stem);
        let scanned_with = scan_key(journals, &run);
        let pointers = match raw.script.as_deref() {
            Some(script) => scanned_with
                .iter()
                .filter_map(|(phase, prefixes)| {
                    pointer(script, phase, prefixes).map(|p| (phase.clone(), p))
                })
                .collect(),
            None => BTreeMap::new(),
        };
        out.push(WorkflowRecord {
            run,
            name: raw.workflow_name,
            status: raw.status,
            start_ms: raw.start_time,
            duration_ms: raw.duration_ms,
            script_path: raw.script_path,
            pointers,
            mtime_ms,
            scanned_with,
        });
    }
    out.sort_by(|a, b| a.run.cmp(&b.run));
    out
}

/// The (phase, label prefixes) of `run`'s journal, in phase order; empty
/// without a journal.
fn scan_key(journals: &[WorkflowJournal], run: &str) -> Vec<(String, Vec<String>)> {
    journals
        .iter()
        .find(|j| j.run == run)
        .map(|j| {
            j.phases
                .iter()
                .map(|p| (p.title.clone(), p.label_prefixes.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// A run with activity this recent is `Live`.
pub const LIVE_MS: i64 = 60_000;
/// A terminal marker survives member lines this far after it: the last
/// transcript write may land just after the notification.
pub const RESUME_SLACK_MS: i64 = 10_000;
/// A `Stalled` run drops off the dashboard after this long.
pub const STALLED_DROP_MS: i64 = 30 * 60_000;

/// Why one workflow agent failed; one per agent, tested in declaration
/// order of [`cause`]'s rules, not of the variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    RateLimitFirst,
    RateLimitMid,
    Overloaded,
    ContextOverflow,
    NoStructuredOutput,
    Killed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Live,
    Stalled,
    Completed,
    Killed,
    /// The notification said the run itself failed.
    Failed,
}

/// One journal phase's figures.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PhaseRow {
    pub title: String,
    pub started: usize,
    pub results: usize,
    /// The journal's count.
    pub failed: usize,
    /// Σ priced cost of the phase's failed agents.
    pub failed_usd: f64,
    /// Σ waste ÷ Σ cost of the phase's agents; `None` at no cost.
    pub waste_pct: Option<f64>,
    /// The agents that got a cause: the failed ones, and in a killed run
    /// the ones cut off.
    pub causes: BTreeMap<Cause, usize>,
    pub pointer: Option<Pointer>,
    /// None of the phase's failed agents started under the record's script.
    pub pointer_stale: bool,
}

/// A fix line: one cause over the run, with the phase that has most of its
/// agents.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Fix {
    pub cause: Cause,
    pub agents: usize,
    pub text: String,
    pub phase: String,
    pub pointer: Option<Pointer>,
    pub pointer_stale: bool,
    pub script_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RunVerdict {
    pub name: Option<String>,
    pub state: RunState,
    pub phases: Vec<PhaseRow>,
    /// The journal's `failed` entries.
    pub failed: usize,
    pub failed_usd: f64,
    pub waste_pct: Option<f64>,
    /// Run $ ÷ main-thread $ over the window; `None` under $0.01 of main
    /// thread.
    pub overhead: Option<f64>,
    pub cold_start_pct: Option<f64>,
    pub started_ms: Option<i64>,
    /// The holding marker's time; `None` while `Live` (the window runs to
    /// now); else the last activity.
    pub ended_ms: Option<i64>,
    /// The latest member line, journal write (capped at the clock) or
    /// launch: what the header strip ranks live runs by.
    pub last_activity_ms: Option<i64>,
    /// The two causes with the most agents, `Killed` excluded.
    pub fixes: Vec<Fix>,
}

/// One agent's cause, from its last API error, the run being killed and
/// whether its phase used a schema — in that order (spec §4.3).
pub fn cause(a: Option<&crate::agents::Agent>, phase_used_schema: bool, killed: bool) -> Cause {
    use crate::harness_facts::workflow_run::PROMPT_TOO_LONG_TOKENS;
    if let Some((a, e)) = a.and_then(|a| a.api_error.as_ref().map(|e| (a, e))) {
        let token = e.token.as_deref();
        return if e.status == Some(429) || token == Some("rate_limit") {
            if a.completed_calls_before_error == 0 {
                Cause::RateLimitFirst
            } else {
                Cause::RateLimitMid
            }
        } else if e.status == Some(529) || matches!(token, Some("overloaded" | "overloaded_error"))
        {
            Cause::Overloaded
        } else if token.is_some_and(|t| PROMPT_TOO_LONG_TOKENS.contains(&t)) {
            Cause::ContextOverflow
        } else {
            Cause::Unknown
        };
    }
    if killed {
        return Cause::Killed;
    }
    if phase_used_schema && a.is_some_and(|a| !a.tools_by_name.contains_key(STRUCTURED_OUTPUT)) {
        return Cause::NoStructuredOutput;
    }
    Cause::Unknown
}

const STRUCTURED_OUTPUT: &str = "StructuredOutput";

/// `"1 agent"` / `"3 agents"`, or the possessive `"1 agent's"` /
/// `"3 agents'"`.
fn agents_n(n: usize, possessive: bool) -> String {
    match (n == 1, possessive) {
        (true, false) => "1 agent".to_string(),
        (true, true) => "1 agent's".to_string(),
        (false, false) => format!("{n} agents"),
        (false, true) => format!("{n} agents'"),
    }
}

/// The fix line for `n` agents of `cause` (spec §4.3); `token` is
/// `Unknown`'s error token. `Killed` has no fix and gets an empty string.
pub fn fix_text(cause: Cause, n: usize, token: Option<&str>) -> String {
    let a = agents_n(n, false);
    match cause {
        Cause::RateLimitFirst => format!(
            "{a} hit the rate limit on their first call — batch this phase's items, or lower its effort/model"
        ),
        Cause::RateLimitMid => format!(
            "{a} hit the rate limit mid-task — batch this phase, then resume the run after the window resets"
        ),
        Cause::Overloaded => {
            format!("{a} met an overloaded API — transient; resume the run, finished agents are cached")
        }
        Cause::ContextOverflow => format!(
            "{} input was too large — pass paths, not contents",
            agents_n(n, true)
        ),
        Cause::NoStructuredOutput => {
            format!("{a} never satisfied the schema — loosen it or split the task")
        }
        Cause::Unknown => format!(
            "{a} failed ({}) — cctop has no fix for this yet",
            token.unwrap_or("no error")
        ),
        Cause::Killed => String::new(),
    }
}

/// What an `Unknown` agent's fix line names: its error token, `no
/// transcript`, else `no error`. An API error without a token names its
/// status.
fn unknown_label(a: Option<&crate::agents::Agent>) -> String {
    match a {
        None => "no transcript".to_string(),
        Some(a) => match &a.api_error {
            Some(e) => e
                .token
                .clone()
                .or_else(|| e.status.map(|s| s.to_string()))
                .unwrap_or_else(|| "no error".to_string()),
            None => "no error".to_string(),
        },
    }
}

/// `run`'s verdict over `rows` (the ledger's, fork skip applied).
pub fn verdict(
    state: &crate::ui::State,
    rows: &[crate::agent_ledger::AgentRow],
    run: &str,
) -> RunVerdict {
    use crate::transcript::TaskStatus;
    let now = state.clock_ms();
    let journal = state.workflow_journals.iter().find(|j| j.run == run);
    let record = state.workflow_records.iter().find(|r| r.run == run);
    let members: Vec<&crate::agents::Agent> = state
        .agents
        .values()
        .filter(|a| a.workflow.as_deref() == Some(run))
        .collect();
    let member_rows: Vec<&crate::agent_ledger::AgentRow> = rows
        .iter()
        .filter(|r| r.workflow.as_deref() == Some(run))
        .collect();
    let row_of = |id: &str| rows.iter().find(|r| r.id == id);
    let launches: Vec<&crate::tools::WorkflowLaunch> = state
        .tools
        .workflow_launches
        .iter()
        .filter(|l| l.run_id.as_deref() == Some(run))
        .collect();

    // Activity: member lines are line-time evidence; the journal's mtime
    // (capped at the clock) only keeps a marker-less run alive, as does a
    // launch (a resume whose journal has not moved yet).
    let line_activity = members.iter().filter_map(|a| a.last_line_at).max();
    let last_activity = line_activity
        .max(journal.and_then(|j| j.mtime_ms).map(|m| m.min(now)))
        .max(launches.iter().filter_map(|l| l.at_ms).max());

    // The terminal marker: the later of the notification and the record's
    // end. Its status is the notification's (else the record's) when both
    // describe the same invocation — no launch of the run lies between them
    // — else the later marker's.
    let runs = crate::agent_ledger::workflow_runs(state, rows);
    let from_notification =
        crate::agent_ledger::notification_key(state, &runs, run).and_then(|k| {
            let at = *state.workflow_notified_at.get(k)?;
            let status = match &state.workflow_notifications.get(k)?.status {
                TaskStatus::Completed => Some(RunState::Completed),
                TaskStatus::Failed => Some(RunState::Failed),
                TaskStatus::Killed => Some(RunState::Killed),
                TaskStatus::Other(_) => None,
            };
            Some((at, status))
        });
    let from_record = record.and_then(|r| {
        let end = r.start_ms? + r.duration_ms? as i64;
        let status = match r.status.as_deref()? {
            "completed" => RunState::Completed,
            "killed" => RunState::Killed,
            _ => return None,
        };
        Some((end, status))
    });
    let launched_between = |a: i64, b: i64| {
        let (lo, hi) = (a.min(b), a.max(b));
        launches
            .iter()
            .any(|l| l.at_ms.is_some_and(|at| at > lo && at < hi))
    };
    let marker = match (from_notification, from_record) {
        (Some((n, ns)), Some((r, rs))) => {
            let status = if !launched_between(n, r) {
                ns.or(Some(rs))
            } else if n >= r {
                ns
            } else {
                Some(rs)
            };
            Some((n.max(r), status))
        }
        (Some((n, ns)), None) => Some((n, ns)),
        (None, Some((r, rs))) => Some((r, Some(rs))),
        (None, None) => None,
    };
    let holding = marker.filter(|(t, _)| {
        !launches.iter().any(|l| l.at_ms.is_some_and(|at| at > *t))
            && line_activity.is_none_or(|l| l <= t + RESUME_SLACK_MS)
    });
    let run_state = match holding {
        Some((_, status)) => status.unwrap_or(RunState::Completed),
        None if last_activity.is_some_and(|t| now - t < LIVE_MS) => RunState::Live,
        None => RunState::Stalled,
    };

    // The window.
    let started_ms = launches
        .iter()
        .filter_map(|l| l.at_ms)
        .chain(record.and_then(|r| r.start_ms))
        .chain(members.iter().filter_map(|a| a.started_at))
        .min();
    let ended_ms = match (holding, run_state) {
        (Some((t, _)), _) => Some(t),
        (None, RunState::Live) => None,
        (None, _) => last_activity,
    };

    // Per phase: the agents that get a cause, and the phase's figures.
    let killed = run_state == RunState::Killed;
    let cost_of = |id: &str| row_of(id).map_or(0.0, |r| r.cost.usd);
    let mut phases = Vec::new();
    // (phase index, cause, Unknown's label) per agent with a cause.
    let mut caused: Vec<(usize, Cause, String)> = Vec::new();
    let no_ids = Vec::new();
    let failed_ids = journal.map_or(&no_ids, |j| &j.failed_ids);
    for (i, p) in journal.map_or(&[][..], |j| &j.phases).iter().enumerate() {
        let failed: Vec<&String> = p
            .agent_ids
            .iter()
            .filter(|id| failed_ids.contains(id))
            .collect();
        let cut: Vec<&String> = if killed {
            p.agent_ids
                .iter()
                .filter(|id| !p.result_ids.contains(id) && !failed_ids.contains(id))
                .collect()
        } else {
            Vec::new()
        };
        let used_schema = p.agent_ids.iter().any(|id| {
            state
                .agents
                .get(id)
                .is_some_and(|a| a.tools_by_name.contains_key(STRUCTURED_OUTPUT))
        });
        let mut causes = BTreeMap::new();
        for id in failed.iter().chain(cut.iter()) {
            let a = state.agents.get(*id);
            let c = cause(a, used_schema, killed);
            *causes.entry(c).or_insert(0) += 1;
            caused.push((i, c, unknown_label(a)));
        }
        let (mut cost, mut waste) = (0.0, 0.0);
        for r in p.agent_ids.iter().filter_map(|id| row_of(id)) {
            cost += r.cost.usd;
            waste += r.waste.map_or(0.0, |w| w.usd);
        }
        // Judged over the journal's failed agents only: agents cut off by a
        // kill ran under the latest script (spec §4.4).
        let pointer = record.and_then(|r| r.pointers.get(&p.title).copied());
        let pointer_stale = pointer.is_some()
            && record.and_then(|r| r.start_ms).is_some_and(|start| {
                !failed.is_empty()
                    && !failed.iter().any(|id| {
                        state
                            .agents
                            .get(id.as_str())
                            .and_then(|a| a.started_at)
                            .is_some_and(|s| s >= start)
                    })
            });
        phases.push(PhaseRow {
            title: p.title.clone(),
            started: p.started,
            results: p.results,
            failed: p.failed,
            failed_usd: usd_sum(failed.iter().map(|id| cost_of(id))),
            waste_pct: (cost > 0.0).then(|| waste / cost),
            causes,
            pointer,
            pointer_stale,
        });
    }

    // The run's figures.
    let run_usd = usd_sum(member_rows.iter().map(|r| r.cost.usd));
    let share = |x: f64| (run_usd > 0.0).then(|| x / run_usd);
    let waste_usd = usd_sum(member_rows.iter().map(|r| r.waste.map_or(0.0, |w| w.usd)));
    // As `agent_ledger::totals` sums `Totals.cold_start_usd`.
    let cold_start_usd = usd_sum(
        member_rows
            .iter()
            .filter(|r| r.cold_start)
            .map(|r| r.cold_start_usd),
    );
    let pricing = state.cost.pricing();
    let overhead = started_ms.and_then(|start| {
        let end = ended_ms.unwrap_or(now);
        let main = usd_sum(
            state
                .agg
                .calls
                .iter()
                .filter(|c| c.at_ms.is_some_and(|t| t >= start && t <= end))
                .filter_map(|c| pricing.estimate(&c.usage, &c.model)),
        );
        (main >= 0.01).then(|| run_usd / main)
    });

    // The fix lines: the two causes with the most agents.
    let mut by_cause: BTreeMap<Cause, usize> = BTreeMap::new();
    for (_, c, _) in &caused {
        if *c != Cause::Killed {
            *by_cause.entry(*c).or_insert(0) += 1;
        }
    }
    let mut ranked: Vec<(Cause, usize)> = by_cause.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let fixes = ranked
        .into_iter()
        .take(2)
        .map(|(c, n)| {
            let mut per_phase: BTreeMap<usize, usize> = BTreeMap::new();
            let mut labels: BTreeMap<&str, usize> = BTreeMap::new();
            for (i, _, label) in caused.iter().filter(|(_, x, _)| *x == c) {
                *per_phase.entry(*i).or_insert(0) += 1;
                *labels.entry(label.as_str()).or_insert(0) += 1;
            }
            // Most agents, first phase on a tie; most frequent label,
            // alphabetical on a tie.
            let phase = per_phase
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                .map(|(i, _)| &phases[*i]);
            let token = labels
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                .map(|(l, _)| *l);
            Fix {
                cause: c,
                agents: n,
                text: fix_text(c, n, token),
                phase: phase.map(|p| p.title.clone()).unwrap_or_default(),
                pointer: phase.and_then(|p| p.pointer),
                pointer_stale: phase.is_some_and(|p| p.pointer_stale),
                script_path: record.and_then(|r| r.script_path.clone()),
            }
        })
        .collect();

    RunVerdict {
        name: launches
            .iter()
            .rev()
            .find_map(|l| l.name.clone())
            .or_else(|| record.and_then(|r| r.name.clone())),
        state: run_state,
        phases,
        failed: journal.map_or(0, |j| j.failed),
        failed_usd: journal.map_or(0.0, |j| usd_sum(j.failed_ids.iter().map(|id| cost_of(id)))),
        waste_pct: share(waste_usd),
        overhead,
        cold_start_pct: share(cold_start_usd),
        started_ms,
        ended_ms,
        last_activity_ms: last_activity,
        fixes,
    }
}

// ---------------------------------------------------------------------------
// Rendered text: the rows every surface draws verbatim (spec §4.5).

/// The run detail's wide layout: the rows inside a 120-column frame.
pub const DETAIL_WIDE: usize = 116;
/// The narrow layout: the rows inside a 56-column frame.
pub const DETAIL_NARROW: usize = 52;

impl RunState {
    /// The group row's state glyph.
    pub fn glyph(self) -> char {
        match self {
            RunState::Live => '▶',
            RunState::Stalled => '‖',
            RunState::Completed => '✓',
            RunState::Killed | RunState::Failed => '✗',
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            RunState::Live => "live",
            RunState::Stalled => "stalled",
            RunState::Completed => "completed",
            RunState::Killed => "killed",
            RunState::Failed => "failed",
        }
    }
}

impl Cause {
    /// The strip's and the phase table's short form, before `×n`.
    pub fn short(self) -> &'static str {
        match self {
            Cause::RateLimitFirst | Cause::RateLimitMid => "429",
            Cause::Overloaded => "529",
            Cause::ContextOverflow => "ctx",
            Cause::NoStructuredOutput => "schema",
            Cause::Killed => "kill",
            Cause::Unknown => "?",
        }
    }
}

/// A phase's causes by short form, most agents first (the causes'
/// declaration order on a tie): `429×201`, `?×1`. Both rate-limit causes count as one
/// `429`.
fn short_causes(p: &PhaseRow) -> Vec<(&'static str, usize)> {
    let mut by: Vec<(&'static str, usize)> = Vec::new();
    for (c, n) in &p.causes {
        match by.iter_mut().find(|(s, _)| *s == c.short()) {
            Some((_, m)) => *m += n,
            None => by.push((c.short(), *n)),
        }
    }
    by.sort_by(|a, b| b.1.cmp(&a.1));
    by
}

/// A sum of dollars folded from `+0.0`: `Iterator::sum` of no `f64` is
/// `-0.0`, which renders as `-0.00` and serialises as `-0.0`.
fn usd_sum(xs: impl Iterator<Item = f64>) -> f64 {
    xs.fold(0.0, |a, b| a + b)
}

/// Dollars to the cent, never `-0.00`.
fn usd2(x: f64) -> String {
    format!("{:.2}", x + 0.0)
}

fn phase_title(p: &PhaseRow) -> &str {
    if p.title.is_empty() {
        "—"
    } else {
        &p.title
    }
}

fn overhead_text(v: &RunVerdict) -> String {
    v.overhead.map_or("—".to_string(), |o| format!("{o:.1}"))
}

fn pct_text(p: Option<f64>) -> String {
    p.map_or("—".to_string(), |p| format!("{:.0} %", p * 100.0))
}

/// The run's dollars after `mark`, or `—` when none of its agents is
/// priced: the group row, the detail header and the strip say the same.
fn run_usd(g: &crate::agent_ledger::WorkflowGroup, mark: &str) -> String {
    if g.cost.source == crate::metrics::cost::Source::Unpriced {
        "—".to_string()
    } else {
        format!("{mark}{}", usd2(g.cost.usd))
    }
}

/// The agents view's group row, cut to the view's width: state glyph, run,
/// name, `✗n` when agents failed, the run's dollars, then the journal's
/// counts — the dollars before the counts, so a cut at real counts (or at
/// the pane's 52 cells) drops the tail of the counts, not the money.
pub fn group_text(g: &crate::agent_ledger::WorkflowGroup) -> String {
    use crate::ui::fmt;
    let mut parts = vec![
        format!("{} launched", g.launched),
        format!("{} done", g.done),
    ];
    if let Some(e) = g.empty_result {
        parts.push(format!("{e} empty"));
    }
    let cost = run_usd(g, "≈$");
    let failed = if g.verdict.failed > 0 {
        format!(" ✗{}", g.verdict.failed)
    } else {
        String::new()
    };
    let row = format!(
        " {} {:<11} {:<12}{failed}  {cost}  {}",
        g.verdict.state.glyph(),
        fmt::clip(&g.run, 11),
        fmt::clip(g.name.as_deref().unwrap_or(""), 12),
        parts.join(" · ")
    );
    fmt::clip(&row, crate::ui::agents_view::WIDTH)
        .trim_end()
        .to_string()
}

/// `text` word-wrapped at `width`: the first row after `indent`, the rest
/// two cells further in; a word longer than a row is broken. Never clipped.
fn wrap(text: &str, width: usize, indent: &str) -> Vec<String> {
    let hang = format!("{indent}  ");
    let mut out = Vec::new();
    let mut line = indent.to_string();
    let mut empty = true;
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        loop {
            let used = line.chars().count();
            let need = word.len() + usize::from(!empty);
            if used + need <= width {
                if !empty {
                    line.push(' ');
                }
                line.extend(word.iter());
                empty = false;
                break;
            }
            if !empty {
                out.push(std::mem::replace(&mut line, hang.clone()));
                empty = true;
                continue;
            }
            // A word wider than a whole row: break it.
            let room = width.saturating_sub(used).max(1);
            let rest = word.split_off(room.min(word.len()));
            line.extend(word.iter());
            out.push(std::mem::replace(&mut line, hang.clone()));
            word = rest;
            if word.is_empty() {
                break;
            }
        }
    }
    if !empty {
        out.push(line);
    }
    out
}

/// The run detail at `width` ([`DETAIL_WIDE`] or [`DETAIL_NARROW`]): a
/// header row, the column heads, one row per phase, a rule, then each fix
/// line and its pointer, wrapped. No row carries trailing spaces or
/// exceeds `width`.
pub fn detail_lines(g: &crate::agent_ledger::WorkflowGroup, width: usize) -> Vec<String> {
    use crate::ui::fmt;
    let v = &g.verdict;
    let wide = width >= DETAIL_WIDE;
    let row = |s: String| fmt::clip(&s, width).trim_end().to_string();
    let mut out = Vec::new();

    let elapsed = v.started_ms.map(|s| {
        fmt::duration_ms(v.ended_ms.unwrap_or(v.last_activity_ms.unwrap_or(s)).max(s) - s)
    });
    let mut head = vec![g.run.clone()];
    if let Some(n) = &g.name {
        head.push(n.clone());
    }
    head.push(v.state.word().to_string());
    head.push(elapsed.unwrap_or_else(|| "—".to_string()));
    head.push(run_usd(g, "$"));
    head.push(format!("{}×main", overhead_text(v)));
    head.push(format!("cold {}", pct_text(v.cold_start_pct)));
    out.push(row(format!(" {}", head.join("  "))));

    let cols =
        |title: &str, start: &str, res: &str, f: &str, fusd: &str, waste: &str, cause: &str| {
            if wide {
                format!(" {title:<12} {start:>5} {res:>5} {f:>4} {fusd:>7} {waste:>6}  {cause}")
            } else {
                format!(" {title:<12} {start:>5} {res:>5} {f:>4}  {cause}")
            }
        };
    let mut heads = cols("phase", "start", "res", "✗", "✗$", "waste", "cause");
    // The notification's empty results have no per-agent cause (§4.3).
    if let Some(n) = g.empty_result.filter(|n| *n > 0) {
        heads.push_str(&format!("  empty {n}"));
    }
    out.push(row(heads));
    for p in &v.phases {
        let causes: Vec<String> = short_causes(p)
            .iter()
            .map(|(s, n)| format!("{s}×{n}"))
            .collect();
        out.push(row(cols(
            &fmt::clip(phase_title(p), 12),
            &p.started.to_string(),
            &p.results.to_string(),
            &p.failed.to_string(),
            &usd2(p.failed_usd),
            &pct_text(p.waste_pct),
            &causes.join(" "),
        )));
    }
    out.push(" ───".to_string());
    for f in &v.fixes {
        out.extend(wrap(&f.text, width, " "));
        if let Some(ptr) = f.pointer {
            let file = f
                .script_path
                .as_deref()
                .and_then(|p| Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("script");
            let mut s = format!("→ {file}:{} {}()", ptr.line, call_name(ptr.call));
            if f.pointer_stale {
                s.push_str(" · script changed since");
            }
            out.extend(wrap(&s, width, "   "));
        }
    }
    out
}

fn call_name(c: crate::workflow_script::Call) -> &'static str {
    use crate::workflow_script::Call;
    match c {
        Call::Parallel => "parallel",
        Call::Pipeline => "pipeline",
        Call::PhaseMarker => "phase",
    }
}

/// The header strip's text after its label (spec §4.5), in order of
/// importance so a narrow cut drops the least useful: the last phase with
/// a start and its figures and top cause, then the run's dollars and
/// overhead, the name last.
pub fn strip_text(g: &crate::agent_ledger::WorkflowGroup) -> String {
    let v = &g.verdict;
    let phase = v.phases.iter().rev().find(|p| p.started > 0);
    let (title, results, started, failed, cause) = match phase {
        Some(p) => (
            crate::ui::fmt::clip(phase_title(p), 10),
            p.results,
            p.started,
            p.failed,
            short_causes(p)
                .first()
                .map(|(s, n)| format!("{s}×{n}"))
                .unwrap_or_default(),
        ),
        None => ("—".to_string(), 0, 0, 0, String::new()),
    };
    format!(
        "▸ {title:<10} {results}/{started} ✗{failed} {cause:<8} {} {}×main  {}",
        run_usd(g, "$"),
        overhead_text(v),
        g.name.as_deref().unwrap_or(&g.run)
    )
}

/// Whether a run belongs on the strip: `Live`, or `Stalled` for less than
/// [`STALLED_DROP_MS`].
fn on_strip(g: &crate::agent_ledger::WorkflowGroup, now: i64) -> bool {
    match g.verdict.state {
        RunState::Live => true,
        RunState::Stalled => g
            .verdict
            .last_activity_ms
            .is_some_and(|t| now - t < STALLED_DROP_MS),
        _ => false,
    }
}

/// The dashboard header's second row: the run with the latest activity
/// among those on the strip, `+n live` for the others; `None` when no run
/// is on it (the header is then as it was, FR-3).
pub fn strip_line(
    state: &crate::ui::State,
    groups: &[crate::agent_ledger::WorkflowGroup],
) -> Option<crate::dashboard::Line> {
    use crate::dashboard::{dim, seg, Tone};
    let now = state.clock_ms();
    let live: Vec<&crate::agent_ledger::WorkflowGroup> =
        groups.iter().filter(|g| on_strip(g, now)).collect();
    let shown = live
        .iter()
        .copied()
        .max_by_key(|g| g.verdict.last_activity_ms)?;
    let failed = shown
        .verdict
        .phases
        .iter()
        .rev()
        .find(|p| p.started > 0)
        .map_or(0, |p| p.failed);
    let tone = if failed > 0 {
        Tone::Crit
    } else if shown.verdict.state == RunState::Stalled {
        Tone::Dim
    } else {
        Tone::Fg
    };
    let mut line = vec![dim(" wf     "), seg(strip_text(shown), tone)];
    if live.len() > 1 {
        line.push(dim(format!(" +{} live", live.len() - 1)));
    }
    Some(line)
}

#[cfg(test)]
pub mod test_support {
    use crate::agents::{Agent, JournalPhase, Meta, WorkflowJournal};
    use crate::metrics::cost::parse_ts_ms;
    use crate::transcript::Line;
    use crate::ui::State;
    use crate::workflow_script::{Call, Pointer};

    pub const NOW: &str = "2026-01-01T00:10:00Z";

    pub fn err(ts: &str, status: u16, token: &str) -> Line {
        Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"e{ts}","model":"<synthetic>","content":[],"usage":{{"input_tokens":0}}}},"isApiErrorMessage":true,"error":"{token}","apiErrorStatus":{status}}}"#)).unwrap()
    }
    /// A priced haiku response; `tool` adds a tool_use block of that name.
    pub fn ok(id: &str, ts: &str, tool: Option<&str>) -> Line {
        let block = match tool {
            Some(t) => format!(r#"{{"type":"tool_use","id":"tu{id}","name":"{t}","input":{{}}}}"#),
            None => r#"{"type":"text","text":"x"}"#.to_string(),
        };
        Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-haiku-4-5-20251001","content":[{block}],"stop_reason":"end_turn","usage":{{"input_tokens":1000,"cache_creation_input_tokens":20000,"output_tokens":200}}}}}}"#)).unwrap()
    }
    pub fn agent(id: &str, run: &str, lines: &[Line]) -> Agent {
        let mut a = Agent::from_lines(id, Meta::default(), lines.iter());
        a.workflow = Some(run.to_string());
        a
    }
    /// `(phase, [(agent id, "ok" | "failed" | "open")])`.
    pub fn journal(run: &str, phases: &[(&str, &[(&str, &str)])], mtime: &str) -> WorkflowJournal {
        let mut j = WorkflowJournal {
            run: run.into(),
            launched: 1,
            mtime_ms: parse_ts_ms(mtime),
            ..Default::default()
        };
        for (title, ids) in phases {
            let mut p = JournalPhase {
                title: title.to_string(),
                label_prefixes: vec![title.to_lowercase()],
                ..Default::default()
            };
            for (id, what) in *ids {
                p.started += 1;
                j.started += 1;
                p.agent_ids.push(id.to_string());
                match *what {
                    "ok" => {
                        p.results += 1;
                        j.results += 1;
                        p.result_ids.push(id.to_string())
                    }
                    "failed" => {
                        p.failed += 1;
                        j.failed += 1;
                        j.failed_ids.push(id.to_string())
                    }
                    _ => {}
                }
            }
            j.phases.push(p);
        }
        j
    }
    pub fn state(agents: Vec<Agent>, journals: Vec<WorkflowJournal>) -> State {
        let mut s = State::new(crate::metrics::Pricing::bundled());
        s.now_ms = parse_ts_ms(NOW).unwrap();
        s.clock_override = true;
        for a in agents {
            s.agents.insert(a.id.clone(), a);
        }
        s.workflow_journals = journals;
        s
    }
    pub fn launch(s: &mut State, run: &str, ts: &str) {
        s.tools
            .workflow_launches
            .push(crate::tools::WorkflowLaunch {
                tool_use_id: format!("tu{ts}"),
                task_id: None,
                run_id: Some(run.into()),
                name: Some("sweep".into()),
                at_ms: parse_ts_ms(ts),
            });
    }
    pub fn record(run: &str, status: &str) -> super::WorkflowRecord {
        super::WorkflowRecord {
            run: run.into(),
            name: Some("sweep".into()),
            status: Some(status.into()),
            start_ms: parse_ts_ms("2026-01-01T00:00:00Z"),
            duration_ms: Some(300_000),
            script_path: Some("/p/.claude/workflows/sweep.js".into()),
            pointers: [(
                "Verify".to_string(),
                Pointer {
                    line: 6,
                    call: Call::Pipeline,
                },
            )]
            .into_iter()
            .collect(),
            mtime_ms: None,
            scanned_with: vec![],
        }
    }
    /// Verify: v1–v3 429 on the first call, v4 returned through StructuredOutput;
    /// Design: d1 429 after one call. Completed, with a record.
    pub fn state_with_run() -> State {
        let t = "2026-01-01T00:01:00Z";
        let agents = vec![
            agent("v1", "wf_t", &[err(t, 429, "rate_limit")]),
            agent("v2", "wf_t", &[err(t, 429, "rate_limit")]),
            agent("v3", "wf_t", &[err(t, 429, "rate_limit")]),
            agent("v4", "wf_t", &[ok("m4", t, Some("StructuredOutput"))]),
            agent(
                "d1",
                "wf_t",
                &[
                    ok("m5", t, None),
                    err("2026-01-01T00:02:00Z", 429, "rate_limit"),
                ],
            ),
        ];
        let j = journal(
            "wf_t",
            &[
                (
                    "Verify",
                    &[
                        ("v1", "failed"),
                        ("v2", "failed"),
                        ("v3", "failed"),
                        ("v4", "ok"),
                    ],
                ),
                ("Design", &[("d1", "failed")]),
            ],
            "2026-01-01T00:05:00Z",
        );
        let mut s = state(agents, vec![j]);
        s.workflow_records = vec![record("wf_t", "completed")];
        s
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::metrics::cost::parse_ts_ms;
    use crate::ui::State;
    use crate::workflow_script::{Call, Pointer};

    #[test]
    fn a_record_keeps_identifiers_and_pointers_only() {
        let dir = std::env::temp_dir().join(format!("cctop-wf-record-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let wf = dir.join("workflows");
        std::fs::create_dir_all(&wf).unwrap();
        std::fs::write(wf.join("wf_t.json"), serde_json::json!({
            "runId": "wf_t", "workflowName": "sweep", "status": "completed", "startTime": 1000,
            "durationMs": 50, "scriptPath": "/x/sweep-2.js",
            "script": "phase('Verify')\nconst v = await pipeline(xs,\n c => agent(p, {label: `verify:${c}`, phase: 'Verify'}))\n",
            "result": "PROSE", "logs": ["PROSE"], "summary": "PROSE", "args": {"q": "PROSE"},
            "phases": [{"title": "Verify", "detail": "PROSE"}]
        }).to_string()).unwrap();
        let j = crate::agents::WorkflowJournal {
            run: "wf_t".into(),
            phases: vec![crate::agents::JournalPhase {
                title: "Verify".into(),
                label_prefixes: vec!["verify".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let records = read_records(&dir, &[j], &[]);
        let _ = std::fs::remove_dir_all(&dir);
        let r = &records[0];
        assert_eq!(r.name.as_deref(), Some("sweep"));
        assert_eq!(
            r.pointers["Verify"],
            Pointer {
                line: 2,
                call: Call::Pipeline
            }
        );
        assert!(!format!("{r:?}").contains("PROSE"));
    }

    #[test]
    fn a_record_is_rescanned_only_when_it_changes() {
        let dir = std::env::temp_dir().join(format!("cctop-wf-rescan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let wf = dir.join("workflows");
        std::fs::create_dir_all(wf.join("scripts")).unwrap();
        std::fs::write(wf.join("scripts").join("sweep-2.js"), "not a record").unwrap();
        std::fs::write(wf.join("wf_bad.json"), "{torn").unwrap();
        std::fs::write(wf.join("wf_t.json"), serde_json::json!({
            "runId": "wf_t", "workflowName": "sweep",
            "script": "phase('Verify')\nconst v = await pipeline(xs,\n c => agent(p, {label: `verify:${c}`, phase: 'Verify'}))\n"
        }).to_string()).unwrap();
        let journal = |prefix: &str| crate::agents::WorkflowJournal {
            run: "wf_t".into(),
            phases: vec![crate::agents::JournalPhase {
                title: "Verify".into(),
                label_prefixes: vec![prefix.into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let sentinel = Pointer {
            line: 999,
            call: Call::Parallel,
        };
        let mut first = read_records(&dir, &[journal("verify")], &[]);
        assert_eq!(
            first.len(),
            1,
            "only wf_*.json files that parse are records"
        );
        first[0].pointers.insert("Verify".into(), sentinel);
        let reused = read_records(&dir, &[journal("verify")], &first);
        let rescanned = read_records(&dir, &[journal("check")], &first);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            reused[0].pointers["Verify"], sentinel,
            "unchanged file and prefixes: reused"
        );
        assert_ne!(
            rescanned[0].pointers.get("Verify"),
            Some(&sentinel),
            "new prefix: re-scanned"
        );
    }

    fn v(s: &State) -> RunVerdict {
        let rows = crate::agent_ledger::rows(s, crate::agent_ledger::Sort::Waste, false);
        verdict(s, &rows, "wf_t")
    }

    #[test]
    fn first_call_and_mid_task_429s_are_told_apart() {
        let s = state_with_run();
        assert_eq!(
            cause(s.agents.get("v1"), true, false),
            Cause::RateLimitFirst
        );
        assert_eq!(cause(s.agents.get("d1"), false, false), Cause::RateLimitMid);
    }

    #[test]
    fn a_529_is_overloaded() {
        let a = agent(
            "o",
            "wf_t",
            &[err("2026-01-01T00:01:00Z", 529, "overloaded")],
        );
        assert_eq!(cause(Some(&a), false, false), Cause::Overloaded);
    }

    #[test]
    fn no_structured_output_needs_a_sibling_that_had_one() {
        let a = agent("n", "wf_t", &[ok("mn", "2026-01-01T00:01:00Z", None)]);
        assert_eq!(cause(Some(&a), false, false), Cause::Unknown);
        assert_eq!(cause(Some(&a), true, false), Cause::NoStructuredOutput);
    }

    #[test]
    fn a_journal_id_without_a_transcript_is_unknown_and_free() {
        // Review Focus 5
        let j = journal(
            "wf_t",
            &[("Verify", &[("ghost", "failed")])],
            "2026-01-01T00:05:00Z",
        );
        let s = state(vec![], vec![j]);
        let r = v(&s);
        assert_eq!(r.phases[0].causes.get(&Cause::Unknown), Some(&1));
        assert_eq!(r.failed, 1);
        assert_eq!(r.failed_usd, 0.0);
    }

    #[test]
    fn overhead_is_none_when_the_main_thread_spent_nothing() {
        // Review Focus 4: state_with_run has no main-transcript calls
        assert_eq!(v(&state_with_run()).overhead, None);
    }

    #[test]
    fn a_live_run_without_a_record_has_no_pointer() {
        // Review Focus 1
        let mut s = state_with_run();
        s.workflow_records.clear();
        s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
        let r = v(&s);
        assert_eq!(r.state, RunState::Live);
        assert!(r.fixes.iter().all(|f| f.pointer.is_none()));
    }

    #[test]
    fn a_quiet_run_without_an_end_is_stalled() {
        let mut s = state_with_run();
        s.workflow_records.clear();
        s.workflow_journals[0].mtime_ms = Some(s.now_ms - 120_000);
        assert_eq!(v(&s).state, RunState::Stalled);
    }

    #[test]
    fn prompt_too_long_is_context_overflow_and_other_tokens_are_unknown() {
        let t = "2026-01-01T00:01:00Z";
        let a = agent(
            "c",
            "wf_t",
            &[
                ok("mc", t, None),
                err("2026-01-01T00:02:00Z", 400, "prompt_too_long"),
            ],
        );
        assert_eq!(
            cause(Some(&a), true, true),
            Cause::ContextOverflow,
            "an API error wins over killed and schema"
        );
        let u = agent("u", "wf_t", &[err(t, 401, "authentication_failed")]);
        assert_eq!(cause(Some(&u), false, false), Cause::Unknown);
    }

    #[test]
    fn an_agent_cut_off_by_a_kill_is_killed_not_schema() {
        // Review Focus 7: x is open (no result, no failed entry) in a schema phase when the run is killed.
        let t = "2026-01-01T00:01:00Z";
        let x = agent("x", "wf_t", &[ok("mx", t, None)]);
        assert_eq!(cause(Some(&x), true, true), Cause::Killed);
        let v4 = agent("v4", "wf_t", &[ok("m4", t, Some("StructuredOutput"))]);
        let j = journal(
            "wf_t",
            &[("Verify", &[("v4", "ok"), ("x", "open")])],
            "2026-01-01T00:05:00Z",
        );
        let mut s = state(vec![v4, x], vec![j]);
        s.workflow_records = vec![record("wf_t", "killed")];
        let r = v(&s);
        assert_eq!(r.state, RunState::Killed);
        assert_eq!(r.phases[0].causes.get(&Cause::Killed), Some(&1));
        assert!(r.fixes.is_empty(), "Killed has no fix line");
    }

    #[test]
    fn a_resumed_run_is_live_again() {
        // Review Focus 6: the previous invocation's `completed` record ends at 00:05.
        let mut s = state_with_run();
        launch(&mut s, "wf_t", "2026-01-01T00:07:00Z");
        s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
        assert_eq!(
            v(&s).state,
            RunState::Live,
            "a later launch supersedes the record"
        );
        let mut s = state_with_run();
        s.agents.insert(
            "n1".into(),
            agent("n1", "wf_t", &[ok("mn1", "2026-01-01T00:09:55Z", None)]),
        );
        assert_eq!(
            v(&s).state,
            RunState::Live,
            "a member line past the marker supersedes it too"
        );
        let mut s = state_with_run();
        s.agents.insert(
            "n1".into(),
            agent("n1", "wf_t", &[ok("mn1", "2026-01-01T00:05:05Z", None)]),
        );
        assert_eq!(
            v(&s).state,
            RunState::Completed,
            "a line within the slack does not"
        );
        let mut s = state_with_run();
        s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
        assert_eq!(
            v(&s).state,
            RunState::Completed,
            "a fresh journal mtime alone (a copied session) does not"
        );
        let mut s = state_with_run();
        launch(&mut s, "wf_t", "2026-01-01T00:09:30Z");
        assert_eq!(
            v(&s).state,
            RunState::Live,
            "a just-resumed run is live before its journal moves"
        );
    }

    #[test]
    fn a_pointer_is_stale_when_its_failures_predate_the_last_script() {
        // The Verify failures started at 00:01; the last invocation began at 00:04.
        let mut s = state_with_run();
        s.workflow_records[0].start_ms = parse_ts_ms("2026-01-01T00:04:00Z");
        s.workflow_records[0].duration_ms = Some(60_000);
        let r = v(&s);
        assert!(r.phases[0].pointer_stale);
        assert!(r.fixes[0].pointer_stale);
        assert!(
            !r.phases[1].pointer_stale,
            "Design's failures predate the script too, but it has no pointer"
        );
        assert!(!r.fixes[1].pointer_stale);
        assert!(!v(&state_with_run()).fixes[0].pointer_stale);
    }

    #[test]
    fn agents_cut_off_by_a_kill_do_not_clear_a_stale_pointer() {
        // v1 failed under an earlier script (00:01); x was cut off by the kill
        // under the last one (started 00:04:30, the record began at 00:04).
        let v1 = agent(
            "v1",
            "wf_t",
            &[err("2026-01-01T00:01:00Z", 429, "rate_limit")],
        );
        let x = agent("x", "wf_t", &[ok("mx", "2026-01-01T00:04:30Z", None)]);
        let j = journal(
            "wf_t",
            &[("Verify", &[("v1", "failed"), ("x", "open")])],
            "2026-01-01T00:05:00Z",
        );
        let mut s = state(vec![v1, x], vec![j]);
        let mut rec = record("wf_t", "killed");
        rec.start_ms = parse_ts_ms("2026-01-01T00:04:00Z");
        rec.duration_ms = Some(60_000);
        s.workflow_records = vec![rec];
        let r = v(&s);
        assert_eq!(r.state, RunState::Killed);
        assert_eq!(r.phases[0].causes.get(&Cause::Killed), Some(&1));
        assert!(r.phases[0].pointer_stale);
        assert_eq!(r.fixes[0].cause, Cause::RateLimitFirst);
        assert!(r.fixes[0].pointer_stale);
    }

    /// A workflow run's notification with `status`, keyed `key`, at `ts`.
    fn notify(s: &mut State, key: &str, status: &str, ts: &str) {
        let text = format!("<task-notification><task-id>{key}</task-id><status>{status}</status><summary>s</summary><usage><agent_count>5</agent_count><agents_done>1</agents_done><agents_error>4</agents_error><agents_skipped>0</agents_skipped><agents_empty_result>0</agents_empty_result></usage></task-notification>");
        let n = crate::transcript::TaskNotification::parse(&text).unwrap();
        assert!(n.workflow.is_some());
        s.workflow_notifications.insert(key.into(), n);
        s.workflow_notified_at
            .insert(key.into(), parse_ts_ms(ts).unwrap());
    }

    #[test]
    fn a_failed_notification_ends_the_run_as_failed() {
        let mut s = state_with_run();
        s.workflow_records.clear();
        notify(&mut s, "wf_t", "failed", "2026-01-01T00:06:00Z");
        let r = v(&s);
        assert_eq!(r.state, RunState::Failed);
        assert_eq!(r.ended_ms, parse_ts_ms("2026-01-01T00:06:00Z"));
    }

    #[test]
    fn the_notification_speaks_for_its_own_invocation() {
        // The record ends at 00:05 `completed`.
        let mut s = state_with_run();
        notify(&mut s, "wf_t", "killed", "2026-01-01T00:05:01Z");
        assert_eq!(
            v(&s).state,
            RunState::Killed,
            "notification after the record, same invocation"
        );
        let mut s = state_with_run();
        notify(&mut s, "wf_t", "killed", "2026-01-01T00:04:00Z");
        assert_eq!(
            v(&s).state,
            RunState::Killed,
            "notification before the record, same invocation"
        );
        let mut s = state_with_run();
        notify(&mut s, "wf_t", "killed", "2026-01-01T00:03:00Z");
        launch(&mut s, "wf_t", "2026-01-01T00:03:30Z");
        let r = v(&s);
        assert_eq!(
            r.state,
            RunState::Completed,
            "a resume between them: the later record speaks"
        );
        assert_eq!(r.ended_ms, parse_ts_ms("2026-01-01T00:05:00Z"));
        let mut s = state_with_run();
        s.workflow_records[0].status = Some("killed".into());
        launch(&mut s, "wf_t", "2026-01-01T00:06:00Z");
        notify(&mut s, "wf_t", "completed", "2026-01-01T00:08:00Z");
        assert_eq!(
            v(&s).state,
            RunState::Completed,
            "a resume between them: the later notification speaks"
        );
    }

    #[test]
    fn a_notification_keyed_by_task_id_still_ends_the_run() {
        // The launch was not seen: the lone unkeyed notification is the run's.
        let mut s = state_with_run();
        s.workflow_records.clear();
        notify(&mut s, "wq12345", "completed", "2026-01-01T00:06:00Z");
        assert_eq!(v(&s).state, RunState::Completed);
    }

    #[test]
    fn a_later_resume_supersedes_the_notification() {
        let mut s = state_with_run();
        s.workflow_records.clear();
        notify(&mut s, "wf_t", "completed", "2026-01-01T00:04:00Z");
        launch(&mut s, "wf_t", "2026-01-01T00:09:30Z");
        let r = v(&s);
        assert_eq!(r.state, RunState::Live);
        assert_eq!(r.ended_ms, None);
    }

    #[test]
    fn a_tokenless_api_error_is_unknown_by_its_status() {
        let mut z = agent("z", "wf_t", &[ok("mz", "2026-01-01T00:01:00Z", None)]);
        z.api_error = Some(crate::agents::ApiError {
            status: Some(500),
            token: None,
        });
        assert_eq!(cause(Some(&z), true, true), Cause::Unknown);
        let j = journal(
            "wf_t",
            &[("Verify", &[("z", "failed")])],
            "2026-01-01T00:05:00Z",
        );
        let r = v(&state(vec![z], vec![j]));
        assert_eq!(
            r.fixes[0].text,
            "1 agent failed (500) — cctop has no fix for this yet"
        );
    }

    #[test]
    fn the_window_starts_at_the_first_invocation() {
        // The record describes the last invocation only (00:04 + 60 s); the members started at 00:01.
        let mut s = state_with_run();
        s.workflow_records[0].start_ms = parse_ts_ms("2026-01-01T00:04:00Z");
        s.workflow_records[0].duration_ms = Some(60_000);
        let r = v(&s);
        assert_eq!(r.started_ms, parse_ts_ms("2026-01-01T00:01:00Z"));
        assert_eq!(r.ended_ms, parse_ts_ms("2026-01-01T00:05:00Z"));
    }

    #[test]
    fn the_count_agrees_with_its_noun() {
        assert_eq!(
            fix_text(Cause::ContextOverflow, 1, None),
            "1 agent's input was too large — pass paths, not contents"
        );
        assert_eq!(
            fix_text(Cause::ContextOverflow, 3, None),
            "3 agents' input was too large — pass paths, not contents"
        );
        assert_eq!(
            fix_text(Cause::Unknown, 1, Some("authentication_failed")),
            "1 agent failed (authentication_failed) — cctop has no fix for this yet"
        );
    }

    #[test]
    fn fixes_are_the_top_two_causes_with_their_phase_pointer() {
        let r = v(&state_with_run());
        assert_eq!(r.state, RunState::Completed);
        assert_eq!(r.fixes.len(), 2);
        assert_eq!(r.fixes[0].cause, Cause::RateLimitFirst);
        assert_eq!(r.fixes[0].phase, "Verify");
        assert_eq!(
            r.fixes[0].pointer,
            Some(Pointer {
                line: 6,
                call: Call::Pipeline
            })
        );
        assert_eq!(r.fixes[0].text, "3 agents hit the rate limit on their first call — batch this phase's items, or lower its effort/model");
        assert_eq!(r.fixes[1].cause, Cause::RateLimitMid);
        assert_eq!(r.fixes[1].text, "1 agent hit the rate limit mid-task — batch this phase, then resume the run after the window resets", "singular");
        assert_eq!(
            r.fixes[1].pointer, None,
            "Design has no pointer in the record"
        );
    }

    #[test]
    fn waste_counts_the_failed_agents_money() {
        let r = v(&state_with_run());
        assert!(r.failed_usd > 0.0, "d1 made one priced call before its 429");
        assert!(r.waste_pct.unwrap() > 0.0 && r.waste_pct.unwrap() < 1.0);
    }

    #[test]
    fn wrap_breaks_a_word_wider_than_the_row_and_hangs_two_cells() {
        let rows = wrap(
            "→ an-extremely-long-script-file-name.js:12 parallel()",
            20,
            "   ",
        );
        assert!(rows.iter().all(|r| r.chars().count() <= 20), "{rows:#?}");
        assert!(
            rows[1..].iter().all(|r| r.starts_with("     ")),
            "{rows:#?}"
        );
        let flat: String = rows.iter().map(|r| r.trim()).collect();
        assert_eq!(flat, "→an-extremely-long-script-file-name.js:12parallel()");
    }

    #[test]
    fn the_strip_counts_other_live_runs_and_drops_a_long_stall() {
        let mut s = state_with_run();
        s.workflow_records.clear();
        s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
        let mut other = s.workflow_journals[0].clone();
        other.run = "wf_u".into();
        other.mtime_ms = Some(s.now_ms - 10 * 60_000); // stalled 10 min
        s.workflow_journals.push(other);
        let groups = |s: &State| {
            let rows = crate::agent_ledger::rows(s, crate::agent_ledger::Sort::Waste, false);
            crate::agent_ledger::workflow_groups(s, &rows)
        };
        let line = strip_line(&s, &groups(&s)).unwrap();
        assert!(
            line[1].text.ends_with("wf_t"),
            "the latest activity is shown"
        );
        assert_eq!(line[2].text, " +1 live");
        // Half an hour on, both runs have stalled past the drop.
        s.now_ms += STALLED_DROP_MS;
        assert_eq!(strip_line(&s, &groups(&s)), None);
    }

    #[test]
    fn a_long_phase_title_keeps_its_ten_cells_on_the_strip() {
        let mut s = state_with_run();
        s.workflow_records.clear();
        s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
        s.workflow_journals[0].phases[1].title = "Design the whole long pass".into(); // 25+ chars
        let rows = crate::agent_ledger::rows(&s, crate::agent_ledger::Sort::Waste, false);
        let groups = crate::agent_ledger::workflow_groups(&s, &rows);
        let line = crate::dashboard::cut(strip_line(&s, &groups).unwrap(), 52);
        let text = crate::dashboard::text_of(&line);
        assert!(text.contains("▸ Design th… 0/1 ✗1 429×1"), "{text}");
    }

    #[test]
    fn an_unpriced_run_shows_a_dash_not_zero_dollars() {
        let s = state_with_run();
        let rows = crate::agent_ledger::rows(&s, crate::agent_ledger::Sort::Waste, false);
        let mut g = crate::agent_ledger::workflow_groups(&s, &rows).remove(0);
        g.cost = crate::metrics::cost::Cost {
            usd: 0.0,
            approx: true,
            source: crate::metrics::cost::Source::Unpriced,
        };
        // The three surfaces share one helper.
        assert_eq!(run_usd(&g, "≈$"), "—");
        assert!(!group_text(&g).contains('$'), "{}", group_text(&g));
        assert!(strip_text(&g).contains(" — "), "{}", strip_text(&g));
        assert!(!strip_text(&g).contains("$0.00"));
        let head = &detail_lines(&g, DETAIL_WIDE)[0];
        assert!(
            head.contains("completed  5:00  —  ") && !head.contains('$'),
            "{head}"
        );
    }

    #[test]
    fn the_group_row_keeps_its_dollars_at_real_counts() {
        let s = state_with_run();
        let rows = crate::agent_ledger::rows(&s, crate::agent_ledger::Sort::Waste, false);
        let mut g = crate::agent_ledger::workflow_groups(&s, &rows).remove(0);
        g.run = "wf_0aa065ff".into();
        g.name = Some("cctop-research-sweep".into());
        g.launched = 300;
        g.done = 65;
        g.failed = 234;
        g.verdict.failed = 234;
        let row = group_text(&g);
        assert!(
            row.chars().count() <= crate::ui::agents_view::WIDTH,
            "{row}"
        );
        assert!(row.contains(" ✗234  ≈$"), "{row}");
        assert!(!row.contains("failed"), "✗n says it once: {row}");
        assert!(row.starts_with(" ✓ wf_0aa065ff cctop-resea…"), "{row}");
    }

    #[test]
    fn the_empty_result_count_sits_beside_the_phase_table() {
        let s = state_with_run();
        let rows = crate::agent_ledger::rows(&s, crate::agent_ledger::Sort::Waste, false);
        let mut g = crate::agent_ledger::workflow_groups(&s, &rows).remove(0);
        for w in [DETAIL_WIDE, DETAIL_NARROW] {
            assert!(!detail_lines(&g, w)[1].contains("empty"));
        }
        g.empty_result = Some(0);
        assert!(!detail_lines(&g, DETAIL_WIDE)[1].contains("empty"));
        g.empty_result = Some(3);
        for w in [DETAIL_WIDE, DETAIL_NARROW] {
            let head = &detail_lines(&g, w)[1];
            assert!(head.ends_with("cause  empty 3"), "{head}");
            assert!(head.chars().count() <= w, "{head}");
        }
    }

    #[test]
    fn a_phase_without_failures_costs_zero_not_minus_zero() {
        let mut s = state_with_run();
        s.workflow_journals[0]
            .phases
            .push(crate::agents::JournalPhase {
                title: "Merge".into(),
                started: 1,
                ..Default::default()
            });
        let rows = crate::agent_ledger::rows(&s, crate::agent_ledger::Sort::Waste, false);
        let g = crate::agent_ledger::workflow_groups(&s, &rows).remove(0);
        let merge = g
            .verdict
            .phases
            .iter()
            .find(|p| p.title == "Merge")
            .unwrap();
        assert!(merge.failed_usd.is_sign_positive(), "an empty sum is +0.0");
        let row = detail_lines(&g, DETAIL_WIDE)
            .into_iter()
            .find(|l| l.starts_with(" Merge"))
            .unwrap();
        assert!(row.contains(" 0.00 ") && !row.contains("-0.00"), "{row}");
    }
}
