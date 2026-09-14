//! Claude Code's own session analysis, read back: `/insights` writes one
//! `usage-data/session-meta/<session>.json` per session (counts: prompts,
//! tools, errors, commits, interruptions, response times) and one
//! `usage-data/facets/<session>.json` (its verdicts: outcome, satisfaction,
//! friction, goal categories). cctop takes the numbers and the enum-like
//! verdicts for a per-project yardstick and the session-start line.
//!
//! Never displayed, and therefore never parsed — the structs below have no
//! field for them: `first_prompt` and the per-message timestamps of a
//! session-meta file; `underlying_goal`, `brief_summary` and
//! `friction_detail` of a facet file (free text about the person's work).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The counts `/insights` kept for one session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub project_path: String,
    /// ISO timestamp of the session's start.
    #[serde(default)]
    pub start_time: String,
    #[serde(default)]
    pub duration_minutes: f64,
    #[serde(default)]
    pub user_message_count: u64,
    #[serde(default)]
    pub assistant_message_count: u64,
    #[serde(default)]
    pub tool_counts: BTreeMap<String, u64>,
    #[serde(default)]
    pub git_commits: u64,
    #[serde(default)]
    pub git_pushes: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub user_interruptions: u64,
    /// Seconds between Claude's stop and the person's next message.
    #[serde(default)]
    pub user_response_times: Vec<f64>,
    #[serde(default)]
    pub tool_errors: u64,
    #[serde(default)]
    pub tool_error_categories: BTreeMap<String, u64>,
    #[serde(default)]
    pub uses_task_agent: bool,
    #[serde(default)]
    pub uses_mcp: bool,
    #[serde(default)]
    pub lines_added: u64,
    #[serde(default)]
    pub lines_removed: u64,
    #[serde(default)]
    pub files_modified: u64,
    /// Hour of day (0–23) of every message: the person's active hours.
    #[serde(default)]
    pub message_hours: Vec<u8>,
}

/// The verdicts `/insights` reached for one session: enum-like labels and
/// counts only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Facet {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub goal_categories: BTreeMap<String, u64>,
    /// `fully_achieved` / `mostly_achieved` / `partially_achieved` / …
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub user_satisfaction_counts: BTreeMap<String, u64>,
    #[serde(default)]
    pub friction_counts: BTreeMap<String, u64>,
    #[serde(default)]
    pub primary_success: Option<String>,
    #[serde(default)]
    pub claude_helpfulness: Option<String>,
    #[serde(default)]
    pub session_type: Option<String>,
}

/// Everything `usage-data` holds, as of `computed_at_ms`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Insights {
    /// The newest file's mtime: when `/insights` last ran.
    pub computed_at_ms: i64,
    pub sessions: Vec<SessionMeta>,
    pub facets: Vec<Facet>,
}

/// One project's medians and verdict counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProjectView {
    pub sessions: usize,
    pub duration_min_p50: Option<f64>,
    pub prompts_p50: Option<f64>,
    pub interruptions_p50: Option<f64>,
    pub tool_errors_p50: Option<f64>,
    pub commits_p50: Option<f64>,
    /// Median of the per-session median response time, seconds.
    pub response_s_p50: Option<f64>,
    /// `satisfied` + `likely_satisfied` + `happy` over every satisfaction
    /// count, when facets exist.
    pub satisfied_share: Option<f64>,
    pub outcomes: BTreeMap<String, u64>,
    /// Friction labels by count, the most frequent first.
    pub friction: Vec<(String, u64)>,
}

/// `~/.claude/usage-data`.
pub fn default_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/usage-data"))
}

fn mtime_ms(path: &Path) -> Option<i64> {
    path.metadata()
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

/// The newest mtime under `dir` (both subdirectories): a change means a
/// re-read.
pub fn newest_mtime(dir: &Path) -> Option<i64> {
    let mut newest = None;
    for sub in ["session-meta", "facets"] {
        let Ok(entries) = std::fs::read_dir(dir.join(sub)) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                newest = newest.max(mtime_ms(&p));
            }
        }
    }
    newest
}

fn read_all<T: for<'de> Deserialize<'de>>(dir: &Path) -> Vec<T> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|t| serde_json::from_str::<T>(&t).ok())
        .collect()
}

/// Read `dir` (`usage-data`); `None` when it holds nothing.
pub fn load(dir: &Path) -> Option<Insights> {
    let computed_at_ms = newest_mtime(dir)?;
    let sessions: Vec<SessionMeta> = read_all(&dir.join("session-meta"));
    let facets: Vec<Facet> = read_all(&dir.join("facets"));
    if sessions.is_empty() && facets.is_empty() {
        return None;
    }
    Some(Insights {
        computed_at_ms,
        sessions,
        facets,
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

impl Insights {
    /// The sessions of `cwd` (its path as `/insights` wrote it).
    pub fn project(&self, cwd: &Path) -> ProjectView {
        let cwd = cwd.to_string_lossy();
        let mine: Vec<&SessionMeta> = self
            .sessions
            .iter()
            .filter(|s| s.project_path == cwd.as_ref())
            .collect();
        let ids: Vec<&str> = mine.iter().map(|s| s.session_id.as_str()).collect();
        let facets: Vec<&Facet> = self
            .facets
            .iter()
            .filter(|f| ids.contains(&f.session_id.as_str()))
            .collect();
        let mut outcomes = BTreeMap::new();
        let mut satisfaction: BTreeMap<String, u64> = BTreeMap::new();
        let mut friction: BTreeMap<String, u64> = BTreeMap::new();
        for f in &facets {
            if let Some(o) = &f.outcome {
                *outcomes.entry(o.clone()).or_default() += 1;
            }
            for (k, n) in &f.user_satisfaction_counts {
                *satisfaction.entry(k.clone()).or_default() += n;
            }
            for (k, n) in &f.friction_counts {
                *friction.entry(k.clone()).or_default() += n;
            }
        }
        let sat_total: u64 = satisfaction.values().sum();
        let satisfied: u64 = satisfaction
            .iter()
            .filter(|(k, _)| matches!(k.as_str(), "satisfied" | "likely_satisfied" | "happy"))
            .map(|(_, n)| *n)
            .sum();
        let mut friction: Vec<(String, u64)> = friction.into_iter().collect();
        friction.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ProjectView {
            sessions: mine.len(),
            duration_min_p50: median(mine.iter().map(|s| s.duration_minutes).collect()),
            prompts_p50: median(mine.iter().map(|s| s.user_message_count as f64).collect()),
            interruptions_p50: median(mine.iter().map(|s| s.user_interruptions as f64).collect()),
            tool_errors_p50: median(mine.iter().map(|s| s.tool_errors as f64).collect()),
            commits_p50: median(mine.iter().map(|s| s.git_commits as f64).collect()),
            response_s_p50: median(
                mine.iter()
                    .filter_map(|s| median(s.user_response_times.clone()))
                    .collect(),
            ),
            satisfied_share: (sat_total > 0).then(|| satisfied as f64 / sat_total as f64),
            outcomes,
            friction,
        }
    }

    /// Messages per hour of day over every session: the person's active
    /// hours.
    pub fn hour_histogram(&self) -> [u64; 24] {
        let mut h = [0u64; 24];
        for s in &self.sessions {
            for hour in &s.message_hours {
                if (*hour as usize) < 24 {
                    h[*hour as usize] += 1;
                }
            }
        }
        h
    }

    /// The one dim line for a session's first turn: the project's medians
    /// and verdicts, dated, with the `/insights` hint once the analysis is
    /// older than 30 days. `None` when the project has no analysed session.
    pub fn start_line(&self, cwd: &Path, now_ms: i64) -> Option<String> {
        let p = self.project(cwd);
        if p.sessions == 0 {
            return None;
        }
        let age_days = (now_ms - self.computed_at_ms).max(0) / 86_400_000;
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!(
            "insights {} · {} session{} here",
            crate::ui::fmt::date_ymd(self.computed_at_ms),
            p.sessions,
            if p.sessions == 1 { "" } else { "s" }
        ));
        if let (Some(d), Some(n)) = (p.duration_min_p50, p.prompts_p50) {
            parts.push(format!(
                "p50 {} min · {} prompts",
                d.round() as u64,
                n.round() as u64
            ));
        }
        if let Some(i) = p.interruptions_p50 {
            parts.push(format!("{} interrupts", i.round() as u64));
        }
        if let Some(s) = p.satisfied_share {
            parts.push(format!("satisfied {:.0} %", s * 100.0));
        }
        if let Some((label, _)) = p.friction.first() {
            parts.push(format!("friction: {}", label.replace('_', " ")));
        }
        if age_days > 30 {
            parts.push(format!("run /insights ({age_days} d old)"));
        }
        Some(parts.join(" · "))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    const SENTINELS: [&str; 5] = [
        "FIRST-PROMPT-NEVER-SHOWN",
        "GOAL-NEVER-SHOWN",
        "SUMMARY-NEVER-SHOWN",
        "FRICTION-NEVER-SHOWN",
        "2026-09-01T09:00:00-NEVER-SHOWN",
    ];

    /// A `usage-data` directory with two sessions of `/p` (one facet), one
    /// of `/q`, every never-display field carrying a sentinel.
    pub fn dir(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("cctop-insights-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("session-meta")).unwrap();
        std::fs::create_dir_all(root.join("facets")).unwrap();
        let meta = |id: &str, project: &str, minutes: u64, prompts: u64, interrupts: u64| {
            format!(
                r#"{{"session_id":"{id}","project_path":"{project}","start_time":"2026-09-01T09:00:00Z","duration_minutes":{minutes},"user_message_count":{prompts},"assistant_message_count":40,"tool_counts":{{"Bash":12,"Edit":3}},"languages":{{"Rust":3}},"git_commits":2,"git_pushes":1,"input_tokens":1000,"output_tokens":500,"first_prompt":"{}","user_interruptions":{interrupts},"user_response_times":[5.0,40.0,600.0],"tool_errors":3,"tool_error_categories":{{"Command Failed":2,"Other":1}},"uses_task_agent":false,"uses_mcp":true,"lines_added":80,"lines_removed":20,"files_modified":4,"message_hours":[9,9,10,14],"user_message_timestamps":["{}"],"transcript_mtime":1}}"#,
                SENTINELS[0], SENTINELS[4]
            )
        };
        std::fs::write(
            root.join("session-meta/s1.json"),
            meta("s1", "/p", 30, 10, 1),
        )
        .unwrap();
        std::fs::write(
            root.join("session-meta/s2.json"),
            meta("s2", "/p", 50, 20, 3),
        )
        .unwrap();
        std::fs::write(root.join("session-meta/s3.json"), meta("s3", "/q", 5, 2, 0)).unwrap();
        std::fs::write(
            root.join("facets/s1.json"),
            format!(
                r#"{{"underlying_goal":"{}","goal_categories":{{"feature_implementation":1}},"outcome":"mostly_achieved","user_satisfaction_counts":{{"likely_satisfied":3,"dissatisfied":1}},"friction_counts":{{"wrong_approach":2,"buggy_code":1}},"friction_detail":"{}","primary_success":"good_debugging","brief_summary":"{}","claude_helpfulness":"very_helpful","session_type":"multi_task","session_id":"s1"}}"#,
                SENTINELS[1], SENTINELS[3], SENTINELS[2]
            ),
        )
        .unwrap();
        root
    }

    #[test]
    fn reads_counts_and_verdicts_never_the_text() {
        let root = dir("read");
        let i = load(&root).expect("files");
        assert_eq!(i.sessions.len(), 3);
        assert_eq!(i.facets.len(), 1);
        assert!(i.computed_at_ms > 0);
        let p = i.project(Path::new("/p"));
        assert_eq!(p.sessions, 2);
        assert_eq!(p.duration_min_p50, Some(40.0));
        assert_eq!(p.prompts_p50, Some(15.0));
        assert_eq!(p.interruptions_p50, Some(2.0));
        assert_eq!(p.commits_p50, Some(2.0));
        assert_eq!(p.response_s_p50, Some(40.0));
        assert_eq!(p.satisfied_share, Some(0.75));
        assert_eq!(p.outcomes["mostly_achieved"], 1);
        assert_eq!(p.friction[0], ("wrong_approach".into(), 2));
        assert_eq!(i.project(Path::new("/nope")).sessions, 0);
        let h = i.hour_histogram();
        assert_eq!((h[9], h[10], h[14], h[0]), (6, 3, 3, 0));
        // The privacy contract: nothing of the never-display fields survives
        // parsing, in any form.
        let debug = format!("{i:?}");
        let json = serde_json::to_string(&i).unwrap();
        for s in SENTINELS {
            assert!(!debug.contains(s), "{s} in Debug");
            assert!(!json.contains(s), "{s} in JSON");
        }
        // The start line, fresh and stale.
        let now = i.computed_at_ms + 86_400_000;
        let line = i.start_line(Path::new("/p"), now).unwrap();
        assert!(line.starts_with("insights "), "{line}");
        assert!(line.contains("2 sessions here · p50 40 min · 15 prompts · 2 interrupts · satisfied 75 % · friction: wrong approach"), "{line}");
        assert!(!line.contains("run /insights"));
        let stale = i
            .start_line(Path::new("/p"), now + 40 * 86_400_000)
            .unwrap();
        assert!(stale.ends_with("run /insights (41 d old)"), "{stale}");
        assert_eq!(i.start_line(Path::new("/nope"), now), None);
        assert_eq!(newest_mtime(&root), Some(i.computed_at_ms));
        assert_eq!(load(Path::new("/nonexistent")), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
