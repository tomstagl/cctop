<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/svg/cctop-lockup-vertical-dark.svg">
    <img src="brand/svg/cctop-lockup-vertical-light.svg" alt="cctop" width="220">
  </picture>
</p>

<!-- hero:start -->
<p align="center"><strong>See what Claude Code is doing — live, in a pane beside it.</strong></p>
<!-- hero:end -->

<p align="center">
  <a href="https://github.com/tomstagl/cctop/releases">Releases</a> ·
  <a href="tasks/prd-cctop.md">PRD</a> ·
  <a href="ralph/prd.json">build plan</a> ·
  <a href="brand/README.md">brand</a> ·
  <a href="LICENSE">MIT</a>
</p>

---

<!-- lede:start -->
**cctop** is an `htop`/`btop`-style terminal dashboard for a running Claude Code session. It shows the internals Claude Code doesn't — context fill and when the next compaction hits, tokens and cost with cache-hit ratio, rate limits with an exhaustion forecast, what the current turn is waiting on, per-tool latency and how much context each tool pushed, subagents and MCP servers, touched files — on one page, in real time, in a right-hand split while you keep working on the left.
<!-- lede:end -->

Type `/cctop` in a Claude Code session and it appears — docked **inside**
Claude Code as a panel when the build supports it, otherwise attached as a
**terminal** split beside it. One command either way; see
[Two ways to see it](#two-ways-to-see-it).

## What it looks like

```
┌─cctop ── cctop-46 ──────────────────── Opus 5 · v2.1.269 ┐
│ ● BUSY  turn 14  02:31  ▶ Bash 0:48  ~/code/cctop main*  │
│ auto · medium · Max · 1h 12m · ≈ API $4.37 · 3% 412 MB   │
├─1 Context ───────────────────────────────────────── 67 % ┤
│ ▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁ │
│ 134k / 200k   sys+tools 28k · msgs 106k · free 66k       │
│ ▁▂▂▃▃▄▄▅▅▆▆▇  +9.4k/turn → autocompact in ~3 turns est   │
│ compactions 1 (turn 9, −71k)                             │
├─2 Tokens & Cost ───────────────────────────────── 1.91 M ┤
│ cache read  ▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇      1.62 M     │
│ cache write ▇▇▇                                 148k     │
│ fresh in    ▇                                    24k     │
│ output      ▇▇                                  118k     │
│  └ thinking ▇                                    61k     │
│ cache hit 90 %  ·  ≈ $4.37 ($3.61/h)  ·  in 12.4k/min    │
│ per turn ▂▁▃▂▄▂▂▆▁▃▂▅▃█   last turn 31k · $0.42          │
├─3 Limits ────────────────────────────────────────────────┤
│ 5 h  ▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▁▁▁▁▁▁▁▁▁▁▁▁▁  62 %  ↺ 1h 48m │
│      at this rate: exhausted in 2h 05m, after reset ✓    │
│ 7 d  ▇▇▇▇▇▇▇▇▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁  23 %  ↺ 4d 03h │
├─4 Turn ──────────────────────────────────────────────────┤
│ elapsed 2:31   api 3 calls · ttft 1.2 s · out 41 tok/s   │
│ ● Bash  0:48  cargo test --workspace          pid 8812   │
│ hooks 6 runs · 84 ms   permission waits 1 · 12 s         │
├─5 Tools ────────────────────────────────────── 212 calls ┤
│ TOOL         N  ERR    p50    p95   LAST  TOKENS→CTX     │
│ Read        71    0   38ms   90ms   0:02      41.2k      │
│ Bash        58    3   1.4s   9.8s   ▶now      22.9k      │
│ Edit        44    1   22ms   55ms   0:09       2.1k      │
│ Grep        23    0   61ms  140ms   1:10      11.6k      │
│ mcp:github   9    1  620ms   2.1s   4:02       8.8k      │
│ top ctx: Read src/render.rs 6.1k · Bash ls -R 4.4k       │
├─6 Agents & MCP ──────────────────────────────────────────┤
│ ◐ Explore  find render call sites    0:41   18k  sonnet  │
│ ✓ fork     review                    3:12   92k  opus    │
│ mcp github      pid 8231   41 MB   9 calls  p95 2.1s     │
│ mcp playwright  pid 8244  188 MB   0 calls  idle 12m     │
│ bg  cargo build --release          1:58   task #3        │
├─7 Files ─────────────────────────── 9 touched · +412 −87 ┤
│ src/render.rs      R×6  E×5  +210 −31   re-read ⚠        │
│ src/collect.rs     R×2  E×3  +98  −40                    │
│ tasks/prd-cctop.md W×1                                   │
├─9 Advisor ─────────────────────────────────────── 1 of 3 ┤
│ ▸ Bash ls -R pushed 4.4k tokens into context, twice.     │
│   Pipe through head -50 or use Glob.   ~4k/turn · n next │
├─8 Events ────────────────────────────────────────────────┤
│ 20:41:02 hook   PostToolUse Edit src/render.rs   12ms    │
│ 20:41:03 tool   Bash cargo test --workspace  ▶           │
│ 20:41:17 perm   Bash allowed (auto)                      │
│ 20:41:39 note   rate-limit 5h crossed 60 %               │
└ ?help 1-9 panels ⇥focus s sort f filter p pause q quit   ┘
```

Claude Code keeps running in the left pane; `cctop` attaches to it from the right. Rendered version with the wide layout: see the PRD.

## Two ways to see it

`/cctop` picks one automatically — it never asks you to choose.

**Terminal view.** The full nine-panel dashboard above, running as its own
process (`cctop run`) in a split of your terminal multiplexer (tmux, zellij,
WezTerm, Kitty, iTerm2). This is what `/cctop` falls back to, and what you get
from `cctop split` directly. See [Install & attach](#install--attach).

**Panel view.** On a Claude Code build with function hooks enabled, `/cctop`
docks the same dashboard *inside* Claude Code, above the prompt, drawn in
Claude Code's own frame and colour style — no multiplexer needed:

```
╭cctop ─ claude-sonnet-5 ──────────────────────────────────╮
│ ● BUSY  turn 1  0:48                                     │
│ auto · medium · $9.90             bin shim hooks 2.1.270 │
╰──────────────────────────────────────────────────────────╯
╭Context ─ 40 % ─────────────╮╭Tokens & Cost ─ 33.6M ──────╮
│ ▇▇▇▇▇▇▇▇▇▇▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁ ││ cache read  ▇▇▇▇▇    33.0M │
│ 396k / 1.0M (40 %)         ││ cache write ▁▁▁▁▁     597k │
│ velocity         +50k/turn ││ fresh input ▁▁▁▁▁      286 │
│ autocompact in    ≈9 turns ││ output      ▁▁▁▁▁      67k │
│ compactions              0 ││ └ thinking  ▁▁▁▁▁      31k │
│                            ││ cache hit             98 % │
│                            ││ cache TTL               1h │
│                            ││ cost                 $9.90 │
│                            ││ burn rate         ≈$26.0/h │
╰────────────────────────────╯╰────────────────────────────╯
╭Limits ─ 5h 42 % · 7d 17 % ─╮╭Turn ─ 0:48 ────────────────╮
│ 5 h          ▇▇▇▁▁▁   42 % ││ state                 busy │
│ 7 d          ▇▁▁▁▁▁   17 % ││ elapsed               0:48 │
│ resets in           2h 29m ││ api / tools   ≈0:02 / 0:46 │
│ exhausted in             — ││ waiting on       Bash 0:46 │
│                            ││ permission w…            — │
│                            ││ queued                   0 │
╰────────────────────────────╯╰────────────────────────────╯
```

This is the Overview; press a digit (`1`–`6`) from the composer to switch to
Tools, Agents, Files, Events or the Advisor, the same way `s`/`f`/`p` work in
the terminal view. It needs `"CLAUDE_CODE_ENABLE_FUNCTION_HOOKS": "1"` in the
`env` block of `~/.claude/settings.json` and `/tui fullscreen`; `cctop pane
status` tells you which prerequisite is missing. The `/diff` panel and the
cctop panel share one dock, so hide one to see the other — see
[`docs/claude-code-panels.md`](docs/claude-code-panels.md). Falls back to the
terminal view automatically when function hooks are off.

## Panels

<!-- panels:start -->
| # | Panel | Answers |
|---|---|---|
| 1 | **Context** | How full is the window, how fast is it filling, how many turns until autocompact |
| 2 | **Tokens & Cost** | Cache read / write / fresh / output / thinking, cache-hit ratio, $ and burn rate |
| 3 | **Limits** | 5 h and 7 d usage, reset countdown, will I run out before the reset |
| 4 | **Turn** | What the turn is doing right now, API vs tool time, hook overhead, permission waits |
| 5 | **Tools** | Calls, errors, p50/p95, and tokens each tool pushed into context |
| 6 | **Agents & MCP** | Subagents, MCP server processes, background tasks |
| 7 | **Files** | Blast radius and wasted re-reads |
| 8 | **Events** | Tool / hook / permission / compaction / note stream |
| 9 | **Advisor** | One evidence-backed recommendation at a time, ranked by tokens saved |
<!-- panels:end -->

The Advisor is rule-based (18 rules, no model call): cache misses, cache expiry, runaway tool results, re-reads, exploring in the main context, compaction churn, idle MCP servers, thinking share, permission waits, long foreground commands, pasted input, chatty turns, rate-limit pacing, subagent model choice, error loops, hook overhead, oversized prefix, missing hand-off.

Some numbers moved with the coach work: the turn count is Claude Code's own (`promptId`; interrupts, slash commands and task notifications no longer count, so it reads ~15 % lower than before), API-error lines no longer set the model or count as a compaction, compactions come from the `compact_boundary` records Claude Code writes since 2.1.263, and the autocompact threshold is the effective window − 13 000 tokens (967 k on 1M-window models) rather than 80 %.

## How it works

Read-only. No changes to Claude Code. Data comes from what Claude Code already writes:

- `~/.claude/sessions/*.json` — which sessions exist and whether they're busy
- `~/.claude/projects/<cwd>/<session>.jsonl` — the transcript: per-response usage (deduplicated by `message.id`), tool calls and results, turn durations, hook timings, Claude Code's own `cost-state`
- `…/<session>/subagents/` — subagent transcripts
- the status-line JSON (via an optional shim) — context size and rate limits
- hooks (optional) — exact tool timings, permission prompts, compactions
- the process tree — running commands, MCP servers, memory

`/clear` starts a new transcript under a new session id in the same Claude
Code process; the dashboard notices within 2 s and re-attaches to the new
session (a toast says so), and `cctop query --session <old id>` still answers
from the old transcript as an ended session.

Everything on screen is defined once in a metrics registry (`src/metrics/registry.rs`) that generates [`docs/metrics.md`](docs/metrics.md) and the reference below; CI fails if either drifts.

<!-- metrics:start -->
### Header

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Status** <a id="session_status"></a> `session_status` | enum | `status` from the session registry (busy/idle); WAITING when a permission request is pending; ENDED when the pid is gone | D1 D4 | — | never |
| **Turn** <a id="turn_number"></a> `turn_number` | count | Prompts the person wrote so far, one per `promptId` (`promptSource` typed / suggestion_accepted / queued, or `origin.kind` human); interrupts, slash commands, task notifications, teammate messages and the compaction summary are not turns | D2 | A resumed session starts counting at the resume point; before Claude Code 2.1.220 every non-meta text line counts | never |
| **Turn elapsed** <a id="turn_elapsed"></a> `turn_elapsed` | ms | `turn_duration.durationMs` once the turn ended, else now − turn start | D2 | — | never |
| **Effort** <a id="effort"></a> `effort` | enum | `perTurnEffort` of the latest assistant line when set, else its `effort`, else the status line's `effort.level`; with thinking on/off and fast mode from the status line | D2 D3 | — | never |
| **Plan** <a id="plan_tier"></a> `plan_tier` | enum | `oauthAccount.userRateLimitTier` (else `organizationRateLimitTier`) from `~/.claude.json` | D12 | The status line never carries a plan; keys are read, never the account's names | never |
| **CPU** <a id="process_cpu"></a> `process_cpu` | % | CPU share of the `claude` process over the last sample interval | D5 | — | never |
| **Memory** <a id="process_rss"></a> `process_rss` | bytes | Resident set size of the `claude` process | D5 | — | never |

### Context

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Context size** <a id="context_size"></a> `context_size` | tokens | `cache_read + cache_write + input` of the turn's last API call — everything the model read | D2 D3 | The status line's `total_input_tokens` is preferred when the shim is installed | est when computed from the transcript alone |
| **Context window** <a id="context_window"></a> `context_window` | tokens | `context_window_size` from the status line, else the model's default window | D3 | — | est without the status-line shim |
| **Fixed prefix** <a id="context_prefix"></a> `context_prefix` | tokens | `cache_read + cache_write` of the session's first API call: system prompt, CLAUDE.md, tool schemas | D2 | With a warm cache the first call is a read, so both fields are summed | never |
| **Context velocity** <a id="context_velocity"></a> `context_velocity` | tokens/turn | Exponential moving average (α = 1/5) of Δ context size per turn | D2 | Turns that compacted are excluded from the average | never |
| **Turns until autocompact** <a id="turns_until_compaction"></a> `turns_until_compaction` | turns | (autocompact threshold − context size) / context velocity | D2 D3 | Threshold = Claude Code's effective window − 13 000 tokens (967 000 on native-1M models, 187 000 on 200 k windows) until a compaction has been observed for the model, then the observed value is used | est until a compaction has been observed |
| **Context anatomy** <a id="context_anatomy"></a> `context_anatomy` | tokens | The stacked bar: prefix (first call's cache read + write) · tool inputs (chars the model wrote / 4) · tool results (`tokens_to_ctx`) · retained thinking · harness (attachments, `rendered` chars / 4) · prose (text blocks / 4) · unattributed (the rest of the size), summed since the last context boundary | D2 | Slices are scaled down together when their estimates overshoot the exact size | ≈ whenever a slice rests on chars / 4 or an attachment fallback |
| **Harness per turn** <a id="harness_tokens"></a> `harness_tokens` | tokens | Attachment tokens (reminders, injected files, listings) ÷ human turns since the last boundary | D2 | `rendered[].content` since Claude Code 2.1.266; per-subtype ratios before | ≈ before 2.1.266 |
| **Context band** <a id="context_band"></a> `context_band` | enum | ok below threshold − 20 000 · warn inside that band (Claude Code's footer turns to "Context low") · blocked at the threshold or window − 3 000; the footer text is Claude Code's own (`N% until auto-compact`, `N% context used` when autocompact is off); precompute armed at 80 % of the window | D2 D3 D12 | Overrides come from settings.json and the claude process environment (`CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, `DISABLE_AUTO_COMPACT`, `autoCompactWindow`, `autoCompactEnabled`) | never |
| **Compactions** <a id="compactions"></a> `compactions` | count | `system/compact_boundary` lines (exact: trigger, pre/post tokens, duration), or a PreCompact hook | D2 D4 | API-error lines (`<synthetic>`, zero usage) never count | Before Claude Code 2.1.263 a ≥ 30 % context drop between turns is taken as a compaction |

### Tokens & Cost

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Cache read** <a id="cache_read"></a> `cache_read` | tokens | Σ `cache_read_input_tokens` over distinct API responses | D2 | Counted once per `message.id`; Claude Code writes one line per content block | never |
| **Cache write** <a id="cache_write"></a> `cache_write` | tokens | Σ `cache_creation_input_tokens`, split into 5-minute and 1-hour TTL from `cache_creation.ephemeral_*` | D2 | — | never |
| **Fresh input** <a id="fresh_input"></a> `fresh_input` | tokens | Σ `input_tokens` (uncached) | D2 | — | never |
| **Output** <a id="output"></a> `output` | tokens | Σ `output_tokens` | D2 | — | never |
| **Thinking** <a id="thinking"></a> `thinking` | tokens | Σ `output_tokens_details.thinking_tokens` (a subset of output) | D2 | — | never |
| **Cache hit ratio** <a id="cache_hit_ratio"></a> `cache_hit_ratio` | ratio | cache_read / (cache_read + cache_write + fresh_input) | D2 | Green ≥ 0.8, amber ≥ 0.5, red below | never |
| **Cache TTL** <a id="cache_ttl"></a> `cache_ttl` | enum | `prompt_cache.ttl` from the status line; else 1h if the latest call reports `ephemeral_1h_input_tokens > 0`, else 5m | D3 D2 | — | ≈ without the shim |
| **Cache warm** <a id="cache_warm"></a> `cache_warm` | bool | `prompt_cache.warm` from the status line; else whether the last API call is younger than the observed TTL | D3 D2 | — | ≈ without the shim |
| **Cache expires in** <a id="cache_expires_in"></a> `cache_expires_in` | ms | `prompt_cache.expires_at` − now, clock-driven between status rewrites; else last API call + observed TTL − now | D3 D2 | The status file is rewritten only at expiry, so the countdown runs on cctop's clock; the shim's figure is ignored when the file predates the last assistant line | ≈ without the shim, or when the status file is stale |
| **Re-cache if cold** <a id="cache_recache_if_cold"></a> `cache_recache_if_cold` | tokens | `prompt_cache.recache_tokens_if_cold`: what the next call re-writes if the cache expires first | D3 | — | never |
| **Cache misses** <a id="cache_misses"></a> `cache_misses` | count | `prompt_cache.misses` with `miss_causes` (model_changed, tools_changed, messages_rewritten, ttl_expired_1h/5m, likely_server_side…) and `expected_rebuilds` | D3 | Claude Code counts a miss when the cache read is < 95 % of the input and ≥ 2 000 tokens were re-processed | never |
| **Cost** <a id="cost"></a> `cost` | USD | Claude Code's `cost-state.totalCostUSD` plus a priced estimate of responses newer than that line | D11 D2 D9 | Subscription plans have no per-token bill; the figure is the API-equivalent list price | ≈ when any part is estimated |
| **Cost by model** <a id="cost_by_model"></a> `cost_by_model` | USD | `cost-state.modelUsage[*].costUSD` plus estimates per model | D11 D9 | — | ≈ when any part is estimated |
| **Burn rate** <a id="burn_rate"></a> `burn_rate` | USD/h | Cost of turns active in the trailing 15 minutes, scaled to an hour over the part of the window they cover | D2 D9 | A turn counts from its start (clamped to the window) to its last line; windows shorter than 1 minute are treated as 1 minute | ≈ (always priced from the table) |
| **Input rate** <a id="input_rate"></a> `input_rate` | tokens/min | Total input tokens of turns started in the trailing 15 minutes ÷ window | D2 | — | never |

### Limits

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **5-hour usage** <a id="limit_5h"></a> `limit_5h` | % | `rate_limits.five_hour.used_percentage` from the status line | D3 | Account-wide: other live sessions contribute | never |
| **7-day usage** <a id="limit_7d"></a> `limit_7d` | % | `rate_limits.seven_day.used_percentage` from the status line | D3 | Account-wide | never |
| **Resets in** <a id="limit_reset"></a> `limit_reset` | duration | `resets_at` − now | D3 | — | never |
| **Projected exhaustion** <a id="limit_exhaustion"></a> `limit_exhaustion` | duration | Least-squares slope of used_percentage samples over the last 30 min, extrapolated to 100 % | D3 | Needs ≥ 3 samples; rate-limit units are plan-specific, so tokens are not used | ≈ always |

### Turn

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Turn duration** <a id="turn_duration"></a> `turn_duration` | ms | `turn_duration.durationMs` system line written when the turn ends | D2 | — | never |
| **API calls** <a id="api_calls"></a> `api_calls` | count | Distinct `message.id`s in the turn | D2 | — | never |
| **API time** <a id="api_time"></a> `api_time` | ms | `cost-state.totalAPIDuration` for the session; per turn, gaps between a user/tool_result line and the next assistant line | D11 D2 | — | ≈ per turn |
| **Retry time** <a id="retry_time"></a> `retry_time` | ms | `totalAPIDuration − totalAPIDurationWithoutRetries` | D11 | — | never |
| **Hook runs** <a id="hook_runs"></a> `hook_runs` | count | Number of `hookInfos` entries in the turn's `stop_hook_summary` | D2 | Only Stop hooks are summarised by Claude Code; other hook events need `cctop install` | never |
| **Hook time** <a id="hook_ms"></a> `hook_ms` | ms | Σ `hookInfos[].durationMs` for the turn | D2 D4 | — | never |
| **Permission wait** <a id="permission_wait"></a> `permission_wait` | ms | PermissionRequest → PostToolUse for the same tool_use_id, minus the tool's median duration | D4 | PreToolUse fires before the prompt, so it cannot bound the wait | ≈ always |
| **Queued prompts** <a id="queued_prompts"></a> `queued_prompts` | count | `queue-operation` enqueue − dequeue/remove; popAll resets to 0 | D2 | — | never |

### Tools

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Calls** <a id="tool_calls"></a> `tool_calls` | count | `tool_use` blocks per tool name; MCP tools grouped as `mcp:<server>` | D2 | — | never |
| **Errors** <a id="tool_errors"></a> `tool_errors` | count | `tool_result` blocks with `is_error` per tool | D2 | — | never |
| **p50 duration** <a id="tool_p50"></a> `tool_p50` | ms | Median of tool_use → tool_result durations | D2 D4 | Transcript timings include any permission wait | ≈ until hook timings replace them |
| **p95 duration** <a id="tool_p95"></a> `tool_p95` | ms | 95th percentile (nearest rank) of durations | D2 D4 | — | ≈ until hook timings replace them |
| **Last call** <a id="tool_last_call"></a> `tool_last_call` | duration | now − the tool's most recent `tool_use` timestamp | D2 | — | never |
| **Tokens → context** <a id="tokens_to_ctx"></a> `tokens_to_ctx` | tokens | Σ len(result text) / 4 per tool | D2 D10 | Heuristic; exact with OpenTelemetry. Uses the truncated text in the transcript, not offloaded `tool-results/` files | ≈ without OTel |
| **Top context consumers** <a id="top_ctx"></a> `top_ctx` | tokens | The n single results with the largest `tokens_to_ctx` | D2 | — | ≈ without OTel |

### Agents & MCP

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Agent state** <a id="agent_state"></a> `agent_state` | enum | running while tool_uses are pending; done when the last response ends with text and no pending tool_use; failed when the last result is an error and nothing followed for 60 s | D2a D4 | — | never |
| **Agent tokens** <a id="agent_tokens"></a> `agent_tokens` | tokens | Deduplicated usage of the agent's own transcript | D2a | — | never |
| **MCP memory** <a id="mcp_rss"></a> `mcp_rss` | bytes | RSS of the MCP server process | D5 | — | never |
| **MCP calls** <a id="mcp_calls"></a> `mcp_calls` | count | Calls of tools named `mcp__<server>__*` | D2 | — | never |

### Files

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Touches** <a id="file_touches"></a> `file_touches` | count | Read / Edit / Write / MultiEdit / NotebookEdit calls per file path, plus Bash `cat` / `sed -n` / `head` / `tail` reads of it | D8 | A read counts when its result arrives | never |
| **Lines ±** <a id="file_lines"></a> `file_lines` | lines | `git diff --numstat` against HEAD at attach time | D7 | Outside a git repo the column is empty | never |
| **Re-reads** <a id="file_rereads"></a> `file_rereads` | count | Whole-file reads (Read or a Bash reader) with no Edit/Write in between; ⚠ at ≥ 3 | D8 | Ranged reads (offset/limit) and `file_unchanged` results do not count; the counter resets when the file changed under the model (an IDE edit, a stale-read recovery) and at every context boundary | never |

### Advisor

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Estimated saving** <a id="advice_saving"></a> `advice_saving` | tokens|seconds | Rule-specific estimate of what following the advice saves per remaining turn | D2 | Ranking key; always an estimate | ≈ always |
<!-- metrics:end -->

## Ask your session about it

`cctop query … --json` exposes every number, and the bundled `cctop-insights` skill teaches Claude Code to use it — ask *"why is my cache hit ratio low?"* in the session and get numbers plus one change to make. See [`plugin/skills/cctop-insights/SKILL.md`](plugin/skills/cctop-insights/SKILL.md).

## Install & attach

<!-- install:start -->
```
brew install tomstagl/tap/cctop           # currently v0.2.0; or: cargo install cctop
claude plugin marketplace add tomstagl/cctop
claude plugin install cctop               # adds /cctop and cctop-insights
/cctop                                     # opens the dashboard: panel or terminal split
```
<!-- install:end -->

`/cctop` is the only command you need — see [Two ways to see
it](#two-ways-to-see-it) for what decides panel vs. terminal, and
[`docs/claude-code-panels.md`](docs/claude-code-panels.md) for how the panel
and the built-in `/diff` panel share one dock. If neither the panel nor a
multiplexer split can attach, `/cctop` prints exactly what is missing and how
to fix it — a rate-limit shim install, a terminal that isn't tmux/zellij/
WezTerm/Kitty/iTerm2, or `cctop run --session <id>` to run it by hand in a
second terminal.

## Repository

| Path | What |
|---|---|
| `tasks/prd-cctop.md` | Product requirements, v1.1 |
| `ralph/prd.json` | 43 dependency-ordered implementation stories |
| `plugin/skills/` | Claude Code plugin skills |
| `brand/` | Logo (SVG/PNG), build script, candidates |

## License

[MIT](LICENSE). Brand fonts are IBM Plex under the [SIL OFL](brand/fonts/LICENSE.txt).
