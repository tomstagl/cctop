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
 cctop  claude-sonnet-5 · turn 6 · 9h 25m · ENDED…  ● COMMITTING · 4c +180 · silent 3:09 · ▸ steer …
  ▄█ █ █   ○ context                              █▀▀ █▀█   ○ cache
   █ ▀▀█   142k of 1.00M                          ▀▀█ ▀▀█   warm · 59m (1h) TTL
   ▀   ▀ % ≈$.03/call                             ▀▀▀ ▀▀▀ m misses 0

       ○ limits                                    ▄█   ● rework
 ▀▀▀   no status line                               █   1 · correction
     %                                              ▀   edits 3 ✓ none 9h00
 ▸ Fixed prefix 49k tokens ≈$0.15/turn at 15 calls — trim CLAUDE.md, move rarely-use… LATER · turn 6
 1 Context   ▇▇▇▇▇▇▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁ prefix 49k · inputs ≈538 · results ≈180 · thi…
             +146k/turn · autocompact 567k · 425k left · compactions 1 · since clear 23:29 · re-rea…
 2 Tokens    cache read 28.46M · write 311k · output 107k · warm 59m ≈
             $18.7 · $1.01/h · $/call ≈$.03 · $/turn ≈$.50 · next 30c ≈$1.0 · skill:admit 1 %
 3 Limits    no status line · weight ×3 (sonnet) · long_context 89 %
             89% of your usage was at >150k context
 4 Turn      COMMITTING · 4c +180 · silent 3:09 · ▸ steer window · api ≈3:48 · tools 8h 35m
             elapsed 8h 40m · 4 api calls · 4 tool calls
 5 Tools     140 calls · 6 err · explore 39 (3 ✗) · commit 8 · test 8 · Read 30 · Edit 29 · AskUser…
             Denied 4 · Other 2
 6 Agents    —

 7 Files     44 touched · 40cde3.md E×1 · c1c889.rs E×2 IDE edit · commit 0:00 ago

 8 Events    23:29 api cost-state $18.75 · api 22:14 · retries… · 23:29 note /clear · continued in …
             23:28 tool Bash ✓ 49 · 23:28 tool Bash git commit -m "$(cat lorem_i lorem… · 23:27 too…
 9 Advisor   LATER A17 · next long-foreground → when the slot frees · `c… · snoozed —
             LATER A10 `cd lorem_ipsum_dolor_sit_amet…` blocke…


 ?help  1-9 open a panel full-screen  c coach  a ask  t theme  q
```

Claude Code keeps running in the left pane; `cctop` attaches to it from the right. Four levels of type: the header line, four tiles (the coach's lights — context, cache, limits, rework — as block digits), the one nudge, and a nine-row ledger whose digit opens that panel full-screen (`Esc` back). Press `c` for the coach view: the same four lights as a 56-column card with the nudge, what is next and what is snoozed.

## Two ways to see it

`/cctop` picks one automatically — it never asks you to choose.

**Terminal view.** The dashboard above, running as its own
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

This is the Overview; the view bar switches to the Coach, Tools, Agents,
Files, Events or the Advisor, the same way `c`/`s`/`f`/`p` work in the
terminal view. The Coach view draws the same 56-column card as the TUI's
`c` view — the state line, four lights, the one nudge — with `[1 fill]`
(writes a prompt-class action into the prompt box; nothing is ever
submitted), `[2 snooze]` and `[3 why]`, and pins the coach's one-line form
under the prompt. It needs `"CLAUDE_CODE_ENABLE_FUNCTION_HOOKS": "1"` in the
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

The Advisor is rule-based (35 rules today, no model call). On the token axis: named cache misses, cache expiry, the cache countdown while a question waits, runaway tool results, re-reads, exploration runs in the main context, post-compaction re-triggers, idle MCP servers and plugins, thinking share, permission waits, long foreground commands, pasted input, chatty turns, rate-limit pacing, subagent model choice, hook overhead, oversized prefix, a warm model switch, a cold resume, the context cost past 200 k, an armed loop. On the outcome axis: the turn that died on an API error, Claude waiting on you, a failure cascade, a denial streak (with the allow rule), a correction streak (Esc Esc), a commit without a check, source edits with no test run, a natural boundary to /clear at, a PR without a review pass, destructive git on a dirty tree, an IDE/Claude edit collision; plan-first and long-context drift sit in the next row only. Every trigger is structural (a tool result, a denial kind, an interrupt marker, an API-error line, a git operation), never a keyword in your prompt. Its engine keeps one nudge in a slot by class (NOW › NEXT › LATER), with hard TTLs, cooldowns, an `acted` predicate per rule and persistent snoozes (`x` five turns, `X` the session), and the coach view (`c`) shows that slot beside four lights: context, cache, limits, rework.

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
| **Cost per call** <a id="cost_per_call"></a> `cost_per_call` | USD | context × cache-read price + median output × output price, at the current context; at the cache-write price of the observed TTL when the cache is cold | D2 D3 D9 | What the next API call costs, not what the last one did | ≈ (always priced from the table) |
| **Cost per turn** <a id="cost_per_turn"></a> `cost_per_turn` | USD | cost per call × the session's own median calls per turn (turns with ≥ 1 call), also given at 100 k of context; `next 30 calls` = cost per call × 30 | D2 D9 | The median is the session's, never a constant | ≈ (always priced from the table) |
| **Where the tokens went** <a id="attribution"></a> `attribution` | ratio | Input tokens of API responses by their `attributionSkill` / `attributionPlugin` / `attributionAgent` / `attributionMcpServer` owner, machine-originated turns under `idle`, the subagents' own usage under `agents`; shares of all input tokens | D2 D6 | A response without an attribution key is the person's own work | never |
| **Agents cost** <a id="agents_cost"></a> `agents_cost` | USD | Priced usage of every subagent transcript (incl. `subagents/workflows/**`) and its share of the session's total | D6 D9 | — | ≈ (priced from the table) |
| **Limit weight** <a id="limit_weight"></a> `limit_weight` | ratio | `/usage`'s weight of a call: (cached + uncached × 10 + cache-create × 12.5 + output × 50) × tier (fable 10, opus 5, sonnet 3, haiku 1) | D2 D12 | Why the limit bar moves faster than dollars | never |
| **Behaviour flags** <a id="behaviour_flags"></a> `behaviour_flags` | % | `/usage`'s five flags as shares of the weighted usage: cache_miss (requests with > 100 k uncached tokens), long_context (> 150 k context), subagent_heavy, high_parallel (≥ 4 live sessions), cron (active ≥ 8 h); shown with Claude Code's own tip text at ≥ 10 % | D2 D1 D12 | — | never |
| **Burn rate** <a id="burn_rate"></a> `burn_rate` | USD/h | Cost of turns active in the trailing 15 minutes, scaled to an hour over the part of the window they cover | D2 D9 | A turn counts from its start (clamped to the window) to its last line; windows shorter than 1 minute are treated as 1 minute | ≈ (always priced from the table) |
| **Input rate** <a id="input_rate"></a> `input_rate` | tokens/min | Total input tokens of turns started in the trailing 15 minutes ÷ window | D2 | — | never |

### Limits

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **5-hour usage** <a id="limit_5h"></a> `limit_5h` | % | `rate_limits.five_hour.used_percentage` from the status line | D3 | Account-wide: other live sessions contribute | never |
| **7-day usage** <a id="limit_7d"></a> `limit_7d` | % | `rate_limits.seven_day.used_percentage` from the status line | D3 | Account-wide | never |
| **Resets in** <a id="limit_reset"></a> `limit_reset` | duration | `resets_at` − now | D3 | — | never |
| **Rate limited** <a id="limit_hit"></a> `limit_hit` | enum | The newest API-error line with `error: rate_limit` (or status 429): `quotaLimits.rateLimitType`, `resetsAt`, `lowPriorityRetryAfterSeconds` — cleared by the next successful call | D2 | Exact without the shim: Claude Code writes the 429 into the transcript | never |
| **Spend limit** <a id="spend_limit"></a> `spend_limit` | % | `rate_limits.spend_limit.used_percentage` from the status line, for accounts with a monthly limit | D3 | — | never |
| **Other sessions** <a id="other_sessions"></a> `other_sessions` | list | Live registry entries other than this one: busy/idle and how long (`statusUpdatedAt`) | D1 | They share the rate limit | never |
| **Projected exhaustion** <a id="limit_exhaustion"></a> `limit_exhaustion` | duration | Least-squares slope of used_percentage samples over the last 30 min, extrapolated to 100 % | D3 | Needs ≥ 3 samples; rate-limit units are plan-specific, so tokens are not used | ≈ always |

### Turn

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Turn duration** <a id="turn_duration"></a> `turn_duration` | ms | `turn_duration.durationMs` system line written when the turn ends | D2 | — | never |
| **API calls** <a id="api_calls"></a> `api_calls` | count | Distinct `message.id`s in the turn | D2 | — | never |
| **API time** <a id="api_time"></a> `api_time` | ms | `cost-state.totalAPIDuration` for the session; per turn, gaps between a user/tool_result line and the next assistant line | D11 D2 | — | ≈ per turn |
| **Retry time** <a id="retry_time"></a> `retry_time` | ms | `totalAPIDuration − totalAPIDurationWithoutRetries` | D11 | — | never |
| **Phase** <a id="phase"></a> `phase` | enum | The last call's phase over the last seven calls (`phase.rs`: EXPLORING / IMPLEMENTING / VERIFYING / COMMITTING / PLANNING / DELEGATING / BROWSING / OPS / WAITING) and its run length; WAITING when a permission dialog, an `AskUserQuestion` or a Notification is pending | D2 D4 | A test-class command is VERIFYING only once its output confirmed a run | never |
| **Last check** <a id="last_check"></a> `last_check` | enum | The newest test-class Bash call whose output confirmed a run (`test result:`, `N passed`, `# pass`…), its verdict and age; `edits since` counts Edit/Write calls after it | D2 | — | never |
| **Waiting on you** <a id="waiting"></a> `waiting` | enum | A pending permission dialog (hook), a running `AskUserQuestion` / `ExitPlanMode`, an `idle_prompt` / `agent_needs_input` notification, or a finished turn whose last text ended with `?`; with the wait's duration | D2 D4 | — | never |
| **Steers** <a id="steers"></a> `steers` | count | Human `queued_command` attachments folded into the turn (absorbed mid-turn); task notifications are machine turns, not steers | D2 | — | never |
| **Interrupts** <a id="interrupts"></a> `interrupts` | count | `[Request interrupted by user…]` lines with `interruptedMessageId`, and the output tokens the cut turns had produced | D2 | — | never |
| **Hook time by command** <a id="hook_by_command"></a> `hook_by_command` | ms | `stop_hook_summary.hookInfos[].command` and `hook_success` attachments summed per command over the session; `preventedContinuation` marks a blocked stop | D2 | — | never |
| **Goal** <a id="goal"></a> `goal` | enum | The last `goal_status` attachment (`/goal`): met, iterations, tokens | D2 | — | never |
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
| **Tokens → context** <a id="tokens_to_ctx"></a> `tokens_to_ctx` | tokens | Σ len(result text) / 4 per tool, plus `w·h/750` per image (1 500 when the size is unknown); cleared results count 0 | D2 D10 | Heuristic; exact with OpenTelemetry. Uses the text in the transcript, not offloaded `tool-results/` files (their size is shown beside it) | ≈ without OTel |
| **Input → context (IN→CTX)** <a id="input_tokens"></a> `input_tokens` | tokens | Characters the model wrote as tool inputs / 4, per tool — they stay in context like results do | D2 | Bash command text is the largest share | ≈ always |
| **Bash by class** <a id="bash_class"></a> `bash_class` | count | Bash calls by the phase classifier's class: explore / implement / test / build-lint / commit / gitread / ops / wait (`Bash·test` rows) | D2 | — | never |
| **Error class** <a id="error_class"></a> `error_class` | count | Failed calls by Claude Code's own taxonomy (Command Failed / User Rejected / Edit Failed / File Changed / File Too Large / File Not Found / Other) plus Content Not Found, Timeout, Tool Not Found and Denied (`toolDenialKind`) | D2 | Classified from the result text, in Claude Code's order | never |
| **Top context consumers** <a id="top_ctx"></a> `top_ctx` | tokens | The n single results with the largest `tokens_to_ctx`; ⊘ marks a result cut at a cap (`truncatedByTokenCap`, a persisted spill) | D2 | — | ≈ without OTel |
| **Re-read tax** <a id="reread_tax"></a> `reread_tax` | USD | API calls since the result landed × its tokens × the cache-read price: what re-reading it has cost so far | D2 D9 | — | ≈ always |
| **ToolSearch loads** <a id="tool_search_loads"></a> `tool_search_loads` | count | Deferred tools loaded through `ToolSearch` per MCP server (`matches` of its result); each load rewrites the cached prefix | D2 | — | never |

### Agents & MCP

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Agent state** <a id="agent_state"></a> `agent_state` | enum | running while tool_uses are pending; done when the last response ends with text and no pending tool_use; failed when the last result is an error and nothing followed for 60 s | D2a D4 | — | never |
| **Agent tokens** <a id="agent_tokens"></a> `agent_tokens` | tokens | Deduplicated usage of the agent's own transcript | D2a | — | never |
| **MCP memory** <a id="mcp_rss"></a> `mcp_rss` | bytes | RSS of the MCP server process | D5 | — | never |
| **MCP calls** <a id="mcp_calls"></a> `mcp_calls` | count | Calls of tools named `mcp__<server>__*` | D2 | — | never |
| **Workflow runs** <a id="agent_workflows"></a> `agent_workflows` | count | `subagents/workflows/<run>/journal.jsonl`: agents launched, finished (`result`) and `failed` per run; the run's agents are scanned like the top-level ones | D2a | — | never |
| **Spawn depth** <a id="agent_depth"></a> `agent_depth` | count | Deepest `spawnDepth` among the agents (Claude Code caps it at 3) | D2a | — | never |
| **Teammates** <a id="teammates"></a> `teammates` | list | Members of `~/.claude/teams/<team>/config.json` when this session leads the team | D12 | — | never |
| **MCP needs auth** <a id="mcp_auth"></a> `mcp_auth` | list | `deferred_tools_delta.needsAuthMcpServers` / `failedMcpServers` from the transcript | D2 | — | never |

### Files

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Touches** <a id="file_touches"></a> `file_touches` | count | Read / Edit / Write / MultiEdit / NotebookEdit calls per file path, plus Bash `cat` / `sed -n` / `head` / `tail` reads of it | D8 | A read counts when its result arrives | never |
| **Lines ±** <a id="file_lines"></a> `file_lines` | lines | `git diff --numstat` against HEAD at attach time | D7 | Outside a git repo the column is empty | never |
| **Uncommitted** <a id="uncommitted"></a> `uncommitted` | lines | `git diff --numstat HEAD` (added, removed, files) and the last commit Claude Code summarised (`gitOperation.commit`) with the edits since it | D7 D2 | — | never |
| **Rewind points** <a id="rewind_points"></a> `rewind_points` | count | `file-history-snapshot` lines in the current turn (checkpoints `/rewind` can restore) and the Bash writes of the turn no checkpoint covers; per file: the checkpoint version (`file-history-delta`, ⚠ at v8+), IDE edits (`edited_text_file`), stale markers (`staleRecovered`, `staleReadFileStateHint`), edit → re-read → edit churn | D2 | — | never |
| **Re-reads** <a id="file_rereads"></a> `file_rereads` | count | Whole-file reads (Read or a Bash reader) with no Edit/Write in between; ⚠ at ≥ 3 | D8 | Ranged reads (offset/limit) and `file_unchanged` results do not count; the counter resets when the file changed under the model (an IDE edit, a stale-read recovery) and at every context boundary | never |

### Advisor

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Estimated saving** <a id="advice_saving"></a> `advice_saving` | tokens|seconds | Rule-specific estimate of what following the advice saves per remaining turn | D2 | Ranking key; always an estimate | ≈ always |

### Coach

| Metric | Unit | How it is computed | Sources | Caveats | Estimate |
|---|---|---|---|---|---|
| **Context light** <a id="coach_context"></a> `coach_context` | percent | The context size as % of the exact window; ○ below 150k, ◐ from 150k (or ≥ 300k on a 1M window while a turn runs), ● inside the autocompact warn band (effective window − 13 000 − 20 000) or at ≥ 300k with a clean stop available | D1 D3 | Never a fixed 80 %: a deliberate 1M session sits amber | ≈ when the window is the model default |
| **Cache light** <a id="coach_cache"></a> `coach_cache` | minutes|tokens | Minutes of cache left (`prompt_cache.expires_at`, else last call + observed TTL ≈), or the re-write size when cold; ◐ inside the countdown band (the last 5 min of a 1 h entry, 2 min of a 5 m one), ● when a reply now would save ≥ 50k | D1 D3 | — | ≈ without the status-line shim |
| **Limits light** <a id="coach_limits"></a> `coach_limits` | percent | The 5 h window used; ◐ when the exhaustion fit lands before the reset or ≥ 80 %, ● on a rate-limit or spend-limit error line; `—` without the status line | D3 D1 | — | never |
| **Rework light** <a id="coach_rework"></a> `coach_rework` | count | Open issues: consecutive failed calls of the turn (denials excluded), corrections (interrupts, rejected calls) in the last three turns, blocked calls; else the source edits since the last confirmed test run. ◐ after 10 min or 14 calls unverified, two fails, a PR without a review, an uncommitted tail; ● on a cascade, a denial streak, a correction streak, destructive git on a dirty tree, a commit without a check | D1 D7 | — | never |
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
