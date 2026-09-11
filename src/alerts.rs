//! Threshold alerts. Each rule is a predicate over the state; it fires once
//! when the predicate turns true and re-arms when it turns false again, so a
//! session that hovers around a threshold does not spam.

use std::collections::HashMap;

use crate::ui::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleId {
    ContextHigh,
    CompactionSoon,
    Limit5h60,
    Limit5h80,
    Limit5h95,
    ExhaustionBeforeReset,
    CacheHitLow,
    ToolRunningLong,
    PermissionWaitLong,
    McpExited,
    ApiRetry,
}

impl RuleId {
    pub const ALL: [RuleId; 11] = [
        RuleId::ContextHigh,
        RuleId::CompactionSoon,
        RuleId::Limit5h60,
        RuleId::Limit5h80,
        RuleId::Limit5h95,
        RuleId::ExhaustionBeforeReset,
        RuleId::CacheHitLow,
        RuleId::ToolRunningLong,
        RuleId::PermissionWaitLong,
        RuleId::McpExited,
        RuleId::ApiRetry,
    ];

    /// Worth a desktop notification.
    pub fn critical(self) -> bool {
        matches!(
            self,
            RuleId::ContextHigh | RuleId::ExhaustionBeforeReset | RuleId::McpExited
        )
    }
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
            (ctx.ratio() > 0.80).then(|| format!("context {:.0} % of window", ctx.ratio() * 100.0))
        }
        RuleId::CompactionSoon => ctx
            .turns_until_compaction()
            .filter(|n| *n <= 2.0)
            .map(|n| format!("autocompact projected in ~{} turn(s)", n.ceil() as u64)),
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
        RuleId::CacheHitLow => {
            let recent: Vec<_> = state
                .agg
                .turns
                .iter()
                .filter(|t| t.api_calls > 0)
                .rev()
                .take(5)
                .collect();
            if recent.len() < 5 {
                return None;
            }
            let mut u = crate::metrics::Usage::default();
            for t in &recent {
                u.add(&t.usage);
            }
            u.cache_hit_ratio()
                .filter(|r| *r < 0.5)
                .map(|r| format!("cache hit ratio {:.0} % over the last 5 turns", r * 100.0))
        }
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
        RuleId::PermissionWaitLong => state
            .session
            .permission_waiting_since_ms
            .filter(|since| state.clock_ms() - since > 30_000)
            .map(|since| {
                format!(
                    "waiting for permission for {}",
                    crate::ui::fmt::duration_ms(state.clock_ms() - since)
                )
            }),
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
}

impl Engine {
    /// Evaluate every rule; returns the ones that just crossed into active.
    pub fn evaluate(&mut self, state: &State) -> Vec<Fired> {
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
        state.events.note(state.now_ms, f.message.clone());
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
        s.apply(&ctx("a", 170_000)); // 85 % of haiku's 200k
        assert!(fires(&mut e, &s, RuleId::ContextHigh));
        assert!(
            !fires(&mut e, &s, RuleId::ContextHigh),
            "no re-fire while high"
        );
        s.context_size_exact = Some(50_000);
        assert!(!fires(&mut e, &s, RuleId::ContextHigh));
        s.context_size_exact = Some(190_000);
        assert!(
            fires(&mut e, &s, RuleId::ContextHigh),
            "fires again after a reset"
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

        // Permission wait > 30 s.
        s.session.permission_waiting_since_ms = Some(s.now_ms - 31_000);
        assert!(fires(&mut e, &s, RuleId::PermissionWaitLong));
        s.session.permission_waiting_since_ms = None;
        assert!(!fires(&mut e, &s, RuleId::PermissionWaitLong));
        s.session.permission_waiting_since_ms = Some(s.now_ms - 31_000);
        assert!(fires(&mut e, &s, RuleId::PermissionWaitLong));

        // MCP exited.
        s.mcp_exited = vec!["github".into()];
        assert!(fires(&mut e, &s, RuleId::McpExited));
        s.mcp_exited.clear();
        assert!(!fires(&mut e, &s, RuleId::McpExited));
        s.mcp_exited = vec!["github".into()];
        assert!(fires(&mut e, &s, RuleId::McpExited));
    }

    #[test]
    fn tool_running_long_and_cache_hit_low() {
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

        // Cache-hit low over 5 turns: five turns of pure fresh input.
        let mut s = state();
        for i in 0..5 {
            s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"go"}}"#).unwrap());
            s.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{{"id":"c{i}","model":"m","content":[],"usage":{{"input_tokens":1000,"output_tokens":1}}}}}}"#)).unwrap());
        }
        assert!(fires(&mut e, &s, RuleId::CacheHitLow));
        assert!(!fires(&mut e, &s, RuleId::CacheHitLow));
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
