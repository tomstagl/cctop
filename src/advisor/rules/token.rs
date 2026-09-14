//! Token-axis rules: what the context costs and why (A01–A04, A07, A08,
//! A11, A12, A14, A17, A25). Phase 5 adds A19–A23, A27 and A30 here.

use super::{calls_per_turn, human_turns_since, model_short, recent_turns, usd_label, PriceKind};
use crate::advisor::{ActionKind, Advice, Rule, Saving, Urgency};
use crate::harness_facts::context_suggestions as ctx_rules;
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
        Box::new(BigPrefix),
        Box::new(ExploreRun),
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
                .filter(|c| c.name == "Read" && c.paths.iter().any(|p| *p == tail))
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

/// A11 — a turn whose first API call carried > 5 k uncached input tokens
/// (a paste), within the last three human turns.
pub struct FreshInputSpike;
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
        let t = state
            .agg
            .turns
            .iter()
            .filter(|t| {
                t.human && t.first_call_input > 5_000 && human_turns_since(state, t.number) < 3
            })
            .max_by_key(|t| t.first_call_input)?;
        let mut a = Advice::new("A11", "fresh-input", Urgency::Later);
        a.headline = format!(
            "Turn {} sent ~{} tokens of uncached input{}",
            t.number,
            fmt::tokens(t.first_call_input),
            usd_label(state, t.first_call_input, PriceKind::Input)
        );
        a.evidence = format!(
            "the prompt itself (a paste, {} images) was billed fresh on the turn's first call and stays in context",
            t.prompt_images
        );
        a.action = "for files, give the path and let Claude read the relevant range; for logs, paste the last 100 lines".into();
        a.saving = Saving::OneOff(t.first_call_input.saturating_sub(1_000));
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

/// A17 — the fixed prefix costs ≥ $0.10 per turn at the cache-read price,
/// or is ≥ 60 k tokens.
pub struct BigPrefix;
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
        let v = state.context();
        if v.prefix == 0 {
            return None;
        }
        let cpt = calls_per_turn(state);
        let per_turn_tokens = (v.prefix as f64 * cpt) as u64;
        let per_turn_usd = super::usd(state, per_turn_tokens, PriceKind::CacheRead);
        if per_turn_usd.is_none_or(|u| u < 0.10) && v.prefix < 60_000 {
            return None;
        }
        let rows = state.prefix.rows(v.prefix);
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
            fmt::tokens(v.prefix),
            per_turn_usd.map(fmt::usd).unwrap_or_else(|| "?".into())
        );
        a.evidence = format!("{biggest}{captured}");
        a.action =
            "trim CLAUDE.md, move rarely-used rules to skills, disable unused MCP servers and plugins"
                .into();
        a.action_kind = ActionKind::Setting;
        // Assumes a fifth of the prefix is trimmable.
        a.saving = Saving::Tokens(per_turn_tokens / 5);
        Some(a)
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
        // 60k of prefix on the first call: over the 60k floor.
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"p1","model":"claude-haiku-4-5","content":[],"usage":{"cache_read_input_tokens":60000,"input_tokens":10}}}"#).unwrap());
        let a = BigPrefix.evaluate(&s).expect("fires");
        assert!(
            a.headline.starts_with("Fixed prefix 60k tokens ≈$"),
            "{}",
            a.headline
        );
        assert!(a.headline.ends_with("/turn at 1 calls"), "{}", a.headline);
        assert_eq!(a.action_kind, ActionKind::Setting);
        assert_eq!(a.saving, Saving::Tokens(12_000));
        // 30k on Opus at one call per turn: $0.015/turn, quiet.
        let mut small = State::new(Pricing::bundled());
        small.apply(&prompt("2026-01-01T00:00:00Z"));
        small.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"p1","model":"claude-opus-5","content":[],"usage":{"cache_read_input_tokens":30000,"input_tokens":10}}}"#).unwrap());
        assert!(BigPrefix.evaluate(&small).is_none());
    }
}
