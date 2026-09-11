---
name: cctop-insights
description: Answer questions about the current Claude Code session's cost, tokens, context, cache behaviour, tools, subagents and cctop's recommendations by querying cctop. Use when the user asks why a session is expensive or slow, what is filling the context, whether they will hit a rate limit, what a cctop metric means, or how to work more efficiently with Claude Code.
---

# cctop insights

cctop watches this session from the outside (transcript, hooks, status line, processes). Everything it shows in the TUI is available as JSON via `cctop query`. Use it instead of reading `~/.claude/projects/**` yourself — it is deduplicated, calibrated against Claude Code's own cost record, and cheap to call.

## Commands

All commands accept `--session <id|name|pid>` (default: the session you are in, from `$CLAUDE_SESSION_ID`) and print JSON.

| Question shape | Command |
|---|---|
| Overall state right now | `cctop query summary --json` |
| Which turns cost what | `cctop query ledger --last 20 --json` |
| Tool usage, slow/noisy tools, context pushed by results | `cctop query tools --json` |
| Files touched, re-reads | `cctop query files --json` |
| Subagents, MCP servers, background tasks | `cctop query agents --json` |
| Current ranked recommendations with evidence | `cctop query advice --json` |
| What is in the fixed prefix (CLAUDE.md, tool schemas, MCP) | `cctop query prefix --json` |
| Recent events (tools, hooks, permissions, compactions) | `cctop query events --since 10m --json` |
| Definition, formula and caveats of a metric | `cctop query explain <metric_id> --json` |
| Compare to the user's own recent sessions | `cctop query baseline --json` |

If `cctop` is not on PATH, say so and give the install line (`brew install <tap>/cctop`); do not fall back to parsing transcripts by hand.

## How to answer

1. Run the narrowest query that answers the question; `summary` first only if the question is vague.
2. Quote the numbers with their units and mark estimates as cctop marks them (`est`, `≈`). Every value carries a `metric_id`; if the user asks what it means, run `explain`.
3. Lead with the one change that saves the most (advice is already ranked by `saving_estimate`). Give the evidence line, then the action. Do not list all recommendations unless asked.
4. When a recommendation is something you can do in this session (delegate exploration to a subagent, read a file range instead of the whole file, run a long command in the background), offer to do it — that is the point.
5. Do not paste raw JSON into the answer. Two to six lines is usually right.

## Examples

**"Why is this session so expensive?"**
`cctop query summary --json`, then `cctop query ledger --last 10 --json`. Answer pattern: total and burn rate; the two or three turns that dominate and what they did (tool results size, compaction, cold cache); the top advice item.

**"Why is my cache hit ratio low?"**
`cctop query advice --json` (rule A01/A02 will be present if relevant) and `cctop query ledger --last 10 --json` to show which turns had cache writes instead of reads. Name the cause cctop found (prompt gap longer than the cache TTL, a changing prefix, a CLAUDE.md edit) and the fix.

**"What is filling my context?"**
`cctop query prefix --json` and `cctop query tools --json` (`top_ctx` field). Answer: prefix size and its biggest parts; the largest individual tool results; turns until autocompact.

**"Will I hit the rate limit?"**
`cctop query summary --json` → `limits.five_hour`. Give used %, reset time, projected exhaustion and whether other live sessions are contributing. If exhaustion is before reset, suggest moving exploration to a cheaper model or pausing heavy work.

**"What does TOKENS→CTX mean?"**
`cctop query explain tokens_to_ctx --json` and restate the definition and caveat in one sentence each.

## Boundaries

- cctop is read-only and local; it never changes the session. Recommendations are rule-based (no model call) — say so if asked how they were produced.
- Cost figures for subscription plans are API-equivalent estimates, shown as `≈ API`.
- If a query returns `"source": "missing"` for a field, tell the user which optional source is absent (`cctop install` adds the status-line shim and hooks) rather than guessing.
