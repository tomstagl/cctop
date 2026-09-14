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
| `summary` | Session identity, turns, api calls, context (size/window/ratio/prefix/velocity/turns_until_compaction/compactions), tokens (five classes + cache_hit_ratio + cache_ttl), cost (+ by model), burn_rate, input_rate, limits (or missing), tool_calls, tokens_to_ctx, agents, hooks_installed, permission_waits (or missing), queued_prompts, advice_count, `insights` (Claude Code's own `/insights` analysis for this cwd: `computed_at_ms`, `sessions`, `project` medians — duration, prompts, interruptions, tool errors, commits, response time — the satisfied share, outcome and friction counts, and the first turn's `start_line`; counts and enum-like verdicts only, never `first_prompt`, `underlying_goal`, `brief_summary` or `friction_detail`; missing on a fixture) |
| `ledger [--last N]` | One object per turn: turn, started_at_ms, duration, api_calls, cache_read, cache_write, fresh_input, output, thinking, cost, tools (`Read×3 Bash×1`), compaction, effort, model |
| `tools` | `tools[]` (tool, calls, errors, running, p50, p95, last_call_at_ms, tokens_to_ctx) sorted by calls, and `top_ctx[]` (the 5 largest single results) |
| `files` | Per touched file: path, reads, edits, writes, touches, lines_added/removed (or missing outside git), reread_warning |
| `agents` | `agents[]` (id, type, description, model, state, elapsed, tokens), `mcp[]` (name, pid, rss, restarts, calls) or missing for fixtures, `tasks[]` |
| `advice` | Schema 2: `schema`, `session_mode` (interactive / loop / machine / team / workflow / remote), `primary` (the coach's slot occupant, with `acting`), `next` (the queued nudge and what promotes it), `items[]` (occupant first, then the ranked queue: rule, family, class NOW/NEXT/LATER, headline, evidence, action, action_text, action_kind, saving, since_turn, window_turns, retires_on, doc_key, explain), `snoozed[]` (rule, until_turn or null for the session), `suppressed[]` (rule, why), `recent[]` (retired nudges). Snoozes, the slot occupant and the promotion budget persist in `~/.cctop/<session>.advisor.json` (the dashboard writes, everyone else reads), so the TUI, the pane and this query show the same nudge |
| `coach [--line] [--columns N] [--snooze RULE] [--snooze-session RULE]` | The coach object ("Lights"): `state` (kind, line, tokens), `lights[4]` (id, level quiet/watch/act, glyph, number, figure, unit, text, source, approx, lines[3]), `agents`, `nudge` (id, family, class, line1, line2, evidence, action_text, action_kind, since_turn, retires_on, acting, queued, saving, explain), `next` (id, class, headline, promotes, row), `snoozed[]`, `recent[]`, `suppressed[]`, `session_mode`, `nudges_this_hour`. Every line is cut at 52 cells so the TUI, the pane and this output show the same text. `--line` prints the one-line form (L0 at ≥ 80 columns, L1 at ≥ 40, L2 below); `--snooze` applies a five-turn snooze first (queued for the dashboard while it runs), `--snooze-session` one for the session. `--lines N` (global) reads only the first N transcript lines of a fixture; `CCTOP_FAKE_NOW` (epoch ms) moves the clock |
| `dashboard` | The drawn dashboard (plan B): `header` (session, model, version, turn, elapsed, cwd, pr, phase {glyph, word, tokens}, line), `tiles[4]` (the coach's lights enlarged: id, level, glyph, figure, unit, name, sub1, sub2, source, approx), `nudge` (id, class, line, tag, acting) or null, `rows[9]` (digit, name, values, detail — lines of `{text, tone}` segments, tone ∈ fg/dim/accent/ok/warn/crit/bold, cut at 118 cells), `session_mode`, `lines` (the coach's L0/L1/L2) |
| `prefix` | `total` and `rows[]` (kind, name, bytes, tokens, count) of what rides on every request |
| `events [--since 10m]` | `[{at_ms, kind, text}]`; `--since` accepts `90s`, `10m`, `2h`, `1d` |
| `baseline` | Your last 7 days as medians (`~/.cctop/baseline.json`, recomputed hourly): sessions, cost_per_turn, tokens_per_turn, calls_per_turn, calls_per_session, cache_hit_ratio, tool_error_rate, error_categories (share by class), interruption_rate, commit_without_check_ratio, model_mix, active_hours (messages per local hour of day from `usage-data`; feeds `limits.exhaustion_in_active_hours`) |
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

$ cctop query coach --line
○142k ≈$.03 · ○cache 59m · ○5h — · ●corrections 1 · ▸ trim CLAUDE.md, move rarely-used rules to skills,…

$ cctop query explain cache_hit_ratio | jq .formula
"cache_read / (cache_read + cache_write + fresh_input)"
```

The `summary` and `ledger` shapes are pinned by snapshot tests in `src/snapshots/`.
