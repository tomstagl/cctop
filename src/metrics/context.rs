//! Context-window arithmetic: how full, how fast it fills, when autocompact
//! will hit, and what compactions already happened.

use super::usage::Aggregate;

/// Default context window per model family when the status line is absent.
pub fn default_window(model: Option<&str>) -> u64 {
    match model {
        Some(m) if m.contains("haiku-4-5") => 200_000,
        Some(m) if m.contains("haiku") => 200_000,
        Some(m) if m.contains("-3-") || m.contains("-4-5") || m.contains("-4-1") => 200_000,
        _ => 1_000_000,
    }
}

/// Autocompact fires at this share of the window until observed otherwise.
pub const DEFAULT_THRESHOLD_RATIO: f64 = 0.80;
/// A drop of at least this share between consecutive turns is a compaction.
pub const COMPACTION_DROP_RATIO: f64 = 0.30;
const EMA_ALPHA: f64 = 1.0 / 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Compaction {
    /// Turn number in which the drop was observed.
    pub turn: usize,
    pub before: u64,
    pub after: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextView {
    /// Context the model last saw.
    pub size: u64,
    pub window: u64,
    /// Window came from the status line (exact) rather than the model default.
    pub window_exact: bool,
    /// Fixed prefix: first API call's cache_read + cache_creation.
    pub prefix: u64,
    /// Per-turn context sizes for turns that made an API call, oldest first.
    pub history: Vec<u64>,
    /// EMA of Δsize per turn (compaction turns excluded).
    pub velocity: f64,
    pub compactions: Vec<Compaction>,
    /// Threshold in tokens; learned from the first observed compaction.
    pub threshold: u64,
    pub threshold_learned: bool,
}

impl ContextView {
    pub fn ratio(&self) -> f64 {
        if self.window == 0 {
            0.0
        } else {
            self.size as f64 / self.window as f64
        }
    }

    pub fn free(&self) -> u64 {
        self.window.saturating_sub(self.size)
    }

    pub fn messages(&self) -> u64 {
        self.size.saturating_sub(self.prefix)
    }

    /// Turns until autocompact at the current velocity; `None` when not filling.
    pub fn turns_until_compaction(&self) -> Option<f64> {
        if self.velocity <= 0.0 {
            return None;
        }
        Some((self.threshold as f64 - self.size as f64).max(0.0) / self.velocity)
    }
}

/// Compute the view from the aggregate. `window_override` and `size_override`
/// come from the status line when the shim is installed; `learned_threshold`
/// is the size observed just before a compaction for this model, if known.
pub fn view(
    agg: &Aggregate,
    window_override: Option<u64>,
    size_override: Option<u64>,
    learned_threshold: Option<u64>,
) -> ContextView {
    let window = window_override.unwrap_or_else(|| default_window(agg.model.as_deref()));
    let history: Vec<u64> = agg
        .turns
        .iter()
        .filter(|t| t.api_calls > 0)
        .map(|t| t.context_size)
        .collect();
    let mut compactions = Vec::new();
    let mut velocity = 0.0;
    let mut have_velocity = false;
    let turn_numbers: Vec<usize> = agg
        .turns
        .iter()
        .filter(|t| t.api_calls > 0)
        .map(|t| t.number)
        .collect();
    for i in 1..history.len() {
        let (prev, cur) = (history[i - 1], history[i]);
        if prev > 0 && (cur as f64) < prev as f64 * (1.0 - COMPACTION_DROP_RATIO) {
            compactions.push(Compaction {
                turn: turn_numbers[i],
                before: prev,
                after: cur,
            });
            continue;
        }
        let delta = cur as f64 - prev as f64;
        if have_velocity {
            velocity = EMA_ALPHA * delta + (1.0 - EMA_ALPHA) * velocity;
        } else {
            velocity = delta;
            have_velocity = true;
        }
    }
    let observed = learned_threshold.or_else(|| compactions.iter().map(|c| c.before).max());
    let threshold = observed.unwrap_or((window as f64 * DEFAULT_THRESHOLD_RATIO) as u64);
    let prefix = agg
        .turns
        .iter()
        .find(|t| t.api_calls > 0)
        .map(|t| t.first_call_prefix)
        .unwrap_or(0);
    ContextView {
        size: size_override.unwrap_or_else(|| history.last().copied().unwrap_or(0)),
        window,
        window_exact: window_override.is_some(),
        prefix,
        history,
        velocity,
        compactions,
        threshold,
        threshold_learned: observed.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{parse_file, Line};
    use std::path::Path;

    fn agg() -> Aggregate {
        let lines =
            parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl"))
                .unwrap();
        Aggregate::from_lines(&lines)
    }

    #[test]
    fn fixture_view() {
        let v = view(&agg(), None, None, None);
        assert_eq!(v.prefix, 60_582);
        assert_eq!(v.size, 396_365);
        assert_eq!(v.window, 1_000_000);
        assert!(!v.window_exact);
        assert_eq!(v.history.len(), 10);
        assert_eq!(v.history[0], 78_509);
        assert!(v.compactions.is_empty());
        assert!(v.velocity > 0.0);
        assert_eq!(v.threshold, 800_000);
        assert!(!v.threshold_learned);
        let n = v.turns_until_compaction().unwrap();
        assert!(n > 0.0 && n < 50.0, "{n}");
        assert!((v.ratio() - 0.396).abs() < 0.001);
        assert_eq!(v.messages(), 396_365 - 60_582);
    }

    #[test]
    fn overrides_and_learned_threshold() {
        let v = view(&agg(), Some(200_000), Some(134_000), Some(185_000));
        assert_eq!(v.window, 200_000);
        assert!(v.window_exact);
        assert_eq!(v.size, 134_000);
        assert_eq!(v.threshold, 185_000);
        assert!(v.threshold_learned);
        assert_eq!(default_window(Some("claude-haiku-4-5-20251001")), 200_000);
        assert_eq!(default_window(Some("claude-opus-5")), 1_000_000);
        assert_eq!(default_window(None), 1_000_000);
    }

    #[test]
    fn compaction_detected_and_learned() {
        let mut a = Aggregate::default();
        let mk = |id: &str, ctx: u64| -> Vec<Line> {
            vec![
                Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"go"}}"#).unwrap(),
                Line::parse(&format!(
                    r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{{"id":"{id}","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}],"usage":{{"input_tokens":{ctx},"output_tokens":1}}}}}}"#
                ))
                .unwrap(),
            ]
        };
        for (i, ctx) in [100_000u64, 150_000, 200_000, 60_000, 90_000]
            .iter()
            .enumerate()
        {
            for l in mk(&format!("m{i}"), *ctx) {
                a.push(&l);
            }
        }
        let v = view(&a, None, None, None);
        assert_eq!(v.compactions.len(), 1);
        assert_eq!(
            v.compactions[0],
            Compaction {
                turn: 4,
                before: 200_000,
                after: 60_000
            }
        );
        assert_eq!(v.threshold, 200_000, "learned from the observed compaction");
        assert!(v.threshold_learned);
        // Velocity ignores the compaction step: deltas 50k, 50k, 30k.
        assert!(
            v.velocity > 30_000.0 && v.velocity < 50_000.0,
            "{}",
            v.velocity
        );
        assert_eq!(v.size, 90_000);
    }

    #[test]
    fn no_velocity_when_shrinking_or_single_turn() {
        let mut a = Aggregate::default();
        a.push(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"x","model":"m","content":[],"usage":{"input_tokens":10}}}"#).unwrap());
        let v = view(&a, None, None, None);
        assert_eq!(v.velocity, 0.0);
        assert_eq!(v.turns_until_compaction(), None);
    }
}
