//! Your own recent sessions as the yardstick: medians over the last N days
//! of cost per turn, tokens per turn, calls per turn, cache-hit ratio, tool
//! error rate and its categories, the interruption rate, the share of
//! commits that landed unchecked, and the model mix; plus the hours of the
//! day you work, from Claude Code's own session analysis. A bare number is
//! trivia; "2.3× your median" is a decision.

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
    /// Median API calls per human turn.
    #[serde(default)]
    pub calls_per_turn: Option<f64>,
    /// Median share of human turns the person interrupted.
    #[serde(default)]
    pub interruption_rate: Option<f64>,
    /// Share of failed tool calls by error class (`Denied`, `Timeout`, …),
    /// over every session.
    #[serde(default)]
    pub error_categories: BTreeMap<String, f64>,
    /// Commits with unchecked source edits over every commit.
    #[serde(default)]
    pub commit_without_check_ratio: Option<f64>,
    /// Messages per hour of day (24 entries) from `usage-data`, the
    /// person's active hours; empty when `/insights` never ran.
    #[serde(default)]
    pub active_hours: Vec<u64>,
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
    interrupts: usize,
    errors_by_class: BTreeMap<String, usize>,
    /// `(unchecked, total)` commits.
    commits: (usize, usize),
}

/// Commits that landed with source edits after the last passing check
/// (the coach's A33 window), and the total.
pub fn unchecked_commits(stats: &tools::Stats) -> (usize, usize) {
    let mut prev = i64::MIN;
    let mut unchecked = 0;
    for (at, _, _) in &stats.commits {
        let verified_at = stats
            .calls
            .iter()
            .filter(|c| c.is_confirmed_test() && !c.is_error)
            .filter(|c| c.test_marker != crate::transcript::TestMarker::Failed)
            .filter_map(|c| c.started_at)
            .filter(|s| *s > prev && *s < *at)
            .max()
            .unwrap_or(prev);
        let edited = stats.calls.iter().any(|c| {
            c.class == crate::phase::ToolClass::Implement
                && c.started_at.is_some_and(|s| s > verified_at && s < *at)
                && (c.paths.is_empty() || c.paths.iter().any(|p| !crate::phase::is_doc_path(p)))
        });
        if edited {
            unchecked += 1;
        }
        prev = *at;
    }
    (unchecked, stats.commits.len())
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
    let errors_by_class = stats
        .errors_by_class()
        .into_iter()
        .map(|(k, n)| (k.label().to_string(), n))
        .collect();
    Some(SessionFigures {
        turns: agg.human_turns(),
        api_calls: agg.api_calls(),
        cost: cost_state.as_ref().map(|c| c.total_cost_usd),
        tokens: agg.total.total(),
        cache_hit: agg.total.cache_hit_ratio(),
        error_rate: (calls > 0).then(|| errors as f64 / calls as f64),
        cost_by_family: by_family,
        interrupts: agg.interrupts.len(),
        errors_by_class,
        commits: unchecked_commits(&stats),
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

/// Compute medians over the recent transcripts; `insights` (Claude Code's
/// `usage-data`, when it exists) supplies the active hours.
pub fn compute(projects_dir: &Path, days: u64, now_ms: i64) -> Baseline {
    compute_with(
        projects_dir,
        days,
        now_ms,
        crate::insights::default_dir()
            .and_then(|d| crate::insights::load(&d))
            .as_ref(),
    )
}

pub fn compute_with(
    projects_dir: &Path,
    days: u64,
    now_ms: i64,
    insights: Option<&crate::insights::Insights>,
) -> Baseline {
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
    let mut classes: BTreeMap<String, f64> = BTreeMap::new();
    for f in &figs {
        for (k, n) in &f.errors_by_class {
            *classes.entry(k.clone()).or_insert(0.0) += *n as f64;
        }
    }
    let errors_total: f64 = classes.values().sum();
    if errors_total > 0.0 {
        for v in classes.values_mut() {
            *v /= errors_total;
        }
    }
    let (unchecked, commits) = figs
        .iter()
        .fold((0, 0), |(u, t), f| (u + f.commits.0, t + f.commits.1));
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
        calls_per_turn: median(
            figs.iter()
                .map(|f| f.api_calls as f64 / f.turns as f64)
                .collect(),
        ),
        interruption_rate: median(
            figs.iter()
                .map(|f| f.interrupts as f64 / f.turns as f64)
                .collect(),
        ),
        error_categories: classes,
        commit_without_check_ratio: (commits > 0).then(|| unchecked as f64 / commits as f64),
        active_hours: insights
            .map(|i| i.hour_histogram().to_vec())
            .unwrap_or_default(),
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
        let insights_dir = crate::insights::tests::dir("baseline");
        let insights = crate::insights::load(&insights_dir);
        let b = compute_with(&projects, 7, now, insights.as_ref());
        assert_eq!(b.sessions, 2, "the 1-turn session is ignored");
        assert!(b.calls_per_turn.unwrap() > 1.0, "{:?}", b.calls_per_turn);
        assert!(b.interruption_rate.unwrap() > 0.0 && b.interruption_rate.unwrap() < 1.0);
        assert!(!b.error_categories.is_empty());
        assert!((b.error_categories.values().sum::<f64>() - 1.0).abs() < 1e-9);
        assert_eq!(b.active_hours.len(), 24);
        assert_eq!(b.active_hours[9], 6);
        let _ = std::fs::remove_dir_all(&insights_dir);
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
