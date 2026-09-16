//! Dollar figures. Claude Code writes its own accounting (`cost-state`); cctop
//! prefers that and only estimates the calls made since it was written.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::agents::Agent;
use crate::transcript::{CacheTtl, CostState, Line};

use super::usage::{Aggregate, Usage};

/// USD per million tokens for one model.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_write_5m: Option<f64>,
    #[serde(default)]
    pub cache_write_1h: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
}

impl Price {
    pub fn cache_write_5m(&self) -> f64 {
        self.cache_write_5m.unwrap_or(self.input * 1.25)
    }
    pub fn cache_write_1h(&self) -> f64 {
        self.cache_write_1h.unwrap_or(self.input * 2.0)
    }
    pub fn cache_read(&self) -> f64 {
        self.cache_read.unwrap_or(self.input * 0.1)
    }

    /// Cost of `u` in USD.
    pub fn cost(&self, u: &Usage) -> f64 {
        (u.input as f64 * self.input
            + u.cache_write_5m as f64 * self.cache_write_5m()
            + u.cache_write_1h as f64 * self.cache_write_1h()
            + u.cache_read as f64 * self.cache_read()
            + u.output as f64 * self.output)
            / 1e6
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PricingFile {
    #[serde(default)]
    models: BTreeMap<String, Price>,
}

/// Price table, bundled defaults merged with the user's overrides.
#[derive(Debug, Clone, Default)]
pub struct Pricing {
    models: BTreeMap<String, Price>,
}

const BUNDLED: &str = include_str!("../../pricing.toml");

impl Pricing {
    pub fn bundled() -> Pricing {
        let f: PricingFile = toml::from_str(BUNDLED).expect("bundled pricing.toml is valid");
        Pricing { models: f.models }
    }

    /// Bundled prices with `~/.config/cctop/pricing.toml` merged on top.
    pub fn load() -> Pricing {
        let mut p = Pricing::bundled();
        if let Some(home) = std::env::var_os("HOME") {
            let path = Path::new(&home).join(".config/cctop/pricing.toml");
            if let Ok(text) = std::fs::read_to_string(path) {
                p.merge_toml(&text);
            }
        }
        p
    }

    pub fn merge_toml(&mut self, text: &str) {
        if let Ok(f) = toml::from_str::<PricingFile>(text) {
            self.models.extend(f.models);
        }
    }

    /// Longest model-id prefix match, so dated ids resolve to their family.
    pub fn price(&self, model: &str) -> Option<&Price> {
        self.models
            .iter()
            .filter(|(k, _)| model.starts_with(k.as_str()))
            .max_by_key(|(k, _)| k.len())
            .map(|(_, p)| p)
    }

    /// `None` for an unknown model: show tokens, not dollars.
    pub fn estimate(&self, u: &Usage, model: &str) -> Option<f64> {
        self.price(model).map(|p| p.cost(u))
    }
}

/// Where a dollar figure comes from. The mark of a sum is the worst of its
/// parts: `Ledger` prints bare, `Priced` and `Mixed` print `≈`, `Unpriced`
/// prints `—` and the token count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Claude Code's own `cost-state` (main, subagents and the calls no
    /// transcript shows, up to the moment it was written).
    Ledger,
    /// cctop's estimate: usage × `pricing.toml`.
    Priced,
    /// A ledger plus priced calls after it.
    Mixed,
    /// Usage on a model the price table does not know: no dollars.
    Unpriced,
}

impl Source {
    /// The source of a sum of two figures.
    pub fn plus(self, other: Source) -> Source {
        use Source::*;
        match (self, other) {
            (Unpriced, _) | (_, Unpriced) => Unpriced,
            (Ledger, Ledger) => Ledger,
            (Priced, Priced) => Priced,
            _ => Mixed,
        }
    }
}

/// Cost figure with provenance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cost {
    pub usd: f64,
    /// True when any part of the figure is cctop's own estimate rather than
    /// Claude Code's `cost-state`.
    pub approx: bool,
    pub source: Source,
}

impl Cost {
    pub fn ledger(usd: f64) -> Cost {
        Cost {
            usd,
            approx: false,
            source: Source::Ledger,
        }
    }

    pub fn priced(usd: f64) -> Cost {
        Cost {
            usd,
            approx: true,
            source: Source::Priced,
        }
    }

    /// `self + other`, the source the worst of the two.
    pub fn plus(self, other: Cost) -> Cost {
        Cost {
            usd: self.usd + other.usd,
            approx: self.approx || other.approx,
            source: self.source.plus(other.source),
        }
    }
}

/// Running cost over a transcript: authoritative up to the last `cost-state`,
/// estimated after it.
#[derive(Debug, Default)]
pub struct CostTracker {
    pricing: Pricing,
    /// Latest `cost-state` seen.
    pub authoritative: Option<CostState>,
    /// The ledger's moment: the last line timestamp seen before the latest
    /// `cost-state` (the line itself carries none; it is written at session
    /// end or on a bridge, 1–5 lines after the last timestamped one).
    pub authoritative_at_ms: Option<i64>,
    /// Timestamp of the last timestamped line pushed.
    last_line_at_ms: Option<i64>,
    /// Usage (per model) of responses seen *after* the latest cost-state.
    since: BTreeMap<String, Usage>,
    /// Ids already attributed since the last cost-state (dedupe).
    seen: std::collections::HashSet<String>,
    /// True if any model since the last cost-state has no price.
    unknown_model: bool,
}

impl CostTracker {
    pub fn new(pricing: Pricing) -> CostTracker {
        CostTracker {
            pricing,
            ..Default::default()
        }
    }

    pub fn pricing(&self) -> &Pricing {
        &self.pricing
    }

    pub fn push(&mut self, line: &Line) {
        let at = match line {
            Line::User(u) => u.timestamp.as_deref(),
            Line::Assistant(a) => a.timestamp.as_deref(),
            Line::System(s) => s.timestamp.as_deref(),
            _ => None,
        }
        .and_then(parse_ts_ms);
        if let Some(at) = at {
            self.last_line_at_ms = Some(self.last_line_at_ms.map_or(at, |m| m.max(at)));
        }
        match line {
            Line::CostState(c) => {
                self.authoritative = Some(c.clone());
                self.authoritative_at_ms = self.last_line_at_ms;
                self.since.clear();
                self.seen.clear();
                self.unknown_model = false;
            }
            Line::Assistant(a) => {
                if !self.seen.insert(a.message.id.clone()) {
                    return;
                }
                let u = Usage::from_api(&a.message.usage);
                self.since
                    .entry(a.message.model.clone())
                    .or_default()
                    .add(&u);
                if self.pricing.price(&a.message.model).is_none() {
                    self.unknown_model = true;
                }
            }
            _ => {}
        }
    }

    /// Estimated cost of everything since the last cost-state.
    fn since_usd(&self) -> f64 {
        self.since
            .iter()
            .filter_map(|(m, u)| self.pricing.estimate(u, m))
            .sum()
    }

    /// Best current figure for the main transcript: the ledger plus what
    /// came after it. `None` when there is no cost-state and no priceable
    /// model at all.
    pub fn current(&self) -> Option<Cost> {
        let since = self.since_usd();
        let any_since = self.since.values().any(|u| u.total() > 0);
        match &self.authoritative {
            Some(c) if any_since => Some(Cost::ledger(c.total_cost_usd).plus(Cost::priced(since))),
            Some(c) => Some(Cost::ledger(c.total_cost_usd)),
            None if any_since && !self.unknown_model => Some(Cost::priced(since)),
            None if any_since => Some(Cost::priced(since)).filter(|c| c.usd > 0.0),
            None => None,
        }
    }

    /// The session's whole spend: `current()` plus the priced calls of its
    /// subagents after the ledger's moment. The ledger already holds the
    /// agents' calls up to that moment (`harness_facts::cost_state`), so a
    /// call counts only when its own line timestamp is later; with no
    /// ledger every agent call counts. With no agents this is `current()`
    /// exactly. A call on a model the table does not know adds nothing.
    pub fn combined<'a>(&self, agents: impl IntoIterator<Item = &'a Agent>) -> Option<Cost> {
        let mut agents_usd = 0.0;
        let mut any_agent = false;
        for a in agents {
            for c in &a.calls {
                let after = match (self.authoritative_at_ms, c.at_ms) {
                    (None, _) => true,
                    (Some(moment), Some(at)) => at > moment,
                    (Some(_), None) => false,
                };
                if !after {
                    continue;
                }
                if let Some(usd) = self.pricing.estimate(&c.usage, &c.model) {
                    agents_usd += usd;
                    any_agent = true;
                }
            }
        }
        match self.current() {
            Some(c) if any_agent => Some(c.plus(Cost::priced(agents_usd))),
            Some(c) => Some(c),
            None if any_agent => Some(Cost::priced(agents_usd)),
            None => None,
        }
    }

    /// Per-model breakdown: authoritative `modelUsage` cost plus estimates.
    pub fn by_model(&self) -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        if let Some(c) = &self.authoritative {
            for (m, mu) in &c.model_usage {
                *out.entry(m.clone()).or_insert(0.0) += mu.cost_usd;
            }
        }
        for (m, u) in &self.since {
            if let Some(e) = self.pricing.estimate(u, m) {
                *out.entry(m.clone()).or_insert(0.0) += e;
            }
        }
        out
    }
}

/// Trailing-window rates from turn timestamps.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rates {
    /// USD per hour over the trailing window (`None` without prices).
    pub usd_per_hour: Option<f64>,
    /// Input tokens per minute over the trailing window.
    pub input_tokens_per_min: f64,
    /// Window actually covered, minutes (≤ 15).
    pub window_min: f64,
}

/// Burn rate over the trailing 15 minutes ending at `now`: turns whose last
/// activity falls in the window. Cost = window cost × (60 / window minutes).
pub fn rates(agg: &Aggregate, pricing: &Pricing, now_ms: i64) -> Rates {
    const WINDOW_MS: i64 = 15 * 60 * 1000;
    let mut usd = 0.0;
    let mut priced = false;
    let mut tokens: u64 = 0;
    let mut earliest = now_ms;
    for t in &agg.turns {
        // A turn counts if its last activity falls in the window.
        let Some(at) = t
            .last_at
            .as_deref()
            .or(t.started_at.as_deref())
            .and_then(parse_ts_ms)
        else {
            continue;
        };
        if at < now_ms - WINDOW_MS || at > now_ms {
            continue;
        }
        let started = t
            .started_at
            .as_deref()
            .and_then(parse_ts_ms)
            .unwrap_or(at)
            .max(now_ms - WINDOW_MS);
        earliest = earliest.min(started);
        tokens += t.usage.total_input();
        for m in &t.models {
            if let Some(c) = pricing.estimate(&t.usage, m) {
                usd += c / t.models.len() as f64;
                priced = true;
            }
        }
    }
    let window_min = ((now_ms - earliest).max(60_000) as f64 / 60_000.0).min(15.0);
    Rates {
        usd_per_hour: priced.then_some(usd / window_min * 60.0),
        input_tokens_per_min: tokens as f64 / window_min,
        window_min,
    }
}

/// `2026-08-27T09:25:06.911Z` → epoch milliseconds. Only the ISO-8601 UTC
/// form Claude Code writes is accepted.
pub fn parse_ts_ms(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, mo, da) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut t = hms.split(':').map(|x| x.parse::<i64>());
    let (h, mi, se) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    let ms: i64 = format!("{:0<3}", frac).get(..3)?.parse().ok()?;
    // Days from civil (Howard Hinnant's algorithm).
    let (y, mo) = if mo <= 2 {
        (y - 1, mo + 9)
    } else {
        (y, mo - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * mo + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((((days * 24 + h) * 60 + mi) * 60 + se) * 1000 + ms)
}

/// What continuing costs at the current context: the price of one call and
/// of one turn, now and at 100 k.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    /// One call at the current context, warm: `ctx × cache read + median
    /// output × output price`; cold: the context at the cache-write price.
    pub per_call: f64,
    /// `per_call × p50 calls per turn`.
    pub per_turn: f64,
    /// The same turn at 100 k of context.
    pub per_turn_at_100k: f64,
    pub next_30_calls: f64,
    /// The session's own p50 calls per turn (turns with ≥ 1 call), floor 1.
    pub calls_per_turn: f64,
    /// Priced at the cache-write price: the cache is cold (or was flipped).
    pub cold: bool,
}

/// The cost gradient for `model` at `ctx` tokens of context.
pub fn gradient(
    pricing: &Pricing,
    model: &str,
    ctx: u64,
    median_output: u64,
    calls_per_turn: f64,
    cold: bool,
    ttl: CacheTtl,
) -> Option<Gradient> {
    let p = pricing.price(model)?;
    let read = if cold {
        cache_write_price(p, ttl)
    } else {
        p.cache_read()
    };
    let per_call = |c: u64| (c as f64 * read + median_output as f64 * p.output) / 1e6;
    let cpt = calls_per_turn.max(1.0);
    Some(Gradient {
        per_call: per_call(ctx),
        per_turn: per_call(ctx) * cpt,
        per_turn_at_100k: per_call(100_000) * cpt,
        next_30_calls: per_call(ctx) * 30.0,
        calls_per_turn: cpt,
        cold,
    })
}

/// Median of the API calls per turn over turns that made a call.
pub fn p50_calls_per_turn(agg: &Aggregate) -> f64 {
    let mut v: Vec<usize> = agg
        .turns
        .iter()
        .filter(|t| t.api_calls > 0)
        .map(|t| t.api_calls)
        .collect();
    if v.is_empty() {
        return 1.0;
    }
    v.sort_unstable();
    v[v.len() / 2] as f64
}

/// Median output tokens per API call.
pub fn median_output(agg: &Aggregate) -> u64 {
    let mut v: Vec<u64> = agg.calls.iter().map(|c| c.usage.output).collect();
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[v.len() / 2]
}

/// `/usage`'s limit weight of one call: `(cached + uncached×10 +
/// cacheCreate×12.5 + output×50) × tier`.
pub fn limit_weight(u: &Usage, model: &str) -> f64 {
    use crate::harness_facts::usage_weight as w;
    (u.cache_read as f64
        + u.input as f64 * w::UNCACHED
        + u.cache_write() as f64 * w::CACHE_CREATE
        + u.output as f64 * w::OUTPUT)
        * w::tier(model)
}

/// `/usage`'s five behaviour flags over this session's calls, as shares
/// of the weighted usage; Claude Code shows a flag at ≥ 10 %.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BehaviourFlags {
    /// Share of weight in requests with > 100 k uncached tokens.
    pub cache_miss_pct: f64,
    pub cache_miss_count: usize,
    /// Share of weight in requests at > 150 k context.
    pub long_context_pct: f64,
    pub long_context_count: usize,
    /// Share of weight spent by subagents.
    pub subagent_pct: f64,
    /// ≥ 4 sessions were live at once.
    pub high_parallel: bool,
    /// The session has been active for ≥ 8 h.
    pub cron: bool,
    pub total_weight: f64,
}

impl BehaviourFlags {
    pub fn compute(
        agg: &Aggregate,
        agent_weight: f64,
        live_sessions: usize,
        active_ms: i64,
    ) -> BehaviourFlags {
        let mut f = BehaviourFlags::default();
        let mut miss = 0.0;
        let mut long = 0.0;
        for c in &agg.calls {
            let w = limit_weight(&c.usage, &c.model);
            f.total_weight += w;
            if c.usage.input > 100_000 {
                miss += w;
                f.cache_miss_count += 1;
            }
            if c.context() > 150_000 {
                long += w;
                f.long_context_count += 1;
            }
        }
        let total = f.total_weight + agent_weight;
        if total > 0.0 {
            f.cache_miss_pct = miss / total * 100.0;
            f.long_context_pct = long / total * 100.0;
            f.subagent_pct = agent_weight / total * 100.0;
        }
        f.high_parallel = live_sessions >= 4;
        f.cron = active_ms >= 8 * 3_600_000;
        f
    }

    /// Claude Code's own tip lines, for the flags at ≥ 10 %.
    pub fn tips(&self) -> Vec<String> {
        let mut out = Vec::new();
        let pct = |p: f64| p.round() as u64;
        if self.cache_miss_pct >= 10.0 {
            out.push(format!(
                "{}% of your usage hit a >100k-token cache miss",
                pct(self.cache_miss_pct)
            ));
        }
        if self.long_context_pct >= 10.0 {
            out.push(format!(
                "{}% of your usage was at >150k context",
                pct(self.long_context_pct)
            ));
        }
        if self.subagent_pct >= 10.0 {
            out.push(format!(
                "{}% of your usage came from subagent-heavy sessions",
                pct(self.subagent_pct)
            ));
        }
        if self.high_parallel {
            out.push("usage while 4+ sessions ran in parallel".to_string());
        }
        if self.cron {
            out.push("usage from a session active for 8+ hours".to_string());
        }
        out
    }
}

/// Epoch milliseconds → the ISO-8601 UTC form Claude Code writes
/// (`2026-08-27T09:25:06.911Z`); the inverse of [`parse_ts_ms`].
pub fn format_ts_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// Price to use for a cache write of a given TTL (helper for callers that
/// price a single call).
pub fn cache_write_price(p: &Price, ttl: CacheTtl) -> f64 {
    match ttl {
        CacheTtl::FiveMinutes => p.cache_write_5m(),
        CacheTtl::OneHour => p.cache_write_1h(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::parse_file;

    fn fixture() -> Vec<Line> {
        parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl")).unwrap()
    }

    #[test]
    fn estimate_matches_cost_state_within_five_percent() {
        let lines = fixture();
        let pricing = Pricing::bundled();
        let cs = lines
            .iter()
            .find_map(|l| match l {
                Line::CostState(c) => Some(c),
                _ => None,
            })
            .unwrap();
        // Recompute from Claude Code's own per-model token counts: the price
        // table must reproduce its dollar figure.
        let mut total = 0.0;
        for (model, mu) in &cs.model_usage {
            let u = Usage {
                input: mu.input_tokens,
                cache_write_1h: mu.cache_creation_input_tokens,
                cache_read: mu.cache_read_input_tokens,
                output: mu.output_tokens,
                ..Default::default()
            };
            total += pricing.estimate(&u, model).unwrap();
        }
        let rel = (total - cs.total_cost_usd).abs() / cs.total_cost_usd;
        assert!(
            rel < 0.01,
            "estimate {total} vs {} ({rel:.3})",
            cs.total_cost_usd
        );
        // And from the deduplicated transcript usage itself (the cost-state was
        // written before the last few responses, so allow 5 %).
        let agg = Aggregate::from_lines(&lines);
        let from_lines = pricing.estimate(&agg.total, "claude-sonnet-5").unwrap();
        let rel = (from_lines - cs.total_cost_usd).abs() / cs.total_cost_usd;
        assert!(
            rel < 0.05,
            "from lines {from_lines} vs {} ({rel:.3})",
            cs.total_cost_usd
        );
    }

    #[test]
    fn ttl_selects_write_price() {
        let p = Pricing::bundled();
        let sonnet = p.price("claude-sonnet-5").unwrap();
        assert_eq!(sonnet.cache_write_5m(), 2.5);
        assert_eq!(sonnet.cache_write_1h(), 4.0);
        assert_eq!(sonnet.cache_read(), 0.2);
        assert_eq!(cache_write_price(sonnet, CacheTtl::FiveMinutes), 2.5);
        assert_eq!(cache_write_price(sonnet, CacheTtl::OneHour), 4.0);
        let five = Usage {
            cache_write_5m: 1_000_000,
            ..Default::default()
        };
        let hour = Usage {
            cache_write_1h: 1_000_000,
            ..Default::default()
        };
        assert_eq!(p.estimate(&five, "claude-sonnet-5"), Some(2.5));
        assert_eq!(p.estimate(&hour, "claude-sonnet-5"), Some(4.0));
        // Explicit override in the table wins over the multiplier.
        assert_eq!(p.price("claude-fable-5-1").unwrap().cache_read(), 0.25);
    }

    #[test]
    fn unknown_model_is_none_and_prefix_match_works() {
        let p = Pricing::bundled();
        assert!(p.price("claude-mystery-9").is_none());
        assert_eq!(p.estimate(&Usage::default(), "claude-mystery-9"), None);
        assert_eq!(p.price("claude-haiku-4-5-20251001").unwrap().input, 1.0);
        assert_eq!(p.price("claude-opus-5").unwrap().output, 25.0);
    }

    #[test]
    fn user_override_merges_by_model() {
        let mut p = Pricing::bundled();
        p.merge_toml("[models.\"claude-sonnet-5\"]\ninput = 1.0\noutput = 2.0\n[models.\"my-proxy\"]\ninput = 0.5\noutput = 0.5\n");
        assert_eq!(p.price("claude-sonnet-5").unwrap().input, 1.0);
        assert_eq!(p.price("my-proxy-v2").unwrap().output, 0.5);
        assert_eq!(p.price("claude-opus-5").unwrap().input, 5.0);
    }

    #[test]
    fn tracker_prefers_cost_state_and_flags_estimates() {
        let lines = fixture();
        let mut t = CostTracker::new(Pricing::bundled());
        let idx = lines
            .iter()
            .position(|l| matches!(l, Line::CostState(_)))
            .unwrap();
        for l in &lines[..=idx] {
            t.push(l);
        }
        let at_cs = t.current().unwrap();
        assert!(!at_cs.approx);
        assert!((at_cs.usd - 9.9035).abs() < 1e-3);
        for l in &lines[idx + 1..] {
            t.push(l);
        }
        let end = t.current().unwrap();
        assert!(end.approx == (end.usd > at_cs.usd));
        assert!(end.usd >= at_cs.usd);
        let by = t.by_model();
        assert!(by.contains_key("claude-sonnet-5"));
        assert!(by.contains_key("claude-haiku-4-5-20251001"));
        // Before any cost-state: pure estimate, approx.
        let mut fresh = CostTracker::new(Pricing::bundled());
        for l in &lines[..idx] {
            fresh.push(l);
        }
        let c = fresh.current().unwrap();
        assert!(c.approx && c.usd > 1.0);
        assert_eq!(c.source, Source::Priced);

        // Fixture A's fork was taken from another session and the anonymiser
        // shifts each file on its own: its 7 own calls post-date the ledger's
        // moment, so the combined figure prices all of them on top of the
        // ledger and is `≈` (correct for what the files say).
        let agents =
            crate::agents::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a"));
        let fork = &agents["a9a92645226d3a561"];
        let moment = t.authoritative_at_ms.unwrap();
        assert!(fork.calls.iter().all(|c| c.at_ms.unwrap() > moment));
        let all = t.combined(agents.values()).unwrap();
        let fork_usd = Pricing::bundled()
            .estimate(&fork.usage, &fork.model)
            .unwrap();
        assert!((all.usd - (end.usd + fork_usd)).abs() < 1e-9);
        assert_eq!(all.source, Source::Mixed);
        assert!(all.approx);
    }

    /// An agent with one priced call per `(timestamp, output tokens)`.
    fn agent(model: &str, calls: &[(&str, u64)]) -> Agent {
        let mut a = Agent::new("x", crate::agents::Meta::default());
        for (i, (ts, out)) in calls.iter().enumerate() {
            let l = Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"m{i}","model":"{model}","content":[{{"type":"text","text":"x"}}],"usage":{{"output_tokens":{out}}}}}}}"#
            ))
            .unwrap();
            a.push(&l);
        }
        a
    }

    fn output_price(model: &str) -> f64 {
        Pricing::bundled().price(model).unwrap().output / 1e6
    }

    #[test]
    fn combined_adds_agent_calls_after_the_ledgers_moment() {
        let ledger =
            Line::parse(r#"{"type":"cost-state","totalCostUSD":10.0,"modelUsage":{}}"#).unwrap();
        let main_line = |ts: &str, id: &str| -> Line {
            Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5","content":[],"usage":{{"output_tokens":1000}}}}}}"#
            ))
            .unwrap()
        };
        let out = output_price("claude-opus-5");
        let eps = 1e-9;

        // No ledger: every part is priced, main and agents alike.
        let mut t = CostTracker::new(Pricing::bundled());
        t.push(&main_line("2026-01-01T00:00:00Z", "a"));
        let agents = [agent("claude-opus-5", &[("2026-01-01T00:00:05Z", 1000)])];
        let c = t.combined(&agents).unwrap();
        assert_eq!(c.source, Source::Priced);
        assert!(c.approx);
        assert!((c.usd - 2000.0 * out).abs() < eps);
        assert_eq!(t.authoritative_at_ms, None);

        // A ledger: its moment is the last timestamped line before it.
        t.push(&main_line("2026-01-01T00:01:00Z", "b"));
        t.push(&ledger);
        assert_eq!(
            t.authoritative_at_ms,
            parse_ts_ms("2026-01-01T00:01:00Z"),
            "the cost-state line has no timestamp of its own"
        );
        assert_eq!(t.current().unwrap(), Cost::ledger(10.0));
        // Agent calls before the moment are inside the ledger; after it they
        // are added; a call straddling it (same second) counts as before.
        let agents = [
            agent("claude-opus-5", &[("2026-01-01T00:00:30Z", 1000)]),
            agent(
                "claude-opus-5",
                &[
                    ("2026-01-01T00:01:00Z", 1000),
                    ("2026-01-01T00:02:00Z", 1000),
                ],
            ),
        ];
        let c = t.combined(&agents).unwrap();
        assert_eq!(c.source, Source::Mixed);
        assert!(c.approx);
        assert!((c.usd - (10.0 + 1000.0 * out)).abs() < eps, "{}", c.usd);
        // Only calls before it: the ledger stands alone, exact.
        let c = t.combined(&agents[..1]).unwrap();
        assert_eq!(c, Cost::ledger(10.0));
        // Main calls after the ledger are priced too (`current()`).
        t.push(&main_line("2026-01-01T00:03:00Z", "c"));
        let c = t.combined(&agents).unwrap();
        assert!((c.usd - (10.0 + 2000.0 * out)).abs() < eps);
        assert_eq!(c.source, Source::Mixed);

        // An agent on a model the table does not know adds nothing and
        // does not change the mark.
        let unknown = [agent("claude-unknown-9", &[("2026-01-01T00:04:00Z", 1000)])];
        let with = t.combined(&unknown).unwrap();
        assert_eq!(with, t.current().unwrap());
        let mut fresh = CostTracker::new(Pricing::bundled());
        assert_eq!(fresh.combined(&unknown), None, "nothing priceable at all");
        fresh.push(&main_line("2026-01-01T00:00:00Z", "a"));
        assert_eq!(fresh.combined(&unknown), fresh.current());

        // No agents: `current()` exactly (FR-6).
        assert_eq!(t.combined(&[]), t.current());
        let mut none = CostTracker::new(Pricing::bundled());
        assert_eq!(none.combined(&[]), None);
        none.push(&ledger);
        assert_eq!(none.combined(&[]), Some(Cost::ledger(10.0)));
        assert_eq!(
            none.authoritative_at_ms, None,
            "a ledger with no timestamped line before it has no moment"
        );
    }

    #[test]
    fn source_of_a_sum_is_the_worst_part() {
        use Source::*;
        assert_eq!(Ledger.plus(Ledger), Ledger);
        assert_eq!(Ledger.plus(Priced), Mixed);
        assert_eq!(Priced.plus(Priced), Priced);
        assert_eq!(Mixed.plus(Priced), Mixed);
        assert_eq!(Mixed.plus(Ledger), Mixed);
        assert_eq!(Ledger.plus(Unpriced), Unpriced);
        let c = Cost::ledger(1.0).plus(Cost::priced(0.5));
        assert_eq!(
            c,
            Cost {
                usd: 1.5,
                approx: true,
                source: Mixed
            }
        );
    }

    #[test]
    fn gradient_weight_and_flags() {
        let p = Pricing::bundled();
        // Opus 5: cache read $0.50/M, output $25/M (pricing.toml).
        let g = gradient(
            &p,
            "claude-opus-5",
            412_000,
            1_000,
            19.0,
            false,
            CacheTtl::OneHour,
        )
        .unwrap();
        let read = p.price("claude-opus-5").unwrap().cache_read();
        assert!((g.per_call - (412_000.0 * read + 1_000.0 * 25.0) / 1e6).abs() < 1e-9);
        assert!((g.per_turn - g.per_call * 19.0).abs() < 1e-9);
        assert!(g.per_turn_at_100k < g.per_turn);
        assert!((g.next_30_calls - g.per_call * 30.0).abs() < 1e-9);
        let cold = gradient(
            &p,
            "claude-opus-5",
            412_000,
            1_000,
            19.0,
            true,
            CacheTtl::OneHour,
        )
        .unwrap();
        assert!(
            cold.cold && cold.per_call > g.per_call * 5.0,
            "cold write ≫ warm read"
        );
        assert!(gradient(&p, "mystery", 1, 1, 1.0, false, CacheTtl::OneHour).is_none());
        let u = Usage {
            input: 100,
            cache_write_1h: 200,
            cache_read: 1000,
            output: 10,
            ..Default::default()
        };
        assert_eq!(
            limit_weight(&u, "claude-opus-5"),
            (1000.0 + 1000.0 + 2500.0 + 500.0) * 5.0
        );
        assert_eq!(limit_weight(&u, "claude-sonnet-5"), 5000.0 * 3.0);
        let agg = Aggregate::from_lines(&fixture());
        assert_eq!(agg.calls.len(), 143);
        assert!(p50_calls_per_turn(&agg) >= 1.0);
        assert!(median_output(&agg) > 0);
        let f = BehaviourFlags::compute(&agg, 0.0, 1, 3_600_000);
        assert!(
            f.long_context_pct > 50.0,
            "{f:?}: most of fixture A ran above 150k"
        );
        assert_eq!(f.cache_miss_count, 0);
        assert!(!f.high_parallel && !f.cron);
        assert!(f
            .tips()
            .iter()
            .any(|t| t.ends_with("of your usage was at >150k context")));
        let f = BehaviourFlags::compute(&agg, f.total_weight, 5, 9 * 3_600_000);
        assert!((f.subagent_pct - 50.0).abs() < 1e-6);
        assert!(f.high_parallel && f.cron);
        assert_eq!(f.tips().len(), 4);
    }

    #[test]
    fn timestamp_parsing_and_rates() {
        assert_eq!(parse_ts_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_ts_ms("1970-01-02T00:00:01Z"), Some(86_401_000));
        assert_eq!(
            parse_ts_ms("2026-08-27T09:25:06.911Z"),
            Some(1_787_822_706_911)
        );
        assert_eq!(parse_ts_ms("garbage"), None);
        for ts in [
            "2026-08-27T09:25:06.911Z",
            "1970-01-01T00:00:00.000Z",
            "2024-02-29T23:59:59.999Z",
            "2026-03-01T00:00:00.000Z",
        ] {
            assert_eq!(format_ts_ms(parse_ts_ms(ts).unwrap()), ts);
        }

        let lines = fixture();
        let agg = Aggregate::from_lines(&lines);
        let last = agg
            .turns
            .iter()
            .filter(|t| t.usage.total_input() > 0)
            .filter_map(|t| t.last_at.as_deref().and_then(parse_ts_ms))
            .max()
            .unwrap();
        let r = rates(&agg, &Pricing::bundled(), last + 1000);
        assert!(r.input_tokens_per_min > 0.0);
        assert!(r.usd_per_hour.unwrap() > 0.0);
        assert!(r.window_min > 0.0 && r.window_min <= 15.0);
        // Far in the future: nothing in the window.
        let r = rates(&agg, &Pricing::bundled(), last + 3_600_000);
        assert_eq!(r.input_tokens_per_min, 0.0);
        assert_eq!(r.usd_per_hour, None);
    }
}
