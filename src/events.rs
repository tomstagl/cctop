//! The event stream: everything notable, in time order, for the Events
//! panel and `cctop query events`.

use std::collections::VecDeque;

use crate::harness_facts::first_seen;
use crate::metrics::cost::parse_ts_ms;
use crate::tools::display_name;
use crate::transcript::{AssistantBlock, Line, PromptKind, SystemKind};

pub const CAPACITY: usize = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tool,
    Hook,
    Perm,
    Agent,
    Compact,
    Api,
    Note,
    Away,
    /// The coach's own lifecycle: a nudge fired, was acted on, expired or
    /// was snoozed.
    Coach,
    /// The ticker's tape: a priced moment worth ≥ 20 k tokens or ≥ $0.10 —
    /// a named cache miss, a cold write, a model switch.
    Cost,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Tool => "tool",
            Kind::Hook => "hook",
            Kind::Perm => "perm",
            Kind::Agent => "agent",
            Kind::Compact => "compact",
            Kind::Api => "api",
            Kind::Note => "note",
            Kind::Away => "away",
            Kind::Coach => "coach",
            Kind::Cost => "cost",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Epoch ms.
    pub at: i64,
    pub kind: Kind,
    pub text: String,
}

/// A priced moment becomes a `cost` row from this many tokens.
pub const COST_ROW_TOKENS: u64 = 20_000;

/// Bounded, time-ordered event log.
#[derive(Debug, Default)]
pub struct Log {
    events: VecDeque<Event>,
    /// Ids of tool_use blocks already announced (dedupe across repeated lines).
    seen_tool_uses: std::collections::HashSet<String>,
    /// Pending tool names by id, for the end event.
    pending: std::collections::HashMap<String, String>,
    last_context: Option<u64>,
    /// Model of the last real API response, for the switch row.
    last_model: Option<String>,
    /// Tool calls issued in the current turn, for the interrupt event.
    turn_calls: usize,
    /// The transcript's version (first seen), for the compaction fallback.
    version: Option<String>,
}

impl Log {
    pub fn push(&mut self, ev: Event) {
        // Keep time order: hook/late events may arrive out of sequence.
        let pos = self
            .events
            .iter()
            .rposition(|e| e.at <= ev.at)
            .map(|i| i + 1)
            .unwrap_or(0);
        self.events.insert(pos, ev);
        while self.events.len() > CAPACITY {
            self.events.pop_front();
        }
    }

    pub fn note(&mut self, at: i64, text: impl Into<String>) {
        self.push(Event {
            at,
            kind: Kind::Note,
            text: text.into(),
        });
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Event> {
        self.events.iter()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The last `n` events, oldest first.
    pub fn tail(&self, n: usize) -> Vec<&Event> {
        let start = self.events.len().saturating_sub(n);
        self.events.iter().skip(start).collect()
    }

    /// Derive events from a main-transcript line.
    pub fn apply(&mut self, line: &Line) {
        if self.version.is_none() {
            self.version = line.version().map(str::to_string);
        }
        match line {
            Line::Assistant(a) => {
                let Some(at) = a.timestamp.as_deref().and_then(parse_ts_ms) else {
                    return;
                };
                if a.is_api_error() {
                    // Claude Code's own error line: zero usage, `<synthetic>`
                    // model. An event, never a compaction.
                    let mut text = format!(
                        "API error: {}",
                        a.error.as_deref().unwrap_or("request failed")
                    );
                    if let Some(s) = a.api_error_status {
                        text.push_str(&format!(" {s}"));
                    }
                    if let Some(q) = &a.quota_limits {
                        if let Some(t) = &q.rate_limit_type {
                            text.push_str(&format!(" · {}", t.replace('_', " ")));
                        }
                        if let Some(r) = q.resets_at {
                            text.push_str(&format!(
                                " · resets {}",
                                crate::ui::fmt::clock_hhmm((r * 1000.0) as i64)
                            ));
                        }
                        if let Some(s) = q.low_priority_retry_after_seconds {
                            text.push_str(&format!(" · low-priority retry {s} s"));
                        }
                    }
                    self.push(Event {
                        at,
                        kind: Kind::Api,
                        text,
                    });
                    return;
                }
                for b in &a.message.content {
                    if let AssistantBlock::ToolUse { id, name, input } = b {
                        if !self.seen_tool_uses.insert(id.clone()) {
                            continue;
                        }
                        self.turn_calls += 1;
                        let (display, mcp) = display_name(name);
                        let summary = crate::tools::summarize_input(name, input);
                        let shown = match mcp {
                            Some(t) => format!("{display} {t} {summary}"),
                            None => format!("{display} {summary}"),
                        };
                        self.pending.insert(id.clone(), display);
                        self.push(Event {
                            at,
                            kind: Kind::Tool,
                            text: format!("{} ▶", shown.trim_end()),
                        });
                    }
                }
                if let Some(miss) = a.cache_miss_reason() {
                    if miss.is_named() {
                        let tokens = miss
                            .cache_missed_input_tokens
                            .unwrap_or(a.message.usage.cache_creation_input_tokens);
                        self.push(Event {
                            at,
                            // ≥ 20 k re-written is a priced moment; smaller
                            // misses are API detail.
                            kind: if tokens >= COST_ROW_TOKENS {
                                Kind::Cost
                            } else {
                                Kind::Api
                            },
                            text: format!(
                                "cache miss: {} {}",
                                miss.kind,
                                crate::ui::fmt::tokens(tokens)
                            ),
                        });
                    }
                }
                // Priced moments without a diagnostic: a model switch, and a
                // cold write (the context re-written at the write price).
                let usage = &a.message.usage;
                let model = a.message.model.clone();
                if !model.is_empty() && !model.starts_with('<') {
                    if let Some(prev) = self.last_model.as_deref().filter(|p| *p != model) {
                        self.push(Event {
                            at,
                            kind: Kind::Cost,
                            text: format!(
                                "model {} → {}",
                                crate::ui::fmt::model_short(prev),
                                crate::ui::fmt::model_short(&model)
                            ),
                        });
                    }
                    self.last_model = Some(model);
                }
                let write = usage.cache_creation_input_tokens;
                let total = usage.total_input();
                if write >= COST_ROW_TOKENS
                    && total > 0
                    && write as f64 >= total as f64 * 0.7
                    && a.cache_miss_reason().is_none_or(|m| !m.is_named())
                {
                    self.push(Event {
                        at,
                        kind: Kind::Cost,
                        text: format!("cold write {}", crate::ui::fmt::tokens(write)),
                    });
                }
                // A context drop is a compaction only on transcripts that
                // cannot say so themselves (`compact_boundary`, 2.1.263+).
                let ctx = a.message.usage.total_input();
                let exact = first_seen::COMPACT_BOUNDARY.at_most(self.version.as_deref());
                if let (Some(prev), false) = (self.last_context, exact) {
                    if prev > 0 && ctx > 0 && (ctx as f64) < prev as f64 * 0.7 {
                        self.push(Event {
                            at,
                            kind: Kind::Compact,
                            text: format!(
                                "context {} → {} (≈ inferred)",
                                crate::ui::fmt::tokens(prev),
                                crate::ui::fmt::tokens(ctx)
                            ),
                        });
                    }
                }
                if ctx > 0 {
                    self.last_context = Some(ctx);
                }
            }
            Line::User(u) => {
                let Some(at) = u.timestamp.as_deref().and_then(parse_ts_ms) else {
                    return;
                };
                match u.prompt_kind() {
                    PromptKind::Human
                    | PromptKind::Machine
                    | PromptKind::TaskNotification
                    | PromptKind::TeammateMessage => self.turn_calls = 0,
                    PromptKind::ToolResult => {
                        for r in u.message.content.tool_results() {
                            if let Some(name) = self.pending.remove(&r.tool_use_id) {
                                let (kind, text) = match (u.denial(), r.is_error) {
                                    (Some(d), _) => (Kind::Perm, format!("{name} ✗ {}", d.label())),
                                    (None, true) => (Kind::Api, format!("{name} ✗ error")),
                                    (None, false) => (
                                        Kind::Tool,
                                        format!(
                                            "{name} ✓ {}",
                                            crate::ui::fmt::tokens((r.text().len() / 4) as u64)
                                        ),
                                    ),
                                };
                                self.push(Event { at, kind, text });
                            }
                        }
                    }
                    PromptKind::Interrupt => self.push(Event {
                        at,
                        kind: Kind::Note,
                        text: format!("interrupted after {} calls", self.turn_calls),
                    }),
                    PromptKind::SlashCommand => {
                        if let Some(cmd) = u.slash_command() {
                            self.push(Event {
                                at,
                                kind: Kind::Note,
                                text: cmd,
                            });
                        }
                    }
                    _ => {}
                }
            }
            Line::System(s) => {
                let Some(at) = s.timestamp.as_deref().and_then(parse_ts_ms) else {
                    return;
                };
                match s.kind() {
                    SystemKind::StopHookSummary => {
                        let ms: u64 = s.hook_infos.iter().map(|h| h.duration_ms).sum();
                        let errs = s.hook_errors.len();
                        let text = if errs > 0 {
                            format!(
                                "Stop hooks {} ran, {errs} failed, {ms}ms",
                                s.hook_infos.len()
                            )
                        } else {
                            format!("Stop hooks {} ran, {ms}ms", s.hook_infos.len())
                        };
                        self.push(Event {
                            at,
                            kind: Kind::Hook,
                            text,
                        });
                    }
                    SystemKind::TurnDuration => {
                        if let Some(ms) = s.duration_ms {
                            self.push(Event {
                                at,
                                kind: Kind::Note,
                                text: format!(
                                    "turn done in {}",
                                    crate::ui::fmt::duration_ms(ms as i64)
                                ),
                            });
                        }
                    }
                    SystemKind::AwaySummary => self.push(Event {
                        at,
                        kind: Kind::Away,
                        text: crate::ui::fmt::clip(s.content.as_deref().unwrap_or("away"), 80),
                    }),
                    SystemKind::CompactBoundary => {
                        let m = s.compact_metadata.clone().unwrap_or_default();
                        self.push(Event {
                            at,
                            kind: Kind::Compact,
                            text: format!(
                                "compacted {} {} → {} in {}",
                                if m.trigger.is_empty() {
                                    "auto"
                                } else {
                                    &m.trigger
                                },
                                crate::ui::fmt::tokens(m.pre_tokens),
                                crate::ui::fmt::tokens(m.post_tokens),
                                crate::ui::fmt::duration_ms(m.duration_ms as i64)
                            ),
                        });
                        self.last_context = Some(m.post_tokens);
                    }
                    SystemKind::MicrocompactBoundary => self.push(Event {
                        at,
                        kind: Kind::Compact,
                        text: "tool results cleared (microcompact)".into(),
                    }),
                    _ => {}
                }
            }
            Line::CostState(c) => {
                let at = self.events.back().map(|e| e.at).unwrap_or(0);
                self.push(Event {
                    at,
                    kind: Kind::Api,
                    text: format!(
                        "cost-state ${:.2} · api {} · retries {}",
                        c.total_cost_usd,
                        crate::ui::fmt::duration_ms(c.total_api_duration as i64),
                        crate::ui::fmt::duration_ms(
                            c.total_api_duration
                                .saturating_sub(c.total_api_duration_without_retries)
                                as i64
                        )
                    ),
                });
            }
            Line::ContinuedIn(c) => {
                if let Some(at) = c.timestamp.as_deref().and_then(parse_ts_ms) {
                    self.push(Event {
                        at,
                        kind: Kind::Note,
                        text: "/clear · continued in a new session".into(),
                    });
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::parse_file;
    use std::path::Path;

    fn log(name: &str) -> Log {
        let mut l = Log::default();
        for line in
            parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{name}.jsonl")))
                .unwrap()
        {
            l.apply(&line);
        }
        l
    }

    #[test]
    fn fixture_events_are_ordered_and_typed() {
        let l = log("session-a");
        assert!(l.len() > 257 * 2, "start + end per tool call");
        let mut prev = 0;
        for e in l.iter() {
            assert!(e.at >= prev, "out of order");
            prev = e.at;
        }
        let kinds = |k: Kind| l.iter().filter(|e| e.kind == k).count();
        assert_eq!(kinds(Kind::Hook), 9);
        assert_eq!(kinds(Kind::Away), 1);
        assert!(kinds(Kind::Api) > 3, "cost-state + Bash errors");
        assert_eq!(kinds(Kind::Compact), 0);
        assert!(l.iter().any(|e| e.text.starts_with("Bash make check ▶")));
        assert!(l.iter().any(|e| e.text == "Bash ✗ error"));
        assert!(l.iter().any(|e| e.text.starts_with("cost-state $9.90")));
        // The interrupt that ended the session is a note.
        assert!(l
            .iter()
            .any(|e| e.kind == Kind::Note && e.text == "interrupted after 125 calls"));
    }

    #[test]
    fn fixture_b_errors_denials_interrupt_and_exact_compaction() {
        let l = log("session-b");
        let api_errors: Vec<&Event> = l
            .iter()
            .filter(|e| e.text.starts_with("API error:"))
            .collect();
        assert_eq!(api_errors.len(), 2, "{api_errors:?}");
        assert!(api_errors
            .iter()
            .any(|e| e.text.contains("invalid_request")));
        let compactions: Vec<&Event> = l.iter().filter(|e| e.kind == Kind::Compact).collect();
        assert_eq!(
            compactions.len(),
            1,
            "exact, never inferred: {compactions:?}"
        );
        assert_eq!(compactions[0].text, "compacted auto 567k → 230k in 1:20");
        let denials: Vec<&Event> = l
            .iter()
            .filter(|e| e.kind == Kind::Perm && e.text.contains('✗'))
            .collect();
        assert_eq!(denials.len(), 4);
        assert!(denials.iter().any(|e| e.text.ends_with("automode-blocked")));
        assert!(l
            .iter()
            .any(|e| e.kind == Kind::Note && e.text.starts_with("interrupted after")));
        assert!(l.iter().any(|e| e.text == "/clear"));
        assert!(l
            .iter()
            .any(|e| e.text == "/clear · continued in a new session"));
    }

    #[test]
    fn ring_buffer_caps_and_note_inserts_in_order() {
        let mut l = Log::default();
        for i in 0..(CAPACITY + 10) as i64 {
            l.note(i, "x");
        }
        assert_eq!(l.len(), CAPACITY);
        assert_eq!(l.iter().next().unwrap().at, 10);
        let mut l = Log::default();
        l.note(10, "a");
        l.note(30, "c");
        l.note(20, "b");
        let texts: Vec<_> = l.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c"]);
        assert_eq!(l.tail(2).len(), 2);
        assert_eq!(l.tail(2)[0].text, "b");
    }

    #[test]
    fn inferred_compaction_only_before_compact_boundary_existed() {
        let mk = |id: &str, ctx: u64, t: &str, version: &str| {
            Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:{t}Z","version":"{version}","message":{{"id":"{id}","model":"m","content":[],"usage":{{"input_tokens":{ctx}}}}}}}"#
            ))
            .unwrap()
        };
        let mut l = Log::default();
        l.apply(&mk("a", 200_000, "01", "2.1.247"));
        l.apply(&mk("b", 60_000, "02", "2.1.247"));
        assert_eq!(l.iter().filter(|e| e.kind == Kind::Compact).count(), 1);
        assert!(l.iter().last().unwrap().text.contains("200k → 60k"));
        let mut l = Log::default();
        l.apply(&mk("a", 200_000, "01", "2.1.270"));
        l.apply(&mk("b", 60_000, "02", "2.1.270"));
        assert_eq!(l.iter().filter(|e| e.kind == Kind::Compact).count(), 0);
        // An error line never produces a drop, on any version.
        let mut l = Log::default();
        l.apply(&mk("a", 200_000, "01", "2.1.247"));
        l.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","version":"2.1.247","message":{"id":"e","model":"<synthetic>","content":[{"type":"text","text":"x"}],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour","resetsAt":1789244400,"lowPriorityRetryAfterSeconds":20}}"#).unwrap());
        assert_eq!(l.iter().filter(|e| e.kind == Kind::Compact).count(), 0);
        let err = l.iter().last().unwrap();
        assert_eq!(err.kind, Kind::Api);
        assert!(
            err.text
                .starts_with("API error: rate_limit 429 · five hour · resets "),
            "{}",
            err.text
        );
        assert!(err.text.ends_with("· low-priority retry 20 s"));
        // A named cache miss is an event.
        let mut l = Log::default();
        l.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"c","model":"m","content":[],"usage":{"input_tokens":2,"cache_creation_input_tokens":427000},"diagnostics":{"cache_miss_reason":{"type":"model_changed"}}}}"#).unwrap());
        assert_eq!(
            l.iter().last().unwrap().text,
            "cache miss: model_changed 427k"
        );
    }
}
