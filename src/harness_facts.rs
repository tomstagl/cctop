//! Facts about Claude Code that cctop must agree with, each tagged with the
//! version it was read from. Two kinds: the binary's constants (autocompact
//! arithmetic, `/usage` weights, `/context` thresholds) and the version at
//! which a transcript field first appeared, so a parser can fall back on
//! older transcripts instead of misreading them.
//!
//! `scripts/check-plugin-types.sh` warns when the installed `claude` is newer
//! than [`READ_FROM`]: the numbers below are then unverified, not wrong.

use std::cmp::Ordering;

/// The Claude Code version these facts were recovered from.
pub const READ_FROM: &str = "2.1.270";

/// A `major.minor.patch` Claude Code version, comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// Parse `2.1.269` (or `2.1.269 (Claude Code)`); `None` for anything else.
    pub fn parse(s: &str) -> Option<Version> {
        let head = s.trim().split(|c: char| c.is_whitespace()).next()?;
        let mut it = head.split('.').map(|x| x.parse::<u32>().ok());
        Some(Version(it.next()??, it.next()??, it.next()??))
    }

    /// `true` when `s` parses and is at least `self`. An unparsable or absent
    /// version is treated as *older* than everything: a parser that gates on
    /// a field must fall back, not assume.
    pub fn at_most(self, s: Option<&str>) -> bool {
        matches!(s.and_then(Version::parse), Some(v) if v.cmp(&self) != Ordering::Less)
    }
}

/// Transcript fields by the version that first wrote them (the research
/// trail's T38 map, 2.1.220 → 2.1.269, re-checked on this machine's corpus).
/// Before the listed version the field is absent and the older heuristic
/// applies.
pub mod first_seen {
    use super::Version;
    /// `promptId`, `promptSource`, `origin`, `effort`, `toolUseResult`.
    pub const PROMPT_ID: Version = Version(2, 1, 220);
    /// `system/turn_duration`, `away_summary`, `local_command`, `task_reminder`.
    pub const TURN_DURATION: Version = Version(2, 1, 226);
    /// `toolDenialKind`, `attributionMcpServer`, `pendingBackgroundAgentCount`.
    pub const DENIAL_KIND: Version = Version(2, 1, 227);
    /// `interruptedMessageId`, `total_tokens_reminder`, `userFeedback`.
    pub const INTERRUPTED_MESSAGE_ID: Version = Version(2, 1, 231);
    /// `isApiErrorMessage`, `error`, `plan_mode`, `system/informational`.
    pub const API_ERROR_MESSAGE: Version = Version(2, 1, 232);
    /// `hook_success` attachments.
    pub const HOOK_SUCCESS: Version = Version(2, 1, 260);
    /// `system/compact_boundary`, `isCompactSummary`, `apiErrorStatus`, `goal_status`.
    pub const COMPACT_BOUNDARY: Version = Version(2, 1, 263);
    /// Line-level `rendered[]` on attachments, `prompt_snapshot`, `instructions`.
    pub const RENDERED: Version = Version(2, 1, 266);
    /// `perTurnEffort`, `quotaLimits`, `batching_reminder_sent`, `silent_turn_reminder`.
    pub const PER_TURN_EFFORT: Version = Version(2, 1, 269);
    /// `continued-in` (what `/clear` leaves in the old transcript), `sessionKind`.
    pub const CONTINUED_IN: Version = Version(2, 1, 270);
}

/// What Claude Code's `cost-state` line holds (read on the 2.1.247 – 2.1.272
/// transcripts of one machine: 79 sessions with a cost-state, 8 with subagent
/// usage; `scripts/ledger-vs-agents.py` prints the table).
pub mod cost_state {
    /// `modelUsage` was ≥ main + subagents on every session and matched
    /// main + agents where the agents dominate (a 336-agent run: cache
    /// reads 0.2 % apart, cache writes 0.7 %). Adding priced agents on top
    /// of the ledger double counts; only calls after its moment are added.
    pub const INCLUDES_SUBAGENTS: bool = true;
    /// The ledger also holds calls no transcript shows (haiku side calls:
    /// 1.5 M input tokens on one session; `inputTokens` is 20–100× the
    /// transcript's), so `ledger − priced(main)` is not the agents' share
    /// and neither part is ever derived by subtraction.
    pub const INCLUDES_UNTRANSCRIBED_CALLS: bool = true;
    /// 115 cost-states in 79 sessions: 68 the file's last line (after
    /// `last-prompt`), 26 after `bridge-session`, 1–5 per session, none
    /// with a timestamp. The nearest timestamped line is a `system` line
    /// 1–5 lines above; a live session has no ledger until it ends.
    pub const WRITTEN_AT: &str = "session end (after last-prompt) or bridge-session; no timestamp";
}

/// The `<task-notification>` a finished agent (or background task) sends
/// back as a `user` line with `origin.kind = "task-notification"` (387 on
/// the same corpus, Claude Code 2.1.231 – 2.1.272).
pub mod task_notification {
    /// `<status>` values seen: 294 / 25 / 9.
    pub const STATUSES: [&str; 3] = ["completed", "failed", "killed"];
    /// `<usage>` (`subagent_tokens`, `tool_uses`, `duration_ms`) is optional
    /// on agent notifications of every version seen (2.1.231 – 2.1.270; 18
    /// with, 31 without, both in 2.1.269). Nothing may depend on it.
    pub const USAGE_OPTIONAL: bool = true;
    /// A background shell task's notification has a `<task-id>` and no
    /// `<tool-use-id>`; a workflow run's carries `<agent_count>`,
    /// `<agents_done>`, `<agents_error>`, `<agents_skipped>`,
    /// `<agents_empty_result>` and `<failures>` instead of a result.
    pub const SHELL_TASKS_HAVE_NO_TOOL_USE_ID: bool = true;
}

/// Autocompact arithmetic (recovered from the 2.1.269 binary and the debug
/// log's `autocompact: tokens=… effectiveWindow=…` line).
pub mod autocompact {
    /// The threshold is the effective window minus this.
    pub const BUFFER_TOKENS: u64 = 13_000;
    /// The warn band starts this far below the threshold.
    pub const WARN_TOKENS: u64 = 20_000;
    /// Requests are blocked this far below the window itself.
    pub const BLOCK_TOKENS: u64 = 3_000;
    /// Precomputed compaction arms at this share of the window.
    pub const PRECOMPUTE_RATIO: f64 = 0.80;

    /// Effective window Claude Code reasons with, by nominal window.
    pub fn effective_window(nominal: u64) -> u64 {
        match nominal {
            1_000_000 => 980_000,
            n => n,
        }
    }

    /// `effective window − 13 000`: 967 000 on native-1M models, 187 000 on
    /// 200 k windows.
    pub fn threshold(nominal: u64) -> u64 {
        effective_window(nominal).saturating_sub(BUFFER_TOKENS)
    }
}

/// `/usage` limit weight: `(cached + uncached×10 + cacheCreate×12.5 +
/// output×50) × tier`.
pub mod usage_weight {
    pub const UNCACHED: f64 = 10.0;
    pub const CACHE_CREATE: f64 = 12.5;
    pub const OUTPUT: f64 = 50.0;

    /// Tier multiplier by model family.
    pub fn tier(model: &str) -> f64 {
        if model.contains("fable") {
            10.0
        } else if model.contains("opus") {
            5.0
        } else if model.contains("sonnet") {
            3.0
        } else {
            1.0
        }
    }
}

/// `/context`'s per-tool suggestion thresholds.
pub mod context_suggestions {
    /// A tool's results must be at least this share of the window …
    pub const TOOL_WINDOW_SHARE: f64 = 0.15;
    /// … and at least this many tokens (Read: 5 % of the window).
    pub const TOOL_MIN_TOKENS: u64 = 10_000;
    pub const READ_WINDOW_SHARE: f64 = 0.05;
    pub const MEMORY_WINDOW_SHARE: f64 = 0.05;
    pub const MEMORY_MIN_TOKENS: u64 = 5_000;
    /// The context-level suggestion appears at this fill.
    pub const CONTEXT_SHARE: f64 = 0.80;
}

/// `effort_cost_index` per model family, by effort level: what a level costs
/// relative to `high`.
pub fn effort_cost_index(model: &str, level: &str) -> Option<f64> {
    let fable = model.contains("fable");
    let sonnet = model.contains("sonnet");
    Some(match (level, fable, sonnet) {
        ("low", true, _) => 0.75,
        ("medium", true, _) => 0.86,
        ("high", true, _) => 1.0,
        ("xhigh", true, _) => 1.38,
        ("max", true, _) => 1.74,
        ("low", _, true) => 0.47,
        ("medium", _, true) => 0.74,
        ("high", _, true) => 1.0,
        ("xhigh", _, true) => 2.41,
        ("max", _, true) => 5.59,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(Version::parse("2.1.269"), Some(Version(2, 1, 269)));
        assert_eq!(
            Version::parse("2.1.270 (Claude Code)"),
            Some(Version(2, 1, 270))
        );
        assert_eq!(Version::parse("garbage"), None);
        assert!(Version(2, 1, 263) < Version(2, 1, 270));
        assert!(first_seen::COMPACT_BOUNDARY.at_most(Some("2.1.263")));
        assert!(first_seen::COMPACT_BOUNDARY.at_most(Some("2.1.270")));
        assert!(!first_seen::COMPACT_BOUNDARY.at_most(Some("2.1.258")));
        assert!(
            !first_seen::COMPACT_BOUNDARY.at_most(None),
            "absent = older"
        );
        assert!(Version::parse(READ_FROM).is_some());
    }

    #[test]
    fn autocompact_threshold_matches_the_docs() {
        assert_eq!(autocompact::threshold(1_000_000), 967_000);
        assert_eq!(autocompact::threshold(200_000), 187_000);
        assert_eq!(usage_weight::tier("claude-opus-5"), 5.0);
        assert_eq!(usage_weight::tier("claude-haiku-4-5-20251001"), 1.0);
        assert_eq!(effort_cost_index("claude-fable-5-1", "max"), Some(1.74));
        assert_eq!(effort_cost_index("claude-sonnet-5", "low"), Some(0.47));
        assert_eq!(effort_cost_index("claude-opus-5", "high"), None);
    }

    #[test]
    fn ledger_and_notification_facts() {
        assert!(cost_state::WRITTEN_AT.contains("no timestamp"));
        assert!(task_notification::STATUSES.contains(&"killed"));
    }
}
