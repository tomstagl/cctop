# Hand-off: implementing the agent-costs PRD, then the team-costs PRD

Written 2026-09-15, `main` at `85ded3d` (PR #5 merged: both PRDs at v1.1),
CI green, cctop 0.3.1 and Claude Code 2.1.272 on this machine. Read this,
then `tasks/prd-cctop-agent-costs.md` in full (its two boxes at the top,
then §3 and §4 before the stories) and `tasks/prd-cctop-team-costs.md` §3.2.

## Where things stand

- **Both PRDs are contracts now, not drafts of the code.** v1.0 was written
  from the code; v1.1 checked every claim against `main` and against the
  sessions on this machine, and the design moved on what it found. The
  boxes at the top of each file list what changed and why; §3.2 in each
  holds the measurements. Do not re-derive them — re-run them only when
  `claude` moves past `harness_facts::READ_FROM`.
- **Nothing is implemented.** The one artefact from the check is
  `scripts/ledger-vs-agents.py` (US-001's first bullet, committed with this
  hand-off; `python3 scripts/ledger-vs-agents.py` prints the §3.2 table in a
  few seconds).
- **Two bugs in `main` fall out of the check and are worth their own commit
  before any feature** (agent PRD §3.2, US-001's last bullet):
  1. `Agent::push` (`src/agents.rs`) adds a message's usage on its first line
     and ignores the later lines of the same `message.id`. Main transcripts
     never vary across a message's lines; subagent transcripts stream
     `output_tokens` and only the last line is complete — 59 % of 2 292
     subagent messages here, 3.2 M output tokens short in total.
  2. A fork's first assistant message is the parent's launching response
     (same `message.id`, present in the parent transcript; it carries the
     `Agent` tool_use whose id is `Meta.tool_use_id`). Skip the first
     assistant `message.id` after a `fork-context-ref` line. On fixture A
     the fork's hand counts become 7 own calls and 304 output tokens (188 by
     first line); the test's comment says why.
  Both change `agent_tokens` and `agents_cost` for every user and are
  independently releasable (0.3.2 if the person wants them out first).
- **A display bug beside them**: Panel 6 prints `↰32` from
  `fork-context-ref.contextLength`, which is not tokens (32 and 789 against
  first-own-call cache reads of 62 690 and 279 924). Show the fork's first
  own call's `cache_read` instead (agent PRD §11). Tiny, separate commit.
- **Fixture A's fork is from another session**: `session-a.jsonl` (1 463
  lines, 2026-08-27 after the shift) does not contain the fork's launching
  call or its task notification; the fork's message ids post-date every
  main id. It still serves as the "Done, no notification → waste 0" case.
- Open from the previous hand-off (`tasks/handoff-after-v0.3.1.md`): issue
  #3 is fixed but not closed — the live check of `docs/verification/pane.md`
  item 25 has no `Result:` line yet; issue #4 (CI against a real `claude`)
  is untouched; `make check-types` warns because `READ_FROM = "2.1.270"` <
  2.1.272. None of this blocks the PRDs; do not let the PRD work close #3
  without the person's live check.

## The facts the code will rest on (measured; see the PRDs for the tables)

- `cost-state` includes subagent calls and calls no transcript shows; it is
  written after `last-prompt` (session end) or `bridge-session`, has no
  timestamp, and the nearest timestamped line is a `system` line 1–5 lines
  above. A live session has none.
- Agent launches are `toolUseResult.status = "async_launched"`; the result
  is a later `user` line with `origin.kind = "task-notification"` whose
  content is `<task-notification>` with `<task-id>` (the agent id),
  `<tool-use-id>`, `<status>` (`completed` / `failed` / `killed`),
  `<result>`, `<summary>`, `<note>`, `<output-file>` and an optional
  `<usage>` (`subagent_tokens`, `tool_uses`, `duration_ms`). Background
  shell tasks send the same envelope without `<tool-use-id>`; workflow runs
  send `<agent_count>` / `<agents_done>` / `<agents_error>` /
  `<agents_skipped>` / `<agents_empty_result>` / `<failures>`.
- `state.tools.agent_spawns` (`src/tools.rs`, `AgentSpawn`) already records
  every `Agent` result with `agent_id`, `turn`, `is_async`; nothing reads
  it. It is the main-transcript side of the link.
- Overlays are `state.overlay = Some(panel id)` and the panel's
  `has_overlay` / `render_overlay` / `handle_key` (`ui/panel.rs`; the ledger
  view in `ui/ledger_view.rs` over pure rows from `src/ledger.rs` is the
  template). Panel 6 has no `handle_key` yet.
- The 7-day baseline's `cost_per_turn` is `cost-state.totalCostUSD ÷ human
  turns` (`baseline.rs` `figures`), i.e. combined; `×N$` on Panel 2 divides
  the main-only live figure by turns today.

## Order of work — agent PRD first

One commit per item; `make check` before each; snapshots via
`INSTA_UPDATE=always cargo test`, then strip `assertion_line:` and delete
`*.snap.new`; `scripts/pane-fixtures.sh` (and `… fixtures/session-b.jsonl b`)
after any change to the dashboard or coach objects; `cctop metrics --md >
docs/metrics.md && cctop metrics --readme README.md` after any registry
change — the tests fail on drift, so run them before `make check`.

1. **The two `Agent::push` fixes and the `↰` display** (above). Fixture A's
   test counts change; add a synthetic test for the streaming case (three
   lines, one id, growing `output_tokens` → the last wins) and one for the
   fork skip (a `fork-context-ref` line, then two messages → the first is
   not counted, `api_calls == 1`).
2. **US-001, the rest**: `harness_facts::cost_state` and
   `task_notification` modules; `CostTracker::authoritative_at_ms` and
   `combined(...)` with `Cost.source`; `READ_FROM` to 2.1.272 only if the
   binary's other constants were re-checked (issue #4 item 5) — otherwise
   note the version in the new module's doc comment and leave `READ_FROM`.
3. **US-003 before US-002**: the notification parser
   (`transcript/task_notification.rs`), the `Agent` / `AgentSpawn` fields,
   the `tool_use_id → agent id` index and the pending map in `State`,
   `WorkflowJournal.failed_ids`, `src/agent_ledger.rs` with its per-reason
   tests, the watcher-vs-load equality test, the registry ids. Compose
   fixture C here (below). Everything that draws depends on this.
4. **US-002**: Panel 2 lines 6/7/10, dashboard row 2's detail, `cctop
   report`, the coach's limits-light lines and `agents_cell`, `cost_combined`
   in `query summary` / `dashboard`, `×N$` on the combined figure. Check
   FR-6 by diffing the snapshots of fixtures A and B: only `cost_combined`
   may appear; on A (a `cost-state`, a fork with 7 own calls whose
   timestamps are *after* the ledger's moment because of the per-file
   shift) the headline becomes `≈` — that is correct and the snapshot's
   comment should say so.
5. **US-004** the view, **US-005** query / row 6 / pane / MCP, **US-006** the
   rule (A48 `agents-waste`, `ORDER_LATER`, CLAUDE.md's rule count 35 → 36).
6. Then the team PRD, in its own order (US-001 there is now facts + fixture
   D + two small parser additions; the collector is US-002).

## Fixtures C and D — where the sources are

The PRDs name the fixtures; the sources are on this machine only. Never
hand-edit a fixture; extend `scripts/compose-fixture.py` with the
`--shared-shift` mode (one time shift and one id map across a lead, its
`subagents/`, its teammates and a team file) and regenerate.

- **C (agent PRD US-003)** — the session whose fork fixture A already ships:
  `find ~/.claude/projects -name 'agent-a9a92645226d3a561.meta.json'` gives
  the `subagents/` directory; the lead is the `.jsonl` beside it (224 lines,
  339 KB). It has the `async_launched` call (line 46), the notification
  (line 54: `completed`, `<usage>`, a 2 069-char `<result>`) and the fork.
  Splice one `failed` and one `killed` notification from other sessions
  (`grep -l '<status>killed</status>' ~/.claude/projects/*/*.jsonl`); the
  composer finds segments by predicate, so the recipe in `fixtures/README.md`
  names no ids. Check the anonymiser keeps the notification's element names
  and numbers and replaces its text (`<summary>`, `<result>`, `<note>`,
  `<output-file>`), and keeps `toolUseResult.status` / `isAsync` / `agentId`.
- **D (team PRD US-001)** — the lead in this repo's project directory with a
  `teammate_spawned` result: `grep -l teammate_spawned
  ~/.claude/projects/-Users-tom-code-cctop/*.jsonl` (718 lines, 2.4 MB;
  `--max-str 2000` shrinks it) and its teammate, found by
  `grep -l '"teamName":"session-<lead id8>"' ~/.claude/projects/-Users-tom-code-cctop/*.jsonl`
  (101 lines, ends with a `cost-state`). The team directory is gone, which
  is the point: D exercises the transcript path. `fixtures/session-d.team.json`
  comes from a *live* team's `config.json` (`ls ~/.claude/teams/`, the one
  with a `tmux` member) for the config-path tests. A teammate cut before its
  `cost-state` and a spawn with no transcript are composed, not copied.
- `fixtures/README.md` gets a paragraph per fixture (what it is, how to
  rebuild), like A's and B's.

## Decisions the person has already taken — do not reopen

The four in the box at the top of the agent PRD and the three in the team
PRD's. Two v1.1 calls were mine and are flagged for a veto, not a debate:
`dup` is out of v1 (no Claude Code signal; a relaunch after `failed` is the
fix, not the waste), and `killed` counts as waste (spent, nothing came
back). If the person says otherwise, change the PRD in the same commit.

## Working practices that held

Edits by small substitutions; `make check` (fmt, clippy `-D warnings`, tests)
before every commit and `npm run typecheck && npm test` when the pane or its
fixtures change; `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin validate
--strict ./plugin` after touching `pane.tsx`; commit subjects as prose
(`Agents: …`, `Cost: …`, `Docs: …`) with the trailer `Co-Authored-By: Claude
Opus 5 <noreply@anthropic.com>`; push and `gh run list` (CI's clippy is
newer than the local one). Rustfmt reformats whole files — fold drift in.
Fixture transcripts are never hand-edited; timestamps are shifted per file
by the anonymiser, so cross-file ordering questions need the shared shift.
Never run `cctop run` or `claude` in the foreground of a tool call; `cctop
query <verb> --session fixtures/…` with `--lines N` and `CCTOP_FAKE_NOW` is
how to look at a moment. Read-only, no prose: lengths, counts, enum values,
ids — never the text of a prompt, result, summary or message.

## Hand-off prompt

```
Implement tasks/prd-cctop-agent-costs.md per @tasks/handoff-agent-costs.md
(read the hand-off, then the PRD's two top boxes, §3, §4, then the stories;
skim tasks/prd-cctop-team-costs.md §3.2). main is at 85ded3d, CI green,
cctop 0.3.1 and Claude Code 2.1.272 on this machine. Start with item 1 of
the hand-off's order (the two Agent::push fixes and the ↰ display) as
separate commits with tests, then US-001, US-003, US-002, US-004, US-005,
US-006 in that order, one commit per story or smaller; make check before
each commit, regenerate docs/metrics.md, the README block, the insta
snapshots and the pane fixtures whenever the tests say they drifted; push
and check gh run list. Compose fixture C from the source the hand-off
names before US-003's tests. Ask me before changing anything the PRD's
decision boxes fix; tell me if you disagree with dropping `dup` or counting
`killed`. Read-only rule and validator constraints as in CLAUDE.md.
```
