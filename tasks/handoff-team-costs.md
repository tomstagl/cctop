# Hand-off: after the agent-costs PRD — the team-costs PRD

Written 2026-09-16, branch `agent-costs` at its tip (draft PR #6 against
`main`), CI green, cctop 0.3.1 + the PRD's changes unreleased, Claude Code
2.1.273 on this machine. Read this, then `tasks/prd-cctop-team-costs.md`
in full, then `tasks/handoff-agent-costs.md` for fixture D's sources.

## Where things stand

- **The agent-costs PRD is implemented**, one commit per story on PR #6
  (the two `Agent::push` fixes, `↰`, US-001, US-003, US-002, US-004,
  US-005, US-006). Not merged, not released; the next release is 0.4.0
  (`Cargo.toml`, `plugin/.claude-plugin/plugin.json`, the CHANGELOG, the
  README's install lines — see the coach release commits for the shape).
  `docs/verification/pane.md` item 26 is the pending live check of the
  pane's Agents view and the combined cost.
- **What the check found that the PRD did not know** is in the PRD's
  top box (2026-09-16 entry) and `harness_facts::task_notification`:
  notifications arrive three ways (user line, `queue-operation` enqueue,
  `queued_command` attachment) and every `killed` agent here came only as
  an attachment; shell tasks' notifications do carry `<tool-use-id>` and
  are told apart by the id's shape; no `failed` agent notification exists
  on this machine; A48 fires zero times on fixture C (under its floor).
- **Decisions kept**: `dup` stays out; `killed` counts as waste. The
  cost cache of FR-7 was not built — `agent_ledger::rows` prices per
  render (a prefix match per agent, negligible at 40 agents).
- Open from before: issue #3's live check, issue #4, `READ_FROM` at
  2.1.270 (claude is 2.1.273; the binary's constants were not
  re-checked).

## The shape the team PRD lands in

- The main-transcript side of an agent is `State.agent_links`
  (`AgentLink`: launch, notification, sync result), applied to `agents`
  on every `merge_agents`; `note_agent_spawn` skips a `teammate_spawned`
  result's `<name>@<team>` id on purpose — the team PRD reads it.
- `Agent.calls: Vec<AgentCall>` is what `CostTracker::agents_after`
  places against the ledger's moment; a teammate's own `cost-state`
  (team PRD §4.1) is a second ledger and needs its own moment.
- `agent_ledger::rows` / `totals` / `workflow_groups` feed the agents view,
  `query::agents`, dashboard row 6 and A48; teammate rows (team PRD
  US-004) go through the same module so the surfaces cannot disagree.
- `compose-fixture.py --agent-killed` is the pattern for fixture D:
  a segment found by predicate plus the subagent's files copied with the
  segment's delta (`copy_agent`). A teammate transcript lives in the
  project directory, not under `subagents/`, so D needs a sibling of
  `copy_agent` that writes `fixtures/session-d/<teammate>.jsonl` (or
  wherever the collector will look) and a `--teammate` finder on
  `teamName` / `agentName`. The anonymiser keeps `agentId` / `agent_id`
  verbatim now; add `teamName` / `agentName` to `KEEP_KEYS` (the names
  are roles, and the team key must match the config file).

## Hand-off prompt

```
Implement tasks/prd-cctop-team-costs.md per @tasks/handoff-team-costs.md
(read the hand-off, then the PRD, then tasks/handoff-agent-costs.md for
fixture D's sources). Branch from agent-costs (or from main once PR #6 is
merged). Its US-001 first (facts, fixture D, the two parser additions),
then US-002 the collector, US-003 Panel 2, US-004 the rows, US-005 budget
and drift; one commit per story; make check, npm test and the plugin
validator before each commit; regenerate docs/metrics.md, the README
block, the insta snapshots and the pane fixtures when the tests say so;
push and check gh run list. Read-only rule and validator constraints as
in CLAUDE.md.
```
