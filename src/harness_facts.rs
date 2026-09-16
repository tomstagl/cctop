//! Facts about Claude Code that cctop must agree with, each tagged with the
//! version it was read from. Two kinds: the binary's constants (autocompact
//! arithmetic, `/usage` weights, `/context` thresholds) and the version at
//! which a transcript field first appeared, so a parser can fall back on
//! older transcripts instead of misreading them.
//!
//! `scripts/check-plugin-types.sh` warns when the installed `claude` is newer
//! than [`READ_FROM`] and fails past `FACTS_MAX_LAG` releases: the numbers
//! below are then unverified, not wrong. `scripts/check-harness-facts.py`
//! re-reads the binary's constants from the installed bundle (each by a
//! stable anchor, never a minified name) and the team facts from the newest
//! team directory, and prints what differs; the bump is then mechanical.

use std::cmp::Ordering;

/// The Claude Code version these facts were recovered from.
pub const READ_FROM: &str = "2.1.273";

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
    /// `agentName` / `teamName` on every `user`, `assistant`, `system` and
    /// `attachment` line of a teammate's transcript. The oldest teammate
    /// transcript on this machine (2.1.232) has them from line 4; the
    /// version that introduced them is not known, so this is an upper
    /// bound (team PRD §10.3).
    pub const TEAM_NAME: Version = Version(2, 1, 232);
}

/// How Claude Code records an agent team (read on this machine's 19 team
/// directories and 23 teammate transcripts, 2.1.232 – 2.1.272; the keys are
/// in `docs/teams.md`).
pub mod teams {
    /// `~/.claude/teams/<team>/` goes when the team ends; the teammates'
    /// transcripts stay. Membership after the fact comes from the
    /// transcripts' `teamName`, never from the directory.
    pub const DIR_REMOVED_AT_END: bool = true;
    /// A `config.json` member (`agentId`, `agentType`, `backendType`, `cwd`,
    /// `joinedAt`, `name`, `tmuxPaneId`, and on spawned teammates `color`,
    /// `isActive`, `model`, `planModeRequired`, `prompt`) carries no session
    /// id: the transcript is found by `teamName == <team dir>` and
    /// `agentName == member.name`.
    pub const MEMBER_HAS_SESSION_ID: bool = false;
    /// Claude Code's own liveness flag on a `tmux` member (absent on the
    /// lead, whose `backendType` is `in-process`).
    pub const LIVENESS_KEY: &str = "isActive";
    /// The team directory's name: `session-` and the first eight characters
    /// of the lead's session id; `teamName` on a teammate's lines equals it.
    pub const NAME_PREFIX: &str = "session-";
    pub const LEAD_ID_CHARS: usize = 8;
    /// The first line carrying the team keys is line 4 of every teammate
    /// transcript seen (after `agent-setting`, `mode`, `permission-mode`);
    /// a head scan reads at most this many lines.
    pub const HEAD_SCAN_LINES: usize = 10;
    /// A teammate writes `cost-state` like the lead: at its end or a bridge
    /// (20 of 23 transcripts, all ended), never while it runs.
    pub const COST_STATE_AT_END: bool = true;
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
    /// `totalCostUSD` is the *writing process's* running total, not the
    /// transcript's: `startTime` names the process, and two processes can
    /// append to one file (a `--resume` beside the live session, a bridge),
    /// each writing its own ledger. 130 cost-states in 91 sessions on
    /// 2026-09-16: every one carries `startTime`; 2 sessions hold two
    /// processes' ledgers, the later line the smaller total (a $29 session
    /// ending on a $7 ledger; a $0 ledger from a process that did nothing is
    /// the same shape). The session's figure is the sum of the latest ledger
    /// per `startTime`, never the last line alone.
    pub const PER_PROCESS: bool = true;
}

/// The `<task-notification>` a finished agent, background shell command or
/// workflow run sends back (the same corpus, Claude Code 2.1.231 – 2.1.272,
/// re-read on 2026-09-16 over 505 transcripts).
pub mod task_notification {
    /// `<status>` values seen.
    pub const STATUSES: [&str; 3] = ["completed", "failed", "killed"];
    /// `<usage>` (`subagent_tokens`, `tool_uses`, `duration_ms`) is optional
    /// on agent notifications of every version seen (2.1.231 – 2.1.270; 18
    /// with, 31 without, both in 2.1.269). Nothing may depend on it.
    pub const USAGE_OPTIONAL: bool = true;
    /// Claude Code delivers one notification three ways, all with the same
    /// text: a `user` line (`promptSource: system`, `origin.kind:
    /// task-notification`) when the model is idle; when it is busy, a
    /// `queue-operation` `enqueue` line whose `content` is the text, then
    /// either a `queued_command` attachment (`prompt`) or, after a
    /// `dequeue`, the user line. On this machine every `killed` agent
    /// notification came as an attachment and none as a user line, so a
    /// reader must take all three and key by `<task-id>`.
    pub const DELIVERIES: [&str; 3] = ["user", "queue-operation", "attachment"];
    /// The `<task-id>` is the agent id (17 hex characters, the transcript
    /// file's name) for an `Agent`; a background shell command's is 9
    /// base-36 characters and its `<tool-use-id>` names a `Bash` call (it
    /// is present, contrary to v1.1's reading); a workflow run's carries
    /// `<agent_count>`, `<agents_done>`, `<agents_error>`,
    /// `<agents_skipped>`, `<agents_empty_result>` and `<failures>`.
    pub const AGENT_ID_HEX_LEN: usize = 17;
}

/// Autocompact arithmetic (recovered from the 2.1.269 binary and the debug
/// log's `autocompact: tokens=… effectiveWindow=…` line; re-read from the
/// 2.1.273 bundle by `scripts/check-harness-facts.py`).
pub mod autocompact {
    /// The threshold is the effective window minus this.
    pub const BUFFER_TOKENS: u64 = 13_000;
    /// The warn band starts this far below the threshold.
    pub const WARN_TOKENS: u64 = 20_000;
    /// Requests are blocked this far below the window itself.
    pub const BLOCK_TOKENS: u64 = 3_000;
    /// Precomputed compaction arms at this share of the window (the
    /// default `precomputeBufferFraction` is 1 − this; a per-window table
    /// can override it).
    pub const PRECOMPUTE_RATIO: f64 = 0.80;
    /// The effective window is the nominal one less
    /// `min(model max output tokens, this)`. Every model in the 2.1.273
    /// catalog but Claude 3.x defaults to 32 000 or 64 000 output tokens,
    /// so this is what comes off every window: 980 000 on 1M, 180 000 on
    /// 200 k. (Until 2.1.273 was re-read this was taken as a 1M-only
    /// special case; the 200 k figures were 20 000 high.)
    pub const OUTPUT_RESERVE_TOKENS: u64 = 20_000;

    /// Effective window Claude Code reasons with, by nominal window.
    pub fn effective_window(nominal: u64) -> u64 {
        nominal.saturating_sub(OUTPUT_RESERVE_TOKENS)
    }

    /// `effective window − 13 000`: 967 000 on native-1M models, 167 000 on
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
    /// Tier multiplier by the family named in the model id, first match.
    pub const TIERS: &[(&str, f64)] = &[("fable", 10.0), ("opus", 5.0), ("haiku", 1.0)];
    /// Sonnet, and any model naming none of the families (Claude Code's
    /// own fall-through; until 2.1.273 was re-read cctop took it as 1).
    pub const TIER_DEFAULT: f64 = 3.0;

    /// Tier multiplier by model family.
    pub fn tier(model: &str) -> f64 {
        let model = model.to_ascii_lowercase();
        TIERS
            .iter()
            .find(|(family, _)| model.contains(family))
            .map_or(TIER_DEFAULT, |(_, tier)| *tier)
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

/// `effort_cost_index` per model in Claude Code's catalog (2.1.273; the
/// same in 2.1.270): what a level costs relative to `high`, as
/// `[low, medium, high, xhigh, max]`. Models without an entry there (Haiku
/// 4.5, Sonnet ≤ 4.6, Opus ≤ 4.7, Mythos 5) have none here either. Matched
/// by the longest id contained in the model name, so `claude-fable-5-1`
/// does not read as Fable 5.
pub const EFFORT_COST_INDEX: &[(&str, [f64; 5])] = &[
    ("claude-sonnet-5", [0.47, 0.74, 1.0, 2.41, 5.59]),
    ("claude-opus-4-8", [0.72, 0.9, 1.0, 1.65, 1.88]),
    ("claude-opus-5", [0.67, 0.76, 1.0, 1.6, 1.7]),
    ("claude-fable-5", [0.6, 0.77, 1.0, 1.74, 1.91]),
    ("claude-fable-5-1", [0.75, 0.86, 1.0, 1.38, 1.74]),
    ("claude-mythos-5-1", [0.75, 0.86, 1.0, 1.38, 1.74]),
];

/// `effort_cost_index` for a model and level; None when the catalog has no
/// entry for the model or the level is not one of the five.
pub fn effort_cost_index(model: &str, level: &str) -> Option<f64> {
    let i = match level {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        "xhigh" => 3,
        "max" => 4,
        _ => return None,
    };
    EFFORT_COST_INDEX
        .iter()
        .filter(|(id, _)| model.contains(id))
        .max_by_key(|(id, _)| id.len())
        .map(|(_, table)| table[i])
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
        assert_eq!(autocompact::effective_window(1_000_000), 980_000);
        assert_eq!(autocompact::effective_window(200_000), 180_000);
        assert_eq!(autocompact::threshold(1_000_000), 967_000);
        assert_eq!(autocompact::threshold(200_000), 167_000);
        assert_eq!(usage_weight::tier("claude-opus-5"), 5.0);
        assert_eq!(usage_weight::tier("claude-haiku-4-5-20251001"), 1.0);
        assert_eq!(usage_weight::tier("claude-sonnet-5"), 3.0);
        assert_eq!(usage_weight::tier(""), 3.0, "Claude Code's fall-through");
        assert_eq!(effort_cost_index("claude-fable-5-1", "max"), Some(1.74));
        assert_eq!(
            effort_cost_index("claude-fable-5", "max"),
            Some(1.91),
            "Fable 5 is not Fable 5.1"
        );
        assert_eq!(effort_cost_index("claude-sonnet-5", "low"), Some(0.47));
        assert_eq!(effort_cost_index("claude-opus-5", "high"), Some(1.0));
        assert_eq!(effort_cost_index("claude-opus-5", "medium"), Some(0.76));
        assert_eq!(effort_cost_index("claude-opus-4-8", "max"), Some(1.88));
        assert_eq!(effort_cost_index("claude-haiku-4-5-20251001", "high"), None);
        assert_eq!(effort_cost_index("claude-opus-5", "turbo"), None);
    }

    #[test]
    fn the_reverification_script_reads_these_tables() {
        // scripts/check-harness-facts.py parses the consts and tables by
        // their spelling; a rename here must rename its probes too.
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/check-harness-facts.py"),
        )
        .unwrap();
        for name in [
            "READ_FROM",
            "OUTPUT_RESERVE_TOKENS",
            "EFFORT_COST_INDEX",
            "TIERS",
            "TIER_DEFAULT",
            "TOOL_WINDOW_SHARE",
            "LIVENESS_KEY",
            "NAME_PREFIX",
        ] {
            assert!(script.contains(name), "the script probes {name}");
        }
    }

    #[test]
    fn team_facts() {
        assert!(first_seen::TEAM_NAME.at_most(Some("2.1.232")));
        assert!(first_seen::TEAM_NAME <= first_seen::COMPACT_BOUNDARY);
        // The two booleans are read by the collector's discovery order
        // (config → spawns → head scan), so they are consts, not tests.
        assert_eq!(teams::LIVENESS_KEY, "isActive");
        assert_eq!(
            format!(
                "{}{}",
                teams::NAME_PREFIX,
                &"83f0e9b9-08a9-41a4-84a0-3fb55e77b8a2"[..teams::LEAD_ID_CHARS]
            ),
            "session-83f0e9b9"
        );
        assert_eq!(teams::HEAD_SCAN_LINES, 10);
        // The drift warning of `make check-types` covers this module: a
        // newer `claude` than READ_FROM names it among what to re-verify.
        let script = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/check-plugin-types.sh"),
        )
        .unwrap();
        assert!(script.contains("READ_FROM"), "the script reads READ_FROM");
        assert!(
            script.contains("teams module"),
            "the warning names the teams module"
        );
    }

    #[test]
    fn ledger_and_notification_facts() {
        assert!(cost_state::WRITTEN_AT.contains("no timestamp"));
        assert!(task_notification::STATUSES.contains(&"killed"));
        assert_eq!(task_notification::DELIVERIES.len(), 3);
        assert_eq!(
            task_notification::AGENT_ID_HEX_LEN,
            "a9a92645226d3a561".len()
        );
    }
}
