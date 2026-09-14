---
name: cctop-insights
description: Answer questions about the current Claude Code session's cost, tokens, context, cache behaviour, tools, subagents and cctop's recommendations by querying cctop. Use when the user asks why a session is expensive or slow, what is filling the context, whether they will hit a rate limit, what a cctop metric means, or how to work more efficiently with Claude Code.
---

# cctop insights

cctop watches this session from the outside (transcript, hooks, status line, processes). Everything it shows in the TUI is available as JSON via `cctop query`. Use it instead of reading `~/.claude/projects/**` yourself — it is deduplicated, calibrated against Claude Code's own cost record, and cheap to call.

## Commands

All commands accept `--session <id|name|pid>` (default: the session you are in, from `$CLAUDE_SESSION_ID`, the tmux pane, or the working directory) and print JSON. Every numeric value is `{value, unit, metric_id, approx}`; an optional source that is not on disk is `{"source": "missing", "hint": "run cctop install"}`. Full schema: `docs/query.md` in the cctop repo.

| Question shape | Command |
|---|---|
| Overall state right now | `cctop query summary` |
| Which turns cost what | `cctop query ledger --last 20` |
| Tool usage, slow/noisy tools, context pushed by results | `cctop query tools` |
| Files touched, re-reads | `cctop query files` |
| Subagents, MCP servers, background tasks | `cctop query agents` |
| What should I do now / next (the coach: four lights, the one nudge, what is next or snoozed) | `cctop query coach` |
| Current ranked recommendations with evidence and explanation | `cctop query advice` |
| What is in the fixed prefix (CLAUDE.md, tool schemas, MCP) | `cctop query prefix` |
| Recent events (tools, hooks, permissions, compactions) | `cctop query events --since 10m` |
| Definition, formula and caveats of a metric | `cctop query explain <metric_id>` |
| Compare to the user's own recent sessions | `cctop query baseline` |
| One-page end-of-session summary (Markdown) | `cctop report --out -` |

Output is always JSON (no `--json` flag needed). Pipe through `jq` to keep the tool result small, e.g. `cctop query summary | jq '{context, cost, limits}'`.

If `cctop` is not on PATH, say so and give the install line (`brew install <tap>/cctop`); do not fall back to parsing transcripts by hand.

## How to answer

1. Run the narrowest query that answers the question; `summary` first only if the question is vague.
2. Quote the numbers with their units and mark estimates as cctop marks them (`est`, `≈`). Every value carries a `metric_id`; if the user asks what it means, run `explain`.
3. Lead with `advice.primary` — the coach's slot occupant (class NOW › NEXT › LATER, then the published order). Give the evidence line, then the action; when `action_text` is set and its `action_kind` is `prompt` or `slash`, offer that exact text. Do not list all recommendations unless asked, never re-propose a rule listed under `snoozed`, and do not act on a nudge when `session_mode` is `machine` or `loop`.
4. When a recommendation is something you can do in this session (delegate exploration to a subagent, read a file range instead of the whole file, run a long command in the background), offer to do it — that is the point.
5. Do not paste raw JSON into the answer. Two to six lines is usually right.

## Examples

**"Why is this session so expensive?"**
`cctop query summary | jq '{cost, burn_rate, tokens}'`, then `cctop query ledger --last 10 | jq '.[] | [.turn, .api_calls.value, .cost.value, .tools]'`. Answer pattern: total and burn rate; the two or three turns that dominate and what they did (tool results size, compaction, cold cache); the top advice item.

**"Why is my cache hit ratio low?"**
`cctop query advice` (A01 names a cache miss — model switch, tool list change, rewritten message — and A02 a cache that expired between prompts, when either happened in the last three turns) and `cctop query ledger --last 10 | jq '.[] | [.turn, .cache_read.value, .cache_write.value, .miss_cause]'` to show which turns wrote cache instead of reading it. Name the cause cctop found and the fix.

**"Did the last run leave anything unchecked?"**
`cctop query coach | jq '{light: .lights[3], nudge: .nudge}'`. The rework light counts the turn's open issues (fails in a row, corrections, blocked calls, source edits since the last confirmed test); the outcome-axis nudges name the next step: A32 (edits with no test run — offer to run the session's test command), A33 (a commit without a check — offer to run it and amend), A38 (a failure cascade — ask for the missing fact rather than retrying), A45 (auto mode blocked a shape twice — the `permissions.allow` rule to add), A36 (two corrections in a row — suggest Esc Esc / `/rewind` before restating), A42 (a natural boundary on a large context — a hand-off note and `/clear`).

**"What is filling my context?"**
`cctop query prefix | jq '.rows[:5]'` and `cctop query tools | jq '.top_ctx'`. Answer: prefix size and its biggest parts; the largest individual tool results; turns until autocompact.

**"Will I hit the rate limit?"**
`cctop query summary | jq .limits` → `five_hour`, `exhaustion_ms`, `other_live_sessions`; if `source` is `missing`, say the shim is not installed and offer `cctop install`. Give used %, reset time, projected exhaustion and whether other live sessions are contributing. If exhaustion is before reset, suggest moving exploration to a cheaper model or pausing heavy work.

**"What does TOKENS→CTX mean?"**
`cctop query explain tokens_to_ctx` and restate the definition and caveat in one sentence each.

## Boundaries

- cctop is read-only and local; it never changes the session. Recommendations are rule-based (no model call) — say so if asked how they were produced.
- Cost figures for subscription plans are API-equivalent estimates, shown as `≈ API`.
- If a query returns `"source": "missing"` for a field, tell the user which optional source is absent (`cctop install` adds the status-line shim and hooks) rather than guessing.
