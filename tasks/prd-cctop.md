# PRD: cctop — a btop-style live dashboard for Claude Code

**Status:** Draft v1.1 · 2026-09-11 (reviewed against real transcripts, see §14)
**Target:** Claude Code CLI ≥ 2.1 on macOS/Linux, inside tmux / zellij / iTerm2 / WezTerm / Kitty / Ghostty

> Assumptions made in lieu of clarifying questions (the `/goal` directive asked to proceed without pausing):
> A. Primary user is a solo power user watching *their own* interactive session, not an ops team.
> B. v1 is read-only: cctop observes, it never drives the session.
> C. Implementation language is Rust + ratatui (decided 2026-09-11).
> D. Distribution is a Homebrew/cargo binary plus a thin Claude Code plugin that adds `/cctop`.
> Visual mockup (narrow + wide, beside a live session): https://claude.ai/code/artifact/d3ceb159-9033-43b2-9ae4-e5e262407167
> See §9 Decisions for what was settled and the one question still open.

---

## 1. Introduction

Claude Code exposes almost nothing about what it is doing while it works: the status line shows context % and a spinner, and everything else — token spend, cache behaviour, which tool is running, how long a turn has been going, whether a subagent is alive, when the next compaction will hit, how close you are to the 5-hour rate limit — is either hidden or scattered across `~/.claude/*.jsonl` files.

**cctop** is an `htop`/`btop`-style terminal dashboard that renders the internals of one running Claude Code session on a single page, in real time, in a pane beside the session. You keep typing in Claude Code on the left; cctop on the right tells you what's happening under the hood.

It is read-only, needs no changes to Claude Code itself, and gets its data from files and processes Claude Code already produces.

## 2. Goals

- Tell the user **what to change**, not only what is happening: an Advisor panel gives one evidence-backed cost/effectiveness recommendation at a time.
- Show the state of the current session on **one page** with no scrolling required at ≥ 50 cols × 35 rows; degrade gracefully down to 40 × 24.
- **Real-time:** any event visible in cctop within 250 ms of it being written by Claude Code (transcript append, hook fire, status-line update).
- **Side-by-side:** one command (`cctop` or `/cctop`) opens the dashboard in a right-hand split of the same terminal multiplexer, attached to the session in the neighbouring pane, without interrupting the session.
- **Cheap:** < 2 % CPU idle, < 5 % while a turn is running, < 50 MB RSS, < 200 ms cold start.
- **Insightful, not just decorative:** every panel answers a question a user actually asks ("why is context filling so fast?", "am I about to hit the rate limit?", "what is this turn waiting on?").
- Feel like a first-class TUI: mouse + keyboard, vim keys, 256/truecolour themes, braille sparklines, diff-rendered so it never flickers.

## 3. Users & scenarios

| Persona | Scenario | What cctop shows them |
|---|---|---|
| Solo developer on a Pro/Max plan | Long refactor session, worried about hitting the 5 h limit | Rate-limit gauges with countdown + projected time-to-exhaustion at current burn |
| Developer debugging a "stuck" turn | Claude has been "thinking" for 90 s | Turn panel: waiting on `Bash` for 84 s → the command text → child PID |
| Cost-conscious team lead | Wants to know why a session cost $14 | Cost panel: cache-hit ratio 31 % (bad), 4 compactions, top context consumers list |
| Plugin/MCP author | Is their MCP server slow or crashing? | MCP row: pid, RSS, calls, p95 latency, restarts |
| Anyone | "What did it just do?" | Event log, newest at bottom, colour-coded by tool/hook/permission |

## 4. Data sources (verified against Claude Code 2.1.269)

All paths relative to `~/.claude/` unless noted. cctop only reads; the one write is its own event spool.

| # | Source | Mechanism | What it yields |
|---|---|---|---|
| D1 | `sessions/<pid>.json` | poll 1 s + fs watch | Session registry: `pid`, `sessionId`, `cwd`, `name`, `status` (`busy`/`idle`), `startedAt`, `version`, `kind`, `messagingSocketPath` |
| D2 | `projects/<cwd-slug>/<sessionId>.jsonl` | tail-follow (kqueue/inotify), parse from last offset | Main transcript. **One API response is written as several `assistant` lines (one per content block) sharing `message.id` / `requestId` — usage must be deduplicated by `message.id`** (a real session: 386 assistant lines, 143 API calls, up to 13 lines per id). `message.usage` = `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`, `output_tokens_details.thinking_tokens`, **`cache_creation.ephemeral_5m_input_tokens` / `ephemeral_1h_input_tokens`** (cache TTL is observable per call), `service_tier`, `speed` (fast mode); top-level `effort` per assistant line; `tool_use` / `tool_result` paired by `tool_use_id`; `toolUseResult` (structured result: `stdout`/`stderr`/`interrupted`, `file`, `structuredPatch`…); `system` lines with `subtype` **`turn_duration` (`durationMs`)**, **`stop_hook_summary` (`hookInfos[].durationMs`, `hookErrors`)**, `away_summary`; `queue-operation` (queued prompts); `permission-mode`; `file-history-snapshot`; `ai-title` |
| D2a | `projects/<slug>/<sessionId>/subagents/agent-*.jsonl` + `.meta.json` | dir watch + tail | **Subagent transcripts live here, not as `isSidechain` lines in the main file.** `.meta.json` gives `agentType`, `description`, `model`, `spawnDepth`, `isFork`; a `fork-context-ref` line gives inherited context length. Same usage dedupe rule. |
| D2b | `projects/<slug>/<sessionId>/tool-results/<tool_use_id>.txt` | dir watch | Large tool results Claude Code offloaded to disk; what is *in context* is the truncated content in the transcript, so TOKENS→CTX uses the transcript text, not this file. |
| D11 | `cost-state` line in the transcript | parse on append | **Authoritative cumulative cost written by Claude Code:** `totalCostUSD`, `totalAPIDuration`, `totalAPIDurationWithoutRetries` (→ retry time), `totalToolDuration`, `totalLinesAdded/Removed`, `modelUsage` per model (input/output/cacheRead/cacheCreation/costUSD). cctop's own estimate is calibrated against this and marked `≈` only where D11 lags. |
| D3 | Status-line JSON | cctop installs `cctop statusline-shim` which forwards stdin to the user's real status-line command *and* writes it to `~/.cctop/status/<sessionId>.json` | `context_window.total_input_tokens`, `context_window_size`, `rate_limits.five_hour.{used_percentage,resets_at}`, `rate_limits.seven_day.{…}`, `model.display_name`, cost fields where present. **This is the only source for rate limits.** |
| D4 | Hooks | cctop registers a `command` hook `cctop hook` for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `Notification`, `SubagentStart`, `SubagentStop`, `PreCompact`, `Stop`, `SessionEnd`. Hook appends one JSON line to `~/.cctop/events/<sessionId>.jsonl` and exits 0 in < 5 ms. | Precise tool start/stop timestamps, permission-prompt wait time, compaction events, subagent lifecycle, notifications |
| D5 | Process tree | `libproc` (macOS) / `/proc` (Linux) every 1 s | CPU %, RSS of the `claude` process; children = running `Bash` commands, MCP stdio servers, subagent workers; their command lines and ages |
| D6 | `tasks/session-<id8>/` | fs watch | Background tasks (background Bash, agents, workflows, monitors) |
| D7 | Git in `cwd` | `git status --porcelain`, `git diff --numstat` every 5 s | Dirty files, +/- lines since session start |
| D8 | `file-history/` + transcript `Edit`/`Write` inputs | derived | Files touched, edit counts, re-read ratio |
| D9 | Pricing table (bundled, overridable in config) | static | $ estimates from D2 token counts |
| D10 | OpenTelemetry (phase 2) | cctop runs an OTLP/HTTP receiver on `127.0.0.1:4318`; user sets `CLAUDE_CODE_ENABLE_TELEMETRY=1 OTEL_METRICS_EXPORTER=otlp OTEL_LOGS_EXPORTER=otlp` | `claude_code.token.usage`, `cost.usage`, `lines_of_code.count`, `commit.count`, `active_time`, `api_request` / `api_error` / `tool_result` events with durations |

Degradation rule: every panel must render with D1 + D2/D2a alone. D3/D4 add precision; when they are absent the panel shows `—` and a one-word hint (`no shim`, `no hook`), never an error.

## 5. The page

### 5.1 Layout

Two layouts, chosen automatically from terminal width; the user can force one with `--layout`.

- **narrow** (< 100 cols — the right-hand-pane case): panels stack vertically in the order Header → Context → Tokens & Cost → Limits → Turn → Tools → Agents & MCP → Files → Advisor → Events → Footer. Panels shrink in priority order (Files, Agents, Tokens history) when rows are scarce; Events always keeps ≥ 4 lines.
- **wide** (≥ 100 cols): 2-column grid, left = Context/Tokens/Limits/Turn, right = Tools/Agents/Files, Advisor and Events span the full width at the bottom.

Every panel is a bordered box with a title in the top border (btop convention), a hotkey digit, and an optional right-aligned summary figure so a collapsed panel still says something.

### 5.2 Mockup — narrow layout, 60 × 51, mid-turn

Rendered version with colour and the wide layout: https://claude.ai/code/artifact/d3ceb159-9033-43b2-9ae4-e5e262407167

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

Wide layout is the same panels re-flowed into two columns; the mockup is omitted here because the content does not change.

### 5.3 Panels and metrics

Each metric lists its source (D-number from §4) so the reader knows it is buildable.

**Header** — session name (D1), model (D2/D3), CC version (D1), status pill `● BUSY / ○ IDLE / ◆ WAITING (permission) / ⏸ PAUSED` (D1 + D4), turn number (D2), turn elapsed (D2), current tool + elapsed (D4/D2), cwd + git branch + dirty star (D7), permission mode (D2 `permission-mode`), plan (D3), uptime (D1), cost estimate (D9).

**1 · Context** — the panel most users open cctop for.
- Gauge: `total_input_tokens / context_window_size` (D3) with colour bands ≤ 60 % ok, 60–80 % warn, > 80 % critical.
- Breakdown: fixed prefix (system prompt + CLAUDE.md + tool schemas = `cache_read + cache_creation` of the session's first API call), messages, free (D2). With a warm 1 h cache the first call is a cache *read*, so both fields are summed.
- Sparkline of context size per turn; **context velocity** (Δ tokens/turn, EMA over 5 turns) and **turns until autocompact** = (autocompact threshold − current) / velocity (D2).
- Compaction count, when, and how many tokens each one dropped (D4 `PreCompact` + D2 before/after).

**2 · Tokens & Cost**
- Cumulative stacked bars: cache read · cache write · fresh input · output · thinking-of-output (D2).
- **Cache hit ratio** = cache_read / (cache_read + cache_creation + input). This single number explains most cost surprises; show in green ≥ 80 %, amber 50–80 %, red < 50 %.
- Cost: D11 `totalCostUSD` when present (per model from `modelUsage`), else D2 × D9 with **5 m vs 1 h cache-write prices** (1.25× / 2× base input) chosen per call from `cache_creation.ephemeral_*`; **burn rate** $/h over the last 15 min; input tokens/min.
- Per-turn sparkline; last turn tokens and $.

**3 · Limits** (D3 only)
- 5-hour and 7-day gauges, `resets_at` as a countdown.
- **Projected exhaustion**: linear fit of `used_percentage` samples over the last 30 min (the shim gives a time series), not a token-based extrapolation — rate-limit units are plan-specific and not linear in tokens. Marked ✓ if after the reset, ✗ (red) if before.
- **Limits are account-wide.** The header shows how many other live sessions (D1 registry) are contributing, and the projection uses the account-level slope, which already includes them.

**4 · Turn**
- Elapsed (exact from `turn_duration` once the turn ends; live from the first user line), API calls this turn (distinct `message.id`), API time vs tool time (D11 aggregates; D2 timestamps per turn), retry time (`totalAPIDuration − totalAPIDurationWithoutRetries`). Time-to-first-token is **not** derivable from the transcript — shown only with D10.
- What the turn is waiting on right now: tool name, its argument summary, elapsed, child pid (D4 + D5).
- Hook runs, per-hook duration and errors (`stop_hook_summary` in D2; D4 for the other hook events); **permission waits** (count, seconds blocked on a human) — `PermissionRequest` → `PostToolUse` minus the tool's median duration, marked `≈` (PreToolUse fires *before* the permission prompt, so it cannot bound the wait).

**5 · Tools** — the "process table".
- One row per tool name (MCP tools prefixed `mcp:`): calls, errors, p50/p95 duration, time since last call, **tokens pushed into context** by results (D2 tool_result size × tokenizer estimate; D4 for durations).
- Sortable (`s`) by any column; `f` filters by substring.
- "Top ctx" line: the three individual tool results that consumed the most context — usually the quickest way to find a runaway `cat`.
- Detail view (`Enter`): last 20 calls for the selected tool with arguments and durations.

**6 · Agents & MCP**
- Subagents: state glyph (◐ running ✓ done ✗ failed), type/name, description, elapsed, tokens, model (D2 `isSidechain` + D4 `SubagentStart/Stop`).
- MCP servers: pid, RSS, calls, p95, idle time, restart count (D5 + D2).
- Background tasks: kind, command, elapsed, task id (D6).

**7 · Files**
- Files touched this session with read/edit/write counts and net lines (D8 + D7).
- **Re-read warning ⚠**: a file read ≥ 3× without an intervening edit — wasted context.

**9 · Advisor** — one recommendation at a time.
- Shows exactly one line of advice plus one line of evidence, ranked by estimated tokens (or seconds) saved. `n` cycles, `x` dismisses for the session, `Enter` opens the rule's explanation.
- Rules are plain predicates over the metrics above — no LLM call, no network. Each rule has a trigger, an evidence template, an action, and a savings estimate. The full catalog is in §5.7.

**Events** — newest-last log; kinds `tool`, `hook`, `perm`, `agent`, `compact`, `api` (errors/retries), `note` (cctop's own alerts). `Enter` opens a scrollable full-height log.

**Footer** — key hints; replaced by a transient toast for cctop alerts.

### 5.7 Advisor rule catalog

The Advisor exists because the numbers alone don't tell a user *what to do*. Each rule turns an observed pattern into one concrete change to how they work with Claude Code. Rules fire only on evidence from this session; a rule that fired is re-evaluated every turn and retires when the evidence is gone. Ranking = estimated savings per remaining turn, ties broken by recency.

| ID | Trigger (evidence) | Recommendation | Est. saving |
|---|---|---|---|
| A01 Cache miss | Cache-hit ratio < 60 % over 5 turns, and `cache_creation` spikes on turns where no tool output was large | "Something in the prefix changes every turn — check for a hook or status line that injects time/random values, or a CLAUDE.md edited mid-session. Stable prefix = cache reads at 10 % of input price." | Δ(cache_write − cache_read cost) per turn |
| A02 Cache expiry | Gap between consecutive user prompts > the observed TTL (`ephemeral_5m` → 5 min, `ephemeral_1h` → 60 min) *and* the following call shows `cache_creation` ≈ full context | "Your prompt cache expired while you were away (TTL 5 min on this session). Batch your questions or keep the session warm; each cold turn re-writes the whole context." | full-context write cost per cold turn |
| A03 Runaway tool result | A single tool result > 3 k tokens, or the same command's result > 2 k twice | "`ls -R` pushed 4.4k tokens into context, twice. Pipe through `head -50`, use Glob/Grep, or ask for a summary via a subagent." | result size × expected repeats |
| A04 Re-reads | A file read ≥ 3× with no edit in between | "src/render.rs was read 6× — each read costs the whole file again. Read once with an offset/limit, or ask Claude to keep the relevant lines in its summary." | file tokens × (reads − 1) |
| A05 Explore in main context | ≥ 8 consecutive Read/Grep/Glob calls in the main thread with no Edit | "You're exploring in the main context. Delegate discovery to an Explore subagent — it returns a paragraph instead of 30 k tokens of file dumps." | sum of exploration result tokens − 1 k |
| A06 Compaction churn | ≥ 2 compactions, or velocity projects a compaction within 3 turns while the task is unfinished | "Compaction is coming again. Finish this unit of work, then `/clear` or start a fresh session with a hand-off note — you'll spend fewer tokens than compacting twice." | ~compaction summary cost + lost-context re-reads |
| A07 Idle MCP tools | An MCP server with 0 calls for > 20 min whose tool definitions occupy > 2 k tokens of every request | "`playwright` MCP has 0 calls in 42 min but its 14 tool schemas ride along in every request (≈ 3.1 k tokens). Disable it for this project or use deferred loading." | schema tokens × remaining turns |
| A08 Thinking share | Thinking tokens > 40 % of output over 5 turns on routine edits (`effort` field shows the current level) | "Two-fifths of output is thinking on small edits. Lower the effort level for routine work (`/model` or `ultrathink` only when needed)." | thinking tokens × output price |
| A09 Permission waits | Total permission-wait time > 2 min or > 5 prompts for the same tool pattern | "You've approved `cargo test` 6 times (2m 10s waiting). Add it to the allow-list — the model idles while it waits." | seconds saved |
| A10 Long foreground command | A Bash call > 60 s that produces no output the model needs immediately | "`cargo build --release` blocked the turn for 1m 58s. Run it in the background and let Claude continue; it's notified when it exits." | wall-clock seconds |
| A11 Fresh-input spikes | `input_tokens` (uncached) > 5 k on a turn | "You pasted ~6 k tokens raw. For files, give the path and let Claude Read the relevant range; for logs, paste the last 100 lines." | pasted tokens − needed tokens |
| A12 Chatty turns | Median user prompt < 8 tokens and > 20 turns/hour | "Many tiny prompts. Each turn re-sends the full context (cache reads still cost). Batch related instructions into one prompt." | context-read cost × merged turns |
| A13 Rate-limit pacing | Projected 5 h exhaustion before reset | "At this burn you hit the 5 h limit 17 min before it resets. Move exploration to Sonnet subagents or pause the heavy refactor until the reset." | avoids a hard stop |
| A14 Subagent model | A subagent running on Opus with a task description matching search/summarise/list patterns | "The Explore agent is on Opus. Search-and-summarise work runs equally well on Sonnet/Haiku at a fraction of the cost." | (opus − sonnet price) × agent tokens |
| A15 Error loops | The same tool with the same arguments failed ≥ 3× | "`cargo test` failed 3× with the same error. Interrupt and give the fix yourself; each retry re-reads the whole error output." | retry tokens |
| A16 Hook overhead | Hook time > 10 % of turn time | "Hooks add 1.4 s per turn (PostToolUse lint). Make them async or scope the matcher to the files they care about." | seconds per turn |
| A17 Big CLAUDE.md | System-prompt + tool-definition share > 25 % of the window | "Your fixed prefix is 52 k tokens. Trim CLAUDE.md, move rarely-used rules to skills, and disable unused MCP servers." | prefix tokens × turns (cache reads still cost) |
| A18 No session hand-off | Session > 3 h with no `/clear`, context > 70 %, task list shows a natural boundary | "You've been in one context for 3 h. At the next milestone, write a short hand-off and start fresh — cheaper and sharper than one giant session." | avoided compactions |

Rule authoring: rules live in `src/advisor/rules/*.rs` behind a `Rule` trait (`evaluate(&Metrics) -> Option<Advice>`), each with a unit test using a fixture transcript that must fire it and one that must not. User-defined rules (TOML predicates on exported metrics) are a phase-2 item.

### 5.4 Alerts (cctop's own `note` events)

Fired once per crossing, shown in Events and as a 3 s footer toast: context > 80 %; autocompact projected within 2 turns; 5 h limit > 60 %/80 %/95 %; projected exhaustion before reset; cache hit ratio < 50 % over the last 5 turns; a tool call running > 60 s; permission wait > 30 s; MCP server exited; API error/retry.

### 5.5 Interaction

| Key | Action |
|---|---|
| `1`–`9` | Toggle panel visibility (persisted) |
| `n` / `N` | Advisor: next / previous recommendation; `x` dismiss for this session |
| `Tab` / `Shift-Tab` | Cycle panel focus; `j`/`k`/arrows scroll inside focused panel |
| `Enter` | Expand focused panel to full height; `Esc` back |
| `s` / `S` | Sort ascending/descending by next column (Tools, Files) |
| `f` | Filter focused table (substring) |
| `p` | Pause updates (buffered, resume shows a badge) |
| `+` / `-` | Refresh interval 100 ms … 2 s (data is event-driven; this is the render cap) |
| `c` | Toggle cost units ($ vs tokens) |
| `t` | Cycle theme |
| `w` | Toggle narrow/wide layout |
| `L` | Session picker (all sessions from D1: name, cwd, status, age); `Enter` attaches |
| `?` | Help overlay |
| `q` / `Ctrl-C` | Quit (never affects the Claude Code session) |
| mouse | Click panel title = focus; wheel = scroll; click column header = sort |

### 5.6 Visual language

- Box-drawing borders (rounded in truecolour terminals), panel titles in the top border, hotkey digit before the title, summary figure right-aligned.
- Semantic colours only for state (ok/warn/critical); one accent for focus and headers; everything else in the theme's neutrals.
- Braille sparklines, block-element gauges; all numbers right-aligned, `1.62 M` / `148k` / `41ms` scaling, never more than 3 significant digits.
- Ships with `default-dark`, `default-light`, `btop`, `nord`, `gruvber`, `catppuccin-mocha`; themes are TOML in `~/.config/cctop/themes/`.
- Honors `NO_COLOR`, degrades to 16 colours, ASCII fallback for borders when `LANG` isn't UTF-8.

## 6. User stories

### US-001: Discover the session next door
**Description:** As a user, I want `cctop` with no arguments to attach to the Claude Code session running in my current tmux/zellij window so that I never have to copy a session id.

**Acceptance Criteria:**
- [ ] Reads `~/.claude/sessions/*.json`; picks the session whose `pid`'s controlling TTY belongs to a pane in the same tmux/zellij window (via `tmux list-panes -F '#{pane_tty} #{pane_pid}'` / `zellij action list-clients`); falls back to the most recently updated `busy` session whose `cwd` equals `$PWD`, then to the session picker.
- [ ] `cctop --session <id|name|pid>` and `cctop --cwd <path>` override discovery.
- [ ] Exits with a clear message and exit code 2 when no session is found; `--wait` keeps polling until one appears.
- [ ] Unit tests for discovery precedence with fixture registry files.

### US-002: Follow the transcript in real time
**Description:** As a user, I want every new transcript line to be reflected in cctop within 250 ms so that the dashboard is genuinely live.

**Acceptance Criteria:**
- [ ] Opens `projects/<slug>/<sessionId>.jsonl`, parses existing content on start (280 KB in < 100 ms), then follows appends via kqueue/inotify with a 1 s poll fallback.
- [ ] Tolerates partial lines (waits for `\n`), unknown `type`s (ignored), and file rotation.
- [ ] Extracts per-message usage, model, tool_use/tool_result pairs by `tool_use_id`, sidechain flag, timestamps, permission-mode, file-history snapshots.
- [ ] Bench: 10 MB transcript parses in < 500 ms; steady-state CPU < 2 % with a 1-line/s append rate.

### US-003: Layout engine and panel framework
**Description:** As a developer, I need a panel abstraction and narrow/wide layout engine so that every later panel is a small, isolated unit.

**Acceptance Criteria:**
- [ ] `Panel` trait: `title()`, `summary()`, `min_rows()`, `priority()`, `render(area, state)`, `handle_key()`.
- [ ] Layout solver assigns rows by priority, shrinks low-priority panels first, guarantees Events ≥ 4 rows, and switches narrow ↔ wide at 100 cols (overridable).
- [ ] Renders at a capped rate with diff-rendering (ratatui double buffer); no visible flicker on resize.
- [ ] Works at 40 × 24 (panels collapse to title+summary) and at 200 × 60.
- [ ] Snapshot tests of rendered buffers for both layouts at three sizes.

### US-004: Context panel
**Description:** As a user, I want to see how full the context window is and when the next compaction will hit so that I can decide whether to wrap up or start fresh.

**Acceptance Criteria:**
- [ ] Gauge from D3 when present, else estimated from the last assistant message's `cache_read + cache_creation + input` (D2), labelled `est`.
- [ ] Sparkline per turn, velocity EMA(5), "autocompact in ~N turns" (N hidden when velocity ≤ 0).
- [ ] Compaction events counted from `PreCompact` hooks or a ≥ 30 % drop in context between consecutive turns.
- [ ] Colour bands at 60 / 80 %.
- [ ] Snapshot test with a fixture transcript that includes one compaction.

### US-005: Tokens & cost panel
**Description:** As a user, I want cumulative token breakdown, cache-hit ratio, cost and burn rate so that I understand what the session is costing and why.

**Acceptance Criteria:**
- [ ] Stacked bars for cache read / cache write / fresh input / output / thinking, cumulative over the session, subagent tokens included and separable with `a`.
- [ ] Cache hit ratio with green/amber/red thresholds at 80 / 50 %.
- [ ] Cost from bundled per-model pricing (input, cache write, cache read, output) overridable in `~/.config/cctop/pricing.toml`; unknown model → tokens only, no $.
- [ ] Burn rate = cost over trailing 15 min × 4; tokens/min likewise.
- [ ] Per-turn sparkline of total tokens; last-turn figure.

### US-006: Tools panel
**Description:** As a user, I want a sortable table of tool usage with durations, errors and context cost so that I can spot slow or noisy tools.

**Acceptance Criteria:**
- [ ] Rows keyed by tool name; MCP tools shown as `mcp:<server>`; columns N, ERR, p50, p95, LAST, TOKENS→CTX.
- [ ] Durations from D4 hook timestamps when present, else from transcript timestamps of tool_use → tool_result (labelled `≈`).
- [ ] TOKENS→CTX estimated as `len(result)/4` unless D10 supplies exact counts.
- [ ] Sort with `s`/`S`, filter with `f`, `Enter` opens last-20-calls detail.
- [ ] "Top ctx" line lists the 3 largest single results with a 30-char argument summary.

### US-007: Status-line shim for rate limits
**Description:** As a user, I want cctop to see the rate-limit and context figures Claude Code sends to the status line so that the Limits panel is exact.

**Acceptance Criteria:**
- [ ] `cctop install` rewrites `statusLine.command` in `~/.claude/settings.json` to `cctop statusline-shim -- <original command>` after backing up the file; `cctop uninstall` restores it. Both print a diff and ask for confirmation unless `--yes`.
- [ ] Shim forwards stdin to the original command unchanged, passes its stdout/exit code through, and atomically writes the JSON to `~/.cctop/status/<sessionId>.json` in < 3 ms extra latency.
- [ ] Limits panel shows both gauges, countdowns, and projected exhaustion with ✓/✗ vs reset.
- [ ] Without the shim the panel shows `— run cctop install`.

### US-008: Hook emitter
**Description:** As a user, I want cctop to receive precise tool/permission/compaction/subagent events so that timings are exact and the Events panel is complete.

**Acceptance Criteria:**
- [ ] `cctop install` adds a `command` hook `cctop hook` to the events listed in D4 (merging with existing hooks, never replacing them).
- [ ] The hook reads stdin JSON, appends one line to `~/.cctop/events/<sessionId>.jsonl`, exits 0 in < 5 ms; never blocks or fails the tool call, even if the spool directory is unwritable.
- [ ] cctop tails the spool like the transcript and joins `PreToolUse`/`PostToolUse` by `tool_use_id`.
- [ ] Permission wait = `PermissionRequest` → next `PreToolUse` for the same id.
- [ ] Spool files older than 7 days are pruned on start.

### US-009: Agents, MCP and process panel
**Description:** As a user, I want to see live subagents, MCP servers and background tasks so that I know what else is running on my behalf.

**Acceptance Criteria:**
- [ ] Subagents from `<sessionId>/subagents/agent-*.jsonl` + `.meta.json` (`agentType`, `description`, `model`, `spawnDepth`, `isFork`) and `SubagentStart/Stop` hooks: glyph, type, description, elapsed, tokens (deduped), model.
- [ ] Process tree of the `claude` pid via libproc/procfs: MCP servers identified from `mcp.json`/`.mcp.json` commands, shown with pid, RSS, uptime; running Bash children shown in the Turn panel.
- [ ] Background tasks from `~/.claude/tasks/session-<id8>/`.
- [ ] MCP call counts and p95 derived from `mcp__<server>__*` tool rows.
- [ ] Header shows `claude` process CPU % and RSS.

### US-010: Side-by-side launcher and `/cctop` command
**Description:** As a user, I want one command that opens cctop in a right-hand pane and attaches to the session I'm in so that I can keep working without leaving Claude Code.

**Acceptance Criteria:**
- [ ] `cctop split [--size 45%]` detects tmux (`$TMUX`), zellij (`$ZELLIJ`), WezTerm (`wezterm cli split-pane --right`), Kitty (`kitten @ launch --location=vsplit`), iTerm2 (`it2` / AppleScript); errors with instructions on plain terminals.
- [ ] The new pane runs `cctop --session <id>` with the id resolved *before* splitting, so attachment is deterministic.
- [ ] The plugin adds a `/cctop` skill whose only action is running `cctop split` via Bash and telling the user it opened; it must not print the dashboard into the conversation.
- [ ] Closing the pane (`q`) never signals the Claude Code process.
- [ ] Manual verification in tmux, zellij and WezTerm recorded in the README.

### US-011: Events panel and alerts
**Description:** As a user, I want a colour-coded live log and alert toasts so that I notice important transitions without watching the whole page.

**Acceptance Criteria:**
- [ ] Merges transcript, hook spool and cctop notes into one time-ordered stream; kinds coloured as in §5.3.
- [ ] Alert rules from §5.4 fire once per crossing per session; each produces a `note` event and a 3 s footer toast.
- [ ] `Enter` opens a full-height scrollable log with `/` search.
- [ ] Optional `--notify` sends a desktop notification for critical alerts via `osascript`/`notify-send`.

### US-012: Files panel
**Description:** As a user, I want to see which files the session has touched and how much so that I can review the blast radius and catch wasted re-reads.

**Acceptance Criteria:**
- [ ] Rows from `Read`/`Edit`/`Write`/`MultiEdit`/`NotebookEdit` tool_use inputs; counts per kind; net lines from `git diff --numstat` vs the session-start commit when in a git repo.
- [ ] Re-read ⚠ when a file is read ≥ 3× with no edit in between.
- [ ] Sorted by last-touched; `s` cycles.

### US-013: Session picker
**Description:** As a user with several sessions, I want to switch which one cctop watches so that one dashboard serves all my terminals.

**Acceptance Criteria:**
- [ ] `L` lists sessions from D1 with name, cwd, status, model, uptime, context %; `Enter` attaches; `d` shows dead-registry entries (pid gone) greyed and offers to hide them.
- [ ] Switching resets all panels within 300 ms and does not leak file watchers.

### US-014: Config, themes, persistence
**Description:** As a user, I want my layout, theme and panel choices remembered so that cctop opens the way I left it.

**Acceptance Criteria:**
- [ ] `~/.config/cctop/config.toml` with layout, theme, refresh cap, hidden panels, pricing overrides; CLI flags override file.
- [ ] Six bundled themes; user themes in `themes/*.toml` hot-reloaded.
- [ ] `NO_COLOR`, 16-colour and ASCII-border fallbacks verified in `TERM=xterm` and `LANG=C`.

### US-015: OpenTelemetry receiver (phase 2)
**Description:** As a user who has enabled Claude Code telemetry, I want cctop to ingest it locally so that API latency, exact tool durations and cost are authoritative instead of estimated.

**Acceptance Criteria:**
- [ ] `cctop --otlp` binds an OTLP/HTTP receiver on `127.0.0.1:4318` (configurable), accepts metrics + logs, no TLS, no auth, loopback only.
- [ ] Ingests `claude_code.*` metrics and `api_request`, `api_error`, `tool_result` log events; panels drop the `≈`/`est` markers when OTel data is present.
- [ ] `cctop install --otel` prints the env vars to add to `settings.json` `env`; never writes them silently.

### US-016: Advisor panel and rule engine
**Description:** As a user, I want one concrete, evidence-backed recommendation at a time so that I learn how to work more cheaply and effectively with Claude Code without reading a manual.

**Acceptance Criteria:**
- [ ] `Rule` trait with `id`, `evaluate(&Metrics) -> Option<Advice { headline, evidence, saving_estimate, doc_key }>`; the 18 rules in §5.7 implemented.
- [ ] Advisor panel shows exactly one advice (two lines at 60 cols), a `k of n` counter, and supports `n`/`N` cycle, `x` dismiss (session-scoped), `Enter` explanation overlay.
- [ ] Ranking by saving estimate per remaining turn; re-evaluated every turn; retired rules disappear.
- [ ] Each rule has a fixture transcript that fires it and one that must not; no rule may fire on the empty session.
- [ ] Zero network or LLM calls in the advisor path (asserted by test).
- [ ] `cctop advise --session <id>` prints the current ranked list to stdout for scripting.

### US-017: Product website on GitHub Pages
**Description:** As a maintainer, I want a product page built from the repository and deployed on GitHub Pages so that a visitor understands and installs cctop in under a minute.

**Acceptance Criteria:**
- [ ] `site/` static page implementing §12 sections 1–9 with real content pulled from README.md at build time.
- [ ] `make demo` generates the terminal recording and all screenshots from a fixture transcript with a fixed clock; outputs committed to `site/assets/`.
- [ ] `.github/workflows/pages.yml` deploys on push to `main`; PR builds upload a preview artifact.
- [ ] Lighthouse CI ≥ 95 on performance, accessibility and SEO; page works with JavaScript disabled except the copy-to-clipboard button.
- [ ] Light and dark themes via `prefers-color-scheme`; `prefers-reduced-motion` shows a static frame instead of the loop.
- [ ] Verify in browser using dev-browser skill at 400 px and 1280 px widths.

### US-018: Turn ledger
**Description:** As a user, I want a per-turn table of tokens, cost, tools and duration so that I can see exactly which turns were expensive and why.

**Acceptance Criteria:**
- [ ] `Enter` on panel 1 or 2 opens a full-height table: turn #, start time, duration (`turn_duration`), API calls, cache read/write, fresh in, out, thinking, cost (D11-calibrated), tools called (counts by name), compaction marker, effort, model.
- [ ] Usage deduplicated by `message.id`; unit test with a 13-block response fixture asserts a single count.
- [ ] Sort by any column; `Enter` on a row lists that turn's tool calls with their TOKENS→CTX.
- [ ] `cctop query ledger --json` returns the same rows.

### US-019: Prefix inspector
**Description:** As a user, I want to see what makes up the fixed prefix sent on every request so that I can shrink it.

**Acceptance Criteria:**
- [ ] Breakdown: system prompt (remainder), CLAUDE.md files (path + bytes, from `cwd` and `~/.claude`), tool schemas (count, est. tokens; MCP tools grouped by server from `.mcp.json`/`mcp.json`), skills listing, deferred tools count (`deferred_tools_delta`), memory index.
- [ ] Total reconciled against the first API call's `cache_read + cache_creation`; residual shown as "other".
- [ ] Available as `Enter` on the Context panel's prefix line and as `cctop query prefix --json`.

### US-020: Baselines and session report
**Description:** As a user, I want this session compared to my own recent sessions, and a written summary when it ends, so that I can tell whether I am improving.

**Acceptance Criteria:**
- [ ] Baseline = median over the last 7 days of `cost-state` lines across all transcripts (cost/turn, tokens/turn, cache-hit, tool error rate, model mix); shown as `×1.0` style multipliers next to the live figures.
- [ ] `cctop report [--session]` writes `~/.cctop/reports/<date>-<name>.md` (cost, model mix, top context consumers, advisor hits, baseline comparison); the `SessionEnd` hook triggers it automatically when installed.
- [ ] `cctop export --json|--csv` dumps ledger + events.

### US-021: Metrics registry and generated docs
**Description:** As a user, I want every number on screen defined in the README so that I never have to guess what a metric means.

**Acceptance Criteria:**
- [ ] `src/metrics/registry.rs` declares every metric: id, name, unit, formula text, sources, caveats, est/≈ conditions.
- [ ] `cctop metrics --md` generates `docs/metrics.md`; README and site include it; CI fails on drift.
- [ ] `?` overlay shows the focused panel's metric definitions.
- [ ] Debug-build assertion: a panel drawing an unregistered metric id panics; release build logs.

### US-022: Query interface — CLI, skill, ask-from-TUI, MCP
**Description:** As a user, I want to ask my Claude Code session about cctop's numbers so that I can dig into an insight without leaving the conversation.

**Acceptance Criteria:**
- [ ] `cctop query summary|ledger|tools|files|agents|advice|events|prefix|explain <metric_id> [--session] [--since] --json` with a stable schema (documented in `docs/query.md`), reading the same state the TUI shows.
- [ ] Plugin skill `cctop-insights` — see US-023.
- [ ] `a` on any panel drafts a question with that panel's data and sends it to the attached session's messaging socket as an unsent prompt; the draft is previewed in cctop first; disabled when the socket is missing.
- [ ] `cctop mcp` (v1.1) exposes the same queries as tools registered as deferred; schema cost measured and documented.

### US-023: `cctop-insights` skill
**Description:** As a user, I want to ask my session plain questions about cost, context, cache and tools and get numeric answers plus one actionable change, so that cctop's insights are usable without looking at the TUI.

**Acceptance Criteria:**
- [ ] `plugin/skills/cctop-insights/SKILL.md` (drafted, in repo) ships in the plugin; its description triggers on cost/tokens/context/cache/rate-limit/"what does metric X mean" questions.
- [ ] Skill maps question shapes to `cctop query` commands (`summary`, `ledger`, `tools`, `files`, `agents`, `advice`, `prefix`, `events`, `explain`, `baseline`) and prescribes the answer pattern: numbers with units and est/≈ marks, top recommendation with evidence, offer to apply it in-session, no raw JSON.
- [ ] `cctop query` output includes `metric_id` on every value and `"source": "missing"` for absent optional sources, so the skill's fallbacks work.
- [ ] Default session resolution via `$CLAUDE_SESSION_ID`; the skill tells the user how to install when the binary is missing instead of parsing transcripts by hand.
- [ ] Verified in a real session with the five example questions in the skill; each answer cites at least one number and one action. Token cost of the skill's presence measured (its listing line only) and documented.
- [ ] `cctop query baseline` added to the query set (feeds "compare to my recent sessions").

## 7. Functional requirements

- FR-1: cctop must run as a separate process and require no modification of the Claude Code binary.
- FR-2: cctop must never write to `~/.claude/projects/**`, `sessions/**` or the transcript; its only writes are `~/.cctop/**`, `~/.config/cctop/**`, and — only via explicit `cctop install` with confirmation — `~/.claude/settings.json`.
- FR-3: All panels must render with D1 + D2 alone; D3–D10 only upgrade precision and must be marked when absent.
- FR-4: New transcript/hook/status data must be reflected on screen within 250 ms (p95) on a local SSD.
- FR-5: Steady-state resource budget: ≤ 2 % CPU idle, ≤ 5 % during a turn, ≤ 50 MB RSS, ≤ 200 ms cold start on a 10 MB transcript.
- FR-6: The dashboard must fit on one screen with no scrolling at ≥ 50 × 35 and remain usable (collapsed panels) down to 40 × 24.
- FR-7: The side-by-side launcher must work in tmux, zellij, WezTerm, Kitty and iTerm2 and must resolve the target session before creating the pane.
- FR-8: Quitting cctop, killing it, or the pane closing must have no observable effect on the Claude Code session.
- FR-9: Every numeric metric must have a defined source and a defined behaviour when that source is missing (`—` + hint).
- FR-10: Estimated values (cost, tokens→ctx without OTel, durations without hooks) must be visually marked `est`/`≈`.
- FR-11: Every key in §5.5 must be discoverable in the `?` overlay.
- FR-12: cctop must handle a session ending (pid gone) by freezing the last state with a `ENDED hh:mm` badge, not by exiting.
- FR-13: Hook and shim binaries must add < 5 ms and never fail the hosting operation.
- FR-14: cctop must not send any data over the network; the OTLP receiver listens on loopback only.
- FR-15: The Advisor must show at most one recommendation at a time, each with a visible evidence line and a savings estimate, derived from deterministic rules with no LLM or network call.
- FR-16: The product website must be generated from repository content (README, fixture-driven screenshots) and deployed to GitHub Pages by CI on push to `main`.
- FR-17: Token usage must be counted once per API response (`message.id`), never per transcript line.
- FR-18: Every metric rendered must exist in the metrics registry, and `docs/metrics.md` must be generated from it and included in the README; CI fails on drift.
- FR-19: All numbers shown in the TUI must be obtainable as JSON via `cctop query`, from the same state, so that the session can reason about them.
- FR-20: Cost must prefer Claude Code's own `cost-state` when present; estimates are used only to fill the interval between writes and are marked `≈`.

## 8. Non-goals (v1)

- Driving the session: no sending prompts, approving permissions, cancelling tools or killing subagents from cctop.
- Multi-session grid / fleet view (the picker switches; it does not tile).
- Web UI, remote access, or watching cloud/remote Claude Code sessions.
- Historical analytics across sessions (`stats-cache.json`, `usage-data/`) beyond a one-line "today: N sessions, N tool calls" in the header.
- Exact billing: cost is an estimate from public list prices; subscription plans have no per-token cost and show tokens instead.
- Windows support (WSL is fine).
- Replacing the status line: the shim wraps it, it does not restyle it.

## 9. Decisions and open questions

Decided 2026-09-11:

| # | Question | Decision |
|---|---|---|
| 1 | Language | **Rust + ratatui** (crossterm, tokio, notify). Static binary; no runtime. |
| 3 | Context % and rate limits | **Status-line shim** (`cctop statusline-shim`). Revisit only if hook payloads gain these fields. |
| 4 | Tokenizer for TOKENS→CTX | **`len/4` heuristic**, marked `≈`; OTel replaces it in phase 2. |
| 5 | Auto-install from `/cctop` | **Yes** — first `/cctop` run offers `cctop install` with a diff and confirmation; never silent. |
| 6 | $ for subscription users | **Show**, prefixed `≈ API` (list-price estimate). |
| 7 | Packaging | **Homebrew tap + `cargo install`**; the plugin detects a missing binary and prints the install command. |

Still open:

2. **Autocompact threshold:** fixed % of `context_window_size` or model-specific? Needed for "turns until compaction". Until known: 80 %, labelled `est`. cctop will *learn* it: every observed compaction records the context size just before it; after one observation the learned value replaces the default for that model.

## 10. Success metrics

- p95 event-to-screen latency ≤ 250 ms (measured with a synthetic transcript appender).
- Resource budget in FR-5 met on an M1 MacBook Air and a 2-vCPU Linux VM.
- All eight panels populated from D1 + D2 alone on a fresh install with no `cctop install`.
- Cold start ≤ 200 ms on a 10 MB transcript.
- Zero writes outside `~/.cctop`, `~/.config/cctop` in an `fs_usage`/`strace` audit.
- Side-by-side launch works in tmux, zellij, WezTerm, Kitty, iTerm2 (manual matrix in README).
- Dogfood: the author keeps it open for a full working week and every alert in §5.4 has fired at least once correctly with no false positives.
- Advisor: over the dogfood week, following its top recommendation reduces the next day's cost/turn or tokens/turn by ≥ 15 % on at least three rules (A01–A07 are the expected candidates).
- Website: Lighthouse ≥ 95; a first-time visitor reaches the install command without scrolling on a 1280 × 800 viewport.

## 11. Milestones

| Phase | Stories | Outcome |
|---|---|---|
| M0 — Skeleton | US-001, US-002, US-003 | Attaches, parses, draws empty panels in both layouts |
| M1 — Core page | US-004, US-005, US-006, US-011 | Usable dashboard from transcript alone |
| M2 — Precision | US-007, US-008, US-009, US-012, US-018, US-019 | Rate limits, exact timings, agents/MCP, files, ledger, prefix inspector |
| M3 — Ergonomics | US-010, US-013, US-014, US-016, US-020, US-021, US-022 (CLI, `a`), US-023 (skill) | `/cctop` split, picker, themes, config, Advisor, baselines, metrics docs, query — v1.0 |
| M3.5 — Launch | US-017 | Product website live on GitHub Pages |
| M4 — Telemetry | US-015, US-022 (MCP) | OTel-authoritative numbers, MCP server |

## 12. Product website (GitHub Pages)

The repository is the product's home; the site is built from it and hosted on GitHub Pages at `https://<owner>.github.io/cctop/` (custom domain optional). Its job is to make a developer decide to install within 30 seconds.

**Content, top to bottom**
1. **Hero:** the name, one sentence ("See what Claude Code is doing — live, in a pane beside it"), an install line with a copy button (`brew install <tap>/cctop`), and an autoplaying, looping terminal recording of cctop attached to a real session (VHS/asciinema → GIF/WebM; ≤ 3 MB; reduced-motion users get the static frame).
2. **Two-pane screenshot** exactly like the mockup: Claude Code left, cctop right.
3. **Nine panels, nine sentences** — each panel's headline metric and the question it answers.
4. **Advisor** — three example recommendations with their evidence lines.
5. **Install & attach** — Homebrew, cargo, the plugin (`/cctop`), what `cctop install` changes (shim + hooks) and how to undo it.
6. **Works in** — tmux, zellij, WezTerm, Kitty, iTerm2 logos/names; terminal size minimums.
7. **Themes** — six theme screenshots in a row.
8. **Privacy** — one paragraph: local only, no network, reads `~/.claude`, writes `~/.cctop`.
9. Footer: GitHub, releases, changelog, licence.

**Build & hosting**
- Static site in `site/` — plain HTML/CSS (the same type and colour system as this PRD's page), no framework, no build step beyond copying assets; Markdown docs (`docs/*.md`) rendered by a tiny script at build time.
- Deployed by a GitHub Actions workflow on push to `main` (`actions/upload-pages-artifact` + `actions/deploy-pages`); PRs get a preview build artifact.
- Screenshots and the demo recording are generated by a `make demo` target that runs cctop against a fixture transcript with a fixed clock, so they are reproducible and never leak a real session.
- Lighthouse ≥ 95 on performance/accessibility/SEO; works without JavaScript except the copy button; OpenGraph image is the two-pane screenshot.
- README.md is the site's text source of truth: hero copy, install block and panel list are pulled from it so the two never drift.

## 13. Brand

The name is a pun — `cctop` reads like ZZ Top — so the mark leans into it: Claude Code's asterisk spark wearing ZZ Top's beard and round shades.

- **Palette:** terracotta `#D97757` (Claude's spark colour) for the mark, black `#111111` / dark grey `#3A3F47` for the shades, charcoal `#0E1318` ground; teal `#4CC2C2` stays the TUI accent but is not in the logo.
- **Candidates:** four directions generated with Flux Kontext via kie.ai, prompts in `brand/gen-logo.ts`, outputs in `brand/candidates/`. Gallery: https://claude.ai/code/artifact/08c7bf63-e901-4a4e-8569-27086e3c5fff
  - **Chosen (2026-09-11): `gauge_beard_v2`** — five terracotta `#D97757` meter bars form the beard (moustache on top, pointed tip), round sunglasses with black `#111111` frames and dark-grey `#3A3F47` lenses. Reads as a btop gauge and as a bearded face.
  - Not chosen: `spark_beard`, `wordmark`, `terminal_beard`, `cowboy_bars` (teal first pass of the gauge idea).
- **Deliverables (US-017 depends on these):** SVG redraw of the chosen icon and wordmark, a 1-colour monochrome variant for the terminal splash (`cctop --version` banner and the `?` overlay), favicon at 16/32 px, OpenGraph image. The PNGs are references, not final assets.
- **Usage rule:** the beard is the only joke; the TUI itself stays sober — the mark appears in the help overlay and the site, never in the panels.

## 14. Review findings (v1.1, 2026-09-11)

A pass over the plan against three real transcripts in `~/.claude/projects/`. Findings are applied above; listed here so the reasoning survives.

### 14.1 Calculations that were wrong or unsafe

| # | Finding | Fix |
|---|---|---|
| R1 | **Usage double counting.** One API response is written as N `assistant` lines (one per content block) all carrying the same `usage`. Naïve summing overstates tokens ~2.7× on a real session (386 lines / 143 responses). | Dedupe by `message.id`. Test fixture with a 13-block response. |
| R2 | **Cache TTL assumed 5 min.** The transcript reports `cache_creation.ephemeral_1h_input_tokens` — this machine's sessions run a 1 h cache. Rule A02 and the cache-write price (1.25× vs 2×) depend on it. | Read TTL per call; price per call; A02 threshold = observed TTL. |
| R3 | **Cost was going to be estimated when Claude Code already writes it.** `cost-state` carries `totalCostUSD` and per-model usage. | D11 is authoritative; estimate only fills the gap between writes. |
| R4 | **Subagents are not `isSidechain` lines** in the main transcript; they live in `<sessionId>/subagents/agent-*.jsonl` with a `.meta.json`. US-009 as written would have shown zero agents. | D2a; US-009 rewritten. |
| R5 | **Time-to-first-token is not in the transcript.** Only completion timestamps exist. | Removed from D2 claims; OTel-only. |
| R6 | **Permission-wait formula was backwards.** `PreToolUse` fires before the permission prompt. | `PermissionRequest → PostToolUse − median tool duration`, marked ≈. |
| R7 | **Rate-limit exhaustion projected from tokens.** Limit units are plan-specific and account-wide. | Fit the slope of `used_percentage` samples; show contributing sessions. |
| R8 | **Mockup arithmetic.** 1.62 M / (1.62 M + 148 k + 24 k) = 90 %, not 88 %; "~5 turns" implied a 92 % threshold while the text said 80 %. | Mockup corrected to 90 % / ~3 turns. |
| R9 | **Turn and hook timing needed hooks.** `system` lines already carry `turn_duration.durationMs` and `stop_hook_summary.hookInfos[].durationMs` + `hookErrors`. | Panel 4 uses D2 first; hooks only add Pre/PostToolUse precision. |
| R10 | **Prefix estimate assumed a cold first call.** With a warm 1 h cache the first call is a cache read. | Prefix = `cache_read + cache_creation` of call 1. |

### 14.2 What was missing to actually steer harness usage

Added as US-018 … US-022:

- **Turn ledger** (`Enter` on the Context or Tokens panel): one row per turn — tokens in/out, cache read/write, cost, tools called, duration, compaction marker, effort level, model. Sortable. This is the single most useful steering instrument; the panels are summaries of it.
- **Prefix inspector**: what is in the fixed ~28 k that rides on every request — system prompt, CLAUDE.md files (sizes), tool schema count and bytes, MCP servers and their tool counts, skills listing, memory. Data: first call's usage, `skill_listing` / `deferred_tools_delta` / `mcp_instructions_delta` lines, `settings.json`, `.mcp.json`. Without this the user cannot act on A07/A17.
- **Baselines**: this session vs. your own 7-day median (cost/turn, tokens/turn, cache-hit, tool error rate) computed from `cost-state` lines across all transcripts + `stats-cache.json`. A number without a baseline is trivia; "2.3× your median cost/turn" is a decision.
- **Model mix**: Opus / Sonnet / Haiku share of cost from `modelUsage` — the lever for A14.
- **Fleet awareness**: count and burn of other live sessions (they share the rate limit); `L` picker already lists them.
- **Effort and fast-mode** shown in the header (`effort`, `usage.speed`).
- **Queued prompts** (`queue-operation`) count in the Turn panel.
- **End-of-session report** (`cctop report`): one-page Markdown summary — cost, model mix, top context consumers, advisor hits, baseline comparison — written to `~/.cctop/reports/` on `SessionEnd`, so learning survives the session.
- **Export** (`cctop export --json|--csv`) of the ledger and events for your own analysis.

### 14.3 Every metric explained — the metrics registry

Yes, and it is enforced rather than promised. Every metric is declared once in `src/metrics/registry.rs`: `id`, display name, unit, formula (as text), sources (D-ids), caveats, and the `est`/`≈` conditions. From that registry:

- `cctop metrics --md` generates `docs/metrics.md`; the README and the website include it verbatim in CI (a drift check fails the build if the committed file differs).
- The `?` overlay on a focused panel shows the same definitions for that panel's metrics.
- A render-time assertion: no panel may draw a value whose metric id is not in the registry.
- `cctop query` / the MCP tools return `metric_id`s that link to the same doc anchors.

### 14.4 Digging deeper from inside the session

Three interfaces, all planned; the recommendation is to ship the first two in v1 and the third when the API is stable.

1. **CLI + skill (v1, zero prefix cost; decided 2026-09-11, US-023).** `cctop query <what> [--session id] --json` with `what` ∈ `summary | ledger | tools | files | agents | advice | events --since | explain <metric_id>`. The plugin ships a skill `cctop-insights` whose instructions tell Claude how to call it. Claude pulls exactly what it needs; nothing rides in the prefix except the one-line skill entry. Ask in the session: "why is my cache hit ratio low?" → Claude runs `cctop query advice --json` and `cctop query ledger --last 10 --json` and answers with numbers.
2. **`a` — ask from the TUI (v1).** On any panel, `a` composes a question from that panel's current data ("Turn 14 pushed 31 k tokens; Bash `ls -R` result was 4.4 k twice. What should I change?") and sends it to the attached session's messaging socket (`~/.claude/sessions/<pid>.json` → `messagingSocketPath`, the same channel `SendMessage` uses) as a *drafted prompt* the user submits with Enter. Opt-in, because it spends context; the draft is shown before sending.
3. **MCP server (v1.1).** `cctop mcp` exposes the same queries as typed tools (`cctop_summary`, `cctop_ledger`, `cctop_advice`, `cctop_explain_metric`, `cctop_events`) for sessions that prefer tools over Bash, and for other agents/fleets. Tool schemas cost prefix tokens, so the server registers them as deferred tools (loaded via ToolSearch on demand) — otherwise cctop would trigger its own rule A07.

Not chosen: a `UserPromptSubmit` hook injecting a metrics line into every prompt (costs tokens on every turn whether or not you asked) and a chat panel inside cctop (it would need its own model calls, and the session already has one).
