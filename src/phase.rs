//! What a tool call is *for*: the phase word on the coach's state line and
//! the Bash class the rules read (a commit, a test run, an exploration…).
//!
//! Ported from the research reference (`phases.py`, session-phase-model
//! PH-01…PH-14): under auto mode 70 % of calls are Bash, so the command text
//! is classified with a fixed priority — commit › implement › test /
//! build-lint › ops › wait › git read › explore — over the segments of a
//! compound command. A rule set over the last five calls reproduced a finer
//! per-call labeler on 94 % of 6 255 calls; the *verify* word agreed with the
//! model's own description only 47–56 % of the time, so a test run is
//! confirmed from its output ([`crate::transcript::TestMarker`]) and never
//! inferred from the command alone.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// What a Bash command does, from its text alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BashClass {
    /// `git commit/push/tag/merge/rebase/add`, `gh pr create/merge`, `gh release`.
    Commit,
    /// Writes files: `sed -i`, heredocs, redirects into source files, `mv`,
    /// `mkdir`, `git checkout/stash/reset`, package installs.
    Implement,
    /// A test runner (`cargo test`, `pytest`, `npm test`, `go test`, `vitest`,
    /// `jest`, `make check`). Confirmed only by its output.
    Test,
    /// A build, type-check, lint or format run.
    BuildLint,
    /// Deploys, remote calls, process control.
    Ops,
    /// `sleep`, `wait`, polling loops.
    Wait,
    /// `git status/diff/log/show/blame…`.
    GitRead,
    /// Everything else: reading, listing, searching.
    Explore,
}

impl BashClass {
    pub fn label(self) -> &'static str {
        match self {
            BashClass::Commit => "commit",
            BashClass::Implement => "implement",
            BashClass::Test => "test",
            BashClass::BuildLint => "build-lint",
            BashClass::Ops => "ops",
            BashClass::Wait => "wait",
            BashClass::GitRead => "gitread",
            BashClass::Explore => "explore",
        }
    }
}

/// What a tool call is for, before the context rules of [`assign_phases`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolClass {
    Explore,
    Implement,
    /// A test-class Bash command whose output has not (yet) confirmed a run.
    Test,
    BuildLint,
    Commit,
    GitRead,
    Ops,
    Wait,
    Plan,
    Question,
    /// An `Agent` of `subagent_type` Explore.
    ExploreSub,
    /// Any other `Agent` / `Task`.
    Delegate,
    Coord,
    Deliver,
    Browse,
    Mcp,
    Other,
}

/// The phase word of a call once its neighbours are taken into account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Exploring,
    Implementing,
    /// A test run confirmed by its output, or a read-back of a just-edited
    /// file: the model is checking its work.
    Verifying,
    Committing,
    Planning,
    Delegating,
    /// Deploy / remote / process control.
    Ops,
    Waiting,
    Browsing,
    Other,
}

impl Phase {
    /// The state-line word.
    pub fn word(self) -> &'static str {
        match self {
            Phase::Exploring => "EXPLORING",
            Phase::Implementing => "IMPLEMENTING",
            Phase::Verifying => "VERIFYING",
            Phase::Committing => "COMMITTING",
            Phase::Planning => "PLANNING",
            Phase::Delegating => "DELEGATING",
            Phase::Ops => "OPS",
            Phase::Waiting => "WAITING",
            Phase::Browsing => "BROWSING",
            Phase::Other => "WORKING",
        }
    }
}

const EXPLORE_TOOLS: &[&str] = &[
    "Read",
    "Glob",
    "Grep",
    "LS",
    "NotebookRead",
    "WebFetch",
    "WebSearch",
    "ToolSearch",
    "ListMcpResourcesTool",
    "ReadMcpResourceTool",
    "ReadMcpResourceDirTool",
];
const IMPL_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];
const PLAN_TOOLS: &[&str] = &[
    "EnterPlanMode",
    "ExitPlanMode",
    "TodoWrite",
    "TaskCreate",
    "TaskUpdate",
    "TaskList",
    "TaskGet",
];
const COORD_TOOLS: &[&str] = &[
    "SendMessage",
    "ListAgents",
    "RemoteTrigger",
    "ScheduleWakeup",
    "Monitor",
    "TaskStop",
    "Workflow",
    "CronCreate",
    "CronDelete",
    "CronList",
    "TaskOutput",
    "EnterWorktree",
    "ExitWorktree",
];
const DELIVER_TOOLS: &[&str] = &["Artifact", "DesignSync", "SendUserFile", "SendFeedback"];

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("valid regex"));
    };
}

re!(
    RE_COMMIT,
    r"\bgit\s+(-C\s+\S+\s+)?(commit|push|tag|merge|rebase|add)\b|\bgh\s+(pr\s+(create|merge)|release)\b"
);
re!(
    RE_TEST,
    r"\bcargo\s+(test|bench)\b|\bnpm\s+(test|run\s+test)\b|\bpnpm\s+test\b|\byarn\s+test\b|\bbun\s+test\b|\bpytest\b|\bgo\s+test\b|\bvitest\b|\bjest\b|\bmake\s+(check|test)\b|\bpython3?\s+-m\s+(pytest|unittest)\b|\bnode\s+--test\b|\bcargo\s+run\b|(^|[\s;&|])\.?/?target/(debug|release)/"
);
re!(
    RE_BUILD,
    r"\bcargo\s+(build|check|clippy|fmt|doc)\b|\bnpm\s+(run|ci|start|install)\b|\bnpx\b|\bpnpm\b|\byarn\b|\bmake\b|\bgo\s+(build|vet|run)\b|\btsc\b|\beslint\b|\bruff\b|\bmypy\b|\bgradlew\b|\bbun\s+run\b|\bcctop\s+(query|run|metrics|--version|install|split)\b|\bclaude\s+(plugin\s+validate|-p\b|--version)|\bterraform\s+(plan|validate|fmt)\b|\bcurl\b[^|;&]*localhost|\bcurl\b[^|;&]*127\.0\.0\.1|\blighthouse\b|\bshellcheck\b"
);
re!(
    RE_IMPL,
    r#"\bsed\s+-i\b|\bcat\s*>|<<\s*-?['"]?EOF|\btee\b|\bperl\s+-p?i\b|(^|[^0-9&])>{1,2}\s*[\w./~$-]+\.(rs|ts|tsx|js|jsx|md|toml|json|py|yml|yaml|sh|css|html|svg|txt|csv|lock)\b|\bmv\b|\bcp\s|\bmkdir\b|\brm\s|\btouch\b|\bchmod\b|\bln\s|\bpatch\b|\bgit\s+(checkout|stash|restore|apply|worktree|switch|branch\s+-[dD]|reset)\b|\bpython3?\s+-c\b[^;]*open\([^)]*['"]w|\bwrite_text\(|\bnpm\s+(i|install|uninstall)\b|\bcargo\s+(add|remove|new|init)\b|\bbrew\s+(install|upgrade)\b"#
);
re!(
    RE_OPS,
    r"\bssh\b|\baws\b|\bterraform\s+(apply|destroy|import)\b|\bkubectl\b|\bdocker\b|\bgh\s+(workflow|run|issue|api|repo|secret|variable|auth)\b|\bclawctl\b|\bopenclaw\b|\bcurl\b|\boc\s|\bscp\b|\brsync\b|\bpkill\b|\bkill\b|\bsystemctl\b|\blaunchctl\b|\bopen\s+-"
);
re!(
    RE_WAIT,
    r"^\s*sleep\b|\buntil\b|\bwhile\b[^;]*;\s*do[^;]*sleep|\bwait\b"
);
re!(
    RE_GITREAD,
    r"\bgit\s+(-C\s+\S+\s+)?(status|diff|log|show|branch|remote|stash\s+list|blame|describe|rev-parse|ls-files)\b"
);
re!(
    RE_PATH,
    r"(?:[\w./~-]+/)?[\w.-]+\.(?:rs|ts|tsx|js|jsx|md|toml|json|py|yml|yaml|sh|css|html|svg|txt|csv)\b"
);
re!(RE_CD, r"^\s*cd\s+\S+\s*(&&|;)\s*");
re!(RE_ENV, r"^\s*(export\s+)?[A-Z_][A-Z0-9_]*=\S+\s*(;|&&)?\s*");
re!(RE_SEGMENT, r"&&|\|\||;|\n");
// The narrow read-only allowlist of the exploration-run counter (A25): a
// segment must start with one of these and write nothing to count.
re!(
    RE_READ_ONLY,
    r"^\s*(cat|ls|rg|grep|egrep|find|head|tail|wc|tree|sed\s+-n|git\s+(-C\s+\S+\s+)?(log|show|diff|status|blame|ls-files))\b"
);
re!(
    RE_PIPE_WRITE,
    r"[^0-9&]>{1,2}\s*[\w./~$-]|\btee\b|\bxargs\b.*\b(rm|mv|sed\s+-i)\b"
);

/// Classify one Bash command. Within a segment: commit › implement › test ›
/// build-lint › ops › wait › git read › explore; across the segments of a
/// compound command a test or build outranks the implement beside it.
pub fn classify_bash(cmd: &str) -> BashClass {
    if cmd.trim().is_empty() {
        return BashClass::Explore;
    }
    let stripped = RE_ENV.replace(&RE_CD.replace(cmd, ""), "").into_owned();
    let mut seen = [false; 8];
    for seg in RE_SEGMENT
        .split(&stripped)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let class = if RE_COMMIT.is_match(seg) {
            BashClass::Commit
        } else if RE_IMPL.is_match(seg) {
            BashClass::Implement
        } else if RE_TEST.is_match(seg) {
            BashClass::Test
        } else if RE_BUILD.is_match(seg) {
            BashClass::BuildLint
        } else if RE_OPS.is_match(seg) {
            BashClass::Ops
        } else if RE_WAIT.is_match(seg) {
            BashClass::Wait
        } else if RE_GITREAD.is_match(seg) {
            BashClass::GitRead
        } else {
            BashClass::Explore
        };
        seen[class as usize] = true;
    }
    // Across segments a verify beats an implement (`sed -i … && cargo test`
    // is a test run); within a segment an implement beats a verify.
    for class in [
        BashClass::Commit,
        BashClass::Test,
        BashClass::BuildLint,
        BashClass::Implement,
        BashClass::Ops,
        BashClass::Wait,
        BashClass::GitRead,
    ] {
        if seen[class as usize] {
            return class;
        }
    }
    BashClass::Explore
}

/// A Bash command that only reads (the exploration-run allowlist): every
/// segment starts with an allowlisted reader and nothing writes. A mixed
/// read + write command is not read-only.
pub fn read_only_bash(cmd: &str) -> bool {
    let stripped = RE_ENV.replace(&RE_CD.replace(cmd, ""), "").into_owned();
    let segs: Vec<&str> = RE_SEGMENT
        .split(&stripped)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    !segs.is_empty()
        && segs.iter().all(|s| {
            RE_READ_ONLY.is_match(s)
                && !RE_PIPE_WRITE.is_match(s)
                && !s.contains("sed -i")
                && !s.contains("--delete")
        })
}

/// Classify a tool call from its name and input.
pub fn classify_tool(name: &str, input: &Value) -> ToolClass {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    if name == "Bash" {
        return match classify_bash(s("command")) {
            BashClass::Commit => ToolClass::Commit,
            BashClass::Implement => ToolClass::Implement,
            BashClass::Test => ToolClass::Test,
            BashClass::BuildLint => ToolClass::BuildLint,
            BashClass::Ops => ToolClass::Ops,
            BashClass::Wait => ToolClass::Wait,
            BashClass::GitRead => ToolClass::GitRead,
            BashClass::Explore => ToolClass::Explore,
        };
    }
    if EXPLORE_TOOLS.contains(&name) {
        return ToolClass::Explore;
    }
    if IMPL_TOOLS.contains(&name) {
        return ToolClass::Implement;
    }
    if name == "Agent" || name == "Task" {
        return match s("subagent_type").to_ascii_lowercase().as_str() {
            "explore" => ToolClass::ExploreSub,
            "plan" => ToolClass::Plan,
            _ => ToolClass::Delegate,
        };
    }
    if PLAN_TOOLS.contains(&name) {
        return ToolClass::Plan;
    }
    if name == "AskUserQuestion" {
        return ToolClass::Question;
    }
    if COORD_TOOLS.contains(&name) {
        return ToolClass::Coord;
    }
    if DELIVER_TOOLS.contains(&name) {
        return ToolClass::Deliver;
    }
    if name == "Skill" {
        let sk = s("skill").to_ascii_lowercase();
        return if sk.contains("review") || sk.contains("security") {
            ToolClass::Test
        } else if sk == "pr" || sk == "commit" {
            ToolClass::Commit
        } else {
            ToolClass::Plan
        };
    }
    if name.starts_with("mcp__claude-in-chrome") || name.starts_with("mcp__Playwright") {
        return ToolClass::Browse;
    }
    if name.starts_with("mcp__") {
        return ToolClass::Mcp;
    }
    ToolClass::Other
}

/// File basenames a call touches (edits, reads, and the paths named in a
/// Bash command), for the read-back rule.
pub fn paths_of(name: &str, input: &Value) -> Vec<String> {
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    if IMPL_TOOLS.contains(&name) || name == "Read" {
        return input
            .get("file_path")
            .or_else(|| input.get("notebook_path"))
            .and_then(Value::as_str)
            .map(|p| vec![base(p)])
            .unwrap_or_default();
    }
    if name == "Bash" {
        let cmd = input.get("command").and_then(Value::as_str).unwrap_or("");
        let mut v: Vec<String> = RE_PATH.find_iter(cmd).map(|m| base(m.as_str())).collect();
        v.sort();
        v.dedup();
        return v;
    }
    Vec::new()
}

/// The little a phase assignment needs to know about a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallShape {
    pub name: String,
    pub class: ToolClass,
    /// Basenames the call touches.
    pub paths: Vec<String>,
    pub turn: usize,
    /// Result size in bytes, when known.
    pub result_bytes: u64,
    /// The output confirmed a test run (`Test` class only).
    pub test_confirmed: bool,
}

/// Phase per call from its class and its neighbours: a git read or a read
/// of a just-edited file after implementing is verifying; a single
/// explore/verify sandwiched between implements in one turn is implementing.
pub fn assign_phases(calls: &[CallShape]) -> Vec<Phase> {
    let n = calls.len();
    let mut phases: Vec<Phase> = Vec::with_capacity(n);
    for i in 0..n {
        let c = &calls[i];
        let recent_edits = calls[i.saturating_sub(5)..i]
            .iter()
            .filter(|p| p.class == ToolClass::Implement)
            .flat_map(|p| p.paths.iter().cloned())
            .collect::<Vec<_>>();
        let implemented_recently = calls[i.saturating_sub(6)..i]
            .iter()
            .any(|p| p.class == ToolClass::Implement);
        let phase = match c.class {
            ToolClass::Explore | ToolClass::GitRead => {
                let git_read_after_edit = c.class == ToolClass::GitRead && implemented_recently;
                let read_back = !c.paths.is_empty()
                    && (c.name == "Read" || c.name == "Bash")
                    && c.paths.iter().any(|p| recent_edits.contains(p));
                if git_read_after_edit || read_back {
                    Phase::Verifying
                } else {
                    Phase::Exploring
                }
            }
            ToolClass::Implement => Phase::Implementing,
            // A test-class command is verifying only once its output says a
            // runner ran; a build or lint is verifying by construction.
            ToolClass::Test => {
                if c.test_confirmed {
                    Phase::Verifying
                } else {
                    Phase::Other
                }
            }
            ToolClass::BuildLint => Phase::Verifying,
            ToolClass::Commit => Phase::Committing,
            ToolClass::Ops => Phase::Ops,
            ToolClass::Wait => Phase::Waiting,
            ToolClass::Plan | ToolClass::Question => Phase::Planning,
            ToolClass::ExploreSub => Phase::Exploring,
            ToolClass::Delegate | ToolClass::Coord => Phase::Delegating,
            ToolClass::Browse => Phase::Browsing,
            ToolClass::Deliver | ToolClass::Mcp | ToolClass::Other => Phase::Other,
        };
        phases.push(phase);
    }
    // Smoothing: a single explore/verify sandwiched between implements.
    for i in 1..n.saturating_sub(1) {
        if matches!(phases[i], Phase::Exploring | Phase::Verifying)
            && phases[i - 1] == Phase::Implementing
            && phases[i + 1] == Phase::Implementing
            && calls[i].turn == calls[i - 1].turn
            && calls[i].turn == calls[i + 1].turn
            && calls[i].result_bytes < 4000
        {
            phases[i] = Phase::Implementing;
        }
    }
    phases
}

/// The state-line phase: the last call's phase over a window of the last
/// five calls, and how many calls in a row have had it.
pub fn current(calls: &[CallShape]) -> Option<(Phase, usize)> {
    if calls.is_empty() {
        return None;
    }
    let start = calls.len().saturating_sub(7);
    let phases = assign_phases(&calls[start..]);
    let last = *phases.last()?;
    let run = phases.iter().rev().take_while(|p| **p == last).count();
    Some((last, run))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bash(cmd: &str) -> BashClass {
        classify_bash(cmd)
    }

    #[test]
    fn bash_classes_follow_the_reference_priority() {
        assert_eq!(bash("git add -A && git commit -m 'x'"), BashClass::Commit);
        assert_eq!(bash("gh pr create --title t"), BashClass::Commit);
        assert_eq!(bash("cargo test"), BashClass::Test);
        assert_eq!(bash("cargo test 2>&1 | tail -20"), BashClass::Test);
        assert_eq!(bash("npm test 2>&1 | tail -5"), BashClass::Test);
        assert_eq!(bash("make check"), BashClass::Test);
        assert_eq!(bash("python3 -m pytest tests/"), BashClass::Test);
        assert_eq!(bash("cargo build --release"), BashClass::BuildLint);
        assert_eq!(
            bash("cargo clippy --all-targets -- -D warnings"),
            BashClass::BuildLint
        );
        assert_eq!(bash("npx tsc --noEmit"), BashClass::BuildLint);
        assert_eq!(bash("sed -i '' 's/a/b/' src/x.rs"), BashClass::Implement);
        assert_eq!(
            bash("cat > src/new.rs <<'EOF'\nfn main() {}\nEOF"),
            BashClass::Implement
        );
        assert_eq!(bash("echo hi > notes.md"), BashClass::Implement);
        assert_eq!(
            bash("mkdir -p src/transcript && git mv a b"),
            BashClass::Implement
        );
        assert_eq!(bash("git checkout -b feat"), BashClass::Implement);
        assert_eq!(bash("git stash"), BashClass::Implement);
        assert_eq!(bash("gh api repos/x/y/issues"), BashClass::Ops);
        assert_eq!(bash("curl -s https://example.com/x"), BashClass::Ops);
        assert_eq!(bash("sleep 30"), BashClass::Wait);
        assert_eq!(
            bash("git status --short && git diff --stat"),
            BashClass::GitRead
        );
        assert_eq!(bash("git log --oneline -5"), BashClass::GitRead);
        assert_eq!(bash("cat src/main.rs | head -40"), BashClass::Explore);
        assert_eq!(bash("rg -n 'fn parse' src"), BashClass::Explore);
        assert_eq!(bash("ls -la"), BashClass::Explore);
        assert_eq!(bash(""), BashClass::Explore);
        // Priority over segments: a commit beats the test that precedes it.
        assert_eq!(bash("cargo test && git commit -am x"), BashClass::Commit);
        // `cd x &&` and env prefixes are ignored; `2>&1` is not a redirect.
        assert_eq!(
            bash("cd /repo && RUST_LOG=debug cargo test 2>&1"),
            BashClass::Test
        );
        assert_eq!(bash("cargo run -- query summary"), BashClass::Test);
        assert_eq!(bash("./target/debug/cctop --version"), BashClass::Test);
    }

    #[test]
    fn read_only_allowlist_is_narrow() {
        assert!(read_only_bash("cat src/main.rs | head -40"));
        assert!(read_only_bash("rg -n 'fn parse' src && ls src"));
        assert!(read_only_bash("git log --oneline -5; git diff --stat"));
        assert!(read_only_bash("sed -n '1,40p' src/x.rs"));
        assert!(read_only_bash("cd /repo && grep -rn foo ."));
        assert!(!read_only_bash("cat a.txt > b.txt"));
        assert!(!read_only_bash("sed -i 's/a/b/' x.rs"));
        assert!(!read_only_bash("cat x | tee y"));
        assert!(!read_only_bash("cargo test"));
        assert!(!read_only_bash("echo hi"));
        assert!(!read_only_bash("git stash"));
        assert!(!read_only_bash("ls && rm -rf x"));
        assert!(!read_only_bash(""));
    }

    #[test]
    fn tool_classes_and_paths() {
        let j = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert_eq!(
            classify_tool("Read", &j(r#"{"file_path":"/p/src/a.rs"}"#)),
            ToolClass::Explore
        );
        assert_eq!(classify_tool("Edit", &j("{}")), ToolClass::Implement);
        assert_eq!(
            classify_tool("Agent", &j(r#"{"subagent_type":"Explore"}"#)),
            ToolClass::ExploreSub
        );
        assert_eq!(
            classify_tool("Agent", &j(r#"{"subagent_type":"Plan"}"#)),
            ToolClass::Plan
        );
        assert_eq!(classify_tool("Agent", &j("{}")), ToolClass::Delegate);
        assert_eq!(
            classify_tool("AskUserQuestion", &j("{}")),
            ToolClass::Question
        );
        assert_eq!(classify_tool("TaskUpdate", &j("{}")), ToolClass::Plan);
        assert_eq!(classify_tool("SendMessage", &j("{}")), ToolClass::Coord);
        assert_eq!(classify_tool("Artifact", &j("{}")), ToolClass::Deliver);
        assert_eq!(
            classify_tool("Skill", &j(r#"{"skill":"code-review"}"#)),
            ToolClass::Test
        );
        assert_eq!(
            classify_tool("Skill", &j(r#"{"skill":"pr"}"#)),
            ToolClass::Commit
        );
        assert_eq!(
            classify_tool("Skill", &j(r#"{"skill":"design"}"#)),
            ToolClass::Plan
        );
        assert_eq!(
            classify_tool("mcp__claude-in-chrome__computer", &j("{}")),
            ToolClass::Browse
        );
        assert_eq!(
            classify_tool("mcp__github__get_me", &j("{}")),
            ToolClass::Mcp
        );
        assert_eq!(
            classify_tool("Bash", &j(r#"{"command":"cargo test"}"#)),
            ToolClass::Test
        );
        assert_eq!(classify_tool("Whatever", &j("{}")), ToolClass::Other);
        assert_eq!(
            paths_of("Edit", &j(r#"{"file_path":"/p/src/a.rs"}"#)),
            ["a.rs"]
        );
        assert_eq!(
            paths_of(
                "Bash",
                &j(r#"{"command":"cat src/a.rs src/b.rs | grep x; cat src/a.rs"}"#)
            ),
            ["a.rs", "b.rs"]
        );
        assert!(paths_of("Grep", &j(r#"{"pattern":"x"}"#)).is_empty());
    }

    fn shape(
        name: &str,
        class: ToolClass,
        paths: &[&str],
        turn: usize,
        confirmed: bool,
    ) -> CallShape {
        CallShape {
            name: name.into(),
            class,
            paths: paths.iter().map(|p| p.to_string()).collect(),
            turn,
            result_bytes: 100,
            test_confirmed: confirmed,
        }
    }

    #[test]
    fn phases_from_context_and_the_state_line_word() {
        // explore ×3 → implement → read-back of the edited file → test (confirmed) → commit
        let calls = vec![
            shape("Read", ToolClass::Explore, &["a.rs"], 1, false),
            shape("Bash", ToolClass::GitRead, &[], 1, false),
            shape("Grep", ToolClass::Explore, &[], 1, false),
            shape("Edit", ToolClass::Implement, &["a.rs"], 1, false),
            shape("Read", ToolClass::Explore, &["a.rs"], 1, false),
            shape("Bash", ToolClass::Test, &[], 1, true),
            shape("Bash", ToolClass::Commit, &[], 1, false),
        ];
        let p = assign_phases(&calls);
        assert_eq!(
            p,
            [
                Phase::Exploring,
                Phase::Exploring,
                Phase::Exploring,
                Phase::Implementing,
                Phase::Verifying,
                Phase::Verifying,
                Phase::Committing
            ]
        );
        assert_eq!(current(&calls), Some((Phase::Committing, 1)));
        assert_eq!(current(&calls[..3]), Some((Phase::Exploring, 3)));
        // A git read right after an edit is verifying; an unconfirmed test
        // command is not.
        let calls = vec![
            shape("Edit", ToolClass::Implement, &["a.rs"], 2, false),
            shape("Bash", ToolClass::GitRead, &[], 2, false),
            shape("Bash", ToolClass::Test, &[], 2, false),
        ];
        assert_eq!(
            assign_phases(&calls),
            [Phase::Implementing, Phase::Verifying, Phase::Other]
        );
        // Smoothing: one small read between two edits of the same turn.
        let calls = vec![
            shape("Edit", ToolClass::Implement, &["a.rs"], 3, false),
            shape("Read", ToolClass::Explore, &["b.rs"], 3, false),
            shape("Edit", ToolClass::Implement, &["b.rs"], 3, false),
        ];
        assert_eq!(current(&calls), Some((Phase::Implementing, 3)));
        assert_eq!(current(&[]), None);
        assert_eq!(Phase::Exploring.word(), "EXPLORING");
        assert_eq!(BashClass::BuildLint.label(), "build-lint");
    }
}

#[cfg(test)]
mod parity {
    //! Agreement with the Python reference over this machine's real commands
    //! (`CCTOP_PHASE_PARITY` = a JSONL of `{cmd, cls}` written by the
    //! reference). Ignored by default: the commands are private.
    use super::*;

    #[test]
    #[ignore]
    fn agrees_with_the_reference_classifier() {
        let Ok(path) = std::env::var("CCTOP_PHASE_PARITY") else {
            return;
        };
        let text = std::fs::read_to_string(path).unwrap();
        let mut total = 0;
        let mut agree = 0;
        let mut diffs: std::collections::BTreeMap<(String, String), usize> = Default::default();
        for line in text.lines() {
            let v: Value = serde_json::from_str(line).unwrap();
            let cmd = v["cmd"].as_str().unwrap();
            let expected = v["cls"].as_str().unwrap();
            let got = match classify_bash(cmd) {
                BashClass::Test | BashClass::BuildLint => "verify".to_string(),
                c => c.label().to_string(),
            };
            total += 1;
            if got == expected {
                agree += 1;
            } else {
                *diffs.entry((expected.to_string(), got)).or_default() += 1;
            }
        }
        eprintln!(
            "parity: {agree}/{total} = {:.1} %",
            100.0 * agree as f64 / total as f64
        );
        for ((e, g), n) in &diffs {
            eprintln!("  reference {e} → port {g}: {n}");
        }
        assert!(agree as f64 >= 0.99 * total as f64, "{agree}/{total}");
    }
}
