//! Outcome-and-rework rules (coach PRD §5.2): A32 verification gap, A33
//! commit without a check, A36 correction streak, A38 failure cascade
//! (replaces A15), A40 review before merge, A41 waiting on you, A42
//! natural-boundary checkpoint (replaces A18), A43 destructive git on a
//! dirty tree, A44 stale-context collision, A45 denial streak / allow
//! rule, A47 turn died; next-row only: A34 plan-first, A46 long-context
//! drift. Every trigger is structural — a tool result, a denial kind, an
//! interrupt marker, an API-error line, a git operation — never a keyword
//! in prompt text.

use crate::advisor::{ActionKind, Advice, Rule, Saving, Ttl, Urgency};
use crate::phase::{BashClass, ToolClass};
use crate::tools::Call;
use crate::transcript::DenialKind;
use crate::ui::fmt;
use crate::ui::state::{State, WaitingKind};

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(VerifyGap),
        Box::new(CommitUnchecked),
        Box::new(PlanFirst),
        Box::new(CorrectionStreak),
        Box::new(FailureCascade),
        Box::new(ReviewBeforeMerge),
        Box::new(WaitingOnYou),
        Box::new(Checkpoint),
        Box::new(DestructiveGit),
        Box::new(StaleCollision),
        Box::new(DenialStreak),
        Box::new(LongContextDrift),
        Box::new(TurnDied),
    ]
}

/// A path that is documentation or data, not source (PRD §5.2: `.md`,
/// `.txt`, `.json`, `.yaml`, `.toml`, `.csv`).
fn is_doc(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "md" | "markdown" | "txt" | "rst" | "adoc" | "json" | "yaml" | "yml" | "toml" | "csv"
    )
}

/// The command a shell line is about: the first `&&` segment that is not
/// a `cd`, cut at the first `;` or `|`.
fn command_core(cmd: &str) -> &str {
    let seg = cmd
        .split(" && ")
        .map(str::trim)
        .find(|s| !s.starts_with("cd ") && !s.is_empty())
        .unwrap_or(cmd.trim());
    seg.split([';', '|']).next().unwrap_or(seg).trim()
}

/// Source edits (Edit / Write / MultiEdit / NotebookEdit / implement-class
/// Bash) on non-doc paths after `since_ms`.
fn source_edits_since(state: &State, since_ms: i64) -> Vec<&Call> {
    state
        .tools
        .calls
        .iter()
        .filter(|c| c.class == ToolClass::Implement)
        .filter(|c| c.started_at.is_some_and(|s| s > since_ms))
        .filter(|c| c.paths.is_empty() || c.paths.iter().any(|p| !is_doc(p)))
        .collect()
}

/// The test command this session runs: the last confirmed check, else the
/// last test-class Bash; its first pipeline segment, clipped.
fn known_test(state: &State) -> Option<String> {
    let tests = || {
        state
            .tools
            .calls
            .iter()
            .rev()
            .filter(|c| c.name == "Bash" && c.bash_class == Some(BashClass::Test))
    };
    let c = tests()
        .find(|c| c.is_confirmed_test())
        .or_else(|| tests().next())?;
    let first = command_core(&c.input_summary);
    (!first.is_empty()).then(|| fmt::clip(first, 40))
}

/// A confirmed check that failed: the exit status, or the output's own
/// `FAILED` / `n failed` marker.
fn check_failed(c: &Call) -> bool {
    c.is_error || c.test_marker == crate::transcript::TestMarker::Failed
}

/// Calls of the current turn.
fn turn_calls(state: &State) -> Vec<&Call> {
    let turn = state.agg.current_turn().map(|t| t.number).unwrap_or(0);
    state
        .tools
        .calls
        .iter()
        .filter(|c| c.turn == turn)
        .collect()
}

/// The shape of a call: the tool name, or for Bash the command's argv0 and
/// its word-like subcommands (`gh pr view`, `git push`, `rm`).
fn prefix(c: &Call) -> String {
    if c.name != "Bash" {
        return c.name.clone();
    }
    let core = command_core(&c.input_summary);
    let mut words = core.split_whitespace();
    let mut out: Vec<&str> = words.next().into_iter().collect();
    out.extend(words.take(2).take_while(|w| {
        w.chars().all(|ch| ch.is_ascii_lowercase() || ch == '-') && !w.starts_with('-')
    }));
    out.join(" ")
}

/// A32 — source edits without a test run: nudged at the turn's end once
/// ten minutes or fourteen calls passed since the first unverified edit.
pub struct VerifyGap;
impl Rule for VerifyGap {
    fn id(&self) -> &'static str {
        "A32"
    }
    fn family(&self) -> &'static str {
        "verify-gap"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let test = known_test(state)?;
        let since = state.last_check().map(|(_, _, at)| at).unwrap_or(i64::MIN);
        let edits = source_edits_since(state, since);
        let first = edits.iter().filter_map(|c| c.started_at).min()?;
        // Never while the last call is test-class (a check is under way).
        if state
            .tools
            .calls
            .last()
            .is_some_and(|c| c.bash_class == Some(BashClass::Test))
        {
            return None;
        }
        let now = state.clock_ms();
        let calls_since = state
            .tools
            .calls
            .iter()
            .filter(|c| c.started_at.is_some_and(|s| s > first))
            .count();
        if now - first < 600_000 && calls_since < 14 {
            return None;
        }
        // At the turn's end — and through the next turn while the edits
        // stay unverified (the act is that turn's test run).
        let t = state.agg.current_turn()?;
        let ended = t.duration_ms.is_some() || t.interrupted_after_calls.is_some();
        let earlier_turn = edits.iter().any(|c| c.turn < t.number);
        if !ended && !earlier_turn {
            return None;
        }
        let files: std::collections::BTreeSet<&str> = edits
            .iter()
            .flat_map(|c| c.paths.iter().map(String::as_str))
            .filter(|p| !is_doc(p))
            .collect();
        let mut a = Advice::new("A32", "verify-gap", Urgency::Next);
        a.headline = format!(
            "Edited {} src file{}, no test/build run yet",
            files.len().max(1),
            if files.len() == 1 { "" } else { "s" }
        );
        a.evidence = format!(
            "{} edits since the last check · {} · {} calls",
            edits.len(),
            fmt::duration_ms(now - first),
            calls_since
        );
        a.action = format!("queue: 'run {test}, fix failures' or /goal");
        a.action_text = format!("Run {test} and fix any failures before continuing.");
        a.action_kind = ActionKind::Prompt;
        a.saving = Saving::Avoids;
        a.retires_on = "a test run or a read of an edited file";
        a.mark = state.tools.calls.len() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        let since = fired.mark as usize;
        let edited: Vec<&str> = source_edits_since(state, i64::MIN)
            .into_iter()
            .flat_map(|c| c.paths.iter().map(String::as_str))
            .collect();
        state.tools.calls.iter().skip(since).any(|c| {
            c.is_confirmed_test()
                || (c.name == "Read" && c.paths.iter().any(|p| edited.contains(&p.as_str())))
        })
    }
    fn ttl(&self) -> Ttl {
        Ttl::NextTurnEnd
    }
}

/// A33 — a commit landed with unchecked source edits since the last
/// commit; escalated when the latest test in that window failed.
pub struct CommitUnchecked;
impl Rule for CommitUnchecked {
    fn id(&self) -> &'static str {
        "A33"
    }
    fn family(&self) -> &'static str {
        "commit-unchecked"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let commits = &state.tools.commits;
        let (at, sha, turn) = commits.last()?;
        if state.agg.current_turn().map(|t| t.number) != Some(*turn) {
            return None; // once, in the turn it happened
        }
        let prev = commits
            .len()
            .checked_sub(2)
            .map(|i| commits[i].0)
            .unwrap_or(i64::MIN);
        // Confirmed checks before the commit: edits after the last passing
        // one are unchecked; a failed last check escalates.
        let checks: Vec<&Call> = state
            .tools
            .calls
            .iter()
            .filter(|c| c.is_confirmed_test())
            .filter(|c| c.started_at.is_some_and(|s| s > prev && s < *at))
            .collect();
        let verified_at = checks
            .iter()
            .filter(|c| !check_failed(c))
            .filter_map(|c| c.started_at)
            .max()
            .unwrap_or(prev);
        let edits: Vec<&Call> = source_edits_since(state, verified_at)
            .into_iter()
            .filter(|c| c.started_at.is_some_and(|s| s < *at))
            .collect();
        if edits.is_empty() {
            return None;
        }
        let failed = checks.last().is_some_and(|c| check_failed(c));
        let test = known_test(state)?;
        let mut a = Advice::new("A33", "commit-unchecked", Urgency::Now);
        a.headline = format!(
            "Committed {} with {} unchecked src edit{}{}",
            sha.chars().take(7).collect::<String>(),
            edits.len(),
            if edits.len() == 1 { "" } else { "s" },
            if failed { " (last test failed)" } else { "" }
        );
        a.evidence = format!(
            "{} confirmed test run{} between the commits",
            checks.len(),
            if checks.len() == 1 { "" } else { "s" }
        );
        a.action = format!("queue: 'run {test}; fixup before push'");
        a.action_text =
            format!("Run {test}; if anything fails, fix it and amend the commit before pushing.");
        a.action_kind = ActionKind::Prompt;
        a.saving = Saving::Avoids;
        a.retires_on = "the next commit";
        a.mark = commits.len() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state.tools.commits.len() as u64 > fired.mark
    }
    fn ttl(&self) -> Ttl {
        Ttl::NextPrompt
    }
}

/// A34 — a feature-sized prompt with plan mode off and no plan yet: the
/// `next` row only.
pub struct PlanFirst;
impl Rule for PlanFirst {
    fn id(&self) -> &'static str {
        "A34"
    }
    fn family(&self) -> &'static str {
        "plan-first"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let t = state.agg.current_turn().filter(|t| t.human)?;
        let s = t.prompt_shape;
        let feature = s.chars >= 300 || (s.path_tokens >= 2 && s.implementation_verb);
        if !feature || s.markdown_structured || s.spec_like {
            return None;
        }
        if state.session.permission_mode.as_deref() == Some("plan") {
            return None;
        }
        let planned = state.tools.calls.iter().any(|c| {
            matches!(
                c.name.as_str(),
                "AskUserQuestion" | "EnterPlanMode" | "ExitPlanMode"
            )
        });
        let edited = state
            .tools
            .calls
            .iter()
            .any(|c| c.class == ToolClass::Implement);
        if planned || edited {
            return None;
        }
        let mut a = Advice::new("A34", "plan-first", Urgency::Next);
        a.headline = format!(
            "Feature-sized prompt ({} file{}, {} chars), plan off",
            s.path_tokens,
            if s.path_tokens == 1 { "" } else { "s" },
            s.chars
        );
        a.evidence = "no plan artefact this session and no edit yet".into();
        a.action = "Esc · /plan (Shift+Tab) · resend — or say 'plan first'".into();
        a.action_text =
            "Plan first: list the steps and the files you will touch, then wait for my go.".into();
        a.action_kind = ActionKind::Prompt;
        a.saving = Saving::Avoids;
        a.next_row_only = true;
        a.retires_on = "an AskUserQuestion, a plan mode entry or an edit";
        Some(a)
    }
}

/// A36 — two corrections within four human turns: interrupts, refused
/// calls, feedback on a call.
pub struct CorrectionStreak;
impl Rule for CorrectionStreak {
    fn id(&self) -> &'static str {
        "A36"
    }
    fn family(&self) -> &'static str {
        "correction-streak"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        // The last four human turns, oldest first.
        let mut turns: Vec<&crate::metrics::Turn> = state
            .agg
            .turns
            .iter()
            .filter(|t| t.human)
            .rev()
            .take(4)
            .collect();
        turns.reverse();
        let current = *turns.last()?;
        let corrected: Vec<&crate::metrics::Turn> = turns
            .iter()
            .copied()
            .filter(|t| t.interrupted_after_calls.is_some() || t.rejections > 0)
            .collect();
        let last = *corrected.last()?;
        // The streak is live while the last correction is in this turn, or
        // this turn is the short steer typed right after an Esc.
        let steer_after_esc = last.interrupted_after_calls.is_some()
            && current.number == last.number + 1
            && current.prompt_shape.chars < 200
            && current.tool_calls <= 10
            && current.last_human_input_at == current.started_at;
        if last.number != current.number && !steer_after_esc {
            return None;
        }
        let count =
            |t: &crate::metrics::Turn| t.interrupted_after_calls.is_some() as usize + t.rejections;
        let corrections: usize = corrected.iter().map(|t| count(t)).sum();
        if corrections < 2 {
            return None;
        }
        // Two corrections on the same work: the same turn, consecutive
        // turns, or overlapping edited files.
        let pair_ok = match corrected.len() {
            1 => true,
            _ => {
                let a = corrected[corrected.len() - 2];
                let b = last;
                turns.iter().position(|t| t.number == b.number)
                    == turns
                        .iter()
                        .position(|t| t.number == a.number)
                        .map(|p| p + 1)
                    || a.files_edited.iter().any(|f| b.files_edited.contains(f))
            }
        };
        if !pair_ok {
            return None;
        }
        // In time order: a refusal lands on a tool result, an Esc ends the turn.
        let mut kinds: Vec<&str> = Vec::new();
        for t in &corrected {
            kinds.extend(std::iter::repeat_n("refusal", t.rejections));
            if t.interrupted_after_calls.is_some() {
                kinds.push("Esc");
            }
        }
        let (checkpoints, bash_writes) = state.rewind_points();
        let first_turn = corrected[0].number;
        let mut a = Advice::new("A36", "correction-streak", Urgency::Now);
        a.headline = format!(
            "{corrections} corrections in a row ({}, then {})",
            kinds[0],
            kinds[1..].join(", ")
        );
        a.evidence = format!(
            "rewind: {checkpoints} checkpoint{} since #{first_turn}{}",
            if checkpoints == 1 { "" } else { "s" },
            if bash_writes > 0 {
                " · bash files not covered"
            } else {
                ""
            }
        );
        a.action = format!("Esc Esc → restore turn {first_turn}, restate with the constraint");
        a.action_kind = ActionKind::Key;
        a.action_text = "Esc Esc".into();
        a.saving = Saving::Avoids;
        a.retires_on = "a /rewind";
        a.mark = state.clock_ms() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state
            .history
            .last("/rewind")
            .is_some_and(|r| r.at_ms > fired.mark as i64)
    }
    fn cooldown_turns(&self) -> usize {
        10
    }
}

/// A38 — three consecutive failed calls (denials excluded) in the last
/// ten of the turn, two sharing a command prefix, no edit between.
pub struct FailureCascade;
impl Rule for FailureCascade {
    fn id(&self) -> &'static str {
        "A38"
    }
    fn family(&self) -> &'static str {
        "failure-cascade"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        if !crate::coach::turn_running(state) {
            return None; // phase gate: a running turn only
        }
        let calls = turn_calls(state);
        let finished: Vec<&Call> = calls
            .iter()
            .copied()
            .filter(|c| c.finished_at.is_some() && c.denial.is_none())
            .collect();
        let n = finished.len();
        let last10 = &finished[n.saturating_sub(10)..];
        // The trailing run of failures.
        let run: Vec<&Call> = last10
            .iter()
            .rev()
            .take_while(|c| c.is_error)
            .copied()
            .collect();
        if run.len() < 3 {
            return None;
        }
        if run.iter().any(|c| c.class == ToolClass::Implement) {
            return None;
        }
        let mut by_prefix: std::collections::BTreeMap<String, usize> = Default::default();
        for c in &run {
            *by_prefix.entry(prefix(c)).or_default() += 1;
        }
        if !by_prefix.values().any(|n| *n >= 2) {
            return None;
        }
        let parts: Vec<String> = by_prefix
            .iter()
            .map(|(p, n)| {
                if *n > 1 {
                    format!("{p} ×{n}")
                } else {
                    p.clone()
                }
            })
            .collect();
        let mut a = Advice::new("A38", "failure-cascade", Urgency::Now);
        a.headline = format!("{} fails in a row: {}", run.len(), parts.join(", "));
        let classes: Vec<String> = state
            .tools
            .errors_by_class()
            .into_iter()
            .take(2)
            .map(|(k, n)| format!("{} {n}", k.label()))
            .collect();
        a.evidence = format!("this turn · {}", classes.join(" · "));
        a.action = "Esc, give the missing fact — or run it, paste 20 lines".into();
        a.action_kind = ActionKind::Key;
        a.action_text = "Esc".into();
        a.saving = Saving::Avoids;
        a.retires_on = "a successful call of the failing command";
        a.mark = state.tools.calls.len() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        let failing: Vec<String> = turn_calls(state)
            .iter()
            .filter(|c| c.is_error)
            .map(|c| prefix(c))
            .collect();
        state
            .tools
            .calls
            .iter()
            .skip(fired.mark as usize)
            .any(|c| c.finished_at.is_some() && !c.is_error && failing.contains(&prefix(c)))
    }
}

/// A40 — a PR is open (created, pushed) with a substantial diff and no
/// review pass; once per PR.
pub struct ReviewBeforeMerge;
impl Rule for ReviewBeforeMerge {
    fn id(&self) -> &'static str {
        "A40"
    }
    fn family(&self) -> &'static str {
        "review-before-merge"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let pr = state.status_facts.pr_number.or(state.agg.pr_number)?;
        if state
            .status_facts
            .pr_review_state
            .as_deref()
            .is_some_and(|s| matches!(s, "approved" | "merged" | "changes_requested"))
        {
            return None;
        }
        let opened = state.tools.git_events.iter().any(|(_, what)| {
            what.starts_with("push") || what.contains("created") || what.contains("ready")
        });
        if !opened {
            return None;
        }
        if state
            .tools
            .git_events
            .iter()
            .any(|(_, what)| what.contains("merged"))
        {
            return None;
        }
        let reviewed = state
            .prefix
            .invoked_skills
            .iter()
            .any(|s| s.contains("review") || s.contains("simplify"))
            || state
                .agents
                .values()
                .any(|a| a.agent_type.to_lowercase().contains("review"))
            || state.agg.attribution.keys().any(|k| k.contains("review"));
        if reviewed {
            return None;
        }
        let (added, removed) = state.files.total_lines();
        let doc_lines: u64 = state
            .files
            .files
            .values()
            .filter(|f| is_doc(&f.path))
            .map(|f| f.lines_added.unwrap_or(0) + f.lines_removed.unwrap_or(0))
            .sum();
        if (added + removed).saturating_sub(doc_lines) < 100 {
            return None;
        }
        let files = state
            .files
            .files
            .values()
            .filter(|f| f.lines_added.is_some())
            .count();
        let mut a = Advice::new("A40", "review-before-merge", Urgency::Next);
        a.headline = format!("PR #{pr} open: +{added}/−{removed} in {files} files, no review pass");
        a.evidence = "no code-review skill, review agent or review attribution this session".into();
        a.action = "run /code-review before gh pr merge (fresh context)".into();
        a.action_text = "/code-review".into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::Avoids;
        a.retires_on = "a review marker or the merge";
        a.mark = pr;
        Some(a)
    }
    fn ttl(&self) -> Ttl {
        Ttl::ThreeTurns
    }
    fn acted(&self, state: &State, _fired: &Advice) -> bool {
        state
            .prefix
            .invoked_skills
            .iter()
            .any(|s| s.contains("review"))
            || state
                .tools
                .git_events
                .iter()
                .any(|(_, what)| what.contains("merged"))
    }
    fn cooldown_turns(&self) -> usize {
        usize::MAX / 2
    }
}

/// A41 — Claude waits on you: a question, a permission dialog, a
/// notification, or a `?`-ended turn with nothing back for 30 s.
pub struct WaitingOnYou;
impl Rule for WaitingOnYou {
    fn id(&self) -> &'static str {
        "A41"
    }
    fn family(&self) -> &'static str {
        "waiting"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let w = state.waiting()?;
        let now = state.clock_ms();
        let since = now - w.since_ms;
        if since < 30_000 {
            return None;
        }
        let what = match w.kind {
            WaitingKind::Question => "Claude asked a question",
            WaitingKind::Permission => "a permission dialog is open",
            WaitingKind::Notification => "Claude needs your input",
            WaitingKind::Asked => "the turn ended with a question",
        };
        let mut a = Advice::new("A41", "waiting", Urgency::Now);
        a.headline = format!("◆ WAITING {} · {what}", crate::coach::short_duration(since));
        let cache = state
            .cache_clock()
            .filter(|c| c.remaining_ms > 0 && c.remaining_ms < 600_000);
        a.action = match cache {
            Some(c) => format!(
                "reply now · cache warm {} · cold restart ≈{}",
                crate::coach::short_duration(c.remaining_ms),
                fmt::tokens(state.context().size)
            ),
            None => "reply now · a one-word answer unblocks the task".into(),
        };
        a.evidence = if w.kind == WaitingKind::Permission {
            "approve, deny, or 'always' for the shape".into()
        } else {
            "or /btw <question> if you need a detail first".into()
        };
        a.saving = Saving::Seconds((since / 1000) as u64);
        a.retires_on = "your reply";
        Some(a)
    }
    fn acted(&self, state: &State, _fired: &Advice) -> bool {
        state.waiting().is_none()
    }
    fn ttl(&self) -> Ttl {
        Ttl::NextPrompt
    }
    fn cooldown_turns(&self) -> usize {
        0
    }
}

/// A42 — a natural boundary (a task completed, a done/next prompt, a
/// question-ended turn) on a large context: the moment to hand off.
pub struct Checkpoint;
impl Rule for Checkpoint {
    fn id(&self) -> &'static str {
        "A42"
    }
    fn family(&self) -> &'static str {
        "context-reset"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let ctx = state.context().size;
        if ctx < 200_000 {
            return None;
        }
        let t = state.agg.current_turn()?;
        if !state.session.background_tasks.is_empty()
            || state.session.session_crons > 0
            || t.pending_background_agents.unwrap_or(0) > 0
        {
            return None;
        }
        let task_done = state
            .tools
            .task_completions
            .iter()
            .any(|(_, turn)| *turn == t.number);
        let question_ended = t.duration_ms.is_some()
            && t.ended_with_question
            && state
                .tools
                .running()
                .is_none_or(|c| c.name != "AskUserQuestion");
        let done_prompt = t.human && t.prompt_shape.done_or_next;
        if !(task_done || question_ended || done_prompt) {
            return None;
        }
        let why = if task_done {
            "task completed"
        } else if done_prompt {
            "you said done / next"
        } else {
            "the turn ended with a question"
        };
        let fresh = state.context().prefix.max(20_000);
        let mut a = Advice::new("A42", "context-reset", Urgency::Next);
        a.headline = format!(
            "checkpoint · ctx {} · next task unrelated?",
            fmt::tokens(ctx)
        );
        a.evidence = format!(
            "{why} · a fresh session starts at ≈{} ({}× less per call)",
            fmt::tokens(fresh),
            (ctx / fresh).max(1)
        );
        a.action = "hand-off note → /rename → /clear (fresh ≈ the prefix)".into();
        a.action_text = "/clear".into();
        a.action_kind = ActionKind::Slash;
        a.saving = Saving::Tokens(ctx.saturating_sub(fresh));
        a.retires_on = "a /clear";
        a.mark = state
            .agg
            .boundaries
            .iter()
            .filter(|b| b.kind == crate::metrics::usage::BoundaryKind::Clear)
            .count() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state
            .agg
            .boundaries
            .iter()
            .filter(|b| b.kind == crate::metrics::usage::BoundaryKind::Clear)
            .count() as u64
            > fired.mark
    }
}

/// A43 — a destructive git command ran on a dirty tree this turn.
pub struct DestructiveGit;
impl Rule for DestructiveGit {
    fn id(&self) -> &'static str {
        "A43"
    }
    fn family(&self) -> &'static str {
        "destructive-git"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        if !state.session.git_dirty || !crate::coach::turn_running(state) {
            return None;
        }
        let calls = turn_calls(state);
        let (i, c, what) = calls.iter().enumerate().rev().find_map(|(i, c)| {
            (c.name == "Bash")
                .then(|| crate::phase::destructive_git(&c.input_summary).map(|w| (i, *c, w)))
                .flatten()
        })?;
        let mut a = Advice::new("A43", "destructive-git", Urgency::Now);
        a.headline = format!("Claude ran {what} (#{}) on a dirty tree", i + 1);
        a.evidence = format!(
            "`{}` · rewind covers Edit/Write only",
            fmt::clip(&c.input_summary, 30)
        );
        a.action = "rewind can't undo bash: check git reflog / stash list".into();
        a.action_kind = ActionKind::Advice;
        a.saving = Saving::Avoids;
        a.retires_on = "turn end";
        Some(a)
    }
}

/// A44 — the IDE and Claude edited the same file this turn.
pub struct StaleCollision;
impl Rule for StaleCollision {
    fn id(&self) -> &'static str {
        "A44"
    }
    fn family(&self) -> &'static str {
        "stale-collision"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let t = state.agg.current_turn()?;
        let f = state
            .files
            .files
            .values()
            .filter(|f| f.stale && f.edits_this_turn > 0 && f.ide_edits > 0)
            // Docs the person edits deliberately (two thirds of IDE edits).
            .filter(|f| !is_doc(&f.path))
            .filter(|f| {
                let base = f.path.rsplit('/').next().unwrap_or(&f.path);
                t.files_edited.iter().any(|e| e == base)
            })
            // Fresh: Claude has made at most two calls since the IDE edit.
            .find(|f| {
                state
                    .tools
                    .calls
                    .iter()
                    .filter(|c| c.started_at.is_some_and(|s| s > f.ide_edited_at_ms))
                    .count()
                    <= 2
            })?;
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        let mut a = Advice::new("A44", "stale-collision", Urgency::Now);
        a.headline = format!("IDE + Claude both edited {name} this turn");
        a.evidence = format!(
            "{} IDE edit(s), {} by Claude",
            f.ide_edits, f.edits_this_turn
        );
        a.action = "say which version wins before the next edit".into();
        a.action_text = format!("The file {name} was also changed in my editor — re-read it and keep my version where they differ.");
        a.action_kind = ActionKind::Prompt;
        a.saving = Saving::Avoids;
        a.retires_on = "a re-read or a further edit of the file";
        a.mark = state.tools.calls.len() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        let name = fired
            .headline
            .strip_prefix("IDE + Claude both edited ")
            .and_then(|s| s.split(' ').next())
            .unwrap_or("");
        state.tools.calls.iter().skip(fired.mark as usize).any(|c| {
            matches!(
                c.name.as_str(),
                "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
            ) && c.paths.iter().any(|p| p.rsplit('/').next() == Some(name))
        })
    }
}

/// A45 — auto mode blocked the same command shape twice (or a settings
/// rule denied it once): the allow rule, or the alternative.
pub struct DenialStreak;
impl Rule for DenialStreak {
    fn id(&self) -> &'static str {
        "A45"
    }
    fn family(&self) -> &'static str {
        "denial-streak"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        if !crate::coach::turn_running(state) {
            return None; // phase gate: a running turn only
        }
        let turn = state.agg.current_turn().map(|t| t.number).unwrap_or(0);
        let recent: Vec<&Call> = state
            .tools
            .calls
            .iter()
            .filter(|c| c.turn + 3 > turn && c.denial.is_some())
            .filter(|c| c.denial != Some(DenialKind::UserRejected))
            .collect();
        // A settings deny rule: once, on the first occurrence.
        if let Some(c) = recent
            .iter()
            .rev()
            .find(|c| c.denial == Some(DenialKind::PermissionRule) && c.turn == turn)
        {
            let mut a = Advice::new("A45", "denial-streak", Urgency::Now);
            a.headline = format!("{} is denied by your rules", fmt::clip(&prefix(c), 30));
            a.evidence = "a permissions.deny rule in settings".into();
            a.action = "tell Claude the alternative — it cannot run this".into();
            a.action_text = format!(
                "You can't run `{}` here (it's denied by my settings). Use another way or tell me what you need.",
                prefix(c)
            );
            a.action_kind = ActionKind::Prompt;
            a.saving = Saving::Avoids;
            a.retires_on = "turn end";
            a.mark = state.tools.calls.len() as u64;
            return Some(a);
        }
        let blocked: Vec<&Call> = recent
            .iter()
            .copied()
            .filter(|c| {
                matches!(
                    c.denial,
                    Some(DenialKind::AutoModeBlocked | DenialKind::AutoModeUnavailable)
                )
            })
            .collect();
        let mut by_prefix: std::collections::BTreeMap<String, usize> = Default::default();
        for c in &blocked {
            *by_prefix.entry(prefix(c)).or_default() += 1;
        }
        let (p, n) = by_prefix
            .into_iter()
            .filter(|(_, n)| *n >= 2)
            .max_by_key(|(_, n)| *n)?;
        let last = blocked.iter().rev().find(|c| prefix(c) == p)?;
        // The rule: Claude Code's own suggestion when the spool carried it,
        // else the prefix; never bare `Bash`, never a rule already allowed.
        let key = crate::ui::state::permission_key(
            &last.name,
            (last.name == "Bash").then(|| command_core(&last.input_summary)),
        );
        let rule = state
            .session
            .permission_asks
            .get(&key)
            .and_then(|a| a.rule.clone())
            .filter(|r| r.contains('('))
            .unwrap_or_else(|| {
                if key.starts_with("Bash(") {
                    format!("{}:*)", key.trim_end_matches(')'))
                } else {
                    key.clone()
                }
            });
        if rule == "Bash" || state.allow_rules.contains(&rule) {
            return None;
        }
        let this_turn = blocked.iter().filter(|c| c.turn == turn).count();
        let mut a = Advice::new("A45", "denial-streak", Urgency::Now);
        a.headline = format!(
            "Auto mode blocked {p} ×{n}{} (3 in a row pauses it)",
            if this_turn >= 2 { " this turn" } else { "" }
        );
        a.evidence = format!("{} denials in the last 3 turns", recent.len());
        a.action = format!("allow {rule} or approve it");
        a.action_text = rule;
        a.action_kind = ActionKind::AllowRule;
        a.saving = Saving::Avoids;
        a.retires_on = "a successful call of the shape or the rule in permissions.allow";
        a.mark = state.tools.calls.len() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        if fired.action_kind != ActionKind::AllowRule {
            return false; // the deny-rule notice retires at turn end
        }
        // `Bash(gh api:*)` → the shape `gh api`; a plain tool rule → its name.
        let shape = fired
            .action_text
            .strip_prefix("Bash(")
            .map(|s| s.trim_end_matches(":*)").trim_end_matches(')').to_string())
            .unwrap_or_else(|| fired.action_text.clone());
        state.allow_rules.contains(&fired.action_text)
            || state.tools.calls.iter().skip(fired.mark as usize).any(|c| {
                c.finished_at.is_some()
                    && !c.is_error
                    && c.denial.is_none()
                    && (if c.name == "Bash" {
                        prefix(c) == shape || command_core(&c.input_summary).starts_with(&shape)
                    } else {
                        c.name == shape
                    })
            })
    }
}

/// A46 — the context crossed 150 k / 300 k / 450 k with the last long
/// prompt far behind and no instructions re-injected since: the `next`
/// row only.
pub struct LongContextDrift;
impl Rule for LongContextDrift {
    fn id(&self) -> &'static str {
        "A46"
    }
    fn family(&self) -> &'static str {
        "instruction-drift"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Next
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let v = state.context();
        if v.window < 900_000 {
            return None;
        }
        let step = [450_000u64, 300_000, 150_000]
            .into_iter()
            .find(|s| v.size >= *s)?;
        // The last long typed prompt (≥ ~150 tokens) and where the context was.
        let long = state
            .agg
            .turns
            .iter()
            .rev()
            .find(|t| t.human && t.prompt_shape.chars >= 600)?;
        if v.size.saturating_sub(long.context_size) < 100_000 {
            return None;
        }
        if state
            .agg
            .turns
            .iter()
            .filter(|t| t.number >= long.number)
            .any(|t| t.instructions_seen)
        {
            return None;
        }
        let mut a = Advice::new("A46", "instruction-drift", Urgency::Next);
        a.headline = format!(
            "Ctx {}: your turn-{} constraints are {} tokens back",
            fmt::tokens(v.size),
            long.number,
            fmt::tokens(v.size.saturating_sub(long.context_size))
        );
        a.evidence = format!("crossed {} · nothing re-injected since", fmt::tokens(step));
        a.action = "Restate the 3 that matter, or add them to CLAUDE.md".into();
        a.action_text =
            "Before continuing, restate the constraints I gave earlier and keep to them.".into();
        a.action_kind = ActionKind::Prompt;
        a.saving = Saving::Avoids;
        a.next_row_only = true;
        a.retires_on = "a restated prompt or an instructions attachment";
        Some(a)
    }
}

/// A47 — the turn died on an API error line.
pub struct TurnDied;
impl Rule for TurnDied {
    fn id(&self) -> &'static str {
        "A47"
    }
    fn family(&self) -> &'static str {
        "turn-died"
    }
    fn urgency(&self) -> Urgency {
        Urgency::Now
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let e = state.agg.api_errors.iter().rev().find(|e| e.confirmed)?;
        let at =
            e.at.as_deref()
                .and_then(crate::metrics::cost::parse_ts_ms)?;
        if state.last_api_call_ms().is_some_and(|c| c > at) {
            return None;
        }
        let now = state.clock_ms();
        let err = e.error.as_deref().unwrap_or("");
        let status = e.status.unwrap_or(0);
        let mut a = Advice::new("A47", "turn-died", Urgency::Now);
        let (headline, action, text) = if err == "rate_limit" || status == 429 {
            let kind = e.rate_limit_type.as_deref().unwrap_or("rate limit");
            let kind_word = match kind {
                "five_hour" => "session",
                "seven_day" => "weekly",
                k => k,
            };
            let resets = e.resets_at.map(|s| (s * 1000.0) as i64);
            (
                match resets {
                    Some(r) => format!(
                        "Rate limit ({kind_word}) · resets {} (in {})",
                        fmt::clock_hhmm(r),
                        crate::coach::short_duration(r - now)
                    ),
                    None => format!("Rate limit ({kind_word})"),
                },
                "wait, or /model <other family> to keep working".to_string(),
                "/model".to_string(),
            )
        } else if err.contains("spend") || status == 402 {
            (
                "Monthly spend limit hit · nothing will run".into(),
                "raise the limit, switch account, or stop".into(),
                String::new(),
            )
        } else if err.contains("auth") || status == 401 {
            (
                "Authentication failed · nothing will run".into(),
                "/login, or check the API key".into(),
                "/login".into(),
            )
        } else if err.contains("prompt") && err.contains("long") || status == 413 {
            (
                "Prompt too long: the context exceeds the window".into(),
                "/compact or /clear before the next message".into(),
                "/compact".into(),
            )
        } else {
            (
                format!(
                    "API error{}: {}",
                    if status > 0 {
                        format!(" {status}")
                    } else {
                        String::new()
                    },
                    if err.is_empty() {
                        "request failed"
                    } else {
                        err
                    }
                ),
                "Claude Code retries by itself; wait a minute, then resend".into(),
                String::new(),
            )
        };
        a.headline = headline;
        a.evidence = format!(
            "turn {} · {}",
            e.turn,
            e.low_priority_retry_after_s
                .map(|s| format!("low-priority retry in {s} s"))
                .unwrap_or_else(|| "no successful call since".into())
        );
        a.action = action;
        a.action_kind = if text.is_empty() {
            ActionKind::Advice
        } else {
            ActionKind::Slash
        };
        a.action_text = text;
        a.saving = Saving::Avoids;
        a.retires_on = "the next successful API call";
        a.mark = state.agg.api_calls() as u64;
        Some(a)
    }
    fn acted(&self, state: &State, fired: &Advice) -> bool {
        state.agg.api_calls() as u64 > fired.mark
    }
    fn ttl(&self) -> Ttl {
        Ttl::NextPrompt
    }
    fn cooldown_turns(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{prompt, response};
    use super::*;
    use crate::advisor::Engine;
    use crate::metrics::Pricing;
    use crate::transcript::Line;

    fn t(ts: &str) -> i64 {
        crate::metrics::cost::parse_ts_ms(ts).unwrap()
    }

    /// A tool call and its result: `extra` is spliced into the result's
    /// top-level object (`"is_error":true` goes into the block).
    fn call(s: &mut State, id: &str, ts: &str, name: &str, input: &str, result: &str, extra: &str) {
        call_ctx(s, id, ts, name, input, result, extra, 50_000);
    }

    /// [`call`] on a context of `ctx` tokens.
    #[allow(clippy::too_many_arguments)]
    fn call_ctx(
        s: &mut State,
        id: &str,
        ts: &str,
        name: &str,
        input: &str,
        result: &str,
        extra: &str,
        ctx: u64,
    ) {
        s.apply(&Line::parse(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}],"usage":{{"input_tokens":2,"cache_read_input_tokens":{ctx},"output_tokens":10}}}}}}"#
        )).unwrap());
        s.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{result}"{}}}]}}{extra}}}"#,
            if extra.contains("\"is_error\"") { r#","is_error":true"# } else { "" }
        )).unwrap());
    }

    fn edit(s: &mut State, id: &str, ts: &str, path: &str) {
        call(
            s,
            id,
            ts,
            "Edit",
            &format!(r#"{{"file_path":"{path}","old_string":"a","new_string":"b"}}"#),
            "ok",
            "",
        );
    }

    fn bash(s: &mut State, id: &str, ts: &str, cmd: &str, stdout: &str, extra: &str) {
        call(
            s,
            id,
            ts,
            "Bash",
            &format!(r#"{{"command":"{cmd}"}}"#),
            stdout,
            &format!(
                r#","toolUseResult":{{"stdout":"{stdout}","stderr":"","interrupted":false{extra}}}"#
            ),
        );
    }

    fn turn_end(s: &mut State, ts: &str) {
        s.apply(&Line::parse(&format!(
            r#"{{"type":"system","subtype":"turn_duration","timestamp":"{ts}","durationMs":60000}}"#
        )).unwrap());
    }

    fn interrupt(s: &mut State, ts: &str) {
        s.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"text","text":"[Request interrupted by user for tool use]"}}]}}}}"#
        )).unwrap());
    }

    fn fresh() -> State {
        let mut s = State::new(Pricing::bundled());
        s.session.alive = true;
        s.apply(&prompt("2026-01-01T10:00:00Z"));
        s.apply(&response("m0", "2026-01-01T10:00:01Z", 2, 50_000, 0));
        s.now_ms = t("2026-01-01T10:00:02Z");
        s
    }

    #[test]
    fn a32_verify_gap_needs_a_known_test_src_edits_and_the_turn_end() {
        let mut s = fresh();
        edit(&mut s, "e1", "2026-01-01T10:01:00Z", "/p/src/lib.rs");
        edit(&mut s, "e2", "2026-01-01T10:02:00Z", "/p/src/main.rs");
        s.now_ms = t("2026-01-01T10:20:00Z");
        turn_end(&mut s, "2026-01-01T10:20:00Z");
        assert!(VerifyGap.evaluate(&s).is_none(), "no test command is known");
        // A session that has run tests: the gap is real.
        let mut s = fresh();
        bash(
            &mut s,
            "t0",
            "2026-01-01T10:00:30Z",
            "cargo test",
            "test result: ok. 3 passed; 0 failed",
            "",
        );
        edit(&mut s, "e1", "2026-01-01T10:01:00Z", "/p/src/lib.rs");
        edit(&mut s, "e2", "2026-01-01T10:02:00Z", "/p/src/main.rs");
        edit(&mut s, "e3", "2026-01-01T10:03:00Z", "/p/README.md");
        s.now_ms = t("2026-01-01T10:05:00Z");
        assert!(
            VerifyGap.evaluate(&s).is_none(),
            "4 minutes, 2 calls: not yet, and the turn runs"
        );
        s.now_ms = t("2026-01-01T10:12:00Z");
        assert!(
            VerifyGap.evaluate(&s).is_none(),
            "12 minutes but the turn still runs"
        );
        turn_end(&mut s, "2026-01-01T10:12:00Z");
        let a = VerifyGap.evaluate(&s).expect("fires at the turn end");
        assert_eq!(a.headline, "Edited 2 src files, no test/build run yet");
        assert_eq!(a.action, "queue: 'run cargo test, fix failures' or /goal");
        assert_eq!(
            a.action_text,
            "Run cargo test and fix any failures before continuing."
        );
        assert_eq!(a.urgency, Urgency::Next);
        assert!(!VerifyGap.acted(&s, &a));
        assert_eq!(
            VerifyGap.ttl(),
            Ttl::NextTurnEnd,
            "the act is the next turn's run"
        );
        // The next prompt: the edits are still unverified, the nudge holds.
        s.apply(&prompt("2026-01-01T10:12:30Z"));
        assert!(
            VerifyGap.evaluate(&s).is_some(),
            "edits from an earlier turn"
        );
        // A confirmed test run — passed or failed — is the check and the act.
        bash(
            &mut s,
            "t1",
            "2026-01-01T10:13:00Z",
            "cargo test",
            "test result: FAILED. 2 passed; 1 failed",
            "",
        );
        s.now_ms = t("2026-01-01T10:14:00Z");
        assert!(VerifyGap.acted(&s, &a), "the run after the mark");
        assert!(VerifyGap.evaluate(&s).is_none(), "nothing unverified");
        // Doc-only edits never fire.
        let mut docs = fresh();
        bash(
            &mut docs,
            "t0",
            "2026-01-01T10:00:30Z",
            "cargo test",
            "test result: ok. 3 passed; 0 failed",
            "",
        );
        edit(&mut docs, "e1", "2026-01-01T10:01:00Z", "/p/README.md");
        edit(&mut docs, "e2", "2026-01-01T10:01:30Z", "/p/Cargo.toml");
        docs.now_ms = t("2026-01-01T10:12:00Z");
        turn_end(&mut docs, "2026-01-01T10:12:00Z");
        assert!(VerifyGap.evaluate(&docs).is_none());
    }

    #[test]
    fn a33_commit_unchecked_windows_by_the_last_commit_and_escalates() {
        let commit = |s: &mut State, id: &str, ts: &str, sha: &str| {
            bash(
                s,
                id,
                ts,
                "git commit -m x",
                "1 file changed",
                &format!(
                    r#","gitOperation":{{"commit":{{"sha":"{sha}","kind":"committed","branch":"main"}}}}"#
                ),
            )
        };
        let mut s = fresh();
        bash(
            &mut s,
            "t0",
            "2026-01-01T10:00:30Z",
            "npm test",
            "12 passed in 0.4s",
            "",
        );
        edit(&mut s, "e1", "2026-01-01T10:01:00Z", "/p/src/a.ts");
        edit(&mut s, "e2", "2026-01-01T10:02:00Z", "/p/src/b.ts");
        commit(&mut s, "c1", "2026-01-01T10:03:00Z", "a1b2c3d4e");
        s.now_ms = t("2026-01-01T10:03:01Z");
        let a = CommitUnchecked
            .evaluate(&s)
            .expect("fires on the commit result");
        assert_eq!(a.headline, "Committed a1b2c3d with 2 unchecked src edits");
        assert_eq!(a.action, "queue: 'run npm test; fixup before push'");
        assert_eq!(a.urgency, Urgency::Now);
        assert_eq!(CommitUnchecked.ttl(), Ttl::NextPrompt);
        assert!(!CommitUnchecked.acted(&s, &a));
        // The next commit, checked in between: nothing; and it retires the first.
        edit(&mut s, "e3", "2026-01-01T10:04:00Z", "/p/src/c.ts");
        bash(
            &mut s,
            "t1",
            "2026-01-01T10:05:00Z",
            "npm test",
            "13 passed in 0.4s",
            "",
        );
        commit(&mut s, "c2", "2026-01-01T10:06:00Z", "b2c3d4e5f");
        assert!(
            CommitUnchecked.evaluate(&s).is_none(),
            "checked since the last commit"
        );
        assert!(CommitUnchecked.acted(&s, &a));
        // A failed test in the window escalates.
        edit(&mut s, "e4", "2026-01-01T10:07:00Z", "/p/src/d.ts");
        bash(
            &mut s,
            "t2",
            "2026-01-01T10:08:00Z",
            "npm test",
            "1 failed, 12 passed in 0.4s",
            "",
        );
        commit(&mut s, "c3", "2026-01-01T10:09:00Z", "c3d4e5f6a");
        let a = CommitUnchecked.evaluate(&s).expect("fires");
        assert_eq!(
            a.headline,
            "Committed c3d4e5f with 1 unchecked src edit (last test failed)"
        );
        // Doc-only edits, or no known test: quiet.
        let mut docs = fresh();
        bash(
            &mut docs,
            "t0",
            "2026-01-01T10:00:30Z",
            "npm test",
            "12 passed in 0.4s",
            "",
        );
        edit(&mut docs, "e1", "2026-01-01T10:01:00Z", "/p/docs/x.md");
        commit(&mut docs, "c1", "2026-01-01T10:03:00Z", "a1b2c3d4e");
        assert!(CommitUnchecked.evaluate(&docs).is_none());
        let mut untested = fresh();
        edit(&mut untested, "e1", "2026-01-01T10:01:00Z", "/p/src/a.ts");
        commit(&mut untested, "c1", "2026-01-01T10:03:00Z", "a1b2c3d4e");
        assert!(CommitUnchecked.evaluate(&untested).is_none());
    }

    #[test]
    fn a34_plan_first_is_next_row_only_and_retires_on_a_plan_artefact() {
        let feature = |text: &str| -> State {
            let mut s = State::new(Pricing::bundled());
            s.session.alive = true;
            s.apply(&Line::parse(&format!(
                r#"{{"type":"user","timestamp":"2026-01-01T10:00:00Z","promptSource":"typed","message":{{"role":"user","content":"{text}"}}}}"#
            )).unwrap());
            s.apply(&response("m0", "2026-01-01T10:00:01Z", 2, 50_000, 0));
            s.now_ms = t("2026-01-01T10:00:02Z");
            s
        };
        let s = feature("implement the parser in src/parse.rs and wire it into src/main.rs please");
        let a = PlanFirst
            .evaluate(&s)
            .expect("two paths and an implementation verb");
        assert!(a.next_row_only);
        assert_eq!(
            a.headline,
            "Feature-sized prompt (2 files, 72 chars), plan off"
        );
        assert_eq!(a.urgency, Urgency::Next);
        let long = feature(&"do the thing ".repeat(30));
        assert!(PlanFirst.evaluate(&long).is_some(), "≥ 300 chars");
        assert!(
            PlanFirst
                .evaluate(&feature("fix the typo in src/a.rs"))
                .is_none(),
            "one path, no verb"
        );
        let mut planned =
            feature("implement the parser in src/parse.rs and wire it into src/main.rs please");
        call(
            &mut planned,
            "q",
            "2026-01-01T10:00:03Z",
            "AskUserQuestion",
            r#"{"questions":[]}"#,
            "yes",
            "",
        );
        assert!(
            PlanFirst.evaluate(&planned).is_none(),
            "a plan artefact retires it"
        );
        let mut plan_mode =
            feature("implement the parser in src/parse.rs and wire it into src/main.rs please");
        plan_mode.session.permission_mode = Some("plan".into());
        assert!(PlanFirst.evaluate(&plan_mode).is_none());
        let mut edited =
            feature("implement the parser in src/parse.rs and wire it into src/main.rs please");
        edit(&mut edited, "e1", "2026-01-01T10:00:03Z", "/p/src/parse.rs");
        assert!(
            PlanFirst.evaluate(&edited).is_none(),
            "an edit closes the window"
        );
    }

    #[test]
    fn a36_correction_streak_from_structural_markers_only() {
        // Turn 1: a refused call, then Esc; turn 2: the short steer.
        let mut s = fresh();
        call(
            &mut s,
            "b1",
            "2026-01-01T10:01:00Z",
            "Bash",
            r#"{"command":"git push origin main"}"#,
            "The user doesn't want to proceed",
            r#","is_error":true,"toolDenialKind":"user-rejected""#,
        );
        assert!(CorrectionStreak.evaluate(&s).is_none(), "one correction");
        edit(&mut s, "e1", "2026-01-01T10:02:00Z", "/p/src/a.rs");
        interrupt(&mut s, "2026-01-01T10:03:00Z");
        s.now_ms = t("2026-01-01T10:03:01Z");
        let a = CorrectionStreak.evaluate(&s).expect("two in one turn");
        assert_eq!(a.headline, "2 corrections in a row (refusal, then Esc)");
        assert_eq!(a.evidence, "rewind: 0 checkpoints since #1");
        assert_eq!(
            a.action,
            "Esc Esc → restore turn 1, restate with the constraint"
        );
        assert_eq!(a.action_kind, ActionKind::Key);
        assert_eq!(a.urgency, Urgency::Now);
        assert_eq!(CorrectionStreak.cooldown_turns(), 10);
        // The short steer keeps it live; a long restatement closes it.
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T10:03:10Z","promptSource":"typed","message":{"role":"user","content":"no, keep it"}}"#).unwrap());
        assert!(
            CorrectionStreak.evaluate(&s).is_some(),
            "the steer after Esc"
        );
        let mut restated = fresh();
        call(
            &mut restated,
            "b1",
            "2026-01-01T10:01:00Z",
            "Bash",
            r#"{"command":"git push origin main"}"#,
            "no",
            r#","is_error":true,"toolDenialKind":"user-rejected""#,
        );
        interrupt(&mut restated, "2026-01-01T10:03:00Z");
        restated.apply(&Line::parse(&format!(r#"{{"type":"user","timestamp":"2026-01-01T10:03:10Z","promptSource":"typed","message":{{"role":"user","content":"{}"}}}}"#, "restated at length ".repeat(15))).unwrap());
        assert!(
            CorrectionStreak.evaluate(&restated).is_none(),
            "a long restatement is the fix itself"
        );
        // Two turns apart on the same file: the pair; on different, unrelated turns: not.
        let mut pair = fresh();
        edit(&mut pair, "e1", "2026-01-01T10:01:00Z", "/p/src/a.rs");
        interrupt(&mut pair, "2026-01-01T10:02:00Z");
        pair.apply(&prompt("2026-01-01T10:03:00Z"));
        pair.apply(&response("m1", "2026-01-01T10:03:01Z", 2, 0, 50_000));
        pair.apply(&prompt("2026-01-01T10:04:00Z"));
        edit(&mut pair, "e2", "2026-01-01T10:04:30Z", "/p/src/a.rs");
        call(
            &mut pair,
            "b2",
            "2026-01-01T10:05:00Z",
            "Bash",
            r#"{"command":"git push"}"#,
            "no",
            r#","is_error":true,"toolDenialKind":"user-rejected""#,
        );
        pair.now_ms = t("2026-01-01T10:05:01Z");
        let a = CorrectionStreak
            .evaluate(&pair)
            .expect("same file, two turns apart");
        assert_eq!(a.headline, "2 corrections in a row (Esc, then refusal)");
        assert_eq!(
            a.action,
            "Esc Esc → restore turn 1, restate with the constraint"
        );
        let mut unrelated = fresh();
        interrupt(&mut unrelated, "2026-01-01T10:02:00Z");
        unrelated.apply(&prompt("2026-01-01T10:03:00Z"));
        unrelated.apply(&response("m1", "2026-01-01T10:03:01Z", 2, 0, 50_000));
        unrelated.apply(&prompt("2026-01-01T10:04:00Z"));
        edit(&mut unrelated, "e2", "2026-01-01T10:04:30Z", "/p/src/z.rs");
        call(
            &mut unrelated,
            "b2",
            "2026-01-01T10:05:00Z",
            "Bash",
            r#"{"command":"git push"}"#,
            "no",
            r#","is_error":true,"toolDenialKind":"user-rejected""#,
        );
        assert!(
            CorrectionStreak.evaluate(&unrelated).is_none(),
            "no shared file, not consecutive"
        );
        // Acted: a /rewind row after the fire.
        let mut h = crate::history::History::default();
        h.commands.push(crate::history::Row {
            at_ms: t("2026-01-01T10:06:00Z"),
            session_id: String::new(),
            project: String::new(),
            command: Some("/rewind".into()),
            arg: None,
            pasted_chars: 0,
        });
        pair.history = h;
        assert!(CorrectionStreak.acted(&pair, &a));
    }

    #[test]
    fn a38_failure_cascade_excludes_denials_and_retires_on_a_success() {
        let mut s = fresh();
        for (i, cmd) in ["gh pr view 12", "gh pr view 12 --json", "git push"]
            .iter()
            .enumerate()
        {
            bash(
                &mut s,
                &format!("f{i}"),
                &format!("2026-01-01T10:0{}:00Z", i + 1),
                cmd,
                "error",
                r#","is_error":true"#,
            );
        }
        s.now_ms = t("2026-01-01T10:04:00Z");
        let a = FailureCascade
            .evaluate(&s)
            .expect("three fails, two of one shape");
        assert_eq!(a.headline, "3 fails in a row: gh pr view ×2, git push");
        assert!(a.evidence.starts_with("this turn"), "{}", a.evidence);
        assert_eq!(a.urgency, Urgency::Now);
        assert!(!FailureCascade.acted(&s, &a));
        bash(
            &mut s,
            "ok",
            "2026-01-01T10:05:00Z",
            "gh pr view 12",
            "ok",
            "",
        );
        assert!(FailureCascade.acted(&s, &a), "the shape succeeded");
        assert!(FailureCascade.evaluate(&s).is_none(), "the run is broken");
        // Denials do not count; an edit between resets; three different shapes do not fire.
        let mut denied = fresh();
        for i in 0..3 {
            call(
                &mut denied,
                &format!("d{i}"),
                &format!("2026-01-01T10:0{}:00Z", i + 1),
                "Bash",
                r#"{"command":"gh api x"}"#,
                "blocked",
                r#","is_error":true,"toolDenialKind":"automode-blocked""#,
            );
        }
        assert!(FailureCascade.evaluate(&denied).is_none());
        let mut edited = fresh();
        bash(
            &mut edited,
            "f0",
            "2026-01-01T10:01:00Z",
            "gh pr view 12",
            "error",
            r#","is_error":true"#,
        );
        bash(
            &mut edited,
            "f1",
            "2026-01-01T10:02:00Z",
            "gh pr view 12",
            "error",
            r#","is_error":true"#,
        );
        call(
            &mut edited,
            "e1",
            "2026-01-01T10:03:00Z",
            "Edit",
            r#"{"file_path":"/p/x.rs","old_string":"a","new_string":"b"}"#,
            "err",
            r#","is_error":true"#,
        );
        assert!(
            FailureCascade.evaluate(&edited).is_none(),
            "an edit in the run"
        );
        let mut varied = fresh();
        for (i, cmd) in ["gh pr view 12", "git push", "cargo test"]
            .iter()
            .enumerate()
        {
            bash(
                &mut varied,
                &format!("f{i}"),
                &format!("2026-01-01T10:0{}:00Z", i + 1),
                cmd,
                "error",
                r#","is_error":true"#,
            );
        }
        assert!(
            FailureCascade.evaluate(&varied).is_none(),
            "no repeated shape"
        );
    }

    #[test]
    fn a40_review_before_merge_once_per_pr_with_a_real_diff() {
        let mut s = fresh();
        s.status_facts.pr_number = Some(142);
        edit(&mut s, "e1", "2026-01-01T10:00:30Z", "/p/src/a.rs");
        edit(&mut s, "e2", "2026-01-01T10:00:40Z", "/p/README.md");
        bash(
            &mut s,
            "p",
            "2026-01-01T10:01:00Z",
            "git push -u origin feat",
            "ok",
            r#","gitOperation":{"push":{"branch":"feat"}}"#,
        );
        assert!(
            ReviewBeforeMerge.evaluate(&s).is_none(),
            "no diff figures yet"
        );
        s.files.apply_numstat(
            std::path::Path::new("/p"),
            &[("src/a.rs".into(), 300, 80), ("README.md".into(), 200, 0)],
        );
        let a = ReviewBeforeMerge
            .evaluate(&s)
            .expect("a pushed PR with 380 source lines");
        assert_eq!(
            a.headline,
            "PR #142 open: +500/−80 in 2 files, no review pass"
        );
        assert_eq!(a.action_text, "/code-review");
        assert_eq!(a.urgency, Urgency::Next);
        assert!(!ReviewBeforeMerge.acted(&s, &a));
        // A review skill marks it done; a merge retires it.
        s.prefix.invoked_skills.push("code-review".into());
        assert!(ReviewBeforeMerge.evaluate(&s).is_none());
        assert!(ReviewBeforeMerge.acted(&s, &a));
        let mut small = fresh();
        small.status_facts.pr_number = Some(7);
        edit(&mut small, "e1", "2026-01-01T10:00:30Z", "/p/src/a.rs");
        edit(&mut small, "e2", "2026-01-01T10:00:40Z", "/p/docs/x.md");
        bash(
            &mut small,
            "p",
            "2026-01-01T10:01:00Z",
            "git push",
            "ok",
            r#","gitOperation":{"push":{"branch":"feat"}}"#,
        );
        small.files.apply_numstat(
            std::path::Path::new("/p"),
            &[("src/a.rs".into(), 40, 10), ("docs/x.md".into(), 900, 0)],
        );
        assert!(
            ReviewBeforeMerge.evaluate(&small).is_none(),
            "doc lines do not count"
        );
    }

    #[test]
    fn a41_waiting_on_you_after_thirty_seconds_with_the_cache_clock_when_short() {
        let mut s = fresh();
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:06Z","message":{"id":"m2","model":"claude-opus-5","content":[{"type":"tool_use","id":"q","name":"AskUserQuestion","input":{"questions":[]}}],"usage":{"input_tokens":2,"cache_read_input_tokens":50000,"output_tokens":10}}}"#).unwrap());
        s.now_ms = t("2026-01-01T10:00:20Z");
        assert!(WaitingOnYou.evaluate(&s).is_none(), "14 s");
        s.now_ms = t("2026-01-01T10:04:06Z");
        let a = WaitingOnYou.evaluate(&s).expect("4 minutes");
        assert_eq!(a.headline, "◆ WAITING 4:00 · Claude asked a question");
        assert!(
            a.action.starts_with("reply now · cache warm "),
            "{}",
            a.action
        );
        assert!(a.action.contains("cold restart ≈50k"), "{}", a.action);
        assert_eq!(a.saving, Saving::Seconds(240));
        assert_eq!(a.urgency, Urgency::Now);
        assert!(!WaitingOnYou.acted(&s, &a));
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T10:04:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q","content":"yes"}]}}"#).unwrap());
        assert!(WaitingOnYou.acted(&s, &a), "the reply");
        assert!(WaitingOnYou.evaluate(&s).is_none());
        // A question-ended turn with a warm hour-long cache: no clock.
        let mut asked = fresh();
        asked.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:06Z","message":{"id":"m2","model":"claude-opus-5","content":[{"type":"text","text":"Which one?"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_read_input_tokens":50000,"output_tokens":10,"cache_creation":{"ephemeral_1h_input_tokens":50000,"ephemeral_5m_input_tokens":0}}}}"#).unwrap());
        turn_end(&mut asked, "2026-01-01T10:00:07Z");
        asked.now_ms = t("2026-01-01T10:01:00Z");
        let a = WaitingOnYou
            .evaluate(&asked)
            .expect("the turn ended with a question");
        assert_eq!(
            a.headline,
            "◆ WAITING 0:53 · the turn ended with a question"
        );
        assert_eq!(a.action, "reply now · a one-word answer unblocks the task");
    }

    #[test]
    fn a42_checkpoint_at_a_boundary_on_a_large_context() {
        let big = |ctx: u64| -> State {
            let mut s = State::new(Pricing::bundled());
            s.session.alive = true;
            s.apply(&prompt("2026-01-01T10:00:00Z"));
            s.apply(&response("m0", "2026-01-01T10:00:01Z", 2, ctx, 0));
            s.now_ms = t("2026-01-01T10:00:02Z");
            s
        };
        let done = r#","toolUseResult":{"taskId":"1","updatedFields":["status"],"statusChange":{"from":"in_progress","to":"completed"},"success":true}"#;
        let mut s = big(312_000);
        assert!(Checkpoint.evaluate(&s).is_none(), "no boundary");
        call_ctx(
            &mut s,
            "tu",
            "2026-01-01T10:01:00Z",
            "TaskUpdate",
            r#"{"taskId":"1","status":"completed"}"#,
            "ok",
            done,
            312_000,
        );
        let a = Checkpoint.evaluate(&s).expect("a task completed");
        assert_eq!(a.headline, "checkpoint · ctx 312k · next task unrelated?");
        assert!(
            a.evidence
                .starts_with("task completed · a fresh session starts at ≈"),
            "{}",
            a.evidence
        );
        assert_eq!(
            a.action,
            "hand-off note → /rename → /clear (fresh ≈ the prefix)"
        );
        assert_eq!(a.urgency, Urgency::Next);
        assert!(!a.next_row_only);
        assert!(!Checkpoint.acted(&s, &a));
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T10:02:00Z","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#).unwrap());
        assert!(Checkpoint.acted(&s, &a), "the /clear");
        // Below 200 k: quiet. A background task: quiet. A commit alone: quiet.
        let mut small = big(150_000);
        call_ctx(
            &mut small,
            "tu",
            "2026-01-01T10:01:00Z",
            "TaskUpdate",
            r#"{"taskId":"1","status":"completed"}"#,
            "ok",
            done,
            150_000,
        );
        assert!(Checkpoint.evaluate(&small).is_none());
        let mut busy = big(312_000);
        call_ctx(
            &mut busy,
            "tu",
            "2026-01-01T10:01:00Z",
            "TaskUpdate",
            r#"{"taskId":"1","status":"completed"}"#,
            "ok",
            done,
            312_000,
        );
        busy.session
            .background_tasks
            .push(crate::ui::state::BackgroundTask {
                id: "b1".into(),
                kind: "shell".into(),
                status: "running".into(),
                description: String::new(),
            });
        assert!(Checkpoint.evaluate(&busy).is_none());
        let mut committed = big(312_000);
        call_ctx(
            &mut committed,
            "c",
            "2026-01-01T10:01:00Z",
            "Bash",
            r#"{"command":"git commit -m x"}"#,
            "ok",
            r#","toolUseResult":{"stdout":"ok","stderr":"","interrupted":false,"gitOperation":{"commit":{"sha":"abc1234","kind":"committed","branch":"main"}}}"#,
            312_000,
        );
        assert!(
            Checkpoint.evaluate(&committed).is_none(),
            "a commit is not a boundary"
        );
    }

    #[test]
    fn a43_destructive_git_nudges_on_a_dirty_tree_only() {
        let mut s = fresh();
        bash(
            &mut s,
            "g",
            "2026-01-01T10:01:00Z",
            "git reset --hard HEAD~1",
            "ok",
            "",
        );
        assert!(
            DestructiveGit.evaluate(&s).is_none(),
            "clean tree: an event only"
        );
        s.session.git_dirty = true;
        let a = DestructiveGit.evaluate(&s).expect("dirty");
        assert_eq!(
            a.headline,
            "Claude ran git reset --hard (#1) on a dirty tree"
        );
        assert_eq!(
            a.action,
            "rewind can't undo bash: check git reflog / stash list"
        );
        assert_eq!(a.urgency, Urgency::Now);
        let mut listed = fresh();
        listed.session.git_dirty = true;
        bash(
            &mut listed,
            "g",
            "2026-01-01T10:01:00Z",
            "git stash list",
            "ok",
            "",
        );
        assert!(DestructiveGit.evaluate(&listed).is_none());
    }

    #[test]
    fn a44_stale_collision_only_while_fresh_and_only_on_the_same_file() {
        let mut s = fresh();
        edit(&mut s, "e1", "2026-01-01T10:01:00Z", "/p/src/x.rs");
        assert!(StaleCollision.evaluate(&s).is_none());
        s.apply(&Line::parse(r#"{"type":"attachment","timestamp":"2026-01-01T10:01:30Z","attachment":{"type":"edited_text_file","filename":"/p/src/x.rs","snippet":""}}"#).unwrap());
        let a = StaleCollision
            .evaluate(&s)
            .expect("both edited x.rs this turn");
        assert_eq!(a.headline, "IDE + Claude both edited x.rs this turn");
        assert_eq!(a.evidence, "1 IDE edit(s), 1 by Claude");
        assert_eq!(a.urgency, Urgency::Now);
        assert!(!StaleCollision.acted(&s, &a));
        // Two calls elsewhere keep it; the third retires it.
        bash(&mut s, "b1", "2026-01-01T10:02:00Z", "ls", "ok", "");
        bash(&mut s, "b2", "2026-01-01T10:02:10Z", "ls", "ok", "");
        assert!(StaleCollision.evaluate(&s).is_some());
        bash(&mut s, "b3", "2026-01-01T10:02:20Z", "ls", "ok", "");
        assert!(StaleCollision.evaluate(&s).is_none(), "the moment passed");
        // A re-read of the file is the act.
        call(
            &mut s,
            "r",
            "2026-01-01T10:03:00Z",
            "Read",
            r#"{"file_path":"/p/src/x.rs"}"#,
            "content",
            "",
        );
        assert!(StaleCollision.acted(&s, &a));
        // An IDE edit of a file Claude did not touch this turn: an event only.
        let mut other = fresh();
        edit(&mut other, "e1", "2026-01-01T10:01:00Z", "/p/src/x.rs");
        other.apply(&Line::parse(r#"{"type":"attachment","timestamp":"2026-01-01T10:01:30Z","attachment":{"type":"edited_text_file","filename":"/p/src/y.rs","snippet":""}}"#).unwrap());
        assert!(StaleCollision.evaluate(&other).is_none());
    }

    #[test]
    fn a45_denial_streak_prints_claude_codes_rule_and_never_bare_bash() {
        let blocked = |s: &mut State, id: &str, ts: &str, cmd: &str| {
            call(
                s,
                id,
                ts,
                "Bash",
                &format!(r#"{{"command":"{cmd}"}}"#),
                "blocked",
                r#","is_error":true,"toolDenialKind":"automode-blocked""#,
            );
        };
        let mut s = fresh();
        blocked(&mut s, "d1", "2026-01-01T10:01:00Z", "gh api repos/x/y");
        assert!(DenialStreak.evaluate(&s).is_none(), "one block");
        blocked(
            &mut s,
            "d2",
            "2026-01-01T10:02:00Z",
            "cd /p && gh api repos/x/z",
        );
        let a = DenialStreak.evaluate(&s).expect("the same shape twice");
        assert_eq!(
            a.headline,
            "Auto mode blocked gh api ×2 this turn (3 in a row pauses it)"
        );
        assert_eq!(a.action, "allow Bash(gh api:*) or approve it");
        assert_eq!(a.action_text, "Bash(gh api:*)");
        assert_eq!(a.action_kind, ActionKind::AllowRule);
        assert_eq!(a.urgency, Urgency::Now);
        assert!(!DenialStreak.acted(&s, &a));
        // Claude Code's own suggestion wins when the spool carried it.
        s.session.permission_asks.insert(
            "Bash(gh api)".into(),
            crate::ui::state::PermissionAsk {
                count: 2,
                rule: Some("Bash(gh api:*)".into()),
            },
        );
        assert_eq!(
            DenialStreak.evaluate(&s).unwrap().action_text,
            "Bash(gh api:*)"
        );
        // A success of another shape is not the act; one of this shape is.
        bash(&mut s, "o", "2026-01-01T10:03:00Z", "ls", "ok", "");
        assert!(!DenialStreak.acted(&s, &a));
        bash(
            &mut s,
            "g",
            "2026-01-01T10:04:00Z",
            "gh api repos/x/y",
            "ok",
            "",
        );
        assert!(DenialStreak.acted(&s, &a));
        // Already allowed: quiet, and acted.
        let mut allowed = fresh();
        blocked(
            &mut allowed,
            "d1",
            "2026-01-01T10:01:00Z",
            "gh api repos/x/y",
        );
        blocked(
            &mut allowed,
            "d2",
            "2026-01-01T10:02:00Z",
            "gh api repos/x/z",
        );
        allowed.allow_rules.push("Bash(gh api:*)".into());
        assert!(DenialStreak.evaluate(&allowed).is_none());
        assert!(DenialStreak.acted(&allowed, &a));
        // A settings deny rule: once, on the first occurrence, with the alternative.
        let mut denied = fresh();
        call(
            &mut denied,
            "d1",
            "2026-01-01T10:01:00Z",
            "Bash",
            r#"{"command":"git reset --hard"}"#,
            "denied",
            r#","is_error":true,"toolDenialKind":"permission-rule""#,
        );
        let a = DenialStreak.evaluate(&denied).expect("a deny rule");
        assert_eq!(a.headline, "git reset is denied by your rules");
        assert_eq!(a.action, "tell Claude the alternative — it cannot run this");
        assert_eq!(a.action_kind, ActionKind::Prompt);
        // A user rejection is A36's, never A45's; a bare command never yields `Bash`.
        let mut rejected = fresh();
        for i in 0..2 {
            call(
                &mut rejected,
                &format!("r{i}"),
                &format!("2026-01-01T10:0{}:00Z", i + 1),
                "Bash",
                r#"{"command":"rm -rf x"}"#,
                "no",
                r#","is_error":true,"toolDenialKind":"user-rejected""#,
            );
        }
        assert!(DenialStreak.evaluate(&rejected).is_none());
        let mut bare = fresh();
        for i in 0..2 {
            call(
                &mut bare,
                &format!("b{i}"),
                &format!("2026-01-01T10:0{}:00Z", i + 1),
                "Bash",
                r#"{"command":""}"#,
                "blocked",
                r#","is_error":true,"toolDenialKind":"automode-blocked""#,
            );
        }
        assert!(DenialStreak.evaluate(&bare).is_none());
    }

    #[test]
    fn a46_long_context_drift_is_next_row_only_on_a_million_window() {
        let mut s = State::new(Pricing::bundled());
        s.session.alive = true;
        s.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"2026-01-01T10:00:00Z","promptSource":"typed","message":{{"role":"user","content":"{}"}}}}"#,
            "keep every public function documented ".repeat(20)
        )).unwrap());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:01Z","message":{"id":"m0","model":"claude-opus-5[1m]","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":40000,"cache_read_input_tokens":0,"output_tokens":10}}}"#).unwrap());
        s.apply(&prompt("2026-01-01T11:00:00Z"));
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T11:00:01Z","message":{"id":"m1","model":"claude-opus-5[1m]","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":312000,"output_tokens":10}}}"#).unwrap());
        s.now_ms = t("2026-01-01T11:00:02Z");
        let a = LongContextDrift
            .evaluate(&s)
            .expect("312 k on 1M, the long prompt 270 k back");
        assert!(a.next_row_only);
        assert_eq!(
            a.headline,
            "Ctx 312k: your turn-1 constraints are 272k tokens back"
        );
        assert_eq!(a.evidence, "crossed 300k · nothing re-injected since");
        assert_eq!(a.urgency, Urgency::Next);
        // An instructions attachment since: quiet.
        s.apply(&Line::parse(r#"{"type":"attachment","timestamp":"2026-01-01T11:00:03Z","attachment":{"type":"instructions","content":"x"}}"#).unwrap());
        assert!(LongContextDrift.evaluate(&s).is_none());
        // A 200 k window never fires.
        let mut small = State::new(Pricing::bundled());
        small.session.alive = true;
        small.apply(&Line::parse(&format!(
            r#"{{"type":"user","timestamp":"2026-01-01T10:00:00Z","promptSource":"typed","message":{{"role":"user","content":"{}"}}}}"#,
            "keep every public function documented ".repeat(20)
        )).unwrap());
        small.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:01Z","message":{"id":"m0","model":"claude-haiku-4-5-20251001","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":20000,"cache_read_input_tokens":0,"output_tokens":10}}}"#).unwrap());
        small.apply(&prompt("2026-01-01T11:00:00Z"));
        small.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T11:00:01Z","message":{"id":"m1","model":"claude-haiku-4-5-20251001","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":160000,"output_tokens":10}}}"#).unwrap());
        assert!(LongContextDrift.evaluate(&small).is_none());
    }

    #[test]
    fn a47_turn_died_branches_on_the_error_and_ignores_synthetic_text() {
        let rate = |s: &mut State| {
            s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:01:00Z","message":{"id":"e1","model":"<synthetic>","content":[{"type":"text","text":"Rate limit hit"}],"stop_reason":"stop_sequence","usage":{"input_tokens":0,"output_tokens":0}},"quotaLimits":{"status":"rejected","resetsAt":1767265200,"rateLimitType":"five_hour","lowPriorityRetryAfterSeconds":20},"error":"rate_limit","isApiErrorMessage":true,"apiErrorStatus":429}"#).unwrap());
        };
        let mut s = fresh();
        rate(&mut s);
        s.now_ms = t("2026-01-01T10:01:30Z");
        let a = TurnDied.evaluate(&s).expect("a confirmed 429");
        assert_eq!(a.headline, "Rate limit (session) · resets 11:00 (in 58m)");
        assert_eq!(a.action, "wait, or /model <other family> to keep working");
        assert_eq!(a.evidence, "turn 1 · low-priority retry in 20 s");
        assert_eq!(a.urgency, Urgency::Now);
        assert_eq!(TurnDied.ttl(), Ttl::NextPrompt);
        assert!(!TurnDied.acted(&s, &a));
        s.apply(&response("m1", "2026-01-01T10:05:00Z", 2, 0, 50_000));
        assert!(TurnDied.acted(&s, &a), "a successful call");
        assert!(TurnDied.evaluate(&s).is_none());
        // Branches: spend, auth, prompt too long, server.
        let err = |error: &str, status: u16| -> State {
            let mut s = fresh();
            s.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T10:01:00Z","message":{{"id":"e1","model":"<synthetic>","content":[{{"type":"text","text":"x"}}],"stop_reason":"stop_sequence","usage":{{"input_tokens":0,"output_tokens":0}}}},"error":"{error}","isApiErrorMessage":true,"apiErrorStatus":{status}}}"#)).unwrap());
            s.now_ms = t("2026-01-01T10:01:30Z");
            s
        };
        assert_eq!(
            TurnDied
                .evaluate(&err("spend_limit", 402))
                .unwrap()
                .headline,
            "Monthly spend limit hit · nothing will run"
        );
        assert_eq!(
            TurnDied
                .evaluate(&err("authentication_failed", 401))
                .unwrap()
                .action_text,
            "/login"
        );
        assert_eq!(
            TurnDied
                .evaluate(&err("prompt_too_long", 400))
                .unwrap()
                .action_text,
            "/compact"
        );
        assert_eq!(
            TurnDied
                .evaluate(&err("server_error", 500))
                .unwrap()
                .headline,
            "API error 500: server_error"
        );
        // A `<synthetic>` line without the flag is Claude Code's own text.
        let mut synthetic = fresh();
        synthetic.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:01:00Z","message":{"id":"e1","model":"<synthetic>","content":[{"type":"text","text":"No response requested."}],"stop_reason":"stop_sequence","usage":{"input_tokens":0,"output_tokens":0}}}"#).unwrap());
        assert!(TurnDied.evaluate(&synthetic).is_none());
    }

    /// Fixture B's moments: the deny rule at the denials moment, the
    /// correction streak on the steer after Esc, the question at the
    /// waiting moment; nothing of this axis at the explore, edits, cold and
    /// idle moments or at the end.
    #[test]
    fn fixture_b_outcome_moments() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("fixtures/session-b.jsonl");
        let at = |lines: usize, plus: i64| -> Vec<(String, String)> {
            let info = crate::ui::state::SessionInfo::from_fixture(&path);
            let mut s = crate::load::state_from_prefix(&path, info, lines);
            if plus > 0 {
                s.clock_override = true;
                s.now_ms = s.last_line_at_ms.unwrap() + plus * 1000;
            }
            all()
                .iter()
                .filter_map(|r| r.evaluate(&s))
                .map(|a| (a.rule.to_string(), a.headline))
                .collect()
        };
        assert!(at(214, 0).is_empty(), "explore");
        assert!(at(300, 0).is_empty(), "edits");
        assert_eq!(
            at(733, 0),
            Vec::<(String, String)>::new(),
            "turn 4's IDE edits are long past"
        );
        let denials = at(761, 0);
        assert_eq!(denials.len(), 1, "{denials:?}");
        assert_eq!(denials[0].0, "A45");
        assert_eq!(denials[0].1, "rm is denied by your rules");
        let steer = at(765, 0);
        assert_eq!(steer.len(), 1, "{steer:?}");
        assert_eq!(steer[0].1, "2 corrections in a row (refusal, then Esc)");
        let waiting = at(788, 240);
        assert_eq!(
            waiting.iter().map(|(r, _)| r.as_str()).collect::<Vec<_>>(),
            ["A36", "A41"]
        );
        assert_eq!(waiting[1].1, "◆ WAITING 4:00 · Claude asked a question");
        assert!(at(789, 0).is_empty(), "the answer closes both");
        assert!(at(799, 0).is_empty(), "both commits were checked");
        // The engine ranks the waiting moment: the question takes the slot.
        let info = crate::ui::state::SessionInfo::from_fixture(&path);
        let mut s = crate::load::state_from_prefix(&path, info, 788);
        s.clock_override = true;
        s.now_ms = s.last_line_at_ms.unwrap() + 240_000;
        let e = Engine::for_state(&s);
        assert_eq!(e.session_mode, crate::advisor::SessionMode::Interactive);
        assert_eq!(e.occupant.as_ref().unwrap().advice.rule, "A41");
        assert_eq!(
            e.current.iter().map(|a| a.rule).collect::<Vec<_>>()[..2],
            ["A41", "A36"]
        );
    }
}
