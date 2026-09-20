//! Token-axis rules: what the context costs and why (A01–A04, A07, A08,
//! A11, A12, A14, A17, A25) and the Phase 5 set: A19 cache countdown, A21
//! switch window (warm) / A21b (cold), A23 the cost of continuing, A27 a
//! loop armed on a large context, A30 cold resume. A20 (the named cache
//! miss) is A01's trigger; A22 folded into it.

use super::{calls_per_turn, human_turns_since, model_short, recent_turns, usd_label, PriceKind};
use crate::advisor::{ActionKind, Advice, Rule, Saving, Urgency};
use crate::harness_facts::context_suggestions as ctx_rules;
use crate::metrics::context::Mode;
use crate::phase::ToolClass;
use crate::ui::fmt;
use crate::ui::State;

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(CacheMiss),
        Box::new(CacheExpiry),
        Box::new(RunawayResult),
        Box::new(Rereads),
        Box::new(IdleMcp),
        Box::new(ThinkingShare),
        Box::new(FreshInputSpike),
        Box::new(ChattyTurns),
        Box::new(SubagentModel),
        Box::new(AgentsWaste),
        Box::new(BigPrefix),
        Box::new(ExploreRun),
        Box::new(CacheCountdown),
        Box::new(SwitchWindow),
        Box::new(ColdSwitch),
        Box::new(ContextCost),
        Box::new(LoopArmed),
        Box::new(ColdResume),
    ]
}

/// Named cache misses become a nudge from this many re-written tokens.
pub const MISS_TOKENS: u64 = 20_000;

/// A01 — a named cache miss (`diagnostics.cache_miss_reason`, or the
/// status line's `prompt_cache.last_miss_cause`) that re-wrote ≥ 20 k
/// tokens within the last three human turns.
pub struct CacheMiss;
impl Rule for CacheMiss {
    fn id(&self) -> &'static str {
        "A01"
    }
    fn family(&self) -> &'static str {
        "cache-miss"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let miss = state
            .agg
            .cache_misses
            .iter()
            .rev()
            .find(|m| m.tokens >= MISS_TOKENS && human_turns_since(state, m.turn) < 3)?;
        let model = model_short(state);
        let price = usd_label(state, miss.tokens, PriceKind::CacheWrite);
        let (what, action, action_text) = match miss.kind.as_str() {
            "model_changed" => (
                format!("model → {model}"),
                "a switch re-writes all context — pick the model early, or switch right after /clear".to_string(),
                String::new(),
            ),
            "tools_changed" => {
                let old_cli = !crate::harness_facts::first_seen::RENDERED
                    .at_most(state.agg.version.as_deref());
                (
                    "tool list changed".to_string(),
                    if old_cli {
                        "an MCP connect or a ToolSearch load rebuilt the list; CLI < 2.1.267 rebuilds it often — `claude update`".to_string()
                    } else {
                        "an MCP server connected or ToolSearch loaded a deferred tool — load what you need early, then hold".to_string()
                    },
                    String::new(),
                )
            }
            _ => (
                "an earlier message changed".to_string(),
                "a rewind or an edited turn re-writes from that point — steer forward instead of rewriting".to_string(),
                String::new(),
            ),
        };
        let mut a = Advice::new("A01", "cache-miss", Urgency::Later);
        a.headline = format!(
            "cache miss: {what} · {} re-written{price}",
            fmt::tokens(miss.tokens)
        );
        a.evidence = format!(
            "{} · turn {} · {} named misses this session",
            miss.kind,
            miss.turn,
            state.agg.cache_misses.len()
        );
        a.action = action;
        a.action_text = action_text;
        a.saving = Saving::OneOff(miss.tokens);
        a.window_turns = 3;
        Some(a)
    }
}

/// A02 — a prompt gap longer than the cache TTL followed by a full-context
/// re-write, within the last three human turns (post-mortem; the
/// preventive countdown is A19).
pub struct CacheExpiry;
impl Rule for CacheExpiry {
    fn id(&self) -> &'static str {
        "A02"
    }
    fn family(&self) -> &'static str {
        "cache-expiry"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let ttl_ms = state.cache_ttl_ms();
        let turns: Vec<_> = state.agg.turns.iter().filter(|t| t.api_calls > 0).collect();
        let parse = crate::metrics::cost::parse_ts_ms;
        let mut cold: Option<(usize, i64, u64)> = None;
        for w in turns.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (Some(end), Some(start)) = (
                a.last_at.as_deref().and_then(parse),
                b.started_at.as_deref().and_then(parse),
            ) else {
                continue;
            };
            let gap = start - end;
            let write = b.usage.cache_write();
            if gap > ttl_ms && b.context_size > 0 && write as f64 >= b.context_size as f64 * 0.7 {
                cold = Some((b.number, gap, write));
            }
        }
        let (turn, gap, write) = cold?;
        if human_turns_since(state, turn) >= 3 {
            return None;
        }
        let ttl_label = if ttl_ms >= 3_600_000 { "1 h" } else { "5 min" };
        let mut a = Advice::new("A02", "cache-expiry", Urgency::Later);
        a.headline = format!(
            "Cache expired before turn {turn} · {} re-written{}",
            fmt::tokens(write),
            usd_label(state, write, PriceKind::CacheWrite)
        );
        a.evidence = format!("{} idle > {ttl_label} TTL", fmt::duration_ms(gap));
        a.action = format!(
            "batch your questions or answer within the TTL ({ttl_label} on this session); done? hand-off note + /clear"
        );
        a.saving = Saving::OneOff(write);
        a.window_turns = 3;
        Some(a)
    }
}

/// A03 — one tool type's kept results since the last boundary pass
/// `/context`'s own thresholds (≥ 15 % of the window and ≥ 10 k; Read
/// ≥ 5 %), or a single kept result ≥ 10 k tokens.
pub struct RunawayResult;
impl Rule for RunawayResult {
    fn id(&self) -> &'static str {
        "A03"
    }
    fn family(&self) -> &'static str {
        "runaway-result"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let since = state.since_boundary_turn();
        let window = state.context().window.max(1);
        let calls: Vec<&crate::tools::Call> = state
            .tools
            .calls
            .iter()
            .filter(|c| c.turn >= since && !c.cleared && c.result_tokens_est > 0)
            .collect();
        let mut by_tool: std::collections::BTreeMap<&str, u64> = Default::default();
        for c in &calls {
            *by_tool.entry(c.name.as_str()).or_default() += c.result_tokens_est;
        }
        let over = |name: &str, total: u64| {
            let share = if name == "Read" {
                ctx_rules::READ_WINDOW_SHARE
            } else {
                ctx_rules::TOOL_WINDOW_SHARE
            };
            total >= ctx_rules::TOOL_MIN_TOKENS && total as f64 >= window as f64 * share
        };
        let tool = by_tool
            .iter()
            .filter(|(n, t)| over(n, **t))
            .max_by_key(|(_, t)| **t)
            .map(|(n, t)| (n.to_string(), *t));
        let single = calls
            .iter()
            .filter(|c| c.result_tokens_est >= ctx_rules::TOOL_MIN_TOKENS)
            .max_by_key(|c| c.result_tokens_est)
            .copied();
        let name = match (&tool, single) {
            (Some((n, _)), _) => n.clone(),
            (None, Some(c)) => c.name.clone(),
            _ => return None,
        };
        let total = by_tool.get(name.as_str()).copied().unwrap_or(0);
        // The biggest kept result of that tool, for the headline.
        let biggest = calls
            .iter()
            .filter(|c| c.name == name)
            .max_by_key(|c| c.result_tokens_est)?;
        let produced = biggest
            .persisted_output_size
            .map(|b| {
                format!(
                    " (produced {}, {} kept)",
                    fmt::bytes(b),
                    fmt::tokens(biggest.result_tokens_est)
                )
            })
            .unwrap_or_default();
        let tax = state
            .reread_tax(biggest)
            .filter(|(n, _)| *n > 0)
            .map(|(n, usd)| format!(" · re-read {n}× ≈{}", fmt::usd(usd)))
            .unwrap_or_default();
        let mut a = Advice::new("A03", "runaway-result", Urgency::Later);
        let single_line = format!(
            "`{} {}` pushed {} tokens into context{produced}",
            biggest.name,
            fmt::clip(&biggest.input_summary, 24),
            fmt::tokens(biggest.result_tokens_est)
        );
        let type_line = format!(
            "{name} results: {} since the last boundary ({:.0} % of the window)",
            fmt::tokens(total),
            total as f64 / window as f64 * 100.0
        );
        // The trigger leads: the tool type when its sum crossed `/context`'s
        // bar, else the one big result.
        if tool.is_some() {
            a.headline = type_line;
            a.evidence = format!("biggest: {single_line}{tax}");
        } else {
            a.headline = single_line;
            a.evidence = format!("{type_line}{tax}");
        }
        a.action = "pipe through head/grep or Glob, or ask a subagent for a summary — every later call re-reads it".into();
        a.saving = Saving::Tokens(total);
        a.window_turns = state.agg.human_turns().saturating_sub(since).max(1);
        a.retires_on = "the next boundary";
        Some(a)
    }
}

/// A04 — edit → re-read → edit churn on one file, or a file read ≥ 3×
/// whole with no edit in between (Bash `cat`/`sed -n`/`head`/`tail`
/// count; ranged and unchanged reads do not).
pub struct Rereads;
impl Rule for Rereads {
    fn id(&self) -> &'static str {
        "A04"
    }
    fn family(&self) -> &'static str {
        "reread"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let short = |p: &str| {
            std::path::Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string())
        };
        let per_read = |path: &str| -> u64 {
            let tail = short(path);
            state
                .tools
                .calls
                .iter()
                .filter(|c| c.name == "Read" && c.paths.contains(&tail))
                .map(|c| c.result_tokens_est)
                .max()
                .unwrap_or(1_000)
        };
        let mut a = Advice::new("A04", "reread", Urgency::Later);
        if let Some(f) = state
            .files
            .files
            .values()
            .filter(|f| f.edit_reread_edit >= 2)
            .max_by_key(|f| f.edit_reread_edit)
        {
            let tokens = per_read(&f.path);
            a.headline = format!(
                "{}: edit → re-read → edit ×{}",
                short(&f.path),
                f.edit_reread_edit
            );
            a.evidence = format!(
                "each re-read re-injects ~{} tokens the Edit result already showed",
                fmt::tokens(tokens)
            );
            a.action =
                "queue: 'after an Edit, trust the result — don't re-read before the next edit'"
                    .into();
            a.action_text =
                "After an Edit, trust the tool result — don't re-read the file before the next edit."
                    .into();
            a.action_kind = ActionKind::Prompt;
            a.saving = Saving::Tokens(tokens);
            return Some(a);
        }
        let f = state
            .files
            .files
            .values()
            .filter(|f| f.reread_warning())
            .max_by_key(|f| f.reads_since_edit)?;
        let tokens = per_read(&f.path);
        a.headline = format!(
            "{} was read {}× with no edit in between",
            short(&f.path),
            f.reads_since_edit
        );
        a.evidence = format!(
            "each read re-injects ~{} tokens{}",
            fmt::tokens(tokens),
            if f.bash_reads > 0 {
                format!(" · {} via Bash", f.bash_reads)
            } else {
                String::new()
            }
        );
        a.action = "read once with an offset/limit, or ask Claude to keep the relevant lines in its summary".into();
        a.saving = Saving::Tokens(tokens);
        Some(a)
    }
}

/// A25 — the current read-only run (Read / Grep / Glob / WebFetch and the
/// Bash allowlist) grew past 8 calls and 20 k tokens (or 6 and 25 k);
/// retires when the run ends or an Agent spawns.
pub struct ExploreRun;
impl Rule for ExploreRun {
    fn id(&self) -> &'static str {
        "A25"
    }
    fn family(&self) -> &'static str {
        "explore-delegate"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let (n, tokens) = state.tools.explore_run();
        if !((n >= 8 && tokens >= 20_000) || (n >= 6 && tokens >= 25_000)) {
            return None;
        }
        let mut a = Advice::new("A25", "explore-delegate", Urgency::Next);
        a.headline = format!("EXPLORING ×{n} · +{} ctx this run", fmt::tokens(tokens));
        a.evidence = format!(
            "{} read-only calls in a row in the main context; the files ride on every later request",
            n
        );
        a.action = "queue: 'use an Explore subagent for the rest'".into();
        a.action_text =
            "Use an Explore subagent for the rest of this search and give me a short summary."
                .into();
        a.action_kind = ActionKind::Prompt;
        a.saving = Saving::Tokens(tokens);
        a.retires_on = "the run ending or an Agent spawn";
        a.mark = agent_calls(state);
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        agent_calls(state) > fired.mark
    }
}

fn agent_calls(state: &State) -> u64 {
    state
        .tools
        .calls
        .iter()
        .filter(|c| c.name == "Agent")
        .count() as u64
}

/// A07 — prefix weight nobody uses: an MCP server idle > 20 min whose
/// schemas cost > 2 k tokens (plus its ToolSearch rebuilds), or an enabled
/// plugin with > 2 k always-on tokens unused for ≥ 10 startups.
pub struct IdleMcp;
impl Rule for IdleMcp {
    fn id(&self) -> &'static str {
        "A07"
    }
    fn family(&self) -> &'static str {
        "idle-mcp"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let now = state.clock_ms();
        let stats = state.tools.by_name();
        let session_start = state
            .agg
            .turns
            .first()
            .and_then(|t| t.started_at.as_deref())
            .and_then(crate::metrics::cost::parse_ts_ms)
            .unwrap_or(now);
        let mcp = state
            .prefix
            .mcp_schema_tokens()
            .into_iter()
            .filter(|(_, (tokens, _))| *tokens > 2_000)
            .map(|(server, sizes)| {
                let last = stats
                    .get(&server)
                    .and_then(|t| t.last_call_at)
                    .unwrap_or(session_start);
                (server, sizes, now - last)
            })
            .filter(|(_, _, idle)| *idle > 20 * 60 * 1000)
            .max_by_key(|(_, (tokens, _), _)| *tokens);
        let plugin = state
            .prefix
            .plugins()
            .iter()
            .filter(|(_, tokens, unused)| *tokens > 2_000 && unused.is_some_and(|n| n >= 10))
            .max_by_key(|(_, tokens, _)| *tokens)
            .cloned();
        let mut a = Advice::new("A07", "idle-mcp", Urgency::Later);
        match (mcp, plugin) {
            (Some((server, (tokens, count), idle_ms)), p)
                if p.as_ref().is_none_or(|(_, t, _)| *t <= tokens) =>
            {
                let loads = state
                    .tools
                    .tool_search_loads
                    .get(server.trim_start_matches("mcp:"))
                    .copied()
                    .unwrap_or(0);
                a.headline = format!("{server} idle for {}", fmt::duration_ms(idle_ms));
                a.evidence = format!(
                    "{count} tool schemas ≈ {} tokens in every request{}",
                    fmt::tokens(tokens),
                    if loads > 0 {
                        format!(" · {loads} ToolSearch loads re-wrote the prefix")
                    } else {
                        String::new()
                    }
                );
                a.action =
                    "disable it for this project (.mcp.json) or rely on deferred tool loading"
                        .into();
                a.action_kind = ActionKind::Setting;
                a.saving = Saving::Tokens(tokens);
            }
            (_, Some((name, tokens, unused))) => {
                a.headline = format!(
                    "plugin {name}: {} always-on tokens, unused for {} startups",
                    fmt::tokens(tokens),
                    unused.unwrap_or(0)
                );
                a.evidence = "its listing rides along in every request; Claude Code counts plugin use per startup".into();
                a.action = format!("/plugin — disable {name} for this project");
                a.action_text = "/plugin".into();
                a.action_kind = ActionKind::Slash;
                a.saving = Saving::Tokens(tokens);
            }
            _ => return None,
        }
        Some(a)
    }
}

const EFFORT_RANK: &[(&str, u64)] = &[
    ("low", 1),
    ("medium", 2),
    ("high", 3),
    ("xhigh", 4),
    ("max", 5),
];

fn effort_rank(level: &str) -> u64 {
    EFFORT_RANK
        .iter()
        .find(|(l, _)| *l == level)
        .map(|(_, r)| *r)
        .unwrap_or(3)
}

fn current_effort(state: &State) -> Option<String> {
    recent_turns(state, 1)
        .first()
        .and_then(|t| t.effort.clone())
        .or_else(|| state.status_facts.effort_level.clone())
}

/// A08 — thinking > 40 % of output over five edit-heavy turns at an effort
/// above medium.
pub struct ThinkingShare;
impl Rule for ThinkingShare {
    fn id(&self) -> &'static str {
        "A08"
    }
    fn family(&self) -> &'static str {
        "thinking"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        if state.status_facts.thinking_enabled == Some(false) {
            return None;
        }
        let recent = recent_turns(state, 5);
        if recent.len() < 5 {
            return None;
        }
        let effort = current_effort(state).unwrap_or_else(|| "high".into());
        if effort_rank(&effort) <= 2 {
            return None;
        }
        let out: u64 = recent.iter().map(|t| t.usage.output).sum();
        let think: u64 = recent.iter().map(|t| t.usage.thinking).sum();
        if out == 0 || (think as f64) / (out as f64) <= 0.4 {
            return None;
        }
        // Routine = edit-heavy: half the tool calls of these turns implement.
        let first = recent.iter().map(|t| t.number).min().unwrap_or(0);
        let calls: Vec<_> = state
            .tools
            .calls
            .iter()
            .filter(|c| c.turn >= first)
            .collect();
        let edits = calls
            .iter()
            .filter(|c| c.class == ToolClass::Implement)
            .count();
        if edits < 3 || edits * 2 < calls.len() {
            return None;
        }
        let model = state.model().unwrap_or("");
        let idx = crate::harness_facts::effort_cost_index(model, &effort);
        let idx_medium = crate::harness_facts::effort_cost_index(model, "medium");
        let share = match (idx, idx_medium) {
            (Some(i), Some(m)) if i > 0.0 => (i - m).max(0.0) / i,
            _ => 0.3,
        };
        let per_turn = (out as f64 / 5.0 * share) as u64;
        let mut a = Advice::new("A08", "thinking", Urgency::Later);
        a.headline = format!(
            "{:.0} % of output is thinking on edit-heavy turns (effort {effort})",
            think as f64 / out as f64 * 100.0
        );
        a.evidence = format!(
            "{} thinking tokens over the last 5 turns · {edits} edits in {} calls",
            fmt::tokens(think),
            calls.len()
        );
        a.action = if model.contains("fable") {
            "/effort medium for routine edits; raise it again for design and debugging".into()
        } else {
            "/effort medium at the next /clear (on this model the level lives in the system prompt: a mid-session switch re-writes the cache)".into()
        };
        a.action_text = "/effort medium".into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::Tokens(per_turn.max(1));
        a.window_turns = 5;
        a.mark = effort_rank(&effort);
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        current_effort(state).is_some_and(|e| effort_rank(&e) < fired.mark)
    }
}

/// A11 — a paste of ≥ 20 k characters or ≥ 200 lines in history.jsonl
/// matched to one of the last three human turns (a queued steer's paste
/// lands inside the turn that absorbed it), or a turn whose first API call
/// carried > 5 k uncached `input_tokens` (sessions without a prompt cache).
/// The paste's size comes from history.jsonl — inline characters, or the
/// composer's `+N lines` placeholder for a hashed paste — never its text.
/// On a cached session the paste is not `input_tokens` at all: Claude Code
/// writes the new prompt into the cache, so it lands in the first call's
/// `cache_creation_input_tokens` with the previous turn's tail, which is
/// why the size is read from the history and not from the usage.
pub struct FreshInputSpike;

/// A paste is billed at ~one token per four characters; when only its
/// line count is known, ~25 tokens a line (code and logs sit at 10–50).
const PASTE_CHARS_PER_TOKEN: usize = 4;
const PASTE_TOKENS_PER_LINE: usize = 25;
/// A paste this large is a fresh-input spike on its own (≈ 5 k tokens).
const PASTE_SPIKE_CHARS: usize = 5_000 * PASTE_CHARS_PER_TOKEN;
const PASTE_SPIKE_LINES: usize = 5_000 / PASTE_TOKENS_PER_LINE;

/// The paste that came with turn `i`: submitted from two seconds before the
/// turn's first line up to the same margin before the next turn's (a queued
/// steer absorbed mid-turn sits inside that span).
fn paste_for(state: &State, i: usize) -> Option<&crate::history::Paste> {
    let turns = &state.agg.turns;
    let start = |t: &crate::metrics::Turn| {
        t.started_at
            .as_deref()
            .and_then(crate::metrics::cost::parse_ts_ms)
    };
    let from = start(turns.get(i)?)? - 2_000;
    let to = turns
        .get(i + 1)
        .and_then(start)
        .map(|t| t - 2_000)
        .unwrap_or(i64::MAX);
    state.history.paste_between(from, to)
}

/// A paste's tokens: exact-ish from its characters, `≈` from its lines.
fn paste_tokens(p: &crate::history::Paste) -> (u64, bool) {
    if p.chars > 0 {
        ((p.chars / PASTE_CHARS_PER_TOKEN) as u64, false)
    } else {
        ((p.lines * PASTE_TOKENS_PER_LINE) as u64, true)
    }
}

impl Rule for FreshInputSpike {
    fn id(&self) -> &'static str {
        "A11"
    }
    fn family(&self) -> &'static str {
        "fresh-input"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let (t, fresh, approx, paste) = state
            .agg
            .turns
            .iter()
            .enumerate()
            .filter(|(_, t)| t.human && human_turns_since(state, t.number) < 3)
            .filter_map(|(i, t)| {
                let paste = paste_for(state, i);
                let (pasted, approx) = paste.map_or((0, false), paste_tokens);
                let spike = t.first_call_input > 5_000
                    || paste.is_some_and(|p| {
                        p.chars >= PASTE_SPIKE_CHARS
                            || (p.chars == 0 && p.lines >= PASTE_SPIKE_LINES)
                    });
                let fresh = t.first_call_input.max(pasted);
                spike.then_some((t, fresh, approx && pasted > t.first_call_input, paste))
            })
            .max_by_key(|(_, fresh, _, _)| *fresh)?;
        let mut a = Advice::new("A11", "fresh-input", Urgency::Later);
        a.headline = match paste {
            Some(p) => format!(
                "Turn {} pasted {}{} tokens ({} lines){}",
                t.number,
                if approx { "≈" } else { "~" },
                fmt::tokens(fresh),
                p.lines,
                usd_label(state, fresh, PriceKind::Input)
            ),
            None => format!(
                "Turn {} sent ~{} tokens of uncached input{}",
                t.number,
                fmt::tokens(fresh),
                usd_label(state, fresh, PriceKind::Input)
            ),
        };
        a.evidence = match paste {
            Some(p) if p.chars > 0 => format!(
                "a {}-line paste ({} chars) was written to the cache on the turn's first call and is re-read by every later one",
                p.lines,
                fmt::tokens(p.chars as u64)
            ),
            Some(_) => "the paste (sized from its +N lines placeholder) was written to the cache on the turn's first call and is re-read by every later one".into(),
            None => format!(
                "the prompt itself (a paste, {} images) was billed fresh on the turn's first call and stays in context",
                t.prompt_images
            ),
        };
        a.action = "for files, give the path and let Claude read the relevant range; for logs, paste the last 100 lines".into();
        a.saving = Saving::OneOff(fresh.saturating_sub(1_000));
        a.window_turns = 3;
        Some(a)
    }
}

/// A12 — median prompt < 32 chars and > 20 human turns in the last hour.
pub struct ChattyTurns;
impl Rule for ChattyTurns {
    fn id(&self) -> &'static str {
        "A12"
    }
    fn family(&self) -> &'static str {
        "chatty"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let now = state.clock_ms();
        let parse = crate::metrics::cost::parse_ts_ms;
        let recent: Vec<_> = state
            .agg
            .turns
            .iter()
            .filter(|t| {
                t.human
                    && t.started_at
                        .as_deref()
                        .and_then(parse)
                        .is_some_and(|s| now - s <= 3_600_000)
            })
            .collect();
        if recent.len() <= 20 {
            return None;
        }
        let mut lens: Vec<usize> = recent.iter().map(|t| t.prompt_chars).collect();
        lens.sort_unstable();
        let median = lens[lens.len() / 2];
        if median >= 32 {
            return None;
        }
        let ctx = state.context().size;
        let mut a = Advice::new("A12", "chatty", Urgency::Later);
        a.headline = format!(
            "{} prompts in the last hour, median {median} chars",
            recent.len()
        );
        a.evidence = format!(
            "each turn re-sends the full context ({}{})",
            fmt::tokens(ctx),
            usd_label(state, ctx, PriceKind::CacheRead)
        );
        a.action =
            "batch related instructions into one prompt; queue follow-ups while a turn runs".into();
        a.saving = Saving::Tokens(ctx / 10);
        a.window_turns = recent.len();
        Some(a)
    }
}

/// A14 — an Opus subagent that only searched: ≥ 5 search-shaped calls and
/// no edit.
pub struct SubagentModel;
impl Rule for SubagentModel {
    fn id(&self) -> &'static str {
        "A14"
    }
    fn family(&self) -> &'static str {
        "subagent-model"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let ag = state
            .agents
            .values()
            .filter(|a| a.model.contains("opus") && a.edits() == 0 && a.search_calls() >= 5)
            .max_by_key(|a| a.usage.total())?;
        let pricing = state.cost.pricing();
        let opus = pricing.estimate(&ag.usage, &ag.model).unwrap_or(0.0);
        let sonnet = pricing
            .estimate(&ag.usage, "claude-sonnet-5")
            .unwrap_or(0.0);
        // At $5/MTok input-equivalent.
        let saved_tokens = ((opus - sonnet).max(0.0) / 5.0 * 1e6) as u64;
        let mut a = Advice::new("A14", "subagent-model", Urgency::Later);
        a.headline = format!(
            "{} agent {} ran on Opus: {} searches, 0 edits",
            ag.agent_type,
            fmt::clip(&ag.id, 8),
            ag.search_calls()
        );
        a.evidence = format!(
            "{} tokens ≈{}; the same search on Sonnet ≈{}",
            fmt::tokens(ag.usage.total()),
            fmt::usd(opus),
            fmt::usd(sonnet)
        );
        a.action = "give search and summary subagents model: sonnet (or haiku) in the Agent call or the agent's frontmatter".into();
        a.action_text = "Run search and summary subagents on sonnet (model: \"sonnet\"), keep opus for the main thread.".into();
        a.action_kind = ActionKind::Prompt;
        a.saving = Saving::Tokens(saved_tokens.max(1));
        Some(a)
    }
}

/// A48 — money that went to agents whose work did not come back: the
/// agents ledger's waste is ≥ $1 and ≥ 25 % of the agents' spend, over at
/// least two classified agents (agent PRD US-006). Structural evidence
/// only: the task notifications' statuses and result lengths, the
/// workflow journal, the agents' states and the hook spool.
pub struct AgentsWaste;

/// The waste share the rule fires above, and the one it counts as acted
/// below.
const WASTE_FIRE_RATIO: f64 = 0.25;
const WASTE_ACTED_RATIO: f64 = 0.10;
const WASTE_FIRE_USD: f64 = 1.0;

fn waste_figures(
    state: &State,
) -> (
    crate::agent_ledger::Totals,
    Vec<crate::agent_ledger::AgentRow>,
) {
    let rows = crate::agent_ledger::rows(state, crate::agent_ledger::Sort::Waste, false);
    let totals = crate::agent_ledger::totals(&rows);
    (totals, rows)
}

fn waste_ratio(t: &crate::agent_ledger::Totals) -> f64 {
    if t.cost.usd > 0.0 {
        t.waste_usd / t.cost.usd
    } else {
        0.0
    }
}

impl Rule for AgentsWaste {
    fn id(&self) -> &'static str {
        "A48"
    }
    fn family(&self) -> &'static str {
        "agents-waste"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let (t, rows) = waste_figures(state);
        if t.classified < 2
            || t.waste_usd < WASTE_FIRE_USD
            || t.waste_usd < WASTE_FIRE_RATIO * t.cost.usd
        {
            return None;
        }
        // The reason that cost the most, and how many agents share it.
        let (top, top_usd) = crate::agent_ledger::WasteReason::ALL
            .iter()
            .zip(t.waste_by_reason.iter())
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(r, usd)| (*r, *usd))?;
        let top_n = rows
            .iter()
            .filter(|r| r.waste.is_some_and(|w| w.reason == top))
            .count();
        let mut a = Advice::new("A48", "agents-waste", Urgency::Later);
        a.headline = format!(
            "agents wasted ≈{} of ≈{}: {} {} ≈{}",
            fmt::usd(t.waste_usd),
            fmt::usd(t.cost.usd),
            top_n,
            top.label(),
            fmt::usd(top_usd)
        );
        a.evidence = format!(
            "{} of {} agents classified · {:.0} % of agent spend",
            t.classified,
            t.agents,
            waste_ratio(&t) * 100.0
        );
        a.action = "open the agents view (6, Enter) and see which launches did not pay off before launching more".into();
        a.action_text = "6 Enter".into();
        a.action_kind = ActionKind::Key;
        // The tokens the wasted agents spent: what not launching them
        // would have kept.
        let wasted_tokens: u64 = rows
            .iter()
            .filter(|r| r.waste.is_some())
            .map(|r| r.tokens)
            .sum();
        a.saving = Saving::OneOff(wasted_tokens.max(1));
        a.retires_on = "the view opened or the waste share under 10 %";
        a.mark = state.agents_view_opens;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state.agents_view_opens > fired.mark
            || waste_ratio(&waste_figures(state).0) < WASTE_ACTED_RATIO
    }
    fn cooldown_turns(&self) -> usize {
        10
    }
}

/// A17 — the fixed prefix costs ≥ $0.25 per turn at the cache-read price,
/// or, once a `/context` has measured it, is ≥ 50 k tokens. The prefix is
/// paid on every request, so its cost is absolute, not a share of the window
/// (context-residency PRD, decision 9: a share-of-window rule is inert on a
/// 1 m window, a share-of-size rule fires on half of all sessions). The
/// figure is the residency model's — the `/context` table's when one ran,
/// lowered at a boundary that landed below the first call's. The token arm
/// waits for that calibration (PRD §10.6): the raw first call overstates the
/// prefix by a third and runs 47–62 k on a plugin-heavy setup, so an
/// `Estimated` figure over 50 k gets the sources inspector's own invitation
/// to run `/context`, not a nudge. The price arm reads either figure. Acted
/// on when the person opens either inspector (`i`, `m`).
pub struct BigPrefix;

/// The prefix size A17 fires at when the price arm does not — compared
/// only against a calibrated prefix (`Mode::Calibrated`, a `/context` ran).
pub const PREFIX_TOKENS: u64 = 50_000;

impl Rule for BigPrefix {
    fn id(&self) -> &'static str {
        "A17"
    }
    fn family(&self) -> &'static str {
        "prefix-tip"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let r = state.residency();
        let prefix = r.prefix;
        if prefix == 0 {
            return None;
        }
        let cpt = calls_per_turn(state);
        let per_turn_tokens = (prefix as f64 * cpt) as u64;
        let per_turn_usd = super::usd(state, per_turn_tokens, PriceKind::CacheRead);
        let calibrated = matches!(r.mode, Mode::Calibrated { .. });
        if per_turn_usd.is_none_or(|u| u < 0.25) && !(calibrated && prefix >= PREFIX_TOKENS) {
            return None;
        }
        let rows = state.prefix.rows(prefix);
        let biggest = rows
            .iter()
            .find(|r| r.kind != crate::prefix::Kind::Other)
            .map(|r| format!("largest: {} ({})", r.name, fmt::tokens(r.tokens_est)))
            .unwrap_or_else(|| "press i on Context for the breakdown".into());
        let captured = state
            .prefix
            .context_capture
            .as_ref()
            .map(|c| format!(" · /context: {} used", fmt::tokens(c.used_tokens)))
            .unwrap_or_default();
        let mut a = Advice::new("A17", "prefix-tip", Urgency::Later);
        a.headline = format!(
            "Fixed prefix {} tokens ≈{}/turn at {cpt:.0} calls",
            fmt::tokens(prefix),
            per_turn_usd.map(fmt::usd).unwrap_or_else(|| "?".into())
        );
        a.evidence = format!("{biggest}{captured}");
        a.action =
            "trim CLAUDE.md, move rarely-used rules to skills, disable unused MCP servers and plugins — i on Context shows what the prefix is, m what else fills the window"
                .into();
        a.action_kind = ActionKind::Setting;
        // Assumes a fifth of the prefix is trimmable.
        a.saving = Saving::Tokens(per_turn_tokens / 5);
        a.retires_on = "an inspector opened (i or m on Context)";
        a.mark = state.inspector_opens;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state.inspector_opens > fired.mark
    }
    /// Once per session (PRD §4.5): the prefix moves within a session, but
    /// the lever — which servers and files are always on — does not.
    fn cooldown_turns(&self) -> usize {
        usize::MAX / 2
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{prompt, response, tool};
    use super::*;
    use crate::metrics::Pricing;
    use crate::transcript::Line;
    use crate::ui::state::tests_support::fixture_state;

    #[test]
    fn a01_named_cache_miss_within_three_turns() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"c","model":"claude-opus-5","content":[],"usage":{"input_tokens":2,"cache_creation_input_tokens":427000},"diagnostics":{"cache_miss_reason":{"type":"model_changed","cache_missed_input_tokens":427000}}}}"#).unwrap());
        let a = CacheMiss.evaluate(&s).expect("fires");
        assert!(
            a.headline
                .starts_with("cache miss: model → opus-5 · 427k re-written ≈$"),
            "{}",
            a.headline
        );
        assert_eq!(a.saving, Saving::OneOff(427_000));
        assert_eq!(a.urgency, Urgency::Later);
        // Three human turns later it is history.
        for i in 1..=3 {
            s.apply(&prompt(&format!("2026-01-01T00:0{i}:00Z")));
            s.apply(&response(
                &format!("m{i}"),
                &format!("2026-01-01T00:0{i}:01Z"),
                10,
                0,
                100_000,
            ));
        }
        assert!(CacheMiss.evaluate(&s).is_none());
        // A small miss (< 20 k) is an Events row only; the old byte heuristic is gone.
        let mut small = State::new(Pricing::bundled());
        small.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"c","model":"claude-opus-5","content":[],"usage":{"input_tokens":2,"cache_creation_input_tokens":5000},"diagnostics":{"cache_miss_reason":{"type":"tools_changed"}}}}"#).unwrap());
        assert!(CacheMiss.evaluate(&small).is_none());
        for i in 0..5 {
            small.apply(&prompt("2026-01-01T00:00:00Z"));
            small.apply(&response(
                &format!("w{i}"),
                "2026-01-01T00:00:01Z",
                100,
                50_000,
                10_000,
            ));
        }
        assert!(
            CacheMiss.evaluate(&small).is_none(),
            "cache writes alone never fire A01 now"
        );
        assert!(CacheMiss.evaluate(&fixture_state()).is_none());
    }

    /// What `/context` printed, as Claude Code records it: a `local_command`
    /// stdout with one `Name: Nk tokens` line per category.
    fn context_table(ts: &str, categories: &[(&str, &str)]) -> Line {
        let rows: String = categories
            .iter()
            .map(|(name, tokens)| format!("\\n  {name}: {tokens} tokens (1.0%)"))
            .collect();
        Line::parse(&format!(
            r#"{{"type":"system","subtype":"local_command","timestamp":"{ts}","content":"<local-command-stdout> Context Usage\n  claude-opus-5\n  88.7k/1m tokens (9%){rows}\n</local-command-stdout>"}}"#
        ))
        .unwrap()
    }

    /// Two turns on a raw first-call prefix of 60 k.
    fn sixty_k() -> State {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&response("m1", "2026-01-01T00:00:05Z", 10, 60_000, 0));
        s.apply(&prompt("2026-01-01T00:01:00Z"));
        s.apply(&response("m2", "2026-01-01T00:01:05Z", 10, 0, 60_010));
        s
    }

    #[test]
    fn a17_fires_from_a_calibrated_50k_prefix_and_retires_when_an_inspector_opens() {
        // Decision 9 (context-residency PRD): the prefix is paid on every
        // request, so the rule fires on an absolute size, below the price
        // arm — but only on the figure a /context measured (§10.6): the raw
        // first call overstates it by a third.
        let mut s = sixty_k();
        s.apply(&context_table(
            "2026-01-01T00:01:10Z",
            &[
                ("System prompt", "10k"),
                ("System tools", "45k"),
                ("Messages", "5k"),
            ],
        ));
        assert!(matches!(s.residency().mode, Mode::Calibrated { .. }));
        let a = BigPrefix
            .evaluate(&s)
            .expect("a calibrated 55k prefix fires");
        assert!(
            a.headline.starts_with("Fixed prefix 55k tokens"),
            "{}",
            a.headline
        );
        assert!(a.evidence.ends_with("/context: 88k used"), "{}", a.evidence);
        assert_eq!(a.retires_on, "an inspector opened (i or m on Context)");
        assert!(!BigPrefix.acted(&s, &a));
        s.inspector_opens += 1;
        assert!(BigPrefix.acted(&s, &a), "opening i or m is the act");
        // The same raw 60k, calibrated to fixture D's 46.2k: under the
        // threshold, quiet.
        let mut d = sixty_k();
        d.apply(&context_table(
            "2026-01-01T00:01:10Z",
            &[
                ("System prompt", "10.2k"),
                ("System tools", "31.2k"),
                ("Skills", "4.8k"),
                ("Messages", "42.9k"),
            ],
        ));
        assert_eq!(d.residency().prefix, 46_200);
        assert!(BigPrefix.evaluate(&d).is_none());
        // No /context: the raw 60k is an estimate, and the token arm waits.
        // The sources inspector's footer invites the /context instead.
        let e = sixty_k();
        assert_eq!(e.residency().mode, Mode::Estimated);
        assert_eq!(e.residency().prefix, 60_000);
        assert!(BigPrefix.evaluate(&e).is_none());
    }

    #[test]
    fn a02_cache_expiry_is_a_three_turn_post_mortem() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&response("m1", "2026-01-01T00:00:05Z", 10, 100_000, 0));
        // 12 minutes later (TTL 5 min): full re-write.
        s.apply(&prompt("2026-01-01T00:12:00Z"));
        s.apply(&response("m2", "2026-01-01T00:12:05Z", 10, 100_000, 0));
        let a = CacheExpiry.evaluate(&s).expect("fires");
        assert_eq!(a.evidence, "11:55 idle > 5 min TTL");
        assert!(
            a.headline
                .starts_with("Cache expired before turn 2 · 100k re-written"),
            "{}",
            a.headline
        );
        assert_eq!(a.saving, Saving::OneOff(100_000));
        for i in 0..3 {
            s.apply(&prompt(&format!("2026-01-01T00:1{i}:30Z")));
            s.apply(&response(
                &format!("w{i}"),
                &format!("2026-01-01T00:1{i}:31Z"),
                10,
                500,
                100_000,
            ));
        }
        assert!(CacheExpiry.evaluate(&s).is_none(), "retired after 3 turns");
        // Warm follow-up within TTL: no advice.
        let mut ok = State::new(Pricing::bundled());
        ok.apply(&prompt("2026-01-01T00:00:00Z"));
        ok.apply(&response("m1", "2026-01-01T00:00:05Z", 10, 100_000, 0));
        ok.apply(&prompt("2026-01-01T00:02:00Z"));
        ok.apply(&response("m2", "2026-01-01T00:02:05Z", 10, 500, 100_000));
        assert!(CacheExpiry.evaluate(&ok).is_none());
    }

    #[test]
    fn a03_runaway_result_uses_context_thresholds_since_the_boundary() {
        // Two 5k results of one tool: 10k, below 15 % of a 1M window → quiet.
        let mut s = State::new(Pricing::bundled());
        for i in 0..2 {
            for l in tool(
                &format!("t{i}"),
                "2026-01-01T00:00:00Z",
                "Bash",
                "ls -R",
                20_000,
            ) {
                s.apply(&l);
            }
        }
        assert!(RunawayResult.evaluate(&s).is_none(), "10k of 1M");
        // One 12k result fires on its own.
        for l in tool(
            "big",
            "2026-01-01T00:00:10Z",
            "Bash",
            "find / -name x",
            48_000,
        ) {
            s.apply(&l);
        }
        let a = RunawayResult.evaluate(&s).expect("fires");
        assert!(
            a.headline
                .starts_with("`Bash find / -name x` pushed 12k tokens into context"),
            "{}",
            a.headline
        );
        assert!(
            a.evidence
                .starts_with("Bash results: 22k since the last boundary (2 % of the window)"),
            "{}",
            a.evidence
        );
        assert_eq!(a.saving, Saving::Tokens(22_000));
        // On a 200k window the tool-type threshold (15 % = 30k) fires.
        let mut h = State::new(Pricing::bundled());
        h.apply(&prompt("2026-01-01T00:00:00Z"));
        for i in 0..4 {
            for l in tool(
                &format!("g{i}"),
                "2026-01-01T00:00:02Z",
                "Grep",
                "todo",
                32_000,
            ) {
                h.apply(&l);
            }
        }
        // The session's model decides the window: haiku, 200k.
        h.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"p1","model":"claude-haiku-4-5","content":[],"usage":{"cache_read_input_tokens":1000,"input_tokens":10}}}"#).unwrap());
        let a = RunawayResult.evaluate(&h).expect("fires");
        assert_eq!(
            a.headline,
            "Grep results: 32k since the last boundary (16 % of the window)"
        );
        assert!(
            a.evidence
                .starts_with("biggest: `Grep todo` pushed 8.0k tokens"),
            "{}",
            a.evidence
        );
        // A cleared result no longer counts.
        let mut c = State::new(Pricing::bundled());
        c.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"id":"x","model":"claude-opus-5","content":[{"type":"tool_use","id":"x","name":"Bash","input":{"command":"cat big"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        c.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:01Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"[Old tool result content cleared]"}]}}"#).unwrap());
        assert!(RunawayResult.evaluate(&c).is_none());
    }

    #[test]
    fn a04_rereads_and_edit_reread_edit_churn() {
        let mut s = State::new(Pricing::bundled());
        for i in 0..3 {
            for l in tool(
                &format!("r{i}"),
                "2026-01-01T00:00:00Z",
                "Read",
                "/p/src/render.rs",
                8_000,
            ) {
                s.apply(&l);
            }
        }
        let a = Rereads.evaluate(&s).expect("fires");
        assert_eq!(a.headline, "render.rs was read 3× with no edit in between");
        assert_eq!(a.saving, Saving::Tokens(2_000));
        for l in tool("e1", "2026-01-01T00:00:00Z", "Edit", "/p/src/render.rs", 10) {
            s.apply(&l);
        }
        assert!(Rereads.evaluate(&s).is_none(), "an edit resets the run");
        // edit → re-read → edit, twice: the churn headline.
        for i in 0..2 {
            for l in tool(
                &format!("rr{i}"),
                "2026-01-01T00:00:00Z",
                "Read",
                "/p/src/render.rs",
                8_000,
            ) {
                s.apply(&l);
            }
            for l in tool(
                &format!("ee{i}"),
                "2026-01-01T00:00:00Z",
                "Edit",
                "/p/src/render.rs",
                10,
            ) {
                s.apply(&l);
            }
        }
        let a = Rereads.evaluate(&s).expect("fires");
        assert_eq!(a.headline, "render.rs: edit → re-read → edit ×2");
        assert_eq!(a.action_kind, ActionKind::Prompt);
        assert!(a.action_text.starts_with("After an Edit"));
    }

    #[test]
    fn a25_explore_run_is_bash_aware_and_retires_on_an_agent_spawn() {
        let mut s = State::new(Pricing::bundled());
        for i in 0..8 {
            let (name, input) = if i % 2 == 0 {
                ("Read", "/p/f.rs")
            } else {
                ("Bash", "rg -n todo src")
            };
            for l in tool(
                &format!("g{i}"),
                "2026-01-01T00:00:00Z",
                name,
                input,
                12_000,
            ) {
                s.apply(&l);
            }
        }
        let a = ExploreRun.evaluate(&s).expect("fires");
        assert_eq!(a.headline, "EXPLORING ×8 · +24k ctx this run");
        assert_eq!(a.urgency, Urgency::Next);
        assert_eq!(a.action_kind, ActionKind::Prompt);
        assert!(!ExploreRun.acted(&s, &a));
        // 8 calls but only 8k of context: quiet. 6 calls with 30k: fires.
        let mut small = State::new(Pricing::bundled());
        for i in 0..8 {
            for l in tool(
                &format!("g{i}"),
                "2026-01-01T00:00:00Z",
                "Read",
                "/p/f.rs",
                4_000,
            ) {
                small.apply(&l);
            }
        }
        assert!(ExploreRun.evaluate(&small).is_none());
        let mut six = State::new(Pricing::bundled());
        for i in 0..6 {
            for l in tool(
                &format!("g{i}"),
                "2026-01-01T00:00:00Z",
                "Read",
                "/p/f.rs",
                20_000,
            ) {
                six.apply(&l);
            }
        }
        assert!(ExploreRun.evaluate(&six).is_some());
        // An Agent spawn counts as acted; an Edit ends the run.
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:05Z","message":{"id":"ag","model":"claude-opus-5","content":[{"type":"tool_use","id":"ag","name":"Agent","input":{"prompt":"x","subagent_type":"Explore"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        assert!(ExploreRun.acted(&s, &a));
        assert!(
            ExploreRun.evaluate(&s).is_none(),
            "the Agent call ended the run"
        );
        for l in tool("e", "2026-01-01T00:00:06Z", "Edit", "/p/f.rs", 10) {
            six.apply(&l);
        }
        assert!(ExploreRun.evaluate(&six).is_none());
    }

    #[test]
    fn a07_idle_mcp_and_unused_plugin() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&Line::from_value(serde_json::json!({"type":"attachment","attachment":{"type":"deferred_tools_delta","addedNames":["mcp__playwright__click","mcp__playwright__snapshot"],"addedLines":["x".repeat(6000),"y".repeat(6000)]}})));
        s.session.alive = true;
        let t0 = crate::metrics::cost::parse_ts_ms("2026-01-01T00:00:00Z").unwrap();
        s.now_ms = t0 + 25 * 60_000;
        let a = IdleMcp.evaluate(&s).expect("fires");
        assert_eq!(a.headline, "mcp:playwright idle for 25:00");
        assert!(
            a.evidence.starts_with("2 tool schemas ≈ 3.0k tokens"),
            "{}",
            a.evidence
        );
        assert_eq!(a.saving, Saving::Tokens(3_000));
        // A recent call keeps it quiet.
        for l in tool(
            "m1",
            "2026-01-01T00:20:00Z",
            "mcp__playwright__click",
            "x",
            10,
        ) {
            s.apply(&l);
        }
        assert!(IdleMcp.evaluate(&s).is_none());
        // An enabled plugin unused for 38 startups with 4k always-on tokens.
        let mut p = State::new(Pricing::bundled());
        p.prefix.scan_plugins(
            &serde_json::json!({"enabledPlugins": {"frontend-design@claude-plugins-official": true}}),
            Some(&serde_json::json!({"catalog": [{"plugin": "frontend-design", "tokens": {"claude-opus-5": {"always_on": 4100}}}]})),
            Some(&serde_json::json!({"numStartups": 438, "pluginUsage": {"frontend-design@claude-plugins-official": {"lastUsedNumStartups": 400}}})),
            "claude-opus-5",
        );
        let a = IdleMcp.evaluate(&p).expect("fires");
        assert_eq!(
            a.headline,
            "plugin frontend-design: 4.1k always-on tokens, unused for 38 startups"
        );
        assert_eq!(a.action_text, "/plugin");
    }

    #[test]
    fn a08_thinking_share_on_edit_heavy_turns_prices_the_effort_step() {
        let think = |id: &str, out: u64, thinking: u64| -> Line {
            Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","perTurnEffort":"high","message":{{"id":"{id}","model":"claude-sonnet-5","content":[{{"type":"text","text":"ok"}}],"usage":{{"output_tokens":{out},"output_tokens_details":{{"thinking_tokens":{thinking}}}}}}}}}"#)).unwrap()
        };
        let mut s = State::new(Pricing::bundled());
        for i in 0..5 {
            s.apply(&prompt("2026-01-01T00:00:00Z"));
            for l in tool(
                &format!("e{i}"),
                "2026-01-01T00:00:00Z",
                "Edit",
                "/p/a.rs",
                10,
            ) {
                s.apply(&l);
            }
            s.apply(&think(&format!("t{i}"), 1_000, 600));
        }
        let a = ThinkingShare.evaluate(&s).expect("fires");
        assert_eq!(
            a.headline,
            "60 % of output is thinking on edit-heavy turns (effort high)"
        );
        assert!(a.evidence.ends_with("5 edits in 5 calls"), "{}", a.evidence);
        assert_eq!(a.action_text, "/effort medium");
        assert!(
            a.action.contains("/clear"),
            "non-Fable: switch at a boundary"
        );
        // Sonnet: high 1.0 → medium 0.74, 26 % of 1k output per turn.
        assert_eq!(a.saving, Saving::Tokens(260));
        // Acted once the effort dropped.
        s.status_facts.effort_level = Some("medium".into());
        s.agg.turns.last_mut().unwrap().effort = Some("medium".into());
        assert!(ThinkingShare.acted(&s, &a));
        assert!(ThinkingShare.evaluate(&s).is_none(), "already medium");
        // Not routine: reads, no edits.
        let mut reads = State::new(Pricing::bundled());
        for i in 0..5 {
            reads.apply(&prompt("2026-01-01T00:00:00Z"));
            for l in tool(
                &format!("r{i}"),
                "2026-01-01T00:00:00Z",
                "Read",
                "/p/a.rs",
                10,
            ) {
                reads.apply(&l);
            }
            reads.apply(&think(&format!("t{i}"), 1_000, 600));
        }
        assert!(ThinkingShare.evaluate(&reads).is_none());
    }

    #[test]
    fn a11_fresh_input_uses_the_first_call_only() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&response("f1", "2026-01-01T00:00:01Z", 6_000, 0, 50_000));
        let a = FreshInputSpike.evaluate(&s).expect("fires");
        assert!(
            a.headline
                .starts_with("Turn 1 sent ~6.0k tokens of uncached input"),
            "{}",
            a.headline
        );
        assert_eq!(a.saving, Saving::OneOff(5_000));
        // Many small calls that sum past 5k are not a paste.
        let mut sum = State::new(Pricing::bundled());
        sum.apply(&prompt("2026-01-01T00:00:00Z"));
        for i in 0..4 {
            sum.apply(&response(
                &format!("s{i}"),
                "2026-01-01T00:00:01Z",
                2_000,
                0,
                50_000,
            ));
        }
        assert!(FreshInputSpike.evaluate(&sum).is_none());
        assert!(FreshInputSpike.evaluate(&fixture_state()).is_none());
    }

    #[test]
    fn a11_names_the_paste_history_jsonl_recorded_for_the_turn() {
        use crate::history::Paste;
        let t = |ts: &str| crate::metrics::cost::parse_ts_ms(ts).unwrap();
        // The paste row lands at the prompt's own timestamp (here 300 ms
        // before the transcript line): it is the turn's. On a cached session
        // the first call shows it as a cache write, not as input_tokens, so
        // the size comes from the history and the headline names the paste.
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&response("f1", "2026-01-01T00:00:01Z", 2, 9_800, 50_000));
        s.history.pastes.push(Paste {
            at_ms: t("2026-01-01T00:00:00Z") - 300,
            chars: 38_000,
            lines: 412,
        });
        let a = FreshInputSpike.evaluate(&s).expect("fires");
        assert!(
            a.headline
                .starts_with("Turn 1 pasted ~9.5k tokens (412 lines)"),
            "{}",
            a.headline
        );
        assert_eq!(
            a.evidence,
            "a 412-line paste (38k chars) was written to the cache on the turn's first call and is re-read by every later one"
        );
        assert_eq!(a.saving, Saving::OneOff(8_500));
        // A hashed paste (its text in paste-cache, pruned later) is sized by
        // the composer's placeholder alone: ≈ 25 tokens a line from 200 lines.
        let mut hashed = State::new(Pricing::bundled());
        hashed.apply(&prompt("2026-01-01T00:00:00Z"));
        hashed.apply(&response("h1", "2026-01-01T00:00:01Z", 2, 6_000, 50_000));
        hashed.history.pastes.push(Paste {
            at_ms: t("2026-01-01T00:00:00Z") - 100,
            chars: 0,
            lines: 199,
        });
        assert!(
            FreshInputSpike.evaluate(&hashed).is_none(),
            "under 200 lines"
        );
        hashed.history.pastes[0].lines = 412;
        let a = FreshInputSpike
            .evaluate(&hashed)
            .expect("fires on the placeholder");
        assert!(
            a.headline
                .starts_with("Turn 1 pasted ≈10k tokens (412 lines)"),
            "{}",
            a.headline
        );
        assert!(
            a.evidence.contains("+N lines placeholder"),
            "{}",
            a.evidence
        );
        // A paste queued mid-turn is the turn's from 20k chars, priced at
        // chars / 4; below that, with a small first call, nothing fires.
        let mut queued = State::new(Pricing::bundled());
        queued.apply(&prompt("2026-01-01T00:00:00Z"));
        queued.apply(&response("q1", "2026-01-01T00:00:01Z", 400, 0, 50_000));
        queued.apply(&response("q2", "2026-01-01T00:00:40Z", 7_000, 0, 50_000));
        queued.history.pastes.push(Paste {
            at_ms: t("2026-01-01T00:00:20Z"),
            chars: 19_999,
            lines: 150,
        });
        assert!(
            FreshInputSpike.evaluate(&queued).is_none(),
            "under 20k chars"
        );
        queued.history.pastes[0].chars = 24_000;
        let a = FreshInputSpike
            .evaluate(&queued)
            .expect("the steer's paste fires");
        assert!(
            a.headline
                .starts_with("Turn 1 pasted ~6.0k tokens (150 lines)"),
            "{}",
            a.headline
        );
        assert_eq!(a.saving, Saving::OneOff(5_000));
        // Another turn's paste never counts for this one: the window closes
        // two seconds before the next turn's first line.
        let mut other = State::new(Pricing::bundled());
        other.apply(&prompt("2026-01-01T00:00:00Z"));
        other.apply(&response("o1", "2026-01-01T00:00:01Z", 400, 0, 50_000));
        other.apply(&prompt("2026-01-01T00:05:00Z"));
        other.apply(&response("o2", "2026-01-01T00:05:01Z", 400, 0, 50_000));
        other.history.pastes.push(Paste {
            at_ms: t("2026-01-01T00:05:00Z") - 500,
            chars: 30_000,
            lines: 300,
        });
        let a = FreshInputSpike.evaluate(&other).expect("turn 2's paste");
        assert!(
            a.headline
                .starts_with("Turn 2 pasted ~7.5k tokens (300 lines)"),
            "{}",
            a.headline
        );
    }

    #[test]
    fn a12_chatty_turns_counts_human_prompts_only() {
        let mut s = State::new(Pricing::bundled());
        for i in 0..25 {
            s.apply(&Line::parse(&format!(r#"{{"type":"user","timestamp":"2026-01-01T00:{i:02}:00Z","promptSource":"typed","message":{{"role":"user","content":"ok go"}}}}"#)).unwrap());
            s.apply(&response(
                &format!("c{i}"),
                &format!("2026-01-01T00:{i:02}:01Z"),
                10,
                0,
                100_000,
            ));
        }
        s.session.alive = true;
        s.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:30:00Z").unwrap();
        let a = ChattyTurns.evaluate(&s).expect("fires");
        assert_eq!(a.headline, "25 prompts in the last hour, median 5 chars");
        // Task notifications are machine turns and never count.
        let mut m = State::new(Pricing::bundled());
        for i in 0..25 {
            m.apply(&Line::parse(&format!(r#"{{"type":"user","timestamp":"2026-01-01T00:{i:02}:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":"<task-notification>x</task-notification>"}}}}"#)).unwrap());
            m.apply(&response(
                &format!("c{i}"),
                &format!("2026-01-01T00:{i:02}:01Z"),
                10,
                0,
                100_000,
            ));
        }
        m.session.alive = true;
        m.now_ms = s.now_ms;
        assert!(ChattyTurns.evaluate(&m).is_none());
        assert!(
            ChattyTurns.evaluate(&fixture_state()).is_none(),
            "15 turns only"
        );
    }

    #[test]
    fn a48_agents_waste_fires_on_two_classified_agents_over_a_dollar_and_a_quarter() {
        let mut s = State::new(Pricing::bundled());
        // Four opus agents at ≈$1.25 each (1.5 M cache read at $0.50/M,
        // 20 k output at $25/M).
        let mk = |id: &str| {
            let mut a = crate::agents::Agent::new(
                id,
                crate::agents::Meta {
                    agent_type: "Explore".into(),
                    ..Default::default()
                },
            );
            a.model = "claude-opus-5".into();
            a.usage.cache_read = 1_500_000;
            a.usage.output = 20_000;
            a
        };
        let notify = |s: &mut State, id: &str, status: &str, result: &str| {
            let text = format!(
                "<task-notification><task-id>{id}</task-id><status>{status}</status><summary>s</summary><result>{result}</result></task-notification>"
            );
            s.apply(&Line::parse(&format!(
                r#"{{"type":"user","timestamp":"2026-01-01T00:10:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
                serde_json::to_string(&text).unwrap()
            )).unwrap());
        };
        for id in [
            "0000000000000000a",
            "0000000000000000b",
            "0000000000000000c",
            "0000000000000000d",
        ] {
            s.agents.insert(id.into(), mk(id));
        }
        for id in [
            "0000000000000000a",
            "0000000000000000b",
            "0000000000000000c",
            "0000000000000000d",
        ] {
            notify(&mut s, id, "completed", &"r".repeat(800));
        }
        assert!(AgentsWaste.evaluate(&s).is_none(), "nothing wasted");
        // One failed agent: ≈$1.25 of ≈$5 is 25 %, but one classified.
        notify(&mut s, "0000000000000000a", "failed", "");
        assert!(AgentsWaste.evaluate(&s).is_none(), "one classified agent");
        // A second, killed: ≈$2.50 of ≈$5.
        notify(&mut s, "0000000000000000b", "killed", "");
        let adv = AgentsWaste.evaluate(&s).expect("fires");
        assert_eq!(adv.rule, "A48");
        assert_eq!(adv.urgency, Urgency::Later);
        assert!(
            adv.headline.starts_with("agents wasted ≈$2."),
            "{}",
            adv.headline
        );
        assert!(
            adv.headline.contains("1 failed ≈$1.") || adv.headline.contains("1 killed ≈$1."),
            "{}",
            adv.headline
        );
        assert_eq!(
            adv.evidence,
            "2 of 4 agents classified · 50 % of agent spend"
        );
        assert_eq!(adv.action_kind, ActionKind::Key);
        assert_eq!(adv.action_text, "6 Enter");
        assert!(matches!(adv.saving, Saving::OneOff(t) if t > 0));
        assert_eq!(AgentsWaste.cooldown_turns(), 10);
        assert!(!AgentsWaste.acted(&s, &adv));
        // Opening the agents view acts on it.
        crate::ui::agents_view::open(&mut s);
        assert!(AgentsWaste.acted(&s, &adv));
        // So does the waste share falling under 10 %: eight more agents
        // that returned.
        let mut t = State::new(Pricing::bundled());
        for id in ["0000000000000000a", "0000000000000000b"] {
            t.agents.insert(id.into(), mk(id));
        }
        notify(&mut t, "0000000000000000a", "failed", "");
        notify(&mut t, "0000000000000000b", "failed", "");
        let fired = AgentsWaste.evaluate(&t).expect("fires at 100 %");
        for i in 0..20u64 {
            let id = format!("{:017x}", 0xb000_0000_0000_0000u64 + i);
            t.agents.insert(id.clone(), mk(&id));
            notify(&mut t, &id, "completed", "rrrr");
        }
        assert!(AgentsWaste.acted(&t, &fired), "2 of 22 wasted: under 10 %");
        assert!(AgentsWaste.evaluate(&t).is_none());
        // Below a dollar of waste it stays quiet whatever the share.
        let mut cheap = State::new(Pricing::bundled());
        for id in ["0000000000000000a", "0000000000000000b"] {
            let mut a = mk(id);
            a.usage.cache_read = 40_000;
            a.usage.output = 500;
            cheap.agents.insert(id.into(), a);
        }
        notify(&mut cheap, "0000000000000000a", "failed", "");
        notify(&mut cheap, "0000000000000000b", "failed", "");
        assert!(AgentsWaste.evaluate(&cheap).is_none());
    }

    #[test]
    fn a48_is_silent_on_the_fixtures() {
        // A: a fork with no notification, nothing classified. B: no agents.
        // C: one killed agent, ≈$0.18 — under the dollar and one classified.
        for name in ["session-a", "session-b", "session-c"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("fixtures/{name}.jsonl"));
            let info = crate::ui::state::SessionInfo::from_fixture(&path);
            let s = crate::load::state_from(&path, info);
            assert!(AgentsWaste.evaluate(&s).is_none(), "{name}");
        }
    }

    #[test]
    fn a14_subagent_model_reads_tool_stats_not_descriptions() {
        let mut s = State::new(Pricing::bundled());
        let mk = |id: &str, kind: &str, tools: &[(&str, usize)]| {
            let mut a = crate::agents::Agent::new(
                id,
                crate::agents::Meta {
                    agent_type: kind.into(),
                    ..Default::default()
                },
            );
            a.model = "claude-opus-5".into();
            a.usage.cache_read = 400_000;
            a.usage.output = 5_000;
            for (t, n) in tools {
                a.tools_by_name.insert((*t).into(), *n);
            }
            a
        };
        s.agents
            .insert("x".into(), mk("x", "Explore", &[("Grep", 4), ("Read", 3)]));
        let adv = SubagentModel.evaluate(&s).expect("fires");
        assert_eq!(
            adv.headline,
            "Explore agent x ran on Opus: 7 searches, 0 edits"
        );
        assert!(matches!(adv.saving, Saving::Tokens(t) if t > 0));
        assert_eq!(adv.action_kind, ActionKind::Prompt);
        // An agent that edited is not a search agent, whatever its type.
        s.agents.insert(
            "x".into(),
            mk("x", "Explore", &[("Grep", 4), ("Read", 3), ("Edit", 1)]),
        );
        assert!(SubagentModel.evaluate(&s).is_none());
        // Four searches are too few.
        s.agents
            .insert("x".into(), mk("x", "general-purpose", &[("Bash", 4)]));
        assert!(SubagentModel.evaluate(&s).is_none());
        let mut sonnet = mk("y", "general-purpose", &[("Bash", 9)]);
        sonnet.model = "claude-sonnet-5".into();
        s.agents.insert("y".into(), sonnet);
        assert!(SubagentModel.evaluate(&s).is_none());
    }

    #[test]
    fn a17_big_prefix_is_priced_per_turn() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        // 120k of prefix on the first call, confirmed by a /context: over the
        // token arm at Haiku's cache-read price of a cent per turn.
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"p1","model":"claude-haiku-4-5","content":[],"usage":{"cache_read_input_tokens":120000,"input_tokens":10}}}"#).unwrap());
        assert!(
            BigPrefix.evaluate(&s).is_none(),
            "estimated, the token arm waits for a /context"
        );
        s.apply(&context_table(
            "2026-01-01T00:00:02Z",
            &[
                ("System prompt", "20k"),
                ("System tools", "90k"),
                ("Memory files", "10k"),
                ("Messages", "1k"),
            ],
        ));
        let a = BigPrefix.evaluate(&s).expect("fires");
        assert!(
            a.headline.starts_with("Fixed prefix 120k tokens ≈$"),
            "{}",
            a.headline
        );
        assert!(a.headline.ends_with("/turn at 1 calls"), "{}", a.headline);
        assert_eq!(a.action_kind, ActionKind::Setting);
        assert_eq!(a.saving, Saving::Tokens(24_000));
        // A raw 60k on Opus at one call per turn is every session's on a
        // plugin-heavy setup and $0.03/turn: quiet. Decision 9 of the
        // context-residency PRD put the token arm at 50k because the cost is
        // absolute, and its §10.6 kept it there but gated on a /context — the
        // raw figure runs 47–62k on this machine and overstates by a third.
        let mut small = State::new(Pricing::bundled());
        small.apply(&prompt("2026-01-01T00:00:00Z"));
        small.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"p1","model":"claude-opus-5","content":[],"usage":{"cache_read_input_tokens":60000,"input_tokens":10}}}"#).unwrap());
        assert!(BigPrefix.evaluate(&small).is_none());
        // 60k at 15 calls per turn on Opus: ≈$0.45/turn, fires — the price
        // arm reads the estimate; only the token arm waits for a /context.
        let mut busy = State::new(Pricing::bundled());
        busy.apply(&prompt("2026-01-01T00:00:00Z"));
        for i in 0..15 {
            busy.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:0{}Z","message":{{"id":"c{i}","model":"claude-opus-5","content":[],"usage":{{"cache_read_input_tokens":60000,"input_tokens":10}}}}}}"#, i % 10)).unwrap());
        }
        assert!(BigPrefix.evaluate(&busy).is_some());
    }
}

// ---------------------------------------------------------------- Phase 5

/// A19 — the cache entry expires within the countdown band while the
/// person is asked (or idle) and a reply now would keep ≥ 50 k warm.
pub struct CacheCountdown;
impl Rule for CacheCountdown {
    fn id(&self) -> &'static str {
        "A19"
    }
    fn family(&self) -> &'static str {
        "cache-countdown"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let clock = state.cache_clock()?;
        if clock.remaining_ms <= 0 {
            return None;
        }
        // The shim's figure is ignored when its file predates the last
        // assistant line (`cache_clock` already falls back and marks ≈).
        let rewrite = if state.cache.from_shim && state.cache.recache_tokens_if_cold > 0 {
            state.cache.recache_tokens_if_cold
        } else {
            state.context().size
        };
        if rewrite < 50_000 {
            return None;
        }
        let one_hour = state.cache_ttl_ms() >= 3_600_000;
        let band = if one_hour { 300_000 } else { 120_000 };
        if clock.remaining_ms > band {
            return None;
        }
        // Only while the turn is not running: the person is asked, or idle.
        let turn_running = state
            .agg
            .current_turn()
            .is_some_and(|t| t.duration_ms.is_none() && t.interrupted_after_calls.is_none())
            && state
                .tools
                .running()
                .is_some_and(|c| !matches!(c.name.as_str(), "AskUserQuestion" | "ExitPlanMode"));
        if turn_running {
            return None;
        }
        let idle_ms = state
            .last_line_at_ms
            .map(|t| state.clock_ms() - t)
            .unwrap_or(0);
        if one_hour && idle_ms < 60_000 && state.waiting().is_none() {
            return None;
        }
        let left = crate::coach::short_duration(clock.remaining_ms);
        let warm = super::usd(state, rewrite, super::PriceKind::CacheRead);
        let cold = super::usd(state, rewrite, super::PriceKind::CacheWrite);
        let mut a = Advice::new("A19", "cache-countdown", Urgency::Now);
        a.headline = if one_hour {
            format!(
                "cache cold in {left} · reply now {}, later {}",
                warm.map(|v| format!("≈{}", crate::coach::usd_short(v)))
                    .unwrap_or_else(|| "cheap".into()),
                cold.map(|v| format!("≈{}", crate::coach::usd_short(v)))
                    .unwrap_or_else(|| "a full re-write".into())
            )
        } else {
            format!(
                "cache cold in {left} (5m TTL) · reply or lose {}",
                fmt::tokens(rewrite)
            )
        };
        a.evidence = format!(
            "re-cache {} tok{}",
            fmt::tokens(rewrite),
            if clock.approx { " · clock ≈" } else { "" }
        );
        a.action = "reply now to keep the cache · done? /clear + hand-off note".into();
        a.action_kind = ActionKind::Advice;
        a.saving = Saving::OneOff(rewrite);
        a.retires_on = "the next API call";
        a.mark = state.agg.api_calls() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state.agg.api_calls() as u64 > fired.mark
    }
    fn ttl(&self) -> crate::advisor::Ttl {
        crate::advisor::Ttl::NextPrompt
    }
}

/// A21 — a model switch while the cache is warm and the context large
/// (from `PreModelSwitch` when hooks run, else a `/model` or `/fast` row of
/// history.jsonl); when the cache is cold and this project switches a lot,
/// the cheap moment is named instead.
pub struct SwitchWindow;
impl Rule for SwitchWindow {
    fn id(&self) -> &'static str {
        "A21"
    }
    fn family(&self) -> &'static str {
        "warm-switch"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let now = state.clock_ms();
        let warm = state.cache_warm().is_some_and(|(w, _)| w);
        let ctx = if state.cache.from_shim && state.cache.recache_tokens_if_cold > 0 {
            state.cache.recache_tokens_if_cold
        } else {
            state.context().size
        };
        // The hook's own record wins: it says whether the cache was warm.
        let hook = state
            .model_switches
            .iter()
            .rev()
            .find(|s| now - s.at_ms <= 120_000)
            .filter(|s| s.source.as_deref() != Some("auto"))
            .filter(|s| s.cache_warm == Some(true) && s.context_tokens.unwrap_or(ctx) > 100_000);
        let history = state
            .history
            .switches_since(now - 120_000)
            .last()
            .filter(|_| warm && ctx > 100_000)
            .map(|r| {
                (
                    r.at_ms,
                    format!(
                        "{} {}",
                        r.command.as_deref().unwrap_or("/model"),
                        r.arg.as_deref().unwrap_or("")
                    ),
                )
            });
        let mut a = Advice::new("A21", "warm-switch", Urgency::Now);
        let (at, what) = match (hook, history) {
            (Some(s), _) => (
                s.at_ms,
                format!(
                    "{} → {}",
                    fmt::model_short(&s.from),
                    fmt::model_short(&s.to)
                ),
            ),
            (None, Some((at, cmd))) => (at, cmd.trim().to_string()),
            (None, None) => return None,
        };
        let tokens = hook.and_then(|s| s.context_tokens).unwrap_or(ctx);
        let price = hook
            .and_then(|s| s.estimated_cache_write_usd)
            .or_else(|| super::usd(state, tokens, super::PriceKind::CacheWrite));
        let left = state
            .cache_clock()
            .filter(|c| c.remaining_ms > 0)
            .map(|c| crate::coach::short_duration(c.remaining_ms));
        a.headline = format!(
            "Switch re-reads {} uncached{}",
            fmt::tokens(tokens),
            price
                .map(|p| format!(" (≈{})", crate::coach::usd_short(p)))
                .unwrap_or_default()
        );
        a.evidence = format!(
            "{what} at {} · cache warm{}",
            fmt::clock_hhmm(at),
            left.as_ref()
                .map(|l| format!(" for {l}"))
                .unwrap_or_default()
        );
        a.action = match left {
            Some(l) => format!("cache dies in {l}: /model back, or switch after /clear"),
            None => "/model back, or switch after /clear".into(),
        };
        a.action_text = "/model".into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::OneOff(tokens);
        a.retires_on = "a /model back or a /clear";
        a.mark = at as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        // A later switch (back) or a boundary.
        state
            .history
            .switches_since(fired.mark as i64)
            .iter()
            .any(|r| r.at_ms > fired.mark as i64)
            || state
                .model_switches
                .iter()
                .any(|s| s.at_ms > fired.mark as i64)
            || state
                .agg
                .boundaries
                .last()
                .and_then(|b| b.at.as_deref())
                .and_then(crate::metrics::cost::parse_ts_ms)
                .is_some_and(|t| t > fired.mark as i64)
    }
    fn ttl(&self) -> crate::advisor::Ttl {
        crate::advisor::Ttl::NextPrompt
    }
}

/// A21's cold half — the cache is cold and this project switches models
/// often: now is the free moment.
pub struct ColdSwitch;
impl Rule for ColdSwitch {
    fn id(&self) -> &'static str {
        "A21b"
    }
    fn family(&self) -> &'static str {
        "cold-switch"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let cold = state.cache_warm().is_some_and(|(w, _)| !w);
        if !cold || state.history.project_switches < 3 || state.context().size < 100_000 {
            return None;
        }
        let mut a = Advice::new("A21b", "cold-switch", Urgency::Next);
        a.headline = "Cache is cold: /model or /effort costs nothing now".into();
        a.evidence = format!(
            "{} switches in this project · the next call re-writes {} anyway",
            state.history.project_switches,
            fmt::tokens(state.context().size)
        );
        a.action = "pick one and hold it until the next /clear".into();
        a.action_text = "/model".into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::OneOff(state.context().size);
        a.retires_on = "the next API call";
        a.mark = state.agg.api_calls() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state.agg.api_calls() as u64 > fired.mark
    }
}

/// A23 — the cost of continuing: what every call re-reads at this
/// context. In the `next` row from 200 k; the slot only at a clean turn
/// end above 300 k on a 1M window (or the warn band elsewhere).
pub struct ContextCost;
impl Rule for ContextCost {
    fn id(&self) -> &'static str {
        "A23"
    }
    fn family(&self) -> &'static str {
        "context-reset"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let v = state.context();
        if v.size < 200_000 {
            return None;
        }
        let g = state.gradient_priced(false)?;
        let big = v.window >= 900_000 && v.size >= 300_000 || state.bands().warn_at <= v.size;
        let clean = {
            let t = state.agg.current_turn()?;
            t.duration_ms.is_some()
                && t.last_stop_reason.as_deref() == Some("end_turn")
                && state.session.background_tasks.is_empty()
                && t.pending_background_agents.unwrap_or(0) == 0
        };
        let at_100k = g.per_turn_at_100k;
        let mut a = Advice::new("A23", "context-reset", Urgency::Next);
        a.headline = format!(
            "ctx {} → ≈{}/call, ≈{}/turn (was ≈{})",
            fmt::tokens(v.size),
            crate::coach::usd_short(g.per_call),
            crate::coach::usd_short(g.per_turn),
            crate::coach::usd_short(at_100k)
        );
        a.evidence = format!(
            "{:.0} calls per turn on this session · next 30 calls ≈{}",
            g.calls_per_turn,
            crate::coach::usd_short(g.next_30_calls)
        );
        a.action = if clean {
            "clean stop: /compact <focus>, or hand-off + /clear".into()
        } else {
            "at the next clean stop: /compact <focus>, or hand-off + /clear".into()
        };
        a.action_text = "/compact ".into();
        a.action_kind = ActionKind::Slash;
        a.saving =
            Saving::Tokens((v.size.saturating_sub(100_000) as f64 * g.calls_per_turn) as u64);
        a.next_row_only = !(big && clean);
        a.retires_on = "a /compact or /clear";
        a.mark = boundaries_and_compacts(state);
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        boundaries_and_compacts(state) > fired.mark
    }
}

fn boundaries_and_compacts(state: &State) -> u64 {
    let clears = state
        .agg
        .boundaries
        .iter()
        .filter(|b| {
            matches!(
                b.kind,
                crate::metrics::usage::BoundaryKind::Clear
                    | crate::metrics::usage::BoundaryKind::Fork
                    | crate::metrics::usage::BoundaryKind::Compact
            )
        })
        .count();
    let compacts = state
        .agg
        .slash_commands
        .iter()
        .filter(|(_, c)| c.starts_with("/compact"))
        .count()
        + state
            .history
            .commands
            .iter()
            .filter(|r| r.command.as_deref() == Some("/compact"))
            .count();
    (clears + compacts) as u64
}

/// A27 — a `/loop` or `/goal` is armed on a large context: every wake-up
/// re-reads it.
pub struct LoopArmed;
impl Rule for LoopArmed {
    fn id(&self) -> &'static str {
        "A27"
    }
    fn family(&self) -> &'static str {
        "loop-armed"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let ctx = state.context().size;
        if ctx <= 150_000 {
            return None;
        }
        let armed = state.session.session_crons > 0
            || state.history.last("/loop").is_some()
            || state.history.last("/goal").is_some();
        if !armed {
            return None;
        }
        let loop_row = state.history.last("/loop");
        let interval = loop_row
            .and_then(|r| r.arg.clone())
            .unwrap_or_else(|| "5m".into());
        let minutes: f64 = interval
            .trim_end_matches('m')
            .parse::<f64>()
            .unwrap_or(5.0)
            .max(1.0);
        let per_hour = (60.0 / minutes) as u64 * ctx;
        let mut a = Advice::new("A27", "loop-armed", Urgency::Next);
        a.headline = format!(
            "/loop {interval} on {} ctx ≈ {} cache-read tok/h idle",
            fmt::tokens(ctx),
            fmt::tokens(per_hour)
        );
        a.evidence = format!(
            "{} · each wake-up re-reads the whole context",
            if state.session.session_crons > 0 {
                format!("{} session cron(s)", state.session.session_crons)
            } else {
                "a /loop or /goal in this session".into()
            }
        );
        a.action = "/clear first, widen the interval, or narrow it".into();
        a.action_text = "/clear".into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::Tokens(ctx);
        a.retires_on = "a /clear";
        a.mark = state
            .agg
            .boundaries
            .iter()
            .filter(|b| b.kind == crate::metrics::usage::BoundaryKind::Clear)
            .count() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state
            .agg
            .boundaries
            .iter()
            .filter(|b| b.kind == crate::metrics::usage::BoundaryKind::Clear)
            .count() as u64
            > fired.mark
    }
    fn cooldown_turns(&self) -> usize {
        usize::MAX / 2 // once per session
    }
}

/// A30 — a cold resume: the session idled past the cache TTL on a large
/// context, or Claude Code's own resume said the cache likely expired.
pub struct ColdResume;
impl Rule for ColdResume {
    fn id(&self) -> &'static str {
        "A30"
    }
    fn family(&self) -> &'static str {
        "cold-resume"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let ctx = state.context().size;
        if ctx <= 100_000 {
            return None;
        }
        let now = state.clock_ms();
        let idle = state.last_line_at_ms.map(|t| now - t).unwrap_or(0);
        let ended = state.agg.current_turn().is_some_and(|t| {
            t.duration_ms.is_some() || t.last_stop_reason.as_deref() == Some("end_turn")
        });
        let primary =
            ended && idle > state.cache_ttl_ms() && state.cache_warm().is_some_and(|(w, _)| !w);
        let secondary = state
            .session
            .resume
            .as_ref()
            .filter(|r| matches!(r.source.as_str(), "resume" | "fork"))
            .filter(|r| r.prompt_cache_likely_expired == Some(true))
            .filter(|r| state.last_api_call_ms().is_none_or(|c| c < r.at_ms));
        if !primary && secondary.is_none() {
            return None;
        }
        let rewrite = secondary.and_then(|r| r.context_tokens).unwrap_or(ctx);
        let mut a = Advice::new("A30", "cold-resume", Urgency::Next);
        a.headline = format!(
            "Idle {} · cache cold · next msg re-writes {}",
            fmt::duration_ms(idle),
            fmt::tokens(rewrite)
        );
        a.evidence = format!(
            "{}{}",
            if primary {
                format!(
                    "no call for longer than the {} TTL",
                    if state.cache_ttl_ms() >= 3_600_000 {
                        "1h"
                    } else {
                        "5m"
                    }
                )
            } else {
                "Claude Code's resume says the cache likely expired".into()
            },
            secondary
                .and_then(|r| r.estimated_cache_write_usd)
                .or_else(|| super::usd(state, rewrite, super::PriceKind::CacheWrite))
                .map(|u| format!(" · ≈{}", crate::coach::usd_short(u)))
                .unwrap_or_default()
        );
        a.action = "Done? /clear + hand-off note · else /compact (½)".into();
        a.action_text = "/clear".into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::OneOff(rewrite / 2);
        a.retires_on = "the next API call or a /clear";
        a.mark = state.agg.api_calls() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state.agg.api_calls() as u64 > fired.mark
            || state
                .agg
                .boundaries
                .last()
                .is_some_and(|b| b.kind == crate::metrics::usage::BoundaryKind::Clear)
    }
}

#[cfg(test)]
mod phase5_tests {
    use super::super::fixtures::{prompt, response};
    use super::*;
    use crate::metrics::Pricing;
    use crate::transcript::Line;

    fn t(ts: &str) -> i64 {
        crate::metrics::cost::parse_ts_ms(ts).unwrap()
    }

    /// A session of one turn with a 150k context, the cache written at
    /// 10:00:05 on the 5 m TTL, a question pending since 10:00:06.
    fn asked() -> State {
        let mut s = State::new(Pricing::bundled());
        s.session.alive = true;
        s.apply(&prompt("2026-01-01T10:00:00Z"));
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":2,"cache_creation_input_tokens":150000,"cache_read_input_tokens":0,"output_tokens":10}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:06Z","message":{"id":"m2","model":"claude-opus-5","content":[{"type":"tool_use","id":"q","name":"AskUserQuestion","input":{"questions":[]}}],"usage":{"input_tokens":2,"cache_read_input_tokens":150000,"output_tokens":10}}}"#).unwrap());
        s
    }

    #[test]
    fn a19_cache_countdown_in_the_band_while_asked() {
        let mut s = asked();
        s.now_ms = t("2026-01-01T10:02:00Z"); // 3:06 left of the 5 m entry
        assert!(
            CacheCountdown.evaluate(&s).is_none(),
            "outside the 2-minute band of a 5 m TTL"
        );
        s.now_ms = t("2026-01-01T10:03:30Z"); // 1:36 left
        let a = CacheCountdown.evaluate(&s).expect("fires");
        assert_eq!(
            a.headline,
            "cache cold in 1:36 (5m TTL) · reply or lose 150k"
        );
        assert_eq!(a.urgency, Urgency::Now);
        assert_eq!(
            a.saving,
            Saving::OneOff(150_002),
            "the whole context, inputs included"
        );
        assert!(!CacheCountdown.acted(&s, &a));
        // The reply's API call retires it.
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T10:04:00Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q","content":"yes"}]}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:04:01Z","message":{"id":"m3","model":"claude-opus-5","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":2,"cache_read_input_tokens":150000,"output_tokens":10}}}"#).unwrap());
        assert!(CacheCountdown.acted(&s, &a));
        // A small context never fires; a running turn never fires.
        let mut small = State::new(Pricing::bundled());
        small.session.alive = true;
        small.apply(&prompt("2026-01-01T10:00:00Z"));
        small.apply(&response("m", "2026-01-01T10:00:05Z", 2, 20_000, 0));
        small.now_ms = t("2026-01-01T10:03:30Z");
        assert!(CacheCountdown.evaluate(&small).is_none());
        let mut running = asked();
        running.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T10:00:07Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q","content":"yes"}]}}"#).unwrap());
        running.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:08Z","message":{"id":"m4","model":"claude-opus-5","content":[{"type":"tool_use","id":"b","name":"Bash","input":{"command":"cargo test"}}],"usage":{"input_tokens":2,"cache_read_input_tokens":150000,"output_tokens":10}}}"#).unwrap());
        running.now_ms = t("2026-01-01T10:03:30Z");
        assert!(
            CacheCountdown.evaluate(&running).is_none(),
            "a Bash call runs"
        );
    }

    #[test]
    fn a21_warm_switch_from_the_hook_or_history_and_the_cold_hint() {
        let mut s = asked();
        s.now_ms = t("2026-01-01T10:01:00Z");
        assert!(SwitchWindow.evaluate(&s).is_none());
        // The hook: a warm switch on 150k.
        s.apply_hook(&crate::hooks::HookEvent::from_stdin(t("2026-01-01T10:00:50Z"), serde_json::json!({"hook_event_name":"PreModelSwitch","from_model":"claude-opus-5","to_model":"claude-sonnet-5","source":"command","prompt_cache_warm":true,"context_tokens":150000,"estimated_cache_write_usd":0.9})).unwrap());
        let a = SwitchWindow.evaluate(&s).expect("fires");
        assert_eq!(a.headline, "Switch re-reads 150k uncached (≈$.90)");
        assert!(
            a.evidence
                .starts_with("opus-5 → sonnet-5 at 10:00 · cache warm for "),
            "{}",
            a.evidence
        );
        assert!(a.action.starts_with("cache dies in "), "{}", a.action);
        assert_eq!(a.urgency, Urgency::Now);
        assert!(!SwitchWindow.acted(&s, &a));
        // A switch back (another hook event later) counts as acted.
        s.apply_hook(&crate::hooks::HookEvent::from_stdin(t("2026-01-01T10:00:55Z"), serde_json::json!({"hook_event_name":"PreModelSwitch","from_model":"claude-sonnet-5","to_model":"claude-opus-5","source":"command","prompt_cache_warm":true,"context_tokens":150000})).unwrap());
        assert!(SwitchWindow.acted(&s, &a));
        // `source: auto` (Claude Code's own downgrade) never fires.
        let mut auto = asked();
        auto.now_ms = t("2026-01-01T10:01:00Z");
        auto.apply_hook(&crate::hooks::HookEvent::from_stdin(t("2026-01-01T10:00:50Z"), serde_json::json!({"hook_event_name":"PreModelSwitch","from_model":"claude-opus-5","to_model":"claude-sonnet-5","source":"auto","prompt_cache_warm":true,"context_tokens":150000})).unwrap());
        assert!(SwitchWindow.evaluate(&auto).is_none());
        // Without hooks: a /model row of history.jsonl for this session.
        let mut h = asked();
        h.session.session_id = "s1".into();
        h.now_ms = t("2026-01-01T10:01:00Z");
        let row = crate::history::parse_row(&format!(r#"{{"display":"/model sonnet","pastedContents":{{}},"timestamp":{},"project":"/p","sessionId":"s1"}}"#, t("2026-01-01T10:00:50Z"))).unwrap();
        h.history.absorb(&[row], "s1", std::path::Path::new("/p"));
        let a = SwitchWindow.evaluate(&h).expect("fires from history");
        assert!(
            a.evidence.starts_with("/model sonnet at 10:00"),
            "{}",
            a.evidence
        );
        // Cold and a switching project: the free-moment hint.
        let mut cold = asked();
        cold.now_ms = t("2026-01-01T10:20:00Z");
        cold.history.project_switches = 4;
        let c = ColdSwitch.evaluate(&cold).expect("cold hint");
        assert_eq!(
            c.headline,
            "Cache is cold: /model or /effort costs nothing now"
        );
        assert_eq!(c.urgency, Urgency::Next);
        cold.history.project_switches = 2;
        assert!(ColdSwitch.evaluate(&cold).is_none());
        assert!(ColdSwitch.evaluate(&s).is_none(), "warm");
    }

    #[test]
    fn a23_context_cost_is_next_row_below_the_bar_and_slot_at_a_clean_stop() {
        let big = |ctx: u64, ended: bool| -> State {
            let mut s = State::new(Pricing::bundled());
            s.apply(&prompt("2026-01-01T10:00:00Z"));
            s.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{{"id":"m1","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}],"stop_reason":"end_turn","usage":{{"input_tokens":2,"cache_read_input_tokens":{ctx},"output_tokens":10}}}}}}"#)).unwrap());
            if ended {
                s.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T10:00:06Z","durationMs":6000}"#).unwrap());
            }
            s
        };
        assert!(
            ContextCost.evaluate(&big(150_000, true)).is_none(),
            "below 200k"
        );
        let a = ContextCost.evaluate(&big(250_000, true)).expect("fires");
        assert!(a.next_row_only, "250k on 1M: the next row only");
        assert!(a.headline.starts_with("ctx 250k → ≈$"), "{}", a.headline);
        let a = ContextCost.evaluate(&big(420_000, false)).expect("fires");
        assert!(a.next_row_only, "a turn runs: no clean stop yet");
        assert!(
            a.action.starts_with("at the next clean stop"),
            "{}",
            a.action
        );
        let a = ContextCost.evaluate(&big(420_000, true)).expect("fires");
        assert!(!a.next_row_only, "420k and a clean stop: the slot");
        assert_eq!(
            a.action,
            "clean stop: /compact <focus>, or hand-off + /clear"
        );
        assert_eq!(a.action_text, "/compact ");
        // A /compact counts as acted.
        let mut s = big(420_000, true);
        assert!(!ContextCost.acted(&s, &a));
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T10:01:00Z","message":{"role":"user","content":"<command-name>/compact</command-name><command-message>compact</command-message><command-args>focus</command-args>"}}"#).unwrap());
        assert!(ContextCost.acted(&s, &a));
        // The engine keeps a next-row-only rule out of the slot but in the queue.
        let mut e = crate::advisor::Engine::new(vec![Box::new(ContextCost)]);
        let mut st = big(250_000, true);
        for i in 1..=3 {
            st.apply(&prompt(&format!("2026-01-01T10:0{i}:00Z")));
            st.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T10:0{i}:05Z","message":{{"id":"m{i}","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}],"stop_reason":"end_turn","usage":{{"input_tokens":2,"cache_read_input_tokens":250000,"output_tokens":10}}}}}}"#)).unwrap());
        }
        e.evaluate(&st);
        assert!(e.occupant.is_none());
        assert_eq!(e.current.len(), 1);
        assert_eq!(e.next_up().unwrap().1, "next-row only");
    }

    #[test]
    fn a27_loop_armed_on_a_large_context_once() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T10:00:00Z"));
        s.apply(&response("m1", "2026-01-01T10:00:05Z", 2, 0, 180_000));
        assert!(LoopArmed.evaluate(&s).is_none(), "nothing armed");
        s.session.session_crons = 1;
        let a = LoopArmed.evaluate(&s).expect("fires");
        assert_eq!(
            a.headline,
            "/loop 5m on 180k ctx ≈ 2.16M cache-read tok/h idle"
        );
        assert_eq!(a.action_text, "/clear");
        assert!(LoopArmed.cooldown_turns() > 1_000, "once per session");
        // From history with an interval.
        let mut h = State::new(Pricing::bundled());
        h.session.session_id = "s1".into();
        h.apply(&prompt("2026-01-01T10:00:00Z"));
        h.apply(&response("m1", "2026-01-01T10:00:05Z", 2, 0, 180_000));
        let row = crate::history::parse_row(r#"{"display":"/loop 15m check ci","pastedContents":{},"timestamp":1,"project":"/p","sessionId":"s1"}"#).unwrap();
        h.history.absorb(&[row], "s1", std::path::Path::new("/p"));
        let a = LoopArmed.evaluate(&h).expect("fires");
        assert!(
            a.headline.starts_with("/loop 15m on 180k ctx ≈ 720k"),
            "{}",
            a.headline
        );
        // A small context: quiet.
        let mut small = State::new(Pricing::bundled());
        small.session.session_crons = 2;
        small.apply(&prompt("2026-01-01T10:00:00Z"));
        small.apply(&response("m1", "2026-01-01T10:00:05Z", 2, 0, 50_000));
        assert!(LoopArmed.evaluate(&small).is_none());
    }

    #[test]
    fn a30_cold_resume_from_the_idle_gap_or_the_resume_hook() {
        let mut s = State::new(Pricing::bundled());
        s.session.alive = true;
        s.apply(&prompt("2026-01-01T10:00:00Z"));
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":182000,"output_tokens":10}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T10:00:06Z","durationMs":6000}"#).unwrap());
        s.now_ms = t("2026-01-01T10:03:00Z");
        assert!(ColdResume.evaluate(&s).is_none(), "still warm");
        s.now_ms = t("2026-01-01T12:14:06Z");
        let a = ColdResume.evaluate(&s).expect("fires");
        assert_eq!(
            a.headline,
            "Idle 2h 14m · cache cold · next msg re-writes 182k"
        );
        assert!(
            a.evidence.starts_with("no call for longer than the 5m TTL"),
            "{}",
            a.evidence
        );
        assert_eq!(a.action, "Done? /clear + hand-off note · else /compact (½)");
        assert_eq!(a.family, "cold-resume");
        assert!(!ColdResume.acted(&s, &a));
        s.apply(&prompt("2026-01-01T12:15:00Z"));
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T12:15:05Z","message":{"id":"m2","model":"claude-opus-5","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":2,"cache_creation_input_tokens":182000,"output_tokens":10}}}"#).unwrap());
        assert!(ColdResume.acted(&s, &a));
        // The resume hook's own verdict.
        let mut r = State::new(Pricing::bundled());
        r.session.alive = true;
        r.apply(&prompt("2026-01-01T10:00:00Z"));
        r.apply(&response("m1", "2026-01-01T10:00:05Z", 2, 0, 150_000));
        r.apply_hook(&crate::hooks::HookEvent::from_stdin(t("2026-01-01T10:00:10Z"), serde_json::json!({"hook_event_name":"SessionStart","source":"resume","seconds_since_last_response":7200,"context_tokens":150000,"prompt_cache_likely_expired":true,"estimated_cache_write_usd":1.1})).unwrap());
        r.now_ms = t("2026-01-01T10:00:20Z");
        let a = ColdResume.evaluate(&r).expect("fires");
        assert!(
            a.evidence
                .starts_with("Claude Code's resume says the cache likely expired · ≈$1.1"),
            "{}",
            a.evidence
        );
        // Below 100k: nothing.
        let mut small = State::new(Pricing::bundled());
        small.session.alive = true;
        small.apply(&prompt("2026-01-01T10:00:00Z"));
        small.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":40000,"output_tokens":10}}}"#).unwrap());
        small.now_ms = t("2026-01-01T12:14:06Z");
        assert!(ColdResume.evaluate(&small).is_none());
    }
}
