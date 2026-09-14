//! Threshold alerts. Each rule is a predicate over the state; it fires once
//! when the predicate turns true and re-arms when it turns false again, so a
//! session that hovers around a threshold does not spam.
//!
//! One owner per warning (coach PRD, US-013): `ContextHigh` fires at Claude
//! Code's own warn band, not a fixed 80 %; `CompactionSoon` is gone (Claude
//! Code's footer owns the countdown); the cache-hit and permission-wait
//! alerts were retired in favour of the coach's named cache miss (A20) and
//! waiting-on-you (A41) rules.
//!
//! Desktop notifications (`--notify`) go out in exactly three cases (§6.6):
//! waiting on you after 30 s (a question or a Notification, never a `?`
//! guess), the cache countdown at T−5 / T−2 while a question or permission
//! is pending, and a turn that died on an API error (a 429 included).

use std::collections::HashMap;

use crate::ui::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleId {
    /// The context entered Claude Code's warn band (threshold − 20 000).
    ContextHigh,
    Limit5h60,
    Limit5h80,
    Limit5h95,
    ExhaustionBeforeReset,
    ToolRunningLong,
    McpExited,
    ApiRetry,
    /// A 429 landed in the transcript (the rate-limit branch of a dead turn).
    RateLimited,
    /// Any other API error line newer than the last successful call.
    TurnDied,
    /// Claude asked a question (or a Notification says it needs input) and
    /// 30 s passed.
    WaitingOnYou,
    /// The cache expires within five minutes (two on a 5 m TTL) while a
    /// question or a permission prompt is pending.
    CacheCountdown5,
    /// … within two minutes (one on a 5 m TTL).
    CacheCountdown2,
}

impl RuleId {
    pub const ALL: [RuleId; 13] = [
        RuleId::ContextHigh,
        RuleId::Limit5h60,
        RuleId::Limit5h80,
        RuleId::Limit5h95,
        RuleId::ExhaustionBeforeReset,
        RuleId::ToolRunningLong,
        RuleId::McpExited,
        RuleId::ApiRetry,
        RuleId::RateLimited,
        RuleId::TurnDied,
        RuleId::WaitingOnYou,
        RuleId::CacheCountdown5,
        RuleId::CacheCountdown2,
    ];

    /// Worth a desktop notification: the three cases of the coach PRD (a
    /// dead turn, waiting on you, the cache countdown while you are asked).
    pub fn critical(self) -> bool {
        matches!(
            self,
            RuleId::RateLimited
                | RuleId::TurnDied
                | RuleId::WaitingOnYou
                | RuleId::CacheCountdown5
                | RuleId::CacheCountdown2
        )
    }
}

/// A question or a permission prompt is open (a `?`-ended turn is a guess
/// and never notifies).
fn asked(state: &State) -> bool {
    use crate::ui::state::WaitingKind;
    state.waiting().is_some_and(|w| {
        matches!(
            w.kind,
            WaitingKind::Question | WaitingKind::Notification | WaitingKind::Permission
        )
    })
}

/// A rule that just fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fired {
    pub rule: RuleId,
    pub message: String,
}

/// Evaluate one rule: `Some(message)` while its condition holds.
fn check(rule: RuleId, state: &State, prev_retry_ms: u64) -> Option<String> {
    let ctx = state.context();
    match rule {
        RuleId::ContextHigh => {
            let bands = state.bands();
            (bands.band(ctx.size) != crate::metrics::context::Band::Ok)
                .then(|| bands.footer(ctx.size))
        }
        RuleId::Limit5h60 | RuleId::Limit5h80 | RuleId::Limit5h95 => {
            let threshold = match rule {
                RuleId::Limit5h60 => 60.0,
                RuleId::Limit5h80 => 80.0,
                _ => 95.0,
            };
            state
                .limits
                .as_ref()
                .filter(|l| l.five_hour_pct > threshold)
                .map(|l| {
                    format!(
                        "rate-limit 5h crossed {threshold:.0} % ({:.0} %)",
                        l.five_hour_pct
                    )
                })
        }
        RuleId::ExhaustionBeforeReset => state
            .limits
            .as_ref()
            .and_then(|l| Some((l.exhaustion_ms?, l.five_hour_resets_at_ms?)))
            .filter(|(ex, reset)| ex < reset)
            .map(|(ex, _)| {
                format!(
                    "5h limit projected to run out in {} — before it resets",
                    crate::ui::fmt::duration_ms(ex - state.clock_ms())
                )
            }),
        RuleId::ToolRunningLong => state
            .tools
            .running()
            .and_then(|c| Some((c, c.started_at?)))
            .filter(|(_, st)| state.clock_ms() - st > 60_000)
            .map(|(c, st)| {
                format!(
                    "{} {} running for {}",
                    c.name,
                    c.input_summary,
                    crate::ui::fmt::duration_ms(state.clock_ms() - st)
                )
            }),
        RuleId::RateLimited => state.rate_limit_hit().map(|(kind, resets, _)| {
            let when = resets
                .map(|r| {
                    format!(
                        " · resets in {}",
                        crate::ui::fmt::duration_ms(r - state.clock_ms())
                    )
                })
                .unwrap_or_default();
            format!("rate limited ({}){when}", kind.replace('_', " "))
        }),
        RuleId::TurnDied => {
            let e = state.agg.api_errors.last()?;
            if e.error.as_deref() == Some("rate_limit") || e.status == Some(429) {
                return None; // RateLimited's
            }
            let at =
                e.at.as_deref()
                    .and_then(crate::metrics::cost::parse_ts_ms)?;
            if state.last_api_call_ms().is_some_and(|c| c > at) {
                return None;
            }
            Some(format!(
                "turn died: {}{}",
                e.error.as_deref().unwrap_or("API error"),
                e.status.map(|s| format!(" {s}")).unwrap_or_default()
            ))
        }
        RuleId::WaitingOnYou => {
            use crate::ui::state::WaitingKind;
            let w = state.waiting()?;
            let since = state.clock_ms() - w.since_ms;
            (matches!(w.kind, WaitingKind::Question | WaitingKind::Notification) && since >= 30_000)
                .then(|| {
                    format!(
                        "Claude is waiting on you ({}) for {}",
                        match w.kind {
                            WaitingKind::Question => "a question",
                            _ => "needs your input",
                        },
                        crate::ui::fmt::duration_ms(since)
                    )
                })
        }
        RuleId::CacheCountdown5 | RuleId::CacheCountdown2 => {
            // T−5 / T−2 of a 1 h entry; T−2 / T−1 of a 5 m one.
            let one_hour = state.cache_ttl_ms() >= 3_600_000;
            let limit = match (rule, one_hour) {
                (RuleId::CacheCountdown5, true) => 300_000,
                (RuleId::CacheCountdown5, false) => 120_000,
                (_, true) => 120_000,
                (_, false) => 60_000,
            };
            let c = state.cache_clock()?;
            (asked(state) && c.remaining_ms > 0 && c.remaining_ms <= limit).then(|| {
                format!(
                    "cache cold in {} — reply now to keep {} warm",
                    crate::coach::short_duration(c.remaining_ms),
                    crate::ui::fmt::tokens(state.context().size)
                )
            })
        }
        RuleId::McpExited => (!state.mcp_exited.is_empty())
            .then(|| format!("MCP server exited: {}", state.mcp_exited.join(", "))),
        RuleId::ApiRetry => {
            let retry = state
                .cost
                .authoritative
                .as_ref()
                .map(|c| {
                    c.total_api_duration
                        .saturating_sub(c.total_api_duration_without_retries)
                })
                .unwrap_or(0);
            (retry > prev_retry_ms).then(|| {
                format!(
                    "API retries: +{} spent retrying",
                    crate::ui::fmt::duration_ms((retry - prev_retry_ms) as i64)
                )
            })
        }
    }
}

#[derive(Debug, Default)]
pub struct Engine {
    active: HashMap<RuleId, bool>,
    /// Retry time at the last evaluation (ApiRetry fires on increase).
    retry_ms: u64,
    /// The first evaluation only latches baselines; historical retries in a
    /// freshly loaded transcript are not news.
    primed: bool,
}

impl Engine {
    /// Evaluate every rule; returns the ones that just crossed into active.
    pub fn evaluate(&mut self, state: &State) -> Vec<Fired> {
        if !self.primed {
            self.primed = true;
            if let Some(c) = &state.cost.authoritative {
                self.retry_ms = c
                    .total_api_duration
                    .saturating_sub(c.total_api_duration_without_retries);
            }
        }
        let mut fired = Vec::new();
        for rule in RuleId::ALL {
            let msg = check(rule, state, self.retry_ms);
            let was = self.active.get(&rule).copied().unwrap_or(false);
            let now = msg.is_some();
            if now && !was {
                fired.push(Fired {
                    rule,
                    message: msg.unwrap_or_default(),
                });
            }
            self.active.insert(rule, now);
        }
        // ApiRetry is edge-triggered on the counter, so latch the new value
        // and let the rule re-arm immediately.
        if let Some(c) = &state.cost.authoritative {
            self.retry_ms = c
                .total_api_duration
                .saturating_sub(c.total_api_duration_without_retries);
        }
        self.active.insert(RuleId::ApiRetry, false);
        fired
    }
}

/// Deliver `fired` to the state (note event + toast) and, when enabled,
/// critical ones to the desktop.
pub fn deliver(fired: &[Fired], state: &mut State, desktop: bool) {
    for f in fired {
        state.events.note(state.clock_ms(), f.message.clone());
        state.set_toast(f.message.clone());
        if desktop && f.rule.critical() {
            desktop_notify(&f.message);
        }
    }
}

fn desktop_notify(msg: &str) {
    let msg = msg.replace('"', "'");
    #[cfg(target_os = "macos")]
    let cmd = std::process::Command::new("osascript")
        .args([
            "-e",
            &format!("display notification \"{msg}\" with title \"cctop\""),
        ])
        .spawn();
    #[cfg(not(target_os = "macos"))]
    let cmd = std::process::Command::new("notify-send")
        .args(["cctop", &msg])
        .spawn();
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Pricing;
    use crate::transcript::Line;
    use crate::ui::state::Limits;

    fn state() -> State {
        let mut s = State::new(Pricing::bundled());
        s.session.alive = true;
        s.now_ms = 1_000_000;
        s
    }

    fn fires(e: &mut Engine, s: &State, rule: RuleId) -> bool {
        e.evaluate(s).iter().any(|f| f.rule == rule)
    }

    /// Every level-triggered rule fires exactly once per crossing.
    #[test]
    fn each_rule_fires_once_per_crossing() {
        let mut e = Engine::default();
        let mut s = state();
        // Context high: push a huge call, then a small one, then huge again.
        let ctx = |id: &str, tokens: u64| {
            Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{{"id":"{id}","model":"claude-haiku-4-5","content":[],"usage":{{"input_tokens":{tokens}}}}}}}"#
            ))
            .unwrap()
        };
        let prompt = || {
            Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"go"}}"#).unwrap()
        };
        s.apply(&prompt());
        s.apply(&ctx("a", 170_000)); // 85 % of haiku's 200k: still below the warn band (167k)
        assert!(
            fires(&mut e, &s, RuleId::ContextHigh),
            "170k ≥ warn at 167k"
        );
        assert!(
            !fires(&mut e, &s, RuleId::ContextHigh),
            "no re-fire while high"
        );
        s.context_size_exact = Some(160_000);
        assert!(
            !fires(&mut e, &s, RuleId::ContextHigh),
            "160k is below Claude Code's warn band"
        );
        s.context_size_exact = Some(190_000);
        let f = e.evaluate(&s);
        let high = f
            .iter()
            .find(|x| x.rule == RuleId::ContextHigh)
            .expect("fires again after a reset");
        assert!(
            high.message.starts_with("Context low ("),
            "{}",
            high.message
        );

        // Limits 60/80/95 and exhaustion.
        let mut lim = Limits {
            five_hour_pct: 65.0,
            seven_day_pct: 10.0,
            five_hour_resets_at_ms: Some(s.now_ms + 3_600_000),
            seven_day_resets_at_ms: None,
            exhaustion_ms: None,
        };
        s.limits = Some(lim.clone());
        let f = e.evaluate(&s);
        assert!(f.iter().any(|x| x.rule == RuleId::Limit5h60));
        assert!(!f.iter().any(|x| x.rule == RuleId::Limit5h80));
        lim.five_hour_pct = 96.0;
        lim.exhaustion_ms = Some(s.now_ms + 1_800_000);
        s.limits = Some(lim.clone());
        let f = e.evaluate(&s);
        assert!(f.iter().any(|x| x.rule == RuleId::Limit5h80));
        assert!(f.iter().any(|x| x.rule == RuleId::Limit5h95));
        assert!(f.iter().any(|x| x.rule == RuleId::ExhaustionBeforeReset));
        assert!(e.evaluate(&s).is_empty(), "steady state fires nothing");
        lim.five_hour_pct = 10.0;
        lim.exhaustion_ms = None;
        s.limits = Some(lim.clone());
        assert!(e.evaluate(&s).is_empty());
        lim.five_hour_pct = 61.0;
        s.limits = Some(lim);
        assert!(fires(&mut e, &s, RuleId::Limit5h60));

        // A 429 in the transcript.
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","message":{"id":"e1","model":"<synthetic>","content":[{"type":"text","text":"x"}],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour","resetsAt":1767229200}}"#).unwrap());
        s.now_ms = crate::metrics::cost::parse_ts_ms("2026-01-01T00:00:03Z").unwrap();
        let f = e.evaluate(&s);
        let hit = f
            .iter()
            .find(|x| x.rule == RuleId::RateLimited)
            .expect("rate limited");
        assert_eq!(hit.message, "rate limited (five hour) · resets in 59:57");
        assert!(RuleId::RateLimited.critical());
        assert!(
            !fires(&mut e, &s, RuleId::TurnDied),
            "a 429 is RateLimited's"
        );

        // MCP exited.
        s.mcp_exited = vec!["github".into()];
        assert!(fires(&mut e, &s, RuleId::McpExited));
        s.mcp_exited.clear();
        assert!(!fires(&mut e, &s, RuleId::McpExited));
        s.mcp_exited = vec!["github".into()];
        assert!(fires(&mut e, &s, RuleId::McpExited));
    }

    /// Exactly three desktop cases: a dead turn (a 429 included), waiting
    /// on you after 30 s, the cache countdown while you are asked.
    #[test]
    fn desktop_cases_are_the_three_of_the_coach_prd() {
        let critical: Vec<RuleId> = RuleId::ALL.into_iter().filter(|r| r.critical()).collect();
        assert_eq!(
            critical,
            [
                RuleId::RateLimited,
                RuleId::TurnDied,
                RuleId::WaitingOnYou,
                RuleId::CacheCountdown5,
                RuleId::CacheCountdown2
            ]
        );
        let mut e = Engine::default();
        let mut s = state();
        // A question pending: nothing at 20 s, the notification at 30 s, once.
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"go"}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"q","model":"claude-opus-5","content":[{"type":"tool_use","id":"q","name":"AskUserQuestion","input":{"questions":[]}}],"usage":{"input_tokens":150000,"output_tokens":1}}}"#).unwrap());
        let t0 = crate::metrics::cost::parse_ts_ms("2026-01-01T00:00:01Z").unwrap();
        s.now_ms = t0 + 20_000;
        assert!(!fires(&mut e, &s, RuleId::WaitingOnYou));
        s.now_ms = t0 + 31_000;
        let f = e.evaluate(&s);
        let w = f
            .iter()
            .find(|x| x.rule == RuleId::WaitingOnYou)
            .expect("waiting");
        assert_eq!(w.message, "Claude is waiting on you (a question) for 0:31");
        assert!(!fires(&mut e, &s, RuleId::WaitingOnYou), "once");
        // The cache countdown while the question is pending: this session
        // has the 5 m TTL, so the two cases are T−2 and T−1 (the entry
        // expires at t0 + 5 m), each once.
        assert!(!fires(&mut e, &s, RuleId::CacheCountdown5), "4:29 left");
        s.now_ms = t0 + 300_000 - 119_000;
        assert!(fires(&mut e, &s, RuleId::CacheCountdown5), "1:59 left");
        assert!(
            !fires(&mut e, &s, RuleId::CacheCountdown2),
            "not yet one minute"
        );
        s.now_ms = t0 + 300_000 - 55_000;
        let f = e.evaluate(&s);
        let c2 = f
            .iter()
            .find(|x| x.rule == RuleId::CacheCountdown2)
            .expect("T−1");
        assert_eq!(
            c2.message,
            "cache cold in 0:55 — reply now to keep 150k warm"
        );
        assert!(!f.iter().any(|x| x.rule == RuleId::CacheCountdown5), "once");
        // No question pending: the countdown is silent.
        let mut quiet = state();
        quiet.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"go"}}"#).unwrap());
        quiet.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"a","model":"claude-opus-5","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":150000,"output_tokens":1}}}"#).unwrap());
        quiet.now_ms = t0 + 300_000 - 90_000;
        let mut e2 = Engine::default();
        assert!(!fires(&mut e2, &quiet, RuleId::CacheCountdown2));
        assert!(!fires(&mut e2, &quiet, RuleId::WaitingOnYou));
        // A dead turn on a server error.
        quiet.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","message":{"id":"e2","model":"<synthetic>","content":[{"type":"text","text":"x"}],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"server_error","apiErrorStatus":529}"#).unwrap());
        let f = e2.evaluate(&quiet);
        let died = f
            .iter()
            .find(|x| x.rule == RuleId::TurnDied)
            .expect("turn died");
        assert_eq!(died.message, "turn died: server_error 529");
        assert!(!f.iter().any(|x| x.rule == RuleId::RateLimited));
        // A successful call afterwards re-arms it.
        quiet.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"ok","model":"claude-opus-5","content":[],"usage":{"input_tokens":10}}}"#).unwrap());
        assert!(!fires(&mut e2, &quiet, RuleId::TurnDied));
    }

    #[test]
    fn tool_running_long() {
        let mut e = Engine::default();
        let mut s = state();
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"id":"m","model":"m","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 100"}}],"usage":{"output_tokens":1}}}"#).unwrap());
        let start = crate::metrics::cost::parse_ts_ms("2026-01-01T00:00:00Z").unwrap();
        s.now_ms = start + 30_000;
        assert!(!fires(&mut e, &s, RuleId::ToolRunningLong));
        s.now_ms = start + 61_000;
        let f = e.evaluate(&s);
        let long = f
            .iter()
            .find(|x| x.rule == RuleId::ToolRunningLong)
            .unwrap();
        assert!(
            long.message.contains("Bash sleep 100 running for 1:01"),
            "{}",
            long.message
        );
        // Completes → re-arms.
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:01:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#).unwrap());
        assert!(!fires(&mut e, &s, RuleId::ToolRunningLong));
    }

    #[test]
    fn api_retry_fires_on_increase_and_deliver_notes_and_toasts() {
        let mut e = Engine::default();
        let mut s = state();
        let cs = |api: u64, no_retry: u64| {
            Line::parse(&format!(
                r#"{{"type":"cost-state","totalCostUSD":1.0,"totalAPIDuration":{api},"totalAPIDurationWithoutRetries":{no_retry}}}"#
            ))
            .unwrap()
        };
        s.apply(&cs(1000, 1000));
        assert!(!fires(&mut e, &s, RuleId::ApiRetry));
        s.apply(&cs(5000, 2000));
        let f = e.evaluate(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].message.starts_with("API retries: +0:03"));
        assert!(
            !fires(&mut e, &s, RuleId::ApiRetry),
            "same counter, no re-fire"
        );
        s.apply(&cs(9000, 2000));
        assert!(fires(&mut e, &s, RuleId::ApiRetry));

        deliver(&f, &mut s, false);
        assert_eq!(s.toast_text(), Some(f[0].message.as_str()));
        assert!(s
            .events
            .iter()
            .last()
            .unwrap()
            .text
            .starts_with("API retries"));
    }
}
