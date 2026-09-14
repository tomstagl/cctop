//! Your own recent sessions as the yardstick: medians over the last N days
//! of cost per turn, tokens per turn, cache-hit ratio, tool error rate and
//! model mix. A bare number is trivia; "2.3× your median" is a decision.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::metrics::Aggregate;
use crate::tools;
use crate::transcript::Line;

pub const DEFAULT_DAYS: u64 = 7;
/// Sessions shorter than this are noise.
const MIN_TURNS: usize = 3;
const REFRESH_MS: i64 = 3_600_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Baseline {
    pub computed_at_ms: i64,
    pub days: u64,
    /// Sessions that contributed.
    pub sessions: usize,
    pub cost_per_turn: Option<f64>,
    pub tokens_per_turn: Option<f64>,
    pub cache_hit_ratio: Option<f64>,
    pub tool_error_rate: Option<f64>,
    /// Share of cost by model family (`opus`, `sonnet`, `haiku`, …).
    pub model_mix: BTreeMap<String, f64>,
    /// Median API calls per session: the coach's "expected remaining
    /// calls" scale.
    #[serde(default)]
    pub calls_per_session: Option<f64>,
}

/// One session's figures, before taking medians.
#[derive(Debug, Clone, Default)]
struct SessionFigures {
    turns: usize,
    api_calls: usize,
    cost: Option<f64>,
    tokens: u64,
    cache_hit: Option<f64>,
    error_rate: Option<f64>,
    cost_by_family: BTreeMap<String, f64>,
}

fn family(model: &str) -> String {
    for f in ["opus", "sonnet", "haiku", "fable", "mythos"] {
        if model.contains(f) {
            return f.to_string();
        }
    }
    "other".into()
}

fn figures(path: &Path) -> Option<SessionFigures> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut agg = Aggregate::default();
    let mut stats = tools::Stats::default();
    let mut cost_state = None;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(l) = Line::parse(line) else { continue };
        agg.push(&l);
        stats.push(&l);
        if let Line::CostState(c) = &l {
            cost_state = Some(c.clone());
        }
    }
    if agg.human_turns() < MIN_TURNS {
        return None;
    }
    let calls = stats.calls.len();
    let errors = stats.calls.iter().filter(|c| c.is_error).count();
    let mut by_family = BTreeMap::new();
    if let Some(c) = &cost_state {
        for (m, mu) in &c.model_usage {
            *by_family.entry(family(m)).or_insert(0.0) += mu.cost_usd;
        }
    }
    Some(SessionFigures {
        turns: agg.human_turns(),
        api_calls: agg.api_calls(),
        cost: cost_state.as_ref().map(|c| c.total_cost_usd),
        tokens: agg.total.total(),
        cache_hit: agg.total.cache_hit_ratio(),
        error_rate: (calls > 0).then(|| errors as f64 / calls as f64),
        cost_by_family: by_family,
    })
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

/// Transcripts under `projects_dir` modified in the last `days`.
pub fn recent_transcripts(projects_dir: &Path, days: u64, now_ms: i64) -> Vec<PathBuf> {
    let cutoff = now_ms - days as i64 * 86_400_000;
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(projects_dir) else {
        return out;
    };
    for p in projects.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Ok(files) = std::fs::read_dir(&p) else {
            continue;
        };
        for f in files.flatten().map(|e| e.path()) {
            if f.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let modified = f
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if modified >= cutoff {
                out.push(f);
            }
        }
    }
    out
}

/// Compute medians over the recent transcripts.
pub fn compute(projects_dir: &Path, days: u64, now_ms: i64) -> Baseline {
    let figs: Vec<SessionFigures> = recent_transcripts(projects_dir, days, now_ms)
        .iter()
        .filter_map(|p| figures(p))
        .collect();
    let mut mix: BTreeMap<String, f64> = BTreeMap::new();
    for f in &figs {
        for (fam, c) in &f.cost_by_family {
            *mix.entry(fam.clone()).or_insert(0.0) += c;
        }
    }
    let total: f64 = mix.values().sum();
    if total > 0.0 {
        for v in mix.values_mut() {
            *v /= total;
        }
    }
    Baseline {
        computed_at_ms: now_ms,
        days,
        sessions: figs.len(),
        cost_per_turn: median(
            figs.iter()
                .filter_map(|f| f.cost.map(|c| c / f.turns as f64))
                .collect(),
        ),
        tokens_per_turn: median(
            figs.iter()
                .map(|f| f.tokens as f64 / f.turns as f64)
                .collect(),
        ),
        cache_hit_ratio: median(figs.iter().filter_map(|f| f.cache_hit).collect()),
        tool_error_rate: median(figs.iter().filter_map(|f| f.error_rate).collect()),
        model_mix: mix,
        calls_per_session: median(figs.iter().map(|f| f.api_calls as f64).collect()),
    }
}

/// `<home>/baseline.json`, recomputed when older than an hour.
pub fn load_or_compute(home: &Path, projects_dir: &Path, now_ms: i64) -> Baseline {
    let cache = home.join("baseline.json");
    if let Ok(text) = std::fs::read_to_string(&cache) {
        if let Ok(b) = serde_json::from_str::<Baseline>(&text) {
            if now_ms - b.computed_at_ms < REFRESH_MS {
                return b;
            }
        }
    }
    let b = compute(projects_dir, DEFAULT_DAYS, now_ms);
    let _ = std::fs::create_dir_all(home);
    let _ = std::fs::write(&cache, serde_json::to_string_pretty(&b).unwrap_or_default());
    b
}

/// `~/.claude/projects`.
pub fn default_projects_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/projects"))
}

impl Baseline {
    /// `this / median`, when both exist.
    pub fn multiplier(this: Option<f64>, median: Option<f64>) -> Option<f64> {
        match (this, median) {
            (Some(a), Some(m)) if m > 0.0 => Some(a / m),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_from_fixture_and_caches() {
        let now = crate::app::now_ms();
        let root = std::env::temp_dir().join(format!("cctop-baseline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let projects = root.join("projects");
        std::fs::create_dir_all(projects.join("-p1")).unwrap();
        std::fs::create_dir_all(projects.join("-p2")).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        std::fs::copy(&src, projects.join("-p1/a.jsonl")).unwrap();
        std::fs::copy(&src, projects.join("-p2/b.jsonl")).unwrap();
        std::fs::write(
            projects.join("-p2/short.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        )
        .unwrap();
        let b = compute(&projects, 7, now);
        assert_eq!(b.sessions, 2, "the 1-turn session is ignored");
        assert!((b.cost_per_turn.unwrap() - 9.9035 / 14.0).abs() < 1e-3);
        assert!(b.tokens_per_turn.unwrap() > 1_000_000.0);
        assert!(b.cache_hit_ratio.unwrap() > 0.9);
        assert!(b.tool_error_rate.unwrap() > 0.0 && b.tool_error_rate.unwrap() < 0.1);
        assert!(b.model_mix["sonnet"] > 0.99);
        assert_eq!(Baseline::multiplier(Some(2.0), Some(4.0)), Some(0.5));
        assert_eq!(Baseline::multiplier(Some(2.0), None), None);
        // Cache round-trip and hourly refresh.
        let home = root.join("home");
        let c1 = load_or_compute(&home, &projects, now);
        assert_eq!(c1.sessions, 2);
        std::fs::remove_dir_all(&projects).unwrap();
        let c2 = load_or_compute(&home, &projects, now + 1000);
        assert_eq!(c2.sessions, 2, "served from cache within the hour");
        let c3 = load_or_compute(&home, &projects, now + REFRESH_MS + 1);
        assert_eq!(c3.sessions, 0, "recomputed after an hour");
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![1.0, 2.0]), Some(1.5));
        assert_eq!(median(vec![]), None);
    }
}
