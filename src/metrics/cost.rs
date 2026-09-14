//! Dollar figures. Claude Code writes its own accounting (`cost-state`); cctop
//! prefers that and only estimates the calls made since it was written.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

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

/// Cost figure with provenance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cost {
    pub usd: f64,
    /// True when any part of the figure is cctop's own estimate rather than
    /// Claude Code's `cost-state`.
    pub approx: bool,
}

/// Running cost over a transcript: authoritative up to the last `cost-state`,
/// estimated after it.
#[derive(Debug, Default)]
pub struct CostTracker {
    pricing: Pricing,
    /// Latest `cost-state` seen.
    pub authoritative: Option<CostState>,
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
        match line {
            Line::CostState(c) => {
                self.authoritative = Some(c.clone());
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

    /// Best current figure. `None` when there is no cost-state and no
    /// priceable model at all.
    pub fn current(&self) -> Option<Cost> {
        let since = self.since_usd();
        let any_since = self.since.values().any(|u| u.total() > 0);
        match &self.authoritative {
            Some(c) => Some(Cost {
                usd: c.total_cost_usd + since,
                approx: any_since,
            }),
            None if any_since && !self.unknown_model => Some(Cost {
                usd: since,
                approx: true,
            }),
            None if any_since => Some(Cost {
                usd: since,
                approx: true,
            })
            .filter(|c| c.usd > 0.0),
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
