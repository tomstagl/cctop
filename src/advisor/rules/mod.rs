//! The rule catalog. Each rule is a predicate over `State` with the wording
//! from the coach PRD (§5); they fire only on evidence from this session
//! and never read prompt text. `token` holds the token-axis rules, `events`
//! the ones keyed on Claude Code's own event lines (compaction, permission,
//! rate limit, hooks), `outcome` the outcome-and-rework axis (Phase 6).

pub mod events;
pub mod outcome;
pub mod token;

use super::Rule;
use crate::metrics::Turn;
use crate::ui::State;

/// Every rule, in id order.
pub fn all() -> Vec<Box<dyn Rule>> {
    let mut v: Vec<Box<dyn Rule>> = Vec::new();
    v.extend(token::all());
    v.extend(events::all());
    v.extend(outcome::all());
    v.sort_by_key(|r| r.id());
    v
}

/// A small fixed phrase a rule chooses in code and puts into a `{hole}` of
/// its words: listed here, in one table, so that `docs/coach-copy.md` shows
/// every word the person reads (mod-pane PRD US-013).
pub struct Phrase {
    pub rule: &'static str,
    /// The hole (or part of it) the phrase fills.
    pub slot: &'static str,
    /// Which case picks it.
    pub key: &'static str,
    pub text: &'static str,
}

const fn p(
    rule: &'static str,
    slot: &'static str,
    key: &'static str,
    text: &'static str,
) -> Phrase {
    Phrase {
        rule,
        slot,
        key,
        text,
    }
}

pub const PHRASES: &[Phrase] = &[
    // A01: what changed (`cause`) and the habit that avoids it next time.
    p("A01", "cause", "model", "the model changed"),
    p("A01", "habit", "model", "Pick the model at the start of a task, or switch right after /clear while the context is small."),
    p("A01", "cause", "messages", "an earlier message changed (a rewind or an edited turn)"),
    p("A01", "habit", "messages", "Steering forward with a new message is cheaper than rewriting an earlier one."),
    p("A01", "cause", "expired", "the cache had expired between turns"),
    p("A01", "habit", "expired", "Replies within the cache's lifetime keep it warm; a longer break costs one rebuild like this."),
    p("A01", "cause", "tools", "the tool list changed: an MCP server connected or Claude loaded a deferred tool"),
    p("A01", "habit", "tools", "Next time, connect the tools you need early in the session, then leave the list alone."),
    p("A01", "cause", "tools_old", "the tool list changed: an MCP server connected or Claude loaded a deferred tool"),
    p("A01", "habit", "tools_old", "This Claude Code is older than 2.1.267 and rebuilds the list often: run claude update."),
    p("A01", "cause", "system", "the system prompt changed (a settings, CLAUDE.md or output-style edit)"),
    p("A01", "habit", "system", "Make such edits between sessions, not in the middle of one."),
    p("A01", "cause", "effort", "the effort level changed"),
    p("A01", "habit", "effort", "Set the effort at the start of a session, or right after /clear."),
    p("A01", "cause", "betas", "Claude Code's request features changed (an update or a feature toggle)"),
    p("A01", "habit", "betas", "Update Claude Code between sessions."),
    p("A01", "cause", "setting", "a request setting changed"),
    p("A01", "habit", "setting", "Keep request settings fixed within a session."),
    // A23: how the sentence opens.
    p("A23", "when", "clean", "This is a clean stop"),
    p("A23", "when", "not_clean", "At the next clean stop"),
    // A41: what Claude is waiting on, and what to do about it.
    p("A41", "what", "question", "Claude asked a question"),
    p("A41", "what", "permission", "A permission dialog opened"),
    p("A41", "what", "notification", "Claude asked for your input"),
    p("A41", "what", "asked", "The turn ended with a question"),
    p("A41", "what", "elicitation", "An MCP server asked you something"),
    p("A41", "hint", "permission", "Approve, deny, or choose 'always' for this kind of command."),
    p("A41", "hint", "other", "A one-word reply is enough; /btw asks a side question first if you need a detail."),
    // A42: why this is a moment to start fresh.
    p("A42", "why", "task_done", "A task just completed"),
    p("A42", "why", "done_prompt", "You said the work is done"),
    p("A42", "why", "question_ended", "The turn ended with a question"),
    // A48: the reason agents' work did not come back (one, and counted).
    p("A48", "why_one", "failed", "agent that failed"),
    p("A48", "why_many", "failed", "agents that failed"),
    p("A48", "why_one", "killed", "agent that was stopped"),
    p("A48", "why_many", "killed", "agents that were stopped"),
    p("A48", "why_one", "no_return", "agent that returned nothing"),
    p("A48", "why_many", "no_return", "agents that returned nothing"),
    p("A48", "why_one", "idle", "agent that went quiet mid-run"),
    p("A48", "why_many", "idle", "agents that went quiet mid-run"),
];

/// The phrase a rule picks for `slot` in the case `key`.
pub fn phrase(rule: &str, slot: &str, key: &str) -> &'static str {
    match PHRASES
        .iter()
        .find(|p| p.rule == rule && p.slot == slot && p.key == key)
    {
        Some(p) => p.text,
        None => {
            debug_assert!(false, "no phrase {rule} {slot} {key}");
            ""
        }
    }
}

/// Explanation shown for `Enter` / `e` on an advice.
pub fn explain(doc_key: &str) -> &'static str {
    match doc_key {
        "A01" => "A named cache miss means the API could not reuse the cached prefix: a model switch (`model_changed`) re-writes the entire context at the cache-write price, a changed tool list (`tools_changed`: an MCP server connecting, ToolSearch loading a deferred tool, a CLI older than 2.1.267 rebuilding the list) re-writes everything after the tools, and a rewritten earlier message (`messages_changed`: a rewind or an edited turn) re-writes from that point. Each is a one-off cost the size of the context; the fix is timing — switch models and load tools early, or right after /clear.",
        "A02" => "A cache entry lives 5 minutes (or 1 hour, as the API reports for this session). After that, the next request re-writes the entire context. Batching questions or answering within the TTL avoids the cold write; a cold turn on a 400k context costs as much as ~20 warm ones.",
        "A03" => "Every byte of a tool result stays in context for the rest of the session and is re-read (and billed) on every later request; `/context` flags a tool once its results pass 15 % of the window. Narrow the command (head, grep, Glob) or delegate the exploration to a subagent that returns a summary. A result Claude Code spilled to disk (`<persisted-output>`) kept only its head; the rest cost the run, not the context.",
        "A04" => "Reading a file again re-injects it whole. After an Edit the new content is already in the tool result, so an edit → re-read → edit sequence pays for the file twice. A Read with offset/limit on the relevant range costs a fraction; better, ask Claude to keep the lines it needs in its own summary.",
        "A19" => "The cache entry that holds your context expires on a fixed clock (5 minutes, or 1 hour when the session runs on the longer TTL). While it is warm, a reply costs a cache read of the whole context; once it expires, the next request re-writes it at the cache-write price. Answering a pending question before the countdown ends keeps the entry alive; if the task is done, a hand-off note and /clear is cheaper than a warm keep-alive.",
        "A21" => "Switching the model (or /fast) while the cache is warm throws the entry away: the new model cannot read the old one's cache, so the next request re-writes the entire context at the cache-write price. Claude Code confirms a warm switch; the priced figure and the undo are what cctop adds — switch back before the entry dies, or switch right after a /clear when the context is small.",
        "A21b" => "When the cache is cold anyway, the next request re-writes the context whatever you do: a model or effort switch costs nothing extra at that moment. Pick the model for the next stretch and hold it until the next /clear.",
        "A23" => "Every API call re-reads the whole context at the cache-read price, and a turn is many calls. Past 300 k on a 1M window each call costs several times what it did at 100 k, so a clean stop (end_turn, nothing in the background) is the moment to /compact with a focus or to write a hand-off note and /clear. The projection uses this session's own calls per turn, never a constant.",
        "A27" => "A /loop or /goal wakes the session on a timer, and each wake-up sends the whole context again as a cache read even when nothing happened. On a large context that is millions of tokens an hour of idle time: /clear before arming the loop, widen the interval, or narrow it to the check it needs.",
        "A30" => "The session sat idle longer than the cache TTL with a large context: the entry is gone and the next message re-writes it all. If the task is done, a hand-off note and /clear start the next one at a fraction of the size; if not, /compact halves what comes back.",
        "A25" => "A run of read-only calls (Read, Grep, Glob, `cat`, `rg`, `git log`…) in the main thread fills the main context with raw files that persist until compaction. An Explore subagent does the same search in its own context and hands back a paragraph. The counter resets on any write or an Agent spawn.",
        "A06" => "Each compaction spends output tokens on a summary and loses detail the model then re-reads. When the compacted context is still near the threshold it re-triggers within a few turns. `/compact <focus>` at a clean stop, while the cache is warm, keeps what matters; two compactions in one session usually cost more than a hand-off note and a fresh session.",
        "A07" => "Every MCP server's tool schemas and every enabled plugin's always-on text are part of the prefix sent on each request, whether or not they are used, and each ToolSearch load of a deferred tool rewrites the cached prefix. A server or plugin with no calls for a long stretch is pure overhead: disable it for this project (.mcp.json, /plugin) or rely on deferred loading.",
        "A08" => "Thinking tokens are billed as output. On routine edits the model rarely needs deep reasoning; a lower effort level cuts thinking without changing the edit. Reserve high effort for design and debugging. On models other than Fable the effort level is part of the system prompt, so change it at a boundary (after /clear) to avoid a cache re-write.",
        "A09" => "While a permission prompt is open the model does nothing and the wall clock runs. A command shape approved again and again is a candidate for the allow-list (permissions.allow in settings); Claude Code's own suggestion (`permission_suggestions[].ruleContent`) is the safest rule, and a bare `Bash` rule is never it.",
        "A10" => "A foreground Bash call blocks the turn until it exits. For builds and test suites that take more than a minute, run them in the background (`run_in_background`): Claude keeps working and is notified when the command finishes. Calls Claude Code backgrounded itself do not count.",
        "A11" => "A large paste is new context: on a cached session Claude Code writes it into the cache on that turn's first call (`cache_creation_input_tokens`, at the cache-write price) and every later call re-reads it; without a cache it is billed as plain input. cctop sizes the paste from history.jsonl — the inline characters, or the composer's `+N lines` placeholder when the text is only a hash — never from its text, and matches it to the turn by time, so a paste queued mid-turn counts too. Point Claude at the file (it reads only what it needs) or paste the relevant slice.",
        "A12" => "Every turn re-sends the whole context, cache reads included. Many tiny prompts multiply that cost; one prompt with several instructions costs a single context read and usually gets a more coherent answer.",
        "A13" => "Rate limits are account-wide and reset on a fixed schedule. If the current burn reaches 100 % before the reset, the session stops mid-task. Claude Code's own levers: `/model sonnet` roughly doubles the runway, `/effort medium` trims output; moving exploration to cheaper subagents or pausing heavy work until the reset avoids the cut-off.",
        "A48" => "An agent's calls are billed whether or not its result comes back. A `failed` or `killed` agent, one that completed with nothing in its result, and one that has gone quiet mid-run are money the session spent for no answer; the agents view (6, Enter) lists each with its cost and the reason, and the task notification's own status is what it goes by. Before launching another of the same kind, look at why the last ones did not return — a prompt that asks for a result, a narrower task, or the parent doing the work itself.",
        "A14" => "Search, listing and summarising subagents do not need the most capable model. A subagent that made many searches and no edits runs equally well on Sonnet or Haiku at a fraction of the cost, and leaves Opus for the reasoning-heavy main thread. Explore agents already default to a cheaper model on recent versions; other agent types inherit the session's model unless told otherwise.",
        "A16" => "Hooks run synchronously inside the turn. A slow PostToolUse or Stop hook adds its full duration to every tool call or turn. Make expensive hooks asynchronous (`\"async\": true`), or narrow their matcher to the tools and paths they care about.",
        "A17" => "The fixed prefix (system prompt, CLAUDE.md, tool schemas, skills, plugins) is sent on every request. Even as a cache read it is billed on every call, so its cost per turn is prefix × cache-read price × calls per turn; it fires from 50 k tokens whatever the window, because the cost is absolute — once a /context you ran has measured the prefix, since the first call's own figure overstates it by about a third. Trim CLAUDE.md, move rarely used rules into skills, and disable MCP servers and plugins you do not use in this project. On the Context panel, i breaks the prefix down and m shows what else fills the window.",
        "A32" => "Source edits with no test, build or type-check between them accumulate unverified assumptions; the later a failure surfaces, the more of the turn has to be redone. A test run right after the edit is cheap (its output is short) and lets the model fix the failure while the file is still in context. Reads of an edited file count as a check too — only when Claude re-reads the whole result. Documentation edits and sessions that have never run a test are exempt.",
        "A33" => "A commit that lands with unchecked source edits, or right after a failing test, is a rework trap: the next push runs CI on it and the fix arrives as a second commit or a force-push. Run the session's own test command before the push and amend. Commits of documentation only are exempt.",
        "A34" => "A feature-sized prompt (long, or naming several files with an implementation verb) sent with plan mode off makes the model act on its first interpretation. Plan mode (Shift+Tab) or a 'plan first' line costs one short turn and produces a checklist you can correct before any file changes; the edits then follow the corrected plan. This is the next row only: it never takes the slot.",
        "A36" => "Two corrections close together — an Esc and a short steer, a refused call, feedback on a call — on the same work mean the model's understanding drifted and the steers are patching it. Esc Esc (or /rewind) restores the conversation and the files to the checkpoint before the drift, so the restated prompt starts clean; bash-written files are not covered by the checkpoint. cctop counts only structural markers, never keywords in the prompt.",
        "A38" => "Three failed calls in a row with two of them the same command shape is a blind retry: the model repeats the same guess with small variations and each failure adds its error text to the context. Interrupt and supply the missing fact (a path, a flag, an environment detail), or run the command yourself and paste the first twenty lines. Denials are not counted here.",
        "A40" => "A pull request with a substantial diff and no review pass carries whatever the writing session assumed. /code-review runs in a fresh context that has not seen the reasoning behind the diff and checks the change on its own terms; a review agent or review skill this session counts as done. Once per pull request.",
        "A41" => "When Claude asks a question, waits on a permission dialog, or ends the turn with a question, nothing happens until you answer — and the cache entry holding the context keeps expiring on its clock. A one-word reply within the warm window keeps the whole context at the cache-read price; a reply after the entry died re-writes it all.",
        "A42" => "A completed task, a 'done' or 'next' from you, or a turn that ended with a question is a natural boundary. On a large context every later call re-reads everything before the boundary; a hand-off note, /rename and /clear start the next task at the size of the prefix. Never on a commit alone, never while background tasks or a loop are still running.",
        "A43" => "Rewind restores files that Claude edited with Edit and Write, not files that a shell command changed. A git command that rewrites the tree (reset --hard, checkout -- ., clean -f, stash drop, push --force) on a dirty tree can lose work no checkpoint holds; git reflog and git stash list are where it may still be.",
        "A44" => "When your editor and Claude write the same file in one turn, one of them is working from a stale copy; the next Edit fails on old_string or silently drops your change. Say which version wins before the next edit, and let Claude re-read the file.",
        "A45" => "Auto mode blocks a command shape by a classifier; a second block of the same shape within a turn (or three turns) means the model will keep trying variants, and three blocks in a row pause auto mode. An allow rule — Claude Code's own suggestion when it made one, else Tool(argv0 subcommand:*) — lets the shape through; a bare Bash rule allows everything and is never suggested. A settings deny rule is different: the model cannot run it, tell it the alternative.",
        "A46" => "On a 1M window the model's attention to instructions given hundreds of thousands of tokens ago fades, and Claude Code re-injects CLAUDE.md only at boundaries. When the context crossed 150 k, 300 k or 450 k with your last long prompt far behind and nothing re-injected since, restating the three constraints that matter (or moving them to CLAUDE.md) is cheaper than the drift. The next row only: it never takes the slot.",
        "A47" => "The turn ended on an API error, not on a reply. A rate limit (429) resets on the account's clock — another model family has its own limit; a spend limit or an authentication failure stops everything until it is fixed; 'prompt too long' means the context exceeds the window and only /compact or /clear helps; a server error is retried by Claude Code itself. Re-sending into the same error costs the wait again.",
        _ => "No explanation for this rule yet.",
    }
}

/// The last `n` turns that made an API call, newest first.
pub(crate) fn recent_turns(state: &State, n: usize) -> Vec<&Turn> {
    state
        .agg
        .turns
        .iter()
        .filter(|t| t.api_calls > 0)
        .rev()
        .take(n)
        .collect()
}

/// Human turns completed after all-turn number `turn`.
pub(crate) fn human_turns_since(state: &State, turn: usize) -> usize {
    state
        .agg
        .turns
        .iter()
        .filter(|t| t.human && t.number > turn)
        .count()
}

/// USD for `tokens` at the current model's price of `kind`.
pub(crate) fn usd(state: &State, tokens: u64, kind: PriceKind) -> Option<f64> {
    let p = state.cost.pricing().price(state.model()?)?;
    let per_m = match kind {
        PriceKind::CacheRead => p.cache_read(),
        PriceKind::CacheWrite => match state.agg.observed_ttl {
            Some(crate::transcript::CacheTtl::OneHour) => p.cache_write_1h(),
            _ => p.cache_write_5m(),
        },
        PriceKind::Input => p.input,
    };
    Some(tokens as f64 * per_m / 1e6)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PriceKind {
    CacheRead,
    CacheWrite,
    Input,
}

/// `≈$4.7` or nothing when the model has no price.
pub(crate) fn usd_label(state: &State, tokens: u64, kind: PriceKind) -> String {
    usd(state, tokens, kind)
        .map(|v| format!(" ≈{}", crate::ui::fmt::usd(v)))
        .unwrap_or_default()
}

/// The model's short name (`opus-5`).
pub(crate) fn model_short(state: &State) -> String {
    state
        .model()
        .map(crate::ui::fmt::model_short)
        .unwrap_or_else(|| "?".into())
}

/// The session's p50 calls per turn (turns with ≥ 1 call), floor 1.
pub(crate) fn calls_per_turn(state: &State) -> f64 {
    crate::metrics::cost::p50_calls_per_turn(&state.agg).max(1.0)
}

/// `$0.70` (the words' money form).
pub(crate) fn money(v: f64) -> String {
    crate::ui::fmt::usd(v)
}

/// `, about $0.70` at the current model's price of `kind`, or nothing:
/// the words' optional price clause.
pub(crate) fn about_usd(state: &State, tokens: u64, kind: PriceKind) -> String {
    usd(state, tokens, kind)
        .map(|v| format!(", about {}", money(v)))
        .unwrap_or_default()
}

/// A duration in words: `40 seconds`, `4 minutes`, `2 hours 10 minutes`.
pub(crate) fn spoken(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let unit = |n: i64, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    if s < 90 {
        unit(s, "second")
    } else if s < 3600 {
        unit((s + 30) / 60, "minute")
    } else {
        let (h, m) = (s / 3600, (s % 3600) / 60);
        if m == 0 {
            unit(h, "hour")
        } else {
            format!("{} {}", unit(h, "hour"), unit(m, "minute"))
        }
    }
}

/// A structural name (a file's basename, a server, hook, plugin, agent or
/// tool name) as the words carry it: cut at 40 cells.
pub(crate) fn ident(s: &str) -> String {
    crate::ui::fmt::clip(s, 40)
}

/// `1 file`, `3 files`.
pub(crate) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `41 %`.
pub(crate) fn percent(v: f64) -> String {
    format!("{v:.0} %")
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Transcript lines the rule tests share.
    use crate::transcript::Line;

    pub fn prompt(ts: &str) -> Line {
        Line::parse(&format!(
            r#"{{"type":"user","timestamp":"{ts}","promptSource":"typed","message":{{"role":"user","content":"go"}}}}"#
        ))
        .unwrap()
    }

    pub fn response(id: &str, ts: &str, input: u64, write: u64, read: u64) -> Line {
        Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}],"stop_reason":"end_turn","usage":{{"input_tokens":{input},"cache_creation_input_tokens":{write},"cache_read_input_tokens":{read},"output_tokens":10}}}}}}"#)).unwrap()
    }

    /// A tool call and its result of `result_bytes` bytes.
    pub fn tool(id: &str, ts: &str, name: &str, input: &str, result_bytes: usize) -> [Line; 2] {
        let field = if name == "Bash" {
            "command"
        } else {
            "file_path"
        };
        [
            Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{{"{field}":"{input}"}}}}],"usage":{{"output_tokens":1}}}}}}"#)).unwrap(),
            Line::parse(&format!(r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{}"}}]}}}}"#, "x".repeat(result_bytes))).unwrap(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_sorted_and_explained() {
        let ids: Vec<&str> = all().iter().map(|r| r.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        for id in &ids {
            assert_ne!(explain(id), explain("nope"), "{id} has no explanation");
        }
        assert!(explain("A06").contains("/compact"));
        assert!(explain("nope").starts_with("No explanation"));
    }

    /// `A38`, `A21b`: a rule id anywhere in `s`.
    fn has_rule_id(s: &str) -> bool {
        let b = s.as_bytes();
        b.windows(3)
            .any(|w| w[0] == b'A' && w[1].is_ascii_digit() && w[2].is_ascii_digit())
    }

    /// Every nudge the rules fire on the fixture sessions, fed line by line
    /// and read every few lines at two clocks (live, and ten minutes idle):
    /// whole words, no hole left, and an action payload that fits its kind
    /// — something to fill, copy or press, empty only for `read`.
    #[test]
    fn every_fired_nudge_has_whole_words_and_fitting_payloads() {
        use crate::advisor::ActionKind;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let rules = all();
        let mut fired = std::collections::BTreeSet::new();
        for name in ["a", "b", "c", "d", "e", "r", "s", "t", "w", "w-live"] {
            let path = root.join(format!("session-{name}.jsonl"));
            let parsed = crate::transcript::parse_file(&path).unwrap();
            let step = (parsed.len() / 40).max(1);
            let mut s = State::new(crate::metrics::Pricing::bundled());
            s.session = crate::ui::state::SessionInfo::from_fixture(&path);
            for (i, line) in parsed.iter().enumerate() {
                s.apply(line);
                let lines = i + 1;
                if lines % step != 0 && lines != parsed.len() {
                    continue;
                }
                for idle_ms in [30_000, 600_000] {
                    s.clock_override = true;
                    s.now_ms = s.last_line_at_ms.unwrap_or(0) + idle_ms;
                    for r in &rules {
                        let Some(a) = r.evaluate(&s) else { continue };
                        fired.insert(r.id());
                        let at = format!("{} on session-{name} at {lines}", r.id());
                        assert_eq!(a.name, r.name(), "{at}");
                        for text in [&a.body, &a.foot].into_iter().chain(a.fix_prompt.as_ref()) {
                            assert!(!text.trim().is_empty(), "{at}: empty words");
                            assert!(!text.contains('{') && !text.contains('}'), "{at}: {text}");
                        }
                        assert!(!a.actions.is_empty(), "{at}");
                        for x in &a.actions {
                            assert_eq!(
                                x.kind == ActionKind::Advice,
                                x.payload.trim().is_empty(),
                                "{at}: {x:?}"
                            );
                            assert!(!x.label.is_empty(), "{at}: {x:?}");
                            if x.kind == ActionKind::Prompt {
                                assert!(
                                    Some(&x.payload) == a.fix_prompt.as_ref()
                                        || x.payload == a.action_text,
                                    "{at}: {x:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(fired.len() >= 15, "only {fired:?} fired");
    }

    #[test]
    fn every_rule_has_a_plain_name_and_words() {
        let rules = all();
        assert_eq!(rules.len(), 36);
        let mut names = std::collections::BTreeSet::new();
        for r in &rules {
            let (id, name) = (r.id(), r.name());
            assert!(!name.is_empty(), "{id} has no name");
            assert!(name.chars().count() <= 32, "{id}: {name:?} is over 32");
            assert!(!has_rule_id(name), "{id}: {name:?} carries a rule id");
            assert!(names.insert(name), "{id}: {name:?} is not unique");
            let words = r.words();
            assert!(
                words.iter().any(|w| w.variant.is_empty()),
                "{id} has no default words"
            );
            let mut variants = std::collections::BTreeSet::new();
            for w in words {
                assert!(
                    variants.insert(w.variant),
                    "{id}: variant {:?} twice",
                    w.variant
                );
                for text in [w.body, w.foot].into_iter().chain(w.fix_prompt) {
                    assert!(!text.trim().is_empty(), "{id} {:?}: empty words", w.variant);
                    assert!(!has_rule_id(text), "{id}: {text:?} carries a rule id");
                    // Every hole is a plain name.
                    let mut rest = text;
                    while let Some(i) = rest.find('{') {
                        let j = rest[i..].find('}').expect("closed hole") + i;
                        let hole = &rest[i + 1..j];
                        assert!(
                            !hole.is_empty()
                                && hole.chars().all(|c| c.is_ascii_lowercase()
                                    || c.is_ascii_digit()
                                    || c == '_'),
                            "{id}: bad hole {hole:?}"
                        );
                        rest = &rest[j + 1..];
                    }
                }
                assert!(
                    w.body.ends_with('.') || w.body.ends_with('}'),
                    "{id}: {:?}",
                    w.body
                );
            }
        }
    }
}
