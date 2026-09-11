//! Token usage, counted once per API response, grouped into turns.
//!
//! Claude Code writes one `assistant` line per content block, all carrying the
//! same `message.id` and the same `usage`. Summing lines naïvely overstates
//! everything ~2–3×; [`Aggregate`] keys on the id.

use std::collections::HashSet;

use crate::transcript::{AssistantLine, CacheTtl, Content, Line};

/// Token counts by class. `cache_write_5m` + `cache_write_1h` is the raw
/// `cache_creation_input_tokens`; when the API does not break it down the
/// whole amount is attributed to the 5-minute bucket.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
    pub thinking: u64,
}

impl Usage {
    pub fn from_api(u: &crate::transcript::Usage) -> Usage {
        let (w5, w1) = if u.cache_creation.ephemeral_1h_input_tokens > 0
            || u.cache_creation.ephemeral_5m_input_tokens > 0
        {
            (
                u.cache_creation.ephemeral_5m_input_tokens,
                u.cache_creation.ephemeral_1h_input_tokens,
            )
        } else {
            (u.cache_creation_input_tokens, 0)
        };
        Usage {
            input: u.input_tokens,
            cache_write_5m: w5,
            cache_write_1h: w1,
            cache_read: u.cache_read_input_tokens,
            output: u.output_tokens,
            thinking: u.output_tokens_details.thinking_tokens,
        }
    }

    pub fn cache_write(&self) -> u64 {
        self.cache_write_5m + self.cache_write_1h
    }

    /// Everything sent to the model.
    pub fn total_input(&self) -> u64 {
        self.input + self.cache_write() + self.cache_read
    }

    pub fn total(&self) -> u64 {
        self.total_input() + self.output
    }

    /// Share of input served from cache; `None` when nothing was sent.
    pub fn cache_hit_ratio(&self) -> Option<f64> {
        let denom = self.total_input();
        (denom > 0).then(|| self.cache_read as f64 / denom as f64)
    }

    pub fn add(&mut self, o: &Usage) {
        self.input += o.input;
        self.cache_write_5m += o.cache_write_5m;
        self.cache_write_1h += o.cache_write_1h;
        self.cache_read += o.cache_read;
        self.output += o.output;
        self.thinking += o.thinking;
    }
}

/// One user prompt and everything the model did in response.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    /// 1-based.
    pub number: usize,
    /// Timestamp of the user line that started the turn (ISO-8601 as written).
    pub started_at: Option<String>,
    /// Timestamp of the last line seen in this turn.
    pub last_at: Option<String>,
    /// Exact duration from the `turn_duration` system line, when it arrived.
    pub duration_ms: Option<u64>,
    /// Distinct API responses.
    pub api_calls: usize,
    pub usage: Usage,
    /// Models seen, in first-use order.
    pub models: Vec<String>,
    /// Effort level of the last response.
    pub effort: Option<String>,
    /// `cache_read + cache_write + input` of the last API call: the context
    /// size the model saw.
    pub context_size: u64,
    /// Cache TTL on the last call that wrote cache.
    pub cache_ttl: Option<CacheTtl>,
    /// Tool result text length pushed into context this turn (bytes).
    pub tool_result_bytes: u64,
    /// Number of tool calls issued this turn.
    pub tool_calls: usize,
    /// Number of tool results flagged `is_error`.
    pub tool_errors: usize,
}

/// Running aggregate over a transcript. Feed lines in order.
#[derive(Debug, Default)]
pub struct Aggregate {
    pub turns: Vec<Turn>,
    pub total: Usage,
    /// Distinct API responses seen (for dedupe).
    seen_ids: HashSet<String>,
    /// Most recent cache TTL observed on any call.
    pub observed_ttl: Option<CacheTtl>,
    /// Last model used.
    pub model: Option<String>,
}

impl Aggregate {
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a Line>) -> Aggregate {
        let mut a = Aggregate::default();
        for l in lines {
            a.push(l);
        }
        a
    }

    /// Distinct API responses so far.
    pub fn api_calls(&self) -> usize {
        self.seen_ids.len()
    }

    pub fn current_turn(&self) -> Option<&Turn> {
        self.turns.last()
    }

    /// Context size the model last saw (0 before any call).
    pub fn context_size(&self) -> u64 {
        self.turns.last().map(|t| t.context_size).unwrap_or(0)
    }

    pub fn push(&mut self, line: &Line) {
        match line {
            Line::User(u) => {
                let is_prompt = !u.is_meta && u.message.content.tool_results().next().is_none();
                if is_prompt {
                    self.turns.push(Turn {
                        number: self.turns.len() + 1,
                        started_at: u.timestamp.clone(),
                        last_at: u.timestamp.clone(),
                        ..Default::default()
                    });
                } else if let Some(t) = self.turns.last_mut() {
                    if let Content::Blocks(_) = &u.message.content {
                        for r in u.message.content.tool_results() {
                            t.tool_result_bytes += r.text().len() as u64;
                            if r.is_error {
                                t.tool_errors += 1;
                            }
                        }
                    }
                    t.last_at = u.timestamp.clone().or(t.last_at.take());
                }
            }
            Line::Assistant(a) => self.push_assistant(a),
            Line::System(s) => {
                if let (crate::transcript::SystemKind::TurnDuration, Some(ms)) =
                    (s.kind(), s.duration_ms)
                {
                    if let Some(t) = self.turns.last_mut() {
                        t.duration_ms = Some(ms);
                        t.last_at = s.timestamp.clone().or(t.last_at.take());
                    }
                }
            }
            _ => {}
        }
    }

    fn push_assistant(&mut self, a: &AssistantLine) {
        // A response before any user prompt (e.g. resumed session) still
        // needs a turn to live in.
        if self.turns.is_empty() {
            self.turns.push(Turn {
                number: 1,
                started_at: a.timestamp.clone(),
                ..Default::default()
            });
        }
        let t = self.turns.last_mut().expect("turn exists");
        t.tool_calls += a
            .message
            .content
            .iter()
            .filter(|b| matches!(b, crate::transcript::AssistantBlock::ToolUse { .. }))
            .count();
        t.last_at = a.timestamp.clone().or(t.last_at.take());
        if !self.seen_ids.insert(a.message.id.clone()) {
            return; // another block of a response already counted
        }
        let u = Usage::from_api(&a.message.usage);
        t.api_calls += 1;
        t.usage.add(&u);
        t.context_size = u.total_input();
        t.effort = a.effort.clone().or(t.effort.take());
        if !t.models.contains(&a.message.model) {
            t.models.push(a.message.model.clone());
        }
        if let Some(ttl) = a.message.usage.cache_ttl() {
            t.cache_ttl = Some(ttl);
            self.observed_ttl = Some(ttl);
        }
        self.total.add(&u);
        self.model = Some(a.message.model.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::parse_file;
    use std::path::Path;

    fn fixture() -> Vec<Line> {
        parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl")).unwrap()
    }

    #[test]
    fn output_tokens_are_counted_once_per_response() {
        let lines = fixture();
        let agg = Aggregate::from_lines(&lines);
        // Hand-computed from the fixture: 143 distinct ids; naïve per-line sum is 214,682 (3.2×).
        assert_eq!(agg.api_calls(), 143);
        assert_eq!(agg.total.output, 67_061);
        let naive: u64 = lines
            .iter()
            .filter_map(|l| match l {
                Line::Assistant(a) => Some(a.message.usage.output_tokens),
                _ => None,
            })
            .sum();
        assert!(naive > 2 * agg.total.output, "naive {naive} must overstate");
    }

    #[test]
    fn thirteen_block_response_counts_once() {
        let lines = fixture();
        // Find the id that appears 13 times and aggregate only those lines.
        let mut counts = std::collections::HashMap::new();
        for l in &lines {
            if let Line::Assistant(a) = l {
                *counts.entry(a.message.id.clone()).or_insert(0) += 1;
            }
        }
        let (id, n) = counts.iter().max_by_key(|(_, n)| **n).unwrap();
        assert_eq!(*n, 13);
        let only: Vec<&Line> = lines
            .iter()
            .filter(|l| matches!(l, Line::Assistant(a) if &a.message.id == id))
            .collect();
        let agg = Aggregate::from_lines(only.iter().copied());
        let expected = match only[0] {
            Line::Assistant(a) => a.message.usage.output_tokens,
            _ => unreachable!(),
        };
        assert_eq!(agg.api_calls(), 1);
        assert_eq!(agg.total.output, expected);
        assert_eq!(agg.turns[0].tool_calls, 12, "12 tool_use blocks + 1 text");
    }

    #[test]
    fn turn_count_matches_hand_count() {
        let agg = Aggregate::from_lines(&fixture());
        assert_eq!(agg.turns.len(), 15);
        assert_eq!(agg.turns.iter().map(|t| t.api_calls).sum::<usize>(), 143);
        assert!(agg.turns.iter().all(|t| t.number >= 1));
        assert_eq!(
            agg.turns.iter().filter(|t| t.duration_ms.is_some()).count(),
            9
        );
        assert_eq!(agg.turns.iter().map(|t| t.tool_calls).sum::<usize>(), 257);
    }

    #[test]
    fn ttl_and_context_size() {
        let agg = Aggregate::from_lines(&fixture());
        assert_eq!(agg.observed_ttl, Some(CacheTtl::OneHour));
        let last = agg.turns.iter().rev().find(|t| t.api_calls > 0).unwrap();
        assert!(last.context_size > 10_000, "{}", last.context_size);
        assert_eq!(last.usage.cache_write_5m, 0);
        assert!(agg.total.cache_write_1h > 0);
        assert!(agg.total.cache_hit_ratio().unwrap() > 0.9);
        assert_eq!(agg.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(last.effort.as_deref(), Some("medium"));
    }

    #[test]
    fn usage_from_api_without_ttl_breakdown_goes_to_5m() {
        let api = crate::transcript::Usage {
            input_tokens: 10,
            cache_creation_input_tokens: 100,
            cache_read_input_tokens: 1000,
            output_tokens: 5,
            ..Default::default()
        };
        let u = Usage::from_api(&api);
        assert_eq!((u.cache_write_5m, u.cache_write_1h), (100, 0));
        assert_eq!(u.total_input(), 1110);
        assert!((u.cache_hit_ratio().unwrap() - 0.9009).abs() < 1e-3);
    }
}
