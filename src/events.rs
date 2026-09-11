//! The event stream: everything notable, in time order, for the Events
//! panel and `cctop query events`.

use std::collections::VecDeque;

use crate::metrics::cost::parse_ts_ms;
use crate::tools::display_name;
use crate::transcript::{AssistantBlock, Line, SystemKind};

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

/// Bounded, time-ordered event log.
#[derive(Debug, Default)]
pub struct Log {
    events: VecDeque<Event>,
    /// Ids of tool_use blocks already announced (dedupe across repeated lines).
    seen_tool_uses: std::collections::HashSet<String>,
    /// Pending tool names by id, for the end event.
    pending: std::collections::HashMap<String, String>,
    last_context: Option<u64>,
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
        match line {
            Line::Assistant(a) => {
                let Some(at) = a.timestamp.as_deref().and_then(parse_ts_ms) else {
                    return;
                };
                for b in &a.message.content {
                    if let AssistantBlock::ToolUse { id, name, input } = b {
                        if !self.seen_tool_uses.insert(id.clone()) {
                            continue;
                        }
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
                // Context drop = compaction.
                let ctx = a.message.usage.total_input();
                if let Some(prev) = self.last_context {
                    if prev > 0 && (ctx as f64) < prev as f64 * 0.7 {
                        self.push(Event {
                            at,
                            kind: Kind::Compact,
                            text: format!(
                                "context {} → {}",
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
                for r in u.message.content.tool_results() {
                    if let Some(name) = self.pending.remove(&r.tool_use_id) {
                        let text = if r.is_error {
                            format!("{name} ✗ error")
                        } else {
                            format!(
                                "{name} ✓ {}",
                                crate::ui::fmt::tokens((r.text().len() / 4) as u64)
                            )
                        };
                        self.push(Event {
                            at,
                            kind: if r.is_error { Kind::Api } else { Kind::Tool },
                            text,
                        });
                    }
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
                    SystemKind::Other => {}
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
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::parse_file;
    use std::path::Path;

    fn log() -> Log {
        let mut l = Log::default();
        for line in
            parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl"))
                .unwrap()
        {
            l.apply(&line);
        }
        l
    }

    #[test]
    fn fixture_events_are_ordered_and_typed() {
        let l = log();
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
    fn compaction_event_on_context_drop() {
        let mut l = Log::default();
        let mk = |id: &str, ctx: u64, t: &str| {
            Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:{t}Z","message":{{"id":"{id}","model":"m","content":[],"usage":{{"input_tokens":{ctx}}}}}}}"#
            ))
            .unwrap()
        };
        l.apply(&mk("a", 200_000, "01"));
        l.apply(&mk("b", 60_000, "02"));
        assert_eq!(l.iter().filter(|e| e.kind == Kind::Compact).count(), 1);
        assert!(l.iter().last().unwrap().text.contains("200k → 60k"));
    }
}
