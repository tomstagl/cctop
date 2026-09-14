# `cctop query`

Every number the TUI shows, as JSON on stdout. Built from the same collectors
as the dashboard (`cctop::load::state`), so the two never disagree.

```
cctop query <what> [--session <id|name|pid|fixture.jsonl>] [--cwd <dir>] [--wait]
```

Exit codes: `0` ok · `2` no session found (message on stderr) · `1` bad argument.

`--session` takes a session id (or a prefix of at least 8 characters), the
session's name, its pid, or a path to a `.jsonl` fixture. An id the registry
no longer lists — the process exited, or `/clear` gave it a new id — still
answers from its transcript under `~/.claude/projects/`, as an ended session
(`session.alive: false`, `pid: null`); only an id with no transcript on disk
exits 2.

## Value shape

Every measured number is an object:

```json
{"value": 134000, "unit": "tokens", "metric_id": "context_size", "approx": true}
```

- `metric_id` links to [`docs/metrics.md`](metrics.md) (`cctop query explain <metric_id>` returns the same definition).
- `approx` is `true` when the value is an estimate: the TUI shows it with `est` / `≈`.

An optional source that is not on disk is reported, never guessed:

```json
{"source": "missing", "hint": "run cctop install"}
```

## Subcommands

| What | Returns |
|---|---|
| `summary` | Session identity, turns, api calls, context (size/window/ratio/prefix/velocity/turns_until_compaction/compactions), tokens (five classes + cache_hit_ratio + cache_ttl), cost (+ by model), burn_rate, input_rate, limits (or missing), tool_calls, tokens_to_ctx, agents, hooks_installed, permission_waits (or missing), queued_prompts, advice_count |
| `ledger [--last N]` | One object per turn: turn, started_at_ms, duration, api_calls, cache_read, cache_write, fresh_input, output, thinking, cost, tools (`Read×3 Bash×1`), compaction, effort, model |
| `tools` | `tools[]` (tool, calls, errors, running, p50, p95, last_call_at_ms, tokens_to_ctx) sorted by calls, and `top_ctx[]` (the 5 largest single results) |
| `files` | Per touched file: path, reads, edits, writes, touches, lines_added/removed (or missing outside git), reread_warning |
| `agents` | `agents[]` (id, type, description, model, state, elapsed, tokens), `mcp[]` (name, pid, rss, restarts, calls) or missing for fixtures, `tasks[]` |
| `advice` | Schema 2: `schema`, `session_mode` (interactive / loop / machine / team / workflow / remote), `primary` (the coach's slot occupant, with `acting`), `next` (the queued nudge and what promotes it), `items[]` (occupant first, then the ranked queue: rule, family, class NOW/NEXT/LATER, headline, evidence, action, action_text, action_kind, saving, since_turn, window_turns, retires_on, doc_key, explain), `snoozed[]` (rule, until_turn or null for the session), `suppressed[]` (rule, why), `recent[]` (retired nudges). Snoozes persist in `~/.cctop/<session>.advisor.json`, so the TUI, the pane and this query agree |
| `prefix` | `total` and `rows[]` (kind, name, bytes, tokens, count) of what rides on every request |
| `events [--since 10m]` | `[{at_ms, kind, text}]`; `--since` accepts `90s`, `10m`, `2h`, `1d` |
| `explain <metric_id>` | id, panel, name, unit, formula, sources, caveats, estimate_when — no session needed |

## Examples

```
$ cctop query summary | jq '.context.size, .cost'
{"value":396365,"unit":"tokens","metric_id":"context_size","approx":true}
{"value":9.90,"unit":"USD","metric_id":"cost","approx":false}

$ cctop query ledger --last 3 | jq '.[] | [.turn, .api_calls.value, .cost.value]'
[13, 16, 1.02]
[14, 52, 4.07]
[15, 0, null]

$ cctop query advice | jq '.primary | {class, rule, headline, saving}'
{"class":"NEXT","rule":"A25","headline":"EXPLORING ×8 · +31k ctx this run","saving":"~31k/turn"}

$ cctop query explain cache_hit_ratio | jq .formula
"cache_read / (cache_read + cache_write + fresh_input)"
```

The `summary` and `ledger` shapes are pinned by snapshot tests in `src/snapshots/`.
