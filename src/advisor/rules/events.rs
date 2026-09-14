//! Rules keyed on Claude Code's own event lines: compaction boundaries,
//! permission prompts, long foreground calls, the rate-limit fit, hook
//! timings (A06, A09, A10, A13, A16).

use super::recent_turns;
use crate::advisor::{ActionKind, Advice, Rule, Saving, Urgency};
use crate::harness_facts::autocompact;
use crate::ui::fmt;
use crate::ui::State;

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(PostCompaction),
        Box::new(PermissionWaits),
        Box::new(LongForeground),
        Box::new(RateLimitPacing),
        Box::new(HookOverhead),
    ]
}

/// A06 — a compaction happened (exact `compact_boundary`; the ≥ 30 % drop
/// heuristic only before 2.1.263) and either the compacted context is still
/// within 20 k of the threshold, or it was the second one.
pub struct PostCompaction;
impl Rule for PostCompaction {
    fn id(&self) -> &'static str {
        "A06"
    }
    fn family(&self) -> &'static str {
        "post-compaction"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let v = state.context();
        let last = v.compactions.last()?;
        let retrigger = last.after + autocompact::WARN_TOKENS >= v.threshold;
        if v.compactions.len() < 2 && !retrigger {
            return None;
        }
        let approx = if v.compactions_heuristic { " ≈" } else { "" };
        let mut a = Advice::new("A06", "post-compaction", Urgency::Next);
        a.headline = if retrigger {
            format!(
                "Compaction will re-trigger: {} left after it, threshold {}{approx}",
                fmt::tokens(last.after),
                fmt::tokens(v.threshold)
            )
        } else {
            format!(
                "{} compactions this session ({} → {} last){approx}",
                v.compactions.len(),
                fmt::tokens(last.before),
                fmt::tokens(last.after)
            )
        };
        a.evidence = format!(
            "each one spends output on a summary and loses detail the model re-reads; ctx now {}",
            fmt::tokens(v.size)
        );
        a.action = "at the next clean stop: /compact <focus> while the cache is warm, or a hand-off note + /clear".into();
        a.action_text = "/compact ".into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::Avoids;
        a.retires_on = "a /compact or /clear";
        a.mark = boundaries_and_compacts(state);
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        boundaries_and_compacts(state) > fired.mark
    }
}

/// Clears, forks and manual `/compact`s so far.
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
            )
        })
        .count();
    let compacts = state
        .agg
        .slash_commands
        .iter()
        .filter(|(_, c)| c.starts_with("/compact"))
        .count();
    (clears + compacts) as u64
}

/// A09 — one command shape approved ≥ 3 times, or > 2 min spent on
/// permission prompts; the rule text is Claude Code's own suggestion.
pub struct PermissionWaits;
impl Rule for PermissionWaits {
    fn id(&self) -> &'static str {
        "A09"
    }
    fn family(&self) -> &'static str {
        "permission-wait"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let s = &state.session;
        let (key, ask) = s
            .permission_asks
            .iter()
            .filter(|(k, _)| k.as_str() != "Bash" && k.as_str() != "tool")
            .max_by_key(|(_, a)| a.count)?;
        if ask.count < 3 && s.permission_wait_ms <= 120_000 {
            return None;
        }
        let rule = ask
            .rule
            .clone()
            .filter(|r| r.contains('('))
            .unwrap_or_else(|| {
                if key.starts_with("Bash(") {
                    format!("{}:*)", key.trim_end_matches(')'))
                } else {
                    key.clone()
                }
            });
        if state.allow_rules.iter().any(|r| r == &rule) {
            return None;
        }
        let per_turn_s = s.permission_wait_ms as u64 / 1000 / state.agg.human_turns().max(1) as u64;
        let mut a = Advice::new("A09", "permission-wait", Urgency::Next);
        a.headline = format!(
            "You've approved {} {} times ({} waiting)",
            key.trim_start_matches("Bash(").trim_end_matches(')'),
            ask.count,
            fmt::duration_ms(s.permission_wait_ms)
        );
        a.evidence = format!(
            "{} permission prompts this session; the model idles while it waits",
            s.permission_waits
        );
        a.action = format!(
            "allow {rule} (permissions.allow in settings), or approve it once with 'always'"
        );
        a.action_text = rule;
        a.action_kind = ActionKind::AllowRule;
        a.saving = Saving::Seconds(per_turn_s.max(1));
        a.retires_on = "the rule appearing in permissions.allow";
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state.allow_rules.iter().any(|r| r == &fired.action_text)
    }
}

/// A10 — a foreground Bash call longer than 60 s in the last five turns
/// (calls Claude Code backgrounded itself do not count).
pub struct LongForeground;
impl Rule for LongForeground {
    fn id(&self) -> &'static str {
        "A10"
    }
    fn family(&self) -> &'static str {
        "long-foreground"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let now = state.clock_ms();
        let first_turn = recent_turns(state, 5).last().map(|t| t.number).unwrap_or(0);
        let (call, ms) = state
            .tools
            .calls
            .iter()
            .filter(|c| c.name == "Bash" && !c.background && c.turn >= first_turn)
            .map(|c| {
                (
                    c,
                    c.duration_ms
                        .map(|d| d as i64)
                        .or_else(|| c.started_at.map(|s| now - s))
                        .unwrap_or(0),
                )
            })
            .filter(|(_, d)| *d > 60_000)
            .max_by_key(|(_, d)| *d)?;
        let mut a = Advice::new("A10", "long-foreground", Urgency::Later);
        a.headline = format!(
            "`{}` blocked the turn for {}",
            fmt::clip(&call.input_summary, 30),
            fmt::duration_ms(ms)
        );
        a.evidence = "a foreground Bash call longer than a minute; Claude waited for it".into();
        a.action =
            "queue: 'run builds and test suites longer than a minute in the background'".into();
        a.action_text =
            "Run builds and test suites longer than a minute in the background and keep working meanwhile."
                .into();
        a.action_kind = ActionKind::Prompt;
        a.saving = Saving::Seconds((ms / 1000) as u64);
        a.window_turns = 5;
        a.mark = background_calls(state);
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        background_calls(state) > fired.mark
    }
}

fn background_calls(state: &State) -> u64 {
    state
        .tools
        .calls
        .iter()
        .filter(|c| c.name == "Bash" && c.background)
        .count() as u64
}

/// A13 — the 5 h limit is projected to run out before it resets. A limit
/// already hit is A47's (turn died), not a pacing matter.
pub struct RateLimitPacing;
impl Rule for RateLimitPacing {
    fn id(&self) -> &'static str {
        "A13"
    }
    fn family(&self) -> &'static str {
        "rate-limit"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let mut a = Advice::new("A13", "rate-limit", Urgency::Next);
        let model = state.model().unwrap_or("");
        let lever = if model.contains("sonnet") || model.contains("haiku") {
            ("/effort medium", "/effort medium trims output")
        } else {
            (
                "/model sonnet",
                "/model sonnet ≈ 2× the runway, /effort medium trims output",
            )
        };
        if state.rate_limit_hit().is_some() {
            return None; // A47 owns the hit itself
        }
        let l = state.limits.as_ref()?;
        let (ex, reset) = (l.exhaustion_ms?, l.five_hour_resets_at_ms?);
        if ex >= reset {
            return None;
        }
        a.headline = format!(
            "At this burn you hit the 5 h limit {} before it resets",
            fmt::duration_ms(reset - ex)
        );
        a.evidence = format!(
            "{:.0} % used, exhausted in {}{}",
            l.five_hour_pct,
            fmt::duration_ms(ex - state.clock_ms()),
            if l.exhaustion_in_active_hours == Some(false) {
                " · past your usual hours"
            } else {
                ""
            }
        );
        a.action = format!(
            "{}; or move exploration to subagents and pause the heavy work until the reset",
            lever.1
        );
        a.action_text = lever.0.into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::Avoids;
        a.retires_on = "a cheaper model or effort";
        a.mark = (crate::harness_facts::usage_weight::tier(model) * 10.0) as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        let tier =
            (crate::harness_facts::usage_weight::tier(state.model().unwrap_or("")) * 10.0) as u64;
        tier < fired.mark
    }
}

/// A16 — hook time > 10 % of turn time over the last five finished turns,
/// naming the slowest hook (`cctop hook` itself excluded).
pub struct HookOverhead;
impl Rule for HookOverhead {
    fn id(&self) -> &'static str {
        "A16"
    }
    fn family(&self) -> &'static str {
        "hook-overhead"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Later
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let turns: Vec<_> = state
            .agg
            .turns
            .iter()
            .filter(|t| t.duration_ms.is_some() && t.hook_runs > 0)
            .rev()
            .take(5)
            .collect();
        if turns.is_empty() {
            return None;
        }
        let hook: u64 = turns.iter().map(|t| t.hook_ms).sum();
        let total: u64 = turns.iter().filter_map(|t| t.duration_ms).sum();
        if total == 0 || (hook as f64) / (total as f64) <= 0.10 {
            return None;
        }
        // The slowest hook by name, cctop's own spool hook excluded.
        let slowest = state
            .agg
            .hook_ms_by_command
            .iter()
            .filter(|(cmd, _)| !cmd.contains("cctop hook"))
            .max_by_key(|(_, ms)| **ms)
            .map(|(cmd, ms)| (fmt::clip(cmd, 30), *ms))?;
        let per_turn = hook / turns.len() as u64;
        let mut a = Advice::new("A16", "hook-overhead", Urgency::Later);
        a.headline = format!(
            "Hooks add {} per turn ({:.0} % of turn time)",
            fmt::short_ms(per_turn),
            hook as f64 / total as f64 * 100.0
        );
        a.evidence = format!(
            "slowest: `{}` {} in total · {} hook runs over {} turns",
            slowest.0,
            fmt::short_ms(slowest.1),
            turns.iter().map(|t| t.hook_runs).sum::<usize>(),
            turns.len()
        );
        a.action = format!(
            "make `{}` async (\"async\": true) or narrow its matcher to the tools and paths it needs",
            slowest.0
        );
        a.action_kind = ActionKind::Setting;
        a.saving = Saving::Seconds((per_turn / 1000).max(1));
        a.window_turns = turns.len();
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
    fn a06_post_compaction_exact_boundary_and_retrigger() {
        // 2.1.270: only compact_boundary lines count, never a drop.
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","version":"2.1.270","message":{"id":"a","model":"claude-opus-5","content":[],"usage":{"input_tokens":900000}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","version":"2.1.270","message":{"id":"b","model":"claude-opus-5","content":[],"usage":{"input_tokens":200000}}}"#).unwrap());
        assert!(
            PostCompaction.evaluate(&s).is_none(),
            "a drop is not a compaction on 2.1.270"
        );
        // One exact compaction that left the context near the threshold.
        s.apply(&Line::parse(r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-01-01T00:00:03Z","version":"2.1.270","compactMetadata":{"trigger":"auto","preTokens":960000,"postTokens":955000,"durationMs":80000}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:04Z","version":"2.1.270","message":{"id":"c","model":"claude-opus-5","content":[],"usage":{"input_tokens":955000}}}"#).unwrap());
        let a = PostCompaction.evaluate(&s).expect("fires");
        assert_eq!(
            a.headline, "Compaction will re-trigger: 955k left after it, threshold 960k",
            "threshold learned from the observed compaction"
        );
        assert_eq!(a.action_text, "/compact ");
        assert_eq!(a.saving, Saving::Avoids);
        assert!(!PostCompaction.acted(&s, &a));
        // A /clear (continued-in) counts as acted.
        s.apply(&Line::parse(r#"{"type":"continued-in","timestamp":"2026-01-01T00:00:05Z","continuedInSessionId":"next"}"#).unwrap());
        assert!(PostCompaction.acted(&s, &a));
        // Two compactions that left plenty of room still fire (second time).
        let mut two = State::new(Pricing::bundled());
        for i in 0..2 {
            two.apply(&prompt("2026-01-01T00:00:00Z"));
            two.apply(&Line::parse(&format!(r#"{{"type":"system","subtype":"compact_boundary","timestamp":"2026-01-01T00:0{i}:03Z","version":"2.1.270","compactMetadata":{{"trigger":"auto","preTokens":960000,"postTokens":230000,"durationMs":80000}}}}"#)).unwrap());
            two.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T00:0{i}:04Z","version":"2.1.270","message":{{"id":"c{i}","model":"claude-opus-5","content":[],"usage":{{"input_tokens":230000}}}}}}"#)).unwrap());
        }
        let a = PostCompaction.evaluate(&two).expect("fires");
        assert_eq!(a.headline, "2 compactions this session (960k → 230k last)");
        // Fixture B: one exact compaction (567k → 230k on 1M): quiet.
        let mut b = State::new(Pricing::bundled());
        for l in crate::transcript::parse_file(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl"),
        )
        .unwrap()
        {
            b.apply(&l);
        }
        assert!(PostCompaction.evaluate(&b).is_none());
        assert!(PostCompaction.evaluate(&fixture_state()).is_none());
    }

    #[test]
    fn a09_permission_waits_use_claude_codes_rule_and_never_bare_bash() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        let ask = |s: &mut State, cmd: &str, suggestion: Option<&str>| {
            let mut payload = serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":cmd}});
            if let Some(r) = suggestion {
                payload["permission_suggestions"] = serde_json::json!([{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":r}]}]);
            }
            s.apply_hook(&crate::hooks::HookEvent::from_stdin(0, payload).unwrap());
        };
        ask(&mut s, "gh api repos/x", None);
        ask(&mut s, "gh api repos/y --jq .id", Some("gh api:*"));
        assert!(PermissionWaits.evaluate(&s).is_none(), "two asks");
        ask(&mut s, "gh api graphql", None);
        s.session.permission_waits = 3;
        s.session.permission_wait_ms = 45_000;
        let a = PermissionWaits.evaluate(&s).expect("fires");
        assert_eq!(a.headline, "You've approved gh api 3 times (0:45 waiting)");
        assert_eq!(
            a.action_text, "Bash(gh api:*)",
            "Claude Code's own suggestion, verbatim"
        );
        assert_eq!(a.action_kind, ActionKind::AllowRule);
        assert!(!PermissionWaits.acted(&s, &a));
        s.allow_rules.push("Bash(gh api:*)".into());
        assert!(PermissionWaits.acted(&s, &a));
        assert!(PermissionWaits.evaluate(&s).is_none(), "already allowed");
        // Without a suggestion the rule is built from the prefix; a bare
        // command never yields `Bash`.
        let mut t = State::new(Pricing::bundled());
        for _ in 0..3 {
            ask(&mut t, "cargo test --lib", None);
        }
        assert_eq!(
            PermissionWaits.evaluate(&t).unwrap().action_text,
            "Bash(cargo test:*)"
        );
        let mut bare = State::new(Pricing::bundled());
        for _ in 0..3 {
            bare.apply_hook(
                &crate::hooks::HookEvent::from_stdin(
                    0,
                    serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"}),
                )
                .unwrap(),
            );
        }
        bare.session.permission_wait_ms = 200_000;
        assert!(PermissionWaits.evaluate(&bare).is_none(), "never bare Bash");
    }

    #[test]
    fn a10_long_foreground_skips_backgrounded_calls() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"id":"b1","model":"m","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cargo build --release"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:01:58Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"ok"}]}}"#).unwrap());
        let a = LongForeground.evaluate(&s).expect("fires");
        assert_eq!(
            a.headline,
            "`cargo build --release` blocked the turn for 1:58"
        );
        assert_eq!(a.saving, Saving::Seconds(118));
        assert_eq!(a.action_kind, ActionKind::Prompt);
        // Acted: a later background call.
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:02:00Z","message":{"id":"b2","model":"m","content":[{"type":"tool_use","id":"b2","name":"Bash","input":{"command":"cargo test","run_in_background":true}}],"usage":{"output_tokens":1}}}"#).unwrap());
        assert!(LongForeground.acted(&s, &a));
        // A call Claude Code backgrounded itself (timedOutAfterMs) never fires.
        let mut bg = State::new(Pricing::bundled());
        bg.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"id":"b1","model":"m","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"npm run build"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        bg.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:03:00Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"moved to background"}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"timedOutAfterMs":120000,"backgroundTaskId":"b7"}}"#).unwrap());
        assert!(LongForeground.evaluate(&bg).is_none());
        let mut ok = State::new(Pricing::bundled());
        for l in tool("q", "2026-01-01T00:00:00Z", "Bash", "ls", 10) {
            ok.apply(&l);
        }
        assert!(LongForeground.evaluate(&ok).is_none());
    }

    #[test]
    fn a13_rate_limit_pacing_leaves_the_429_to_a47() {
        let mut s = State::new(Pricing::bundled());
        s.session.alive = true;
        s.now_ms = 1_000_000;
        s.limits = Some(crate::ui::state::Limits {
            five_hour_pct: 80.0,
            seven_day_pct: 10.0,
            five_hour_resets_at_ms: Some(2_000_000),
            seven_day_resets_at_ms: None,
            exhaustion_ms: Some(1_500_000),
            exhaustion_in_active_hours: None,
        });
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&response("m", "2026-01-01T00:00:01Z", 10, 0, 1000));
        let a = RateLimitPacing.evaluate(&s).expect("fires");
        assert_eq!(
            a.headline,
            "At this burn you hit the 5 h limit 8:20 before it resets"
        );
        assert_eq!(a.action_text, "/model sonnet");
        assert!(
            a.action.starts_with("/model sonnet ≈ 2× the runway"),
            "{}",
            a.action
        );
        assert_eq!(a.urgency, Urgency::Next);
        // Acted: a cheaper model family took over.
        assert!(!RateLimitPacing.acted(&s, &a));
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"n","model":"claude-sonnet-5","content":[],"usage":{"input_tokens":10}}}"#).unwrap());
        assert!(RateLimitPacing.acted(&s, &a));
        s.limits.as_mut().unwrap().exhaustion_ms = Some(2_500_000);
        assert!(
            RateLimitPacing.evaluate(&s).is_none(),
            "fits before the reset"
        );
        // A 429 that landed is A47's: the pacing rule stays quiet.
        s.limits.as_mut().unwrap().exhaustion_ms = Some(1_500_000);
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:04Z","message":{"id":"e","model":"<synthetic>","content":[{"type":"text","text":"x"}],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour","resetsAt":1789244400}}"#).unwrap());
        assert!(RateLimitPacing.evaluate(&s).is_none());
        assert!(crate::advisor::rules::outcome::TurnDied
            .evaluate(&s)
            .is_some());
    }

    #[test]
    fn a16_hook_overhead_names_the_slow_hook_and_ignores_cctop() {
        let mut s = State::new(Pricing::bundled());
        for i in 0..3 {
            s.apply(&prompt("2026-01-01T00:00:00Z"));
            s.apply(&response(
                &format!("h{i}"),
                "2026-01-01T00:00:01Z",
                10,
                0,
                100,
            ));
            s.apply(&Line::parse(r#"{"type":"system","subtype":"stop_hook_summary","timestamp":"2026-01-01T00:00:02Z","hookInfos":[{"command":"lint","durationMs":1400},{"command":"cctop hook","durationMs":5}],"hookErrors":[]}"#).unwrap());
            s.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T00:00:02Z","durationMs":8000}"#).unwrap());
        }
        let a = HookOverhead.evaluate(&s).expect("fires");
        assert_eq!(a.headline, "Hooks add 1.4s per turn (18 % of turn time)");
        assert!(
            a.evidence.starts_with("slowest: `lint` 4.2s in total"),
            "{}",
            a.evidence
        );
        assert!(a.action.starts_with("make `lint` async"), "{}", a.action);
        assert!(
            HookOverhead.evaluate(&fixture_state()).is_none(),
            "fixture hooks are ~50 ms"
        );
        // Only cctop's own hook: nothing to name, nothing to fire.
        let mut own = State::new(Pricing::bundled());
        own.apply(&prompt("2026-01-01T00:00:00Z"));
        own.apply(&response("h", "2026-01-01T00:00:01Z", 10, 0, 100));
        own.apply(&Line::parse(r#"{"type":"system","subtype":"stop_hook_summary","timestamp":"2026-01-01T00:00:02Z","hookInfos":[{"command":"cctop hook","durationMs":1400}],"hookErrors":[]}"#).unwrap());
        own.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T00:00:02Z","durationMs":8000}"#).unwrap());
        assert!(HookOverhead.evaluate(&own).is_none());
    }
}
