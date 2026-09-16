# Agent teams: what Claude Code writes

How a session that leads an agent team is recorded on disk, read on one
machine's 19 team directories and 23 teammate transcripts written by Claude
Code 2.1.232 – 2.1.272 (2026-09-15/16). Keys only: no member names, no
prompts. The constants cctop rests on are in `src/harness_facts.rs`
(`first_seen::TEAM_NAME`, `teams`); the design that reads them is
`tasks/prd-cctop-team-costs.md`.

## The team directory (live only)

`~/.claude/teams/<team>/config.json`, `<team>` being `session-<id8>` — the
first eight characters of the lead's session id:

```
{createdAt, leadAgentId, leadSessionId, name, members[]}
```

A member is

```
{agentId, agentType, backendType, cwd, joinedAt, name, subscriptions, tmuxPaneId}
```

plus, on a teammate the `Agent` tool spawned, `color`, `isActive`, `model`,
`planModeRequired`, `prompt`.

- `agentId` is `<name>@<team>`; `name` is the member's label (a role).
- **The lead is a member** (`agentType: team-lead`, `backendType:
  in-process`, `tmuxPaneId: leader`, no `isActive`). Teammates are
  `backendType: tmux`. A solo session also has a team directory, with the
  lead as its one member: a one-member team is not a team.
- **No member carries a session id.** The mapping to a transcript is the
  transcript's own `teamName` / `agentName` (below).
- `isActive` on a `tmux` member is Claude Code's liveness flag: `true` on a
  running teammate, `false` on a finished one, absent on the lead.
- `joinedAt` is epoch milliseconds; the teammate's first transcript line is
  within seconds of it.
- Every team seen shared the lead's `cwd`; `member.cwd` is still the field
  to trust for the project slug.
- **The directory is removed when the team ends; the transcripts stay.** Of
  the five teams with teammate transcripts, three had no directory left.
  Membership after the fact comes from the transcripts alone.
- No `inboxes/` directory existed under any team; cctop never opens one.

## The teammate transcript (durable)

A teammate is a full Claude Code session: `<projects>/<slug of
member.cwd>/<sessionId>.jsonl`, `entrypoint: cli`, opening like any session
(`agent-setting`, `mode`, `permission-mode`).

- **Every `user`, `assistant`, `system` and `attachment` line carries
  `agentName` (= `member.name`) and `teamName` (= `<team>`, the directory
  name).** The first such line is line 4 in every transcript seen. Lines of
  other types (`agent-setting`, `mode`, `atis-latch`, `last-prompt`,
  `bridge-session`, `cost-state`) carry neither.
- `cost-state` is written as the lead's is: at the session's end (after
  `last-prompt`) or on a bridge, never periodically. 20 of 23 transcripts
  had one; every one of those had ended. A running teammate has no ledger.
- `<teammate-message>` user lines are the team's messages; cctop counts
  them as machine turns and never reads their body.
- The lead's transcript carries **no** team key on its own lines.

## The spawn result (optional decoration)

When the `Agent` tool created the teammate, the lead's tool_result line has

```
toolUseResult: {status: "teammate_spawned", agent_id, agent_type, color,
                is_splitpane, model, name, plan_mode_required, prompt,
                team_name, teammate_id, tmux_pane_id,
                tmux_session_name, tmux_window_name}
```

`agent_id` = `teammate_id` = `<name>@<team>`; `team_name` = `<team>`. A
team a `Workflow` run assembled has no such result at all, so the spawn
adds a name, a type and a model when present and is never the source of
membership.

## What cctop reads

`teamName` and `agentName` (matching keys, kept as the row's label),
`sessionId`, timestamps, `message.usage`, `message.id`, `message.model`,
`cost-state` — through the same `transcript::Line` parser and collectors as
the lead's transcript. Nothing else of a teammate's transcript is retained.
