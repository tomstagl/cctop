//! `~/.claude.json`, read for a handful of keys: the account's rate-limit
//! tier, the previous session in a directory, and which of Claude Code's own
//! tips it has already shown. Never the account's names or a prompt.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub fn path() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_HOME_JSON")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude.json")))
}

/// The parsed file, or `None` when unreadable.
pub fn read() -> Option<Value> {
    read_at(&path()?)
}

pub fn read_at(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// `oauthAccount.userRateLimitTier`, else `organizationRateLimitTier`.
pub fn rate_limit_tier(v: &Value) -> Option<String> {
    let acct = v.get("oauthAccount")?;
    ["userRateLimitTier", "organizationRateLimitTier"]
        .iter()
        .find_map(|k| acct.get(k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Extra usage (usage credits past the plan) is enabled on the account.
pub fn extra_usage_enabled(v: &Value) -> Option<bool> {
    v.get("oauthAccount")?
        .get("hasExtraUsageEnabled")?
        .as_bool()
}

/// What the previous session in `cwd` cost, per `projects[cwd].last*`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreviousSession {
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub lines_added: Option<u64>,
    pub lines_removed: Option<u64>,
    /// Model ids seen, from `lastModelUsage` (keys only).
    pub models: Vec<String>,
    /// Hook latency Claude Code measured: `(p50, p95, p99)` ms.
    pub hook_ms: Option<(u64, u64, u64)>,
}

pub fn previous_session(v: &Value, cwd: &Path) -> Option<PreviousSession> {
    let p = v.get("projects")?.get(cwd.to_string_lossy().as_ref())?;
    let n = |k: &str| p.get(k).and_then(Value::as_u64);
    let metrics = p.get("lastSessionMetrics");
    let hook = |k: &str| metrics.and_then(|m| m.get(k)).and_then(Value::as_u64);
    Some(PreviousSession {
        cost_usd: p.get("lastCost").and_then(Value::as_f64),
        duration_ms: n("lastDuration"),
        lines_added: n("lastLinesAdded"),
        lines_removed: n("lastLinesRemoved"),
        models: p
            .get("lastModelUsage")
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default(),
        hook_ms: match (
            hook("hook_duration_ms_p50"),
            hook("hook_duration_ms_p95"),
            hook("hook_duration_ms_p99"),
        ) {
            (Some(a), Some(b), Some(c)) => Some((a, b, c)),
            _ => None,
        },
    })
}

/// `tipsHistory`: tip id → the startup count at which it was last shown.
pub fn tips_history(v: &Value) -> std::collections::BTreeMap<String, u64> {
    v.get("tipsHistory")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, n)| Some((k.clone(), n.as_u64()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// Tip ids Claude Code showed within the last `within` startups.
pub fn tips_recent(v: &Value, within: u64) -> std::collections::BTreeSet<String> {
    let Some(now) = num_startups(v) else {
        return Default::default();
    };
    tips_history(v)
        .into_iter()
        .filter(|(_, shown)| now.saturating_sub(*shown) <= within)
        .map(|(id, _)| id)
        .collect()
}

/// `numStartups`, for "shown in the last N startups".
pub fn num_startups(v: &Value) -> Option<u64> {
    v.get("numStartups")?.as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_keys_only() {
        let v: Value = serde_json::json!({
            "numStartups": 431,
            "oauthAccount": {"emailAddress": "x@y", "userRateLimitTier": null, "organizationRateLimitTier": "default_claude_max_20x", "hasExtraUsageEnabled": true},
            "tipsHistory": {"memory-command": 432, "prompt-queue": 34},
            "projects": {"/repo": {"lastCost": 0.16, "lastDuration": 75000, "lastLinesAdded": 3, "lastLinesRemoved": 1, "lastSessionFirstPrompt": "never shown", "lastModelUsage": {"claude-opus-5": {"inputTokens": 1}}, "lastSessionMetrics": {"hook_duration_ms_p50": 12, "hook_duration_ms_p95": 30, "hook_duration_ms_p99": 31}}}
        });
        assert_eq!(
            rate_limit_tier(&v).as_deref(),
            Some("default_claude_max_20x")
        );
        assert_eq!(extra_usage_enabled(&v), Some(true));
        let prev = previous_session(&v, Path::new("/repo")).unwrap();
        assert_eq!(prev.cost_usd, Some(0.16));
        assert_eq!(prev.duration_ms, Some(75_000));
        assert_eq!(prev.models, ["claude-opus-5"]);
        assert_eq!(prev.hook_ms, Some((12, 30, 31)));
        assert!(previous_session(&v, Path::new("/other")).is_none());
        assert_eq!(tips_history(&v)["prompt-queue"], 34);
        assert_eq!(num_startups(&v), Some(431));
        assert_eq!(rate_limit_tier(&serde_json::json!({})), None);
        assert!(read_at(Path::new("/nonexistent/.claude.json")).is_none());
    }
}
