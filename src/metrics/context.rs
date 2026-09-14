//! Context-window arithmetic: how full, how fast it fills, when autocompact
//! will hit, and what compactions already happened.

use crate::harness_facts::{autocompact, first_seen};

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

/// A drop of at least this share between consecutive turns is taken as a
/// compaction on transcripts too old to carry `compact_boundary`.
pub const COMPACTION_DROP_RATIO: f64 = 0.30;
const EMA_ALPHA: f64 = 1.0 / 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Compaction {
    /// Turn number in which the drop was observed.
    pub turn: usize,
    pub before: u64,
    pub after: u64,
    /// `auto` / `manual` from `compact_boundary`; empty for the heuristic.
    pub trigger: String,
    pub duration_ms: Option<u64>,
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
    /// The compactions come from the ≥ 30 % drop heuristic (transcripts
    /// before 2.1.263), not from `compact_boundary` lines.
    pub compactions_heuristic: bool,
    /// Autocompact threshold in tokens: effective window − 13 000, or the
    /// size observed just before a compaction when one was seen.
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
    let turn_numbers: Vec<usize> = agg
        .turns
        .iter()
        .filter(|t| t.api_calls > 0)
        .map(|t| t.number)
        .collect();
    // Exact records when the transcript can carry them; the drop heuristic
    // only on older transcripts (and never on API-error lines, which have no
    // usage and no turn entry here).
    let exact = first_seen::COMPACT_BOUNDARY.at_most(agg.version.as_deref());
    let mut compactions: Vec<Compaction> = if exact {
        agg.compactions
            .iter()
            .map(|c| Compaction {
                turn: c.turn,
                before: c.pre_tokens,
                after: c.post_tokens,
                trigger: c.trigger.clone(),
                duration_ms: Some(c.duration_ms),
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut velocity = 0.0;
    let mut have_velocity = false;
    for i in 1..history.len() {
        let (prev, cur) = (history[i - 1], history[i]);
        let dropped = prev > 0 && (cur as f64) < prev as f64 * (1.0 - COMPACTION_DROP_RATIO);
        if dropped {
            if !exact {
                compactions.push(Compaction {
                    turn: turn_numbers[i],
                    before: prev,
                    after: cur,
                    trigger: String::new(),
                    duration_ms: None,
                });
            }
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
    let threshold = observed.unwrap_or_else(|| autocompact::threshold(window));
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
        compactions_heuristic: !exact,
        threshold,
        threshold_learned: observed.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{parse_file, Line};
    use std::path::Path;

    pub(super) fn agg(name: &str) -> Aggregate {
        let lines = parse_file(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{name}.jsonl")),
        )
        .unwrap();
        Aggregate::from_lines(&lines)
    }

    #[test]
    fn fixture_view() {
        let v = view(&agg("session-a"), None, None, None);
        assert_eq!(v.prefix, 60_582);
        assert_eq!(v.size, 396_365);
        assert_eq!(v.window, 1_000_000);
        assert!(!v.window_exact);
        assert_eq!(v.history.len(), 10);
        assert_eq!(v.history[0], 78_509);
        assert!(v.compactions.is_empty());
        assert!(v.compactions_heuristic, "2.1.247 predates compact_boundary");
        assert!(v.velocity > 0.0);
        assert_eq!(v.threshold, 967_000, "effective window − 13 000");
        assert!(!v.threshold_learned);
        let n = v.turns_until_compaction().unwrap();
        assert!(n > 0.0 && n < 50.0, "{n}");
        assert!((v.ratio() - 0.396).abs() < 0.001);
        assert_eq!(v.messages(), 396_365 - 60_582);
    }

    #[test]
    fn overrides_and_learned_threshold() {
        let v = view(
            &agg("session-a"),
            Some(200_000),
            Some(134_000),
            Some(185_000),
        );
        assert_eq!(v.window, 200_000);
        assert!(v.window_exact);
        assert_eq!(v.size, 134_000);
        assert_eq!(v.threshold, 185_000);
        assert!(v.threshold_learned);
        let v = view(&agg("session-a"), Some(200_000), None, None);
        assert_eq!(v.threshold, 187_000);
        assert_eq!(default_window(Some("claude-haiku-4-5-20251001")), 200_000);
        assert_eq!(default_window(Some("claude-opus-5")), 1_000_000);
        assert_eq!(default_window(None), 1_000_000);
    }

    #[test]
    fn exact_compaction_on_fixture_b_and_no_fake_one_from_error_lines() {
        let a = agg("session-b");
        let v = view(&a, None, None, None);
        assert!(!v.compactions_heuristic);
        assert_eq!(v.compactions.len(), 1);
        let c = &v.compactions[0];
        assert_eq!((c.before, c.after), (567_672, 230_014));
        assert_eq!(c.trigger, "auto");
        assert_eq!(c.duration_ms, Some(80_690));
        assert_eq!(v.threshold, 567_672, "learned from the observed compaction");
        // The two API-error lines carry zero usage; they are not in the
        // history, so no ≥ 30 % drop is invented from them.
        assert!(v.history.iter().all(|&h| h > 0));
    }

    #[test]
    fn compaction_detected_and_learned_on_old_transcripts() {
        let mut a = Aggregate::default();
        let mk = |id: &str, ctx: u64| -> Vec<Line> {
            vec![
                Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","version":"2.1.247","message":{"role":"user","content":"go"}}"#).unwrap(),
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
        assert!(v.compactions_heuristic);
        assert_eq!(v.compactions.len(), 1);
        assert_eq!(
            v.compactions[0],
            Compaction {
                turn: 4,
                before: 200_000,
                after: 60_000,
                trigger: String::new(),
                duration_ms: None,
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
        // The same drop on a 2.1.263+ transcript without a compact_boundary
        // is not a compaction (an error line or a /clear, not a compaction).
        let mut b = Aggregate::default();
        for (i, ctx) in [200_000u64, 60_000].iter().enumerate() {
            for l in mk(&format!("n{i}"), *ctx) {
                let l = match l {
                    Line::User(mut u) => {
                        u.version = Some("2.1.270".into());
                        Line::User(u)
                    }
                    other => other,
                };
                b.push(&l);
            }
        }
        let v = view(&b, None, None, None);
        assert!(!v.compactions_heuristic);
        assert!(v.compactions.is_empty());
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

// ------------------------------------------------------------ autocompact

/// The autocompact settings in force, from `settings.json` and the `claude`
/// process environment (`ps eww` / procfs). Precedence follows Claude Code:
/// environment over settings over the model default.
#[derive(Debug, Clone, PartialEq)]
pub struct AutocompactConfig {
    /// `DISABLE_AUTO_COMPACT`, `DISABLE_COMPACT` or `autoCompactEnabled: false`.
    pub disabled: bool,
    /// `CLAUDE_CODE_AUTO_COMPACT_WINDOW` / `autoCompactWindow`: the window
    /// Claude Code reasons with instead of the model's.
    pub window_override: Option<u64>,
    /// `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`: fire at this share of the window
    /// (never above window − 13 000).
    pub pct_override: Option<f64>,
    /// Where the overrides came from: `env`, `settings`, or `default`.
    pub source: &'static str,
}

impl Default for AutocompactConfig {
    fn default() -> Self {
        AutocompactConfig {
            disabled: false,
            window_override: None,
            pct_override: None,
            source: "default",
        }
    }
}

impl AutocompactConfig {
    pub fn from_sources(
        settings: Option<&serde_json::Value>,
        env: &std::collections::BTreeMap<String, String>,
    ) -> AutocompactConfig {
        let mut c = AutocompactConfig::default();
        if let Some(s) = settings {
            if s.get("autoCompactEnabled").and_then(|v| v.as_bool()) == Some(false) {
                c.disabled = true;
                c.source = "settings";
            }
            if let Some(w) = s.get("autoCompactWindow").and_then(|v| v.as_u64()) {
                c.window_override = Some(w);
                c.source = "settings";
            }
            if let Some(e) = s.get("env").and_then(|v| v.as_object()) {
                let get = |k: &str| e.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
                c.apply_env(&get);
            }
        }
        let get = |k: &str| env.get(k).cloned();
        c.apply_env(&get);
        c
    }

    fn apply_env(&mut self, get: &dyn Fn(&str) -> Option<String>) {
        let truthy = |v: Option<String>| v.is_some_and(|v| !matches!(v.trim(), "" | "0" | "false"));
        if truthy(get("DISABLE_AUTO_COMPACT")) || truthy(get("DISABLE_COMPACT")) {
            self.disabled = true;
            self.source = "env";
        }
        if let Some(w) = get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").and_then(|v| v.trim().parse().ok())
        {
            self.window_override = Some(w);
            self.source = "env";
        }
        if let Some(p) = get("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE").and_then(|v| v.trim().parse().ok())
        {
            self.pct_override = Some(p);
            self.source = "env";
        }
    }
}

/// The three bands of the context gauge and where they start, in tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bands {
    /// The window Claude Code reasons with (980 000 on a 1M model, or the
    /// override).
    pub effective_window: u64,
    /// Where autocompact fires: `effective − 13 000`, lowered by a pct override.
    pub threshold: u64,
    /// `threshold − 20 000`: the footer turns to "Context low".
    pub warn_at: u64,
    /// `window − 3 000`: requests are refused.
    pub block_at: u64,
    /// 80 % of the window: a background summary starts.
    pub precompute_at: u64,
    /// Autocompact is off: the threshold will not fire.
    pub disabled: bool,
}

pub fn bands(window: u64, cfg: &AutocompactConfig) -> Bands {
    let effective = cfg
        .window_override
        .map(|w| w.min(window))
        .unwrap_or_else(|| autocompact::effective_window(window));
    let base = effective.saturating_sub(autocompact::BUFFER_TOKENS);
    let threshold = match cfg.pct_override {
        Some(p) if p > 0.0 => ((effective as f64 * p / 100.0).floor() as u64).min(base),
        _ => base,
    };
    Bands {
        effective_window: effective,
        threshold,
        warn_at: threshold.saturating_sub(autocompact::WARN_TOKENS),
        block_at: window.saturating_sub(autocompact::BLOCK_TOKENS),
        precompute_at: (window as f64 * autocompact::PRECOMPUTE_RATIO) as u64,
        disabled: cfg.disabled,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Ok,
    /// Inside the 20 000-token band before the threshold.
    Warn,
    /// Past the threshold (autocompact imminent, or off and heading for the
    /// hard block).
    Blocked,
}

impl Bands {
    pub fn band(&self, size: u64) -> Band {
        if size >= self.threshold || size >= self.block_at {
            Band::Blocked
        } else if size >= self.warn_at {
            Band::Warn
        } else {
            Band::Ok
        }
    }

    /// The footer as Claude Code prints it: `N% until auto-compact`, or
    /// `Context low (N% remaining) · Run /compact to compact & continue` in
    /// the warn band; `N% context used` when the threshold will not fire.
    pub fn footer(&self, size: u64) -> String {
        let pct_used = if self.effective_window == 0 {
            0
        } else {
            (size * 100 / self.effective_window).min(100)
        };
        if self.disabled {
            return format!("{pct_used}% context used");
        }
        let remaining = self.threshold.saturating_sub(size);
        let pct_left = if self.threshold == 0 {
            0
        } else {
            (remaining * 100 / self.threshold).min(100)
        };
        match self.band(size) {
            Band::Ok => format!("{pct_left}% until auto-compact"),
            Band::Warn => {
                format!("Context low ({pct_left}% remaining) · Run /compact to compact & continue")
            }
            Band::Blocked => {
                "Context low (0% remaining) · Run /compact to compact & continue".to_string()
            }
        }
    }

    pub fn precompute_armed(&self, size: u64) -> bool {
        !self.disabled && size >= self.precompute_at
    }
}

// -------------------------------------------------------------- anatomy

/// What the context is made of since the last boundary, in tokens: the
/// slices of the stacked bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Anatomy {
    /// The fixed prefix (system prompt, tools, skills, CLAUDE.md).
    pub prefix: u64,
    /// What the model wrote into tool inputs (chars / 4).
    pub tool_inputs: u64,
    /// Tool results still in context.
    pub tool_results: u64,
    /// Thinking retained from earlier calls.
    pub thinking: u64,
    /// Harness reminders and injected files (attachments).
    pub harness: u64,
    /// The model's prose (text blocks, chars / 4).
    pub prose: u64,
    /// Prompts and everything the estimate cannot place.
    pub unattributed: u64,
    /// One of the slices rests on an estimate (chars / 4, attachment
    /// fallbacks, images).
    pub approx: bool,
}

impl Anatomy {
    /// Slices in bar order with their labels.
    pub fn slices(&self) -> [(&'static str, u64); 7] {
        [
            ("prefix", self.prefix),
            ("inputs", self.tool_inputs),
            ("results", self.tool_results),
            ("thinking", self.thinking),
            ("harness", self.harness),
            ("prose", self.prose),
            ("other", self.unattributed),
        ]
    }
}

/// Attribute `size` tokens of context. `since_turn` is the first turn after
/// the last boundary (1 when there was none).
pub fn anatomy(
    agg: &Aggregate,
    tools: &crate::tools::Stats,
    size: u64,
    prefix: u64,
    since_turn: usize,
) -> Anatomy {
    let turns = agg.turns.iter().filter(|t| t.number >= since_turn);
    let mut thinking = 0;
    let mut harness = 0;
    let mut prose = 0;
    let mut approx = false;
    for t in turns {
        thinking += t.usage.thinking;
        harness += t.harness_tokens;
        approx |= t.harness_approx;
        prose += (t.prose_chars / 4) as u64;
    }
    let calls = tools.calls.iter().filter(|c| c.turn >= since_turn);
    let mut tool_inputs = 0;
    let mut tool_results = 0;
    for c in calls {
        tool_inputs += (c.input_chars / 4) as u64;
        tool_results += c.result_tokens_est;
    }
    let placed = prefix + tool_inputs + tool_results + thinking + harness + prose;
    let (placed_scaled, unattributed) = if placed > size && size > 0 {
        // Estimates overshoot the exact size: scale them to fit.
        approx = true;
        (size, 0)
    } else {
        (placed, size.saturating_sub(placed))
    };
    let scale = if placed > 0 {
        placed_scaled as f64 / placed as f64
    } else {
        1.0
    };
    let s = |v: u64| (v as f64 * scale) as u64;
    Anatomy {
        prefix: s(prefix),
        tool_inputs: s(tool_inputs),
        tool_results: s(tool_results),
        thinking: s(thinking),
        harness: s(harness),
        prose: s(prose),
        unattributed,
        approx: approx || tool_inputs > 0 || tool_results > 0 || prose > 0,
    }
}

#[cfg(test)]
mod band_tests {
    use super::tests::agg;
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn bands_follow_claude_codes_arithmetic() {
        let cfg = AutocompactConfig::from_sources(None, &BTreeMap::new());
        assert_eq!(cfg.source, "default");
        let b = bands(1_000_000, &cfg);
        assert_eq!(b.effective_window, 980_000);
        assert_eq!(b.threshold, 967_000);
        assert_eq!(b.warn_at, 947_000);
        assert_eq!(b.block_at, 997_000);
        assert_eq!(b.precompute_at, 800_000);
        assert_eq!(b.band(412_000), Band::Ok);
        assert_eq!(b.band(950_000), Band::Warn);
        assert_eq!(b.band(970_000), Band::Blocked);
        assert_eq!(b.footer(412_000), "57% until auto-compact");
        assert_eq!(
            b.footer(950_000),
            "Context low (1% remaining) · Run /compact to compact & continue"
        );
        assert!(!b.precompute_armed(412_000));
        assert!(b.precompute_armed(800_000));
        let b = bands(200_000, &cfg);
        assert_eq!(b.threshold, 187_000);
        assert_eq!(b.footer(134_000), "28% until auto-compact");
    }

    #[test]
    fn overrides_from_settings_and_env() {
        let settings = serde_json::json!({"autoCompactWindow": 400000, "env": {"CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "50"}});
        let cfg = AutocompactConfig::from_sources(Some(&settings), &BTreeMap::new());
        assert_eq!(cfg.window_override, Some(400_000));
        assert_eq!(cfg.pct_override, Some(50.0));
        assert_eq!(cfg.source, "env");
        let b = bands(1_000_000, &cfg);
        assert_eq!(b.effective_window, 400_000);
        assert_eq!(b.threshold, 200_000, "min(400k × 50 %, 400k − 13k)");
        let mut env = BTreeMap::new();
        env.insert("DISABLE_AUTO_COMPACT".to_string(), "1".to_string());
        let cfg = AutocompactConfig::from_sources(Some(&serde_json::json!({})), &env);
        assert!(cfg.disabled);
        let b = bands(1_000_000, &cfg);
        assert_eq!(b.footer(412_000), "42% context used");
        assert!(!b.precompute_armed(900_000));
        let cfg = AutocompactConfig::from_sources(
            Some(&serde_json::json!({"autoCompactEnabled": false})),
            &BTreeMap::new(),
        );
        assert!(cfg.disabled);
    }

    #[test]
    fn anatomy_places_every_token_once() {
        let a = agg("session-b");
        let tools = crate::tools::Stats::from_lines(
            &crate::transcript::parse_file(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl"),
            )
            .unwrap(),
        );
        let size = a.context_size();
        let an = anatomy(&a, &tools, size, 60_000, 1);
        let sum: u64 = an.slices().iter().map(|(_, v)| v).sum();
        assert!(
            sum >= size.saturating_sub(7) && sum <= size,
            "{an:?} vs {size}"
        );
        assert!(an.harness > 0 && an.tool_results > 0 && an.tool_inputs > 0 && an.prose > 0);
        assert!(an.approx);
        // Since a later boundary, less is placed.
        let later = anatomy(&a, &tools, size, 60_000, 5);
        assert!(later.tool_results <= an.tool_results);
    }
}
