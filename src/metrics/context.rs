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
    /// EMA of Δsize per turn (compaction turns excluded); `None` until two
    /// turns have made a call — one delta — so the first turn says `—`,
    /// not `+0/turn` (PRD dashboard-v2 FR-11).
    pub velocity: Option<f64>,
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
        let velocity = self.velocity.filter(|v| *v > 0.0)?;
        Some((self.threshold as f64 - self.size as f64).max(0.0) / velocity)
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
    let mut velocity: Option<f64> = None;
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
        velocity = Some(match velocity {
            Some(v) => EMA_ALPHA * delta + (1.0 - EMA_ALPHA) * v,
            None => delta,
        });
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
        assert!(v.velocity.unwrap() > 0.0);
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
        assert_eq!(
            v.threshold, 167_000,
            "200 k − the 20 k output reserve − 13 k"
        );
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
            v.velocity.unwrap() > 30_000.0 && v.velocity.unwrap() < 50_000.0,
            "{:?}",
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
        // One turn, one call: no delta yet — `None`, never a computed zero.
        let mut a = Aggregate::default();
        a.push(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"id":"x","model":"m","content":[],"usage":{"input_tokens":10}}}"#).unwrap());
        let v = view(&a, None, None, None);
        assert_eq!(v.velocity, None);
        assert_eq!(v.turns_until_compaction(), None);
        // Two turns, the second smaller: a sample, and it says shrinking.
        a.push(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:02Z","promptSource":"typed","message":{"role":"user","content":"more"}}"#).unwrap());
        a.push(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"y","model":"m","content":[],"usage":{"input_tokens":8}}}"#).unwrap());
        let v = view(&a, None, None, None);
        assert_eq!(v.velocity, Some(-2.0));
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
    /// The window Claude Code reasons with: the nominal one less the 20 000
    /// output reserve (980 000 on a 1M model, 180 000 on 200 k), or the
    /// override.
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
        let pct_used = (size * 100)
            .checked_div(self.effective_window)
            .map_or(0, |p| p.min(100));
        if self.disabled {
            return format!("{pct_used}% context used");
        }
        let remaining = self.threshold.saturating_sub(size);
        let pct_left = (remaining * 100)
            .checked_div(self.threshold)
            .map_or(0, |p| p.min(100));
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
/// slices of the stacked bar. Derived from [`Residency`], which does the
/// per-call arithmetic; kept as a plain struct because the bar, the pane's
/// `Body.slices` and `docs/metrics.md` name these seven slices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Anatomy {
    /// The fixed prefix (system prompt, tools, skills, CLAUDE.md).
    pub prefix: u64,
    /// What the model wrote into tool inputs (chars / 4, capped per call at
    /// that call's real output less its thinking).
    pub tool_inputs: u64,
    /// Tool results still in context, reconciled step by step.
    pub tool_results: u64,
    /// Thinking retained from earlier calls (exact).
    pub thinking: u64,
    /// Harness reminders and injected files (attachments).
    pub harness: u64,
    /// The model's prose: `output − thinking − inputs` per call (exact).
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

impl From<&Residency> for Anatomy {
    fn from(r: &Residency) -> Anatomy {
        Anatomy {
            prefix: r.prefix,
            tool_inputs: r.tool_inputs,
            tool_results: r.results(),
            thinking: r.thinking,
            harness: r.source(Source::Harness),
            prose: r.prose,
            unattributed: r.other + r.source(Source::Prompts),
            approx: r.approx,
        }
    }
}

/// Why the current window starts where it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// `/clear` (or `continued-in`).
    Clear,
    /// `system/compact_boundary`.
    Compact,
    Microcompact,
    Resume,
    Fork,
    /// A ≥ 30 % drop with no marker (transcripts before 2.1.263).
    Heuristic,
    /// A smaller drop with no marker: content left the window (the corpus
    /// has a 27 % one — PRD context-residency §3.2 I).
    Shrink,
    /// The model changed and the same conversation re-measured smaller.
    ModelSwitch,
}

impl RefKind {
    pub fn label(self) -> &'static str {
        match self {
            RefKind::Clear => "/clear",
            RefKind::Compact => "compaction",
            RefKind::Microcompact => "microcompact",
            RefKind::Resume => "resume",
            RefKind::Fork => "fork",
            RefKind::Heuristic => "compaction (inferred)",
            RefKind::Shrink => "window shrank",
            RefKind::ModelSwitch => "model switch",
        }
    }

    fn from_boundary(k: &super::usage::BoundaryKind) -> RefKind {
        use super::usage::BoundaryKind as B;
        match k {
            B::Clear => RefKind::Clear,
            B::Compact => RefKind::Compact,
            B::Microcompact => RefKind::Microcompact,
            B::Resume => RefKind::Resume,
            B::Fork => RefKind::Fork,
        }
    }
}

/// Where the window's accounting starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    SessionStart,
    /// The index into `agg.calls` of the window's first call, and the
    /// Δcontext that opened it.
    Boundary {
        kind: RefKind,
        call: usize,
        delta: i64,
    },
}

/// A kind of injected content, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Whole-file reads: `Read`, and a Bash `cat` / `sed -n` / `head` /
    /// `tail` of exactly one path.
    Files,
    /// Bash output not tied to one file (builds, tests, greps, multi-file cats).
    BashOutput,
    McpResults,
    AgentReturns,
    Web,
    /// Grep, Glob, ToolSearch, an Edit's "file updated", everything else.
    OtherResults,
    /// What the person typed (and pasted images).
    Prompts,
    /// Reminders, listings and injected files (attachments).
    Harness,
}

impl Source {
    pub const ALL: [Source; 8] = [
        Source::Files,
        Source::BashOutput,
        Source::McpResults,
        Source::AgentReturns,
        Source::Web,
        Source::OtherResults,
        Source::Prompts,
        Source::Harness,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Source::Files => "files",
            Source::BashOutput => "bash output",
            Source::McpResults => "mcp results",
            Source::AgentReturns => "agent returns",
            Source::Web => "web",
            Source::OtherResults => "other results",
            Source::Prompts => "prompts",
            Source::Harness => "harness",
        }
    }

    fn idx(self) -> usize {
        Source::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// One file's share of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    pub path: String,
    /// Read results in the window (the `files` source), reconciled.
    pub tokens: u64,
    /// Edit / Write input bytes the model wrote for this file (in `inputs`).
    pub written: u64,
    pub reads: usize,
}

/// Whether the prefix comes from the first call or from a `/context` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Estimated,
    /// Calibrated against the `/context` the person ran in this turn.
    Calibrated {
        turn: usize,
    },
}

/// What is in the window right now and what put it there — the per-call
/// model behind [`Anatomy`] (PRD context-residency §4).
///
/// Every estimate is reconciled per step against the exact growth that step
/// could have cost (`Δcontext − previous output_tokens`, FR-16), so the parts
/// never exceed the whole; `overflow_raw` says by how much they would have.
#[derive(Debug, Clone, PartialEq)]
pub struct Residency {
    pub size: u64,
    /// The first call's cached part, lowered to the context right after any
    /// boundary that lands below it (FR-20).
    pub prefix: u64,
    pub prefix_tightened: bool,
    pub since: Reference,
    /// API calls in the window, the in-flight last one included.
    pub calls_since: usize,
    /// Exact, from `thinking_tokens`; the in-flight call excluded.
    pub thinking: u64,
    /// The transcript's version writes `thinking_tokens` at all.
    pub thinking_known: bool,
    /// Tool-use bytes the model wrote (chars / 4), capped per call.
    pub tool_inputs: u64,
    /// `output − thinking − inputs` per call: exact.
    pub prose: u64,
    sources: [u64; 8],
    pub files: Vec<FileRow>,
    /// `size` less everything above: the compaction summary, reminders no
    /// attachment recorded, cache accounting.
    pub other: u64,
    /// What per-step reconciliation removed from the estimates.
    pub reconciled: u64,
    /// What the overflow would have been with no reconciliation at all.
    pub overflow_raw: u64,
    /// A model switch that did not shrink the window: kept, noted.
    pub model_switch_kept: Option<(String, String, i64)>,
    pub approx: bool,
    pub mode: Mode,
}

impl Residency {
    pub fn source(&self, s: Source) -> u64 {
        self.sources[s.idx()]
    }
    /// The six result kinds together.
    pub fn results(&self) -> u64 {
        Source::ALL[..6].iter().map(|s| self.source(*s)).sum()
    }
    /// Everything injected between calls: results, prompts, harness.
    pub fn injected(&self) -> u64 {
        self.sources.iter().sum()
    }
    pub fn messages(&self) -> u64 {
        self.size.saturating_sub(self.prefix)
    }
    /// Rows in display order: the eight sources, then thinking, inputs,
    /// prose, other. `(label, tokens, exact)`.
    pub fn rows(&self) -> Vec<(&'static str, u64, bool)> {
        let mut v: Vec<(&'static str, u64, bool)> = Source::ALL
            .iter()
            .map(|s| (s.label(), self.source(*s), false))
            .collect();
        v.push(("thinking", self.thinking, true));
        v.push(("tool inputs", self.tool_inputs, false));
        v.push(("prose", self.prose, true));
        v.push(("other", self.other, false));
        v
    }
}

fn source_of(c: &crate::tools::Call) -> Source {
    match c.name.as_str() {
        "Read" | "NotebookRead" => Source::Files,
        "Bash" if c.path.is_some() => Source::Files,
        "Bash" => Source::BashOutput,
        n if n.starts_with("mcp:") => Source::McpResults,
        "Agent" | "Task" => Source::AgentReturns,
        "WebFetch" | "WebSearch" => Source::Web,
        _ => Source::OtherResults,
    }
}

fn writes_file(c: &crate::tools::Call) -> bool {
    matches!(
        c.name.as_str(),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
    )
}

/// Attribute `size` tokens of context. `prefix_first` is the first call's
/// cached part ([`ContextView::prefix`]); `cwd` resolves the relative paths
/// a Bash reader names; `capture` is the `/context` table the person ran,
/// with the turn it ran in — its non-`Messages` categories are Claude Code's
/// own prefix and replace the first-call estimate (+33 % on the one ground
/// truth, PRD §3.2 G), unless a model switch since re-measured everything.
pub fn residency(
    agg: &Aggregate,
    tools: &crate::tools::Stats,
    prefix_first: u64,
    size: u64,
    cwd: Option<&std::path::Path>,
    capture: Option<(&crate::transcript::ContextCapture, usize)>,
) -> Residency {
    let calls = &agg.calls;
    let mut r = Residency {
        size,
        prefix: prefix_first,
        prefix_tightened: false,
        since: Reference::SessionStart,
        calls_since: 0,
        thinking: 0,
        thinking_known: first_seen::THINKING_TOKENS.at_most(agg.version.as_deref()),
        tool_inputs: 0,
        prose: 0,
        sources: [0; 8],
        files: Vec::new(),
        other: size.saturating_sub(prefix_first),
        reconciled: 0,
        overflow_raw: 0,
        model_switch_kept: None,
        approx: false,
        mode: Mode::Estimated,
    };
    if calls.is_empty() {
        return r;
    }
    let n = calls.len();
    let at = |k: usize| calls[k].at_ms;

    // An explicit boundary applies from the first call after its line.
    let explicit: Vec<(usize, RefKind)> = agg
        .boundaries
        .iter()
        .filter_map(|b| {
            let k = match b.at.as_deref().and_then(super::cost::parse_ts_ms) {
                Some(ms) => (0..n).find(|&k| at(k).is_some_and(|a| a >= ms)),
                None => (0..n).find(|&k| calls[k].turn >= b.turn),
            }?;
            Some((k, RefKind::from_boundary(&b.kind)))
        })
        .collect();

    // The reference: the last boundary of any kind (FR-17, FR-18). Within a
    // session the context never shrinks except by removal or re-measurement,
    // so any negative step is one.
    let mut start = 0usize;
    let mut prefix = prefix_first;
    if let Some((_, kind)) = explicit.iter().find(|(k, _)| *k == 0) {
        r.since = Reference::Boundary {
            kind: *kind,
            call: 0,
            delta: 0,
        };
    }
    let mut last_model: Option<&str> = None;
    for k in 0..n {
        let ctx = calls[k].context();
        let mdl = calls[k].model.as_str();
        let model_changed = last_model.is_some_and(|m| m != mdl);
        if k > 0 {
            let prev = calls[k - 1].context();
            let delta = ctx as i64 - prev as i64;
            let explicit_here = explicit.iter().find(|(i, _)| *i == k).map(|(_, kd)| *kd);
            let kind = if ctx < prev {
                Some(explicit_here.unwrap_or(if model_changed {
                    RefKind::ModelSwitch
                } else if (ctx as f64) < prev as f64 * (1.0 - COMPACTION_DROP_RATIO) {
                    RefKind::Heuristic
                } else {
                    RefKind::Shrink
                }))
            } else {
                explicit_here
            };
            if let Some(kind) = kind {
                start = k;
                r.since = Reference::Boundary {
                    kind,
                    call: k,
                    delta,
                };
                r.model_switch_kept = None;
                if ctx < prefix {
                    prefix = ctx;
                    r.prefix_tightened = true;
                }
            } else if model_changed {
                r.model_switch_kept =
                    Some((last_model.unwrap_or("").to_string(), mdl.to_string(), delta));
            }
        }
        if !mdl.is_empty() && mdl != "<synthetic>" {
            last_model = Some(mdl);
        }
    }
    // Calibration: the /context table's own prefix, when the person ran one
    // and no model switch since has re-measured the window (FR-14, decision 4).
    if let Some((cap, turn)) = capture {
        let switched_since = matches!(
            r.since,
            Reference::Boundary { kind: RefKind::ModelSwitch, call, .. } if calls[call].turn > turn
        );
        let cal: u64 = cap
            .categories
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("Messages"))
            .map(|(_, t)| *t)
            .sum();
        if cal > 0 && !switched_since {
            // The running minimum still applies: a boundary below the table's
            // figure is the tighter bound.
            let floor = if r.prefix_tightened { prefix } else { u64::MAX };
            prefix = cal.min(floor);
            r.mode = Mode::Calibrated { turn };
        }
    }
    r.prefix = prefix;
    let m = n - start;
    r.calls_since = m;

    // Every estimate lands on a step: the growth from one call to the next.
    const NS: usize = 8;
    let mut inj: Vec<[u64; NS]> = vec![[0; NS]; m];
    let mut inputs_at: Vec<u64> = vec![0; m];
    // (step, path, tokens, is a read)
    let mut file_steps: Vec<(usize, String, u64, bool)> = Vec::new();
    let resolve = |p: &str| -> String {
        if std::path::Path::new(p).is_absolute() {
            p.to_string()
        } else {
            cwd.map(|c| c.join(p).to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string())
        }
    };
    // A result finished at `f` is in the context of the first call after it.
    let result_step = |f: i64| -> Option<usize> {
        let k = (0..n).find(|&k| at(k).is_some_and(|a| a >= f))?;
        (k >= start).then(|| k - start)
    };
    // A tool_use started at `s` was written by the last call at or before it.
    let issue_step = |s: i64| -> Option<usize> {
        let k = (0..n).rev().find(|&k| at(k).is_some_and(|a| a <= s))?;
        (k >= start).then(|| k - start)
    };
    for c in &tools.calls {
        if let Some(step) = c.started_at.and_then(issue_step) {
            let tok = (c.input_chars / 4) as u64;
            inputs_at[step] += tok;
            if let (Some(p), true) = (&c.path, writes_file(c)) {
                file_steps.push((step, resolve(p), tok, false));
            }
        }
        // No result yet, or one that landed after the last call: not resident
        // (FR-12). A cleared result has `result_tokens_est == 0` already.
        if let Some(step) = c.finished_at.and_then(result_step) {
            let source = source_of(c);
            inj[step][source.idx()] += c.result_tokens_est;
            if let (Some(p), Source::Files) = (&c.path, source) {
                file_steps.push((step, resolve(p), c.result_tokens_est, true));
            }
        }
    }
    // A turn's prompt lands at the turn's first call. A turn that began before
    // the window lost its prompt to the boundary.
    let first_step_of_turn = |turn: usize| -> Option<usize> {
        if (0..start).any(|j| calls[j].turn == turn) {
            return None;
        }
        (start..n)
            .find(|&k| calls[k].turn == turn)
            .map(|k| k - start)
    };
    for t in &agg.turns {
        if let Some(step) = first_step_of_turn(t.number) {
            inj[step][Source::Prompts.idx()] +=
                (t.prompt_chars / 4) as u64 + t.prompt_images as u64 * 1_500;
        }
    }
    // An attachment is in the context of the first call after it, like a tool
    // result — an attachment with no timestamp falls to its turn's first call.
    // Attachments before the window's start left with the boundary.
    for e in &agg.harness_events {
        let step = match e.at_ms {
            Some(ms) => result_step(ms),
            None => first_step_of_turn(e.turn),
        };
        if let Some(step) = step {
            inj[step][Source::Harness.idx()] += e.tokens;
            r.approx |= e.approx;
        }
    }
    // Reconcile each step against the exact room it had (FR-16). The first
    // step of the window measures from the prefix: the uncached first prompt
    // (FR-19), or everything the boundary left behind.
    let mut raw_injected = 0u64;
    let mut scale_at: Vec<f64> = vec![1.0; m];
    for s in 0..m {
        let k = start + s;
        let ctx = calls[k].context() as i64;
        let (d, out_prev) = if s == 0 {
            (ctx - prefix as i64, 0u64)
        } else {
            (
                ctx - calls[k - 1].context() as i64,
                calls[k - 1].usage.output,
            )
        };
        let budget = (d - out_prev as i64).max(0) as u64;
        let est: u64 = inj[s].iter().sum();
        raw_injected += est;
        if est > budget && est > 0 {
            let f = budget as f64 / est as f64;
            scale_at[s] = f;
            for v in inj[s].iter_mut() {
                *v = (*v as f64 * f) as u64;
            }
            r.reconciled += est - inj[s].iter().sum::<u64>();
        }
    }
    // The model's own output, exactly; the in-flight last call excluded — its
    // output is not in any context yet.
    for k in start..n.saturating_sub(1) {
        let u = &calls[k].usage;
        let think = u.thinking.min(u.output);
        let inputs = inputs_at[k - start].min(u.output - think);
        r.thinking += think;
        r.tool_inputs += inputs;
        r.prose += u.output - think - inputs;
    }
    for row in &inj {
        for (i, v) in row.iter().enumerate() {
            r.sources[i] += v;
        }
    }
    r.approx |= r.injected() > 0 || r.tool_inputs > 0;
    let mut files: std::collections::BTreeMap<String, FileRow> = Default::default();
    for (step, path, tok, is_read) in file_steps {
        let e = files.entry(path.clone()).or_insert_with(|| FileRow {
            path,
            tokens: 0,
            written: 0,
            reads: 0,
        });
        if is_read {
            e.tokens += (tok as f64 * scale_at[step]) as u64;
            e.reads += 1;
        } else {
            e.written += tok;
        }
    }
    let mut files: Vec<FileRow> = files.into_values().collect();
    files.sort_by(|a, b| {
        (b.tokens + b.written)
            .cmp(&(a.tokens + a.written))
            .then_with(|| a.path.cmp(&b.path))
    });
    r.files = files;
    let exact = prefix + r.thinking + r.tool_inputs + r.prose;
    r.overflow_raw = (exact + raw_injected).saturating_sub(size);
    let placed = exact + r.injected();
    if placed > size {
        // Cannot happen after per-step reconciliation unless `size` is an
        // override below the last call's context; scale the estimates, never
        // the exact parts.
        let room = size.saturating_sub(exact);
        let inj_total = r.injected();
        if inj_total > 0 {
            let f = room as f64 / inj_total as f64;
            for v in r.sources.iter_mut() {
                *v = (*v as f64 * f) as u64;
            }
        }
        r.other = 0;
    } else {
        r.other = size - placed;
    }
    r
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
        assert_eq!(b.effective_window, 180_000);
        assert_eq!(b.threshold, 167_000);
        assert_eq!(b.footer(134_000), "19% until auto-compact");
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

    fn synth(lines: Vec<serde_json::Value>) -> (Aggregate, crate::tools::Stats) {
        let parsed: Vec<crate::transcript::Line> = lines
            .into_iter()
            .map(crate::transcript::Line::from_value)
            .collect();
        (
            Aggregate::from_lines(&parsed),
            crate::tools::Stats::from_lines(&parsed),
        )
    }
    fn ts(sec: u32) -> String {
        format!("2026-01-01T00:{:02}:{:02}Z", sec / 60, sec % 60)
    }
    fn user(sec: u32, text: &str) -> serde_json::Value {
        serde_json::json!({"type":"user","timestamp":ts(sec),"message":{"role":"user","content":text}})
    }
    /// One API response; `ctx` is `(cache_read, uncached input)`, summed.
    fn call(
        sec: u32,
        id: &str,
        model: &str,
        ctx: (u64, u64),
        out: u64,
        think: u64,
        content: serde_json::Value,
    ) -> serde_json::Value {
        let (cache_read, input) = ctx;
        serde_json::json!({"type":"assistant","timestamp":ts(sec),"requestId":id,
            "message":{"id":id,"model":model,"content":content,
                "usage":{"input_tokens":input,"cache_read_input_tokens":cache_read,"cache_creation_input_tokens":0,
                         "output_tokens":out,"output_tokens_details":{"thinking_tokens":think}}}})
    }
    fn text() -> serde_json::Value {
        serde_json::json!([{"type":"text","text":"ok"}])
    }
    fn read(id: &str, path: &str) -> serde_json::Value {
        serde_json::json!([{"type":"tool_use","id":id,"name":"Read","input":{"file_path":path}}])
    }
    fn result(sec: u32, id: &str, chars: usize) -> serde_json::Value {
        serde_json::json!({"type":"user","timestamp":ts(sec),"message":{"role":"user",
            "content":[{"type":"tool_result","tool_use_id":id,"content":"x".repeat(chars)}]}})
    }
    fn placed(r: &Residency) -> u64 {
        r.prefix + r.thinking + r.tool_inputs + r.prose + r.injected() + r.other
    }

    #[test]
    fn residency_places_every_token_once_on_every_fixture() {
        for name in [
            "session-a",
            "session-b",
            "session-c",
            "session-d",
            "session-e",
        ] {
            let a = agg(name);
            let tools = crate::tools::Stats::from_lines(
                &crate::transcript::parse_file(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join(format!("fixtures/{name}.jsonl")),
                )
                .unwrap(),
            );
            let v = view(&a, None, None, None);
            let r = residency(&a, &tools, v.prefix, v.size, None, None);
            assert_eq!(placed(&r), v.size, "{name}: {r:?}");
            let an = Anatomy::from(&r);
            let sum: u64 = an.slices().iter().map(|(_, v)| v).sum();
            assert_eq!(sum, v.size, "{name}: the bar's slices are the same tokens");
            assert!(r.prefix <= v.prefix, "{name}: FR-20 only lowers the prefix");
            // Whatever would have overflowed was reconciled away (FR-16) —
            // guaranteed by construction while `other ≥ 0` — and it is small:
            // the fixtures' strings are capped (PRD §3.2 C), so results
            // undershoot; only a prompt or attachment estimate can overshoot
            // a step (fixture E: 664 tokens on 59 k, 1.1 %).
            assert!(
                r.reconciled >= r.overflow_raw,
                "{name}: reconciled {} < overflow_raw {}",
                r.reconciled,
                r.overflow_raw
            );
            assert!(
                r.overflow_raw * 50 < v.size,
                "{name}: overflow_raw {} is over 2 % of {}",
                r.overflow_raw,
                v.size
            );
        }
    }

    #[test]
    fn fixture_b_starts_at_its_compaction() {
        let a = agg("session-b");
        let tools = crate::tools::Stats::from_lines(
            &crate::transcript::parse_file(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl"),
            )
            .unwrap(),
        );
        let v = view(&a, None, None, None);
        let r = residency(&a, &tools, v.prefix, v.size, None, None);
        assert!(
            matches!(r.since, Reference::Boundary { .. }),
            "{:?}",
            r.since
        );
        assert!(r.calls_since < a.calls.len());
        assert!(r.thinking_known);
    }

    #[test]
    fn a_context_table_calibrates_the_prefix() {
        // Fixture D holds the corpus's one native /context capture: System
        // prompt 10.2 k + System tools 31.2 k + Skills 4.8 k = 46 200, against a
        // first-call estimate a third larger (PRD §3.2 G).
        let lines = crate::transcript::parse_file(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-d.jsonl"),
        )
        .unwrap();
        let a = Aggregate::from_lines(&lines);
        let tools = crate::tools::Stats::from_lines(&lines);
        let mut pfx = crate::prefix::Prefix::default();
        for l in &lines {
            pfx.push(l);
        }
        let cap = pfx
            .context_capture
            .as_ref()
            .expect("fixture D has the table");
        let turn = a
            .slash_commands
            .iter()
            .rev()
            .find(|(_, c)| c == "/context")
            .map(|(t, _)| *t)
            .unwrap_or(0);
        let v = view(&a, None, None, None);
        let r = residency(&a, &tools, v.prefix, v.size, None, Some((cap, turn)));
        assert_eq!(r.mode, Mode::Calibrated { turn });
        assert_eq!(r.prefix, 46_200, "{r:?}");
        assert!(v.prefix > 46_200, "the first call overstated: {}", v.prefix);
        assert_eq!(placed(&r), v.size);
        // Without the table the first call's figure stands.
        let e = residency(&a, &tools, v.prefix, v.size, None, None);
        assert_eq!(e.mode, Mode::Estimated);
        assert!(e.prefix >= 46_200);
    }

    #[test]
    fn the_first_prompt_is_reconciled_against_the_uncached_first_input() {
        // FR-19: 4 000 chars typed (≈ 1 000 tokens estimated) but the first
        // call's uncached input was 200 — the prompt row can claim 200.
        let (a, t) = synth(vec![
            user(0, &"p".repeat(4_000)),
            call(1, "r1", "m", (10_000, 200), 100, 0, text()),
            call(2, "r2", "m", (10_300, 0), 50, 0, text()),
        ]);
        let r = residency(&a, &t, 10_000, 10_300, None, None);
        assert_eq!(r.source(Source::Prompts), 200, "{r:?}");
        assert_eq!(r.reconciled, 800);
        assert_eq!(placed(&r), 10_300);
    }

    #[test]
    fn a_drop_under_the_heuristic_is_still_a_boundary_and_tightens_the_prefix() {
        // Corpus #104: 83 199 → 60 451 was 27.3 %, under the 30 % rule, and
        // 22 748 tokens left the window with nothing declared (FR-18); a
        // drop below the first call's cached part lowers the prefix (FR-20).
        let (a, t) = synth(vec![
            user(0, "go"),
            call(1, "r1", "m", (60_000, 2_000), 100, 0, text()),
            call(2, "r2", "m", (80_000, 0), 100, 0, text()),
            call(3, "r3", "m", (58_400, 0), 100, 0, text()),
            call(4, "r4", "m", (59_600, 0), 100, 0, text()),
        ]);
        let r = residency(&a, &t, 60_000, 59_600, None, None);
        assert_eq!(
            r.since,
            Reference::Boundary {
                kind: RefKind::Shrink,
                call: 2,
                delta: -21_600
            }
        );
        assert_eq!(r.prefix, 58_400);
        assert!(r.prefix_tightened);
        assert_eq!(r.calls_since, 2);
        assert_eq!(placed(&r), 59_600);
        // A ≥ 30 % drop keeps its old name.
        let (a, t) = synth(vec![
            user(0, "go"),
            call(1, "r1", "m", (60_000, 0), 100, 0, text()),
            call(2, "r2", "m", (80_000, 0), 100, 0, text()),
            call(3, "r3", "m", (40_000, 0), 100, 0, text()),
        ]);
        let r = residency(&a, &t, 60_000, 40_000, None, None);
        assert!(matches!(
            r.since,
            Reference::Boundary {
                kind: RefKind::Heuristic,
                ..
            }
        ));
    }

    #[test]
    fn a_shrinking_model_switch_resets_and_a_growing_one_is_only_noted() {
        // Decision 7. Observed: −28 700 on 219 708 at the opus → sonnet switch.
        let (a, t) = synth(vec![
            user(0, "go"),
            call(1, "r1", "m1", (10_000, 0), 100, 0, text()),
            call(2, "r2", "m1", (12_000, 0), 100, 0, text()),
            call(3, "r3", "m2", (11_000, 0), 100, 0, text()),
        ]);
        let r = residency(&a, &t, 10_000, 11_000, None, None);
        assert!(matches!(
            r.since,
            Reference::Boundary {
                kind: RefKind::ModelSwitch,
                call: 2,
                delta: -1_000
            }
        ));
        assert!(r.model_switch_kept.is_none());
        let (a, t) = synth(vec![
            user(0, "go"),
            call(1, "r1", "m1", (10_000, 0), 100, 0, text()),
            call(2, "r2", "m1", (12_000, 0), 100, 0, text()),
            call(3, "r3", "m2", (13_000, 0), 100, 0, text()),
        ]);
        let r = residency(&a, &t, 10_000, 13_000, None, None);
        assert_eq!(r.since, Reference::SessionStart);
        assert_eq!(
            r.model_switch_kept,
            Some(("m1".to_string(), "m2".to_string(), 1_000))
        );
    }

    #[test]
    fn an_estimate_over_its_step_is_reconciled_on_that_step() {
        // Corpus #60: a result whose chars / 4 claims more than the exact
        // room its step had (+42 % on one call). The excess comes off there.
        let (a, t) = synth(vec![
            user(0, "go"),
            call(1, "r1", "m", (10_000, 0), 100, 0, read("t1", "/p/a.rs")),
            result(2, "t1", 40_000), // ≈ 10 000 tokens claimed
            call(3, "r2", "m", (14_100, 0), 50, 0, text()), // room: 4 100 − 100 = 4 000
        ]);
        let r = residency(&a, &t, 10_000, 14_100, None, None);
        assert_eq!(r.source(Source::Files), 4_000, "{r:?}");
        assert_eq!(r.reconciled, 6_000);
        assert_eq!(r.overflow_raw, 6_000, "what it would have been");
        assert_eq!(r.files.len(), 1);
        assert_eq!(r.files[0].path, "/p/a.rs");
        assert_eq!(r.files[0].tokens, 4_000);
        assert_eq!(r.files[0].reads, 1);
        assert_eq!(placed(&r), 14_100);
        assert_eq!(r.other, 0);
    }

    #[test]
    fn the_in_flight_call_and_a_trailing_result_are_not_resident() {
        // FR-12. The last call's output is in no context yet; a result that
        // landed after it neither.
        let (a, t) = synth(vec![
            user(0, "go"),
            call(1, "r1", "m", (10_000, 0), 100, 20, text()),
            call(2, "r2", "m", (10_100, 0), 800, 500, read("t1", "/p/a.rs")),
            result(3, "t1", 8_000),
        ]);
        let r = residency(&a, &t, 10_000, 10_100, None, None);
        assert_eq!(r.thinking, 20);
        assert_eq!(r.source(Source::Files), 0);
        assert!(r.files.is_empty());
        assert_eq!(placed(&r), 10_100);
    }

    #[test]
    fn prose_is_the_exact_remainder_of_output() {
        let cmd = "x".repeat(800); // 200 tokens of tool input
        let bash = serde_json::json!([{"type":"tool_use","id":"t1","name":"Bash","input":{"command":cmd}}]);
        let (a, t) = synth(vec![
            user(0, "go"),
            call(1, "r1", "m", (10_000, 0), 1_000, 300, bash),
            call(2, "r2", "m", (11_000, 0), 10, 0, text()),
        ]);
        let r = residency(&a, &t, 10_000, 11_000, None, None);
        assert_eq!(r.thinking, 300);
        assert_eq!(r.tool_inputs, 200);
        assert_eq!(r.prose, 500);
    }

    #[test]
    fn anatomy_slices_are_the_residency_in_bar_order() {
        let (a, t) = synth(vec![
            user(0, &"p".repeat(400)),
            call(1, "r1", "m", (10_000, 100), 100, 30, read("t1", "/p/a.rs")),
            result(2, "t1", 2_000),
            call(3, "r2", "m", (10_700, 0), 40, 0, text()),
        ]);
        let r = residency(&a, &t, 10_000, 10_700, None, None);
        let an = Anatomy::from(&r);
        assert_eq!(an.prefix, 10_000);
        assert_eq!(an.thinking, 30);
        assert_eq!(an.tool_results, r.source(Source::Files));
        assert_eq!(an.unattributed, r.other + r.source(Source::Prompts));
        assert_eq!(
            an.slices().map(|(l, _)| l),
            ["prefix", "inputs", "results", "thinking", "harness", "prose", "other"]
        );
    }
}
