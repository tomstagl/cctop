<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/svg/cctop-lockup-vertical-dark.svg">
    <img src="brand/svg/cctop-lockup-vertical-light.svg" alt="cctop" width="220">
  </picture>
</p>

<p align="center"><strong>See what Claude Code is doing — live, in a pane beside it.</strong></p>

<p align="center">
  <a href="tasks/prd-cctop.md">PRD</a> ·
  <a href="ralph/prd.json">build plan</a> ·
  <a href="brand/README.md">brand</a> ·
  <a href="LICENSE">MIT</a>
</p>

---

**cctop** is an `htop`/`btop`-style terminal dashboard for a running Claude Code session. It shows the internals Claude Code doesn't — context fill and when the next compaction hits, tokens and cost with cache-hit ratio, rate limits with an exhaustion forecast, what the current turn is waiting on, per-tool latency and how much context each tool pushed, subagents and MCP servers, touched files — on one page, in real time, in a right-hand split while you keep working on the left.

> **Status: planning.** The PRD is finished and reviewed against real transcripts; implementation (Rust + ratatui) starts from [`ralph/prd.json`](ralph/prd.json). Nothing installable yet.

## What it will look like

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

## Panels

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

The Advisor is rule-based (18 rules, no model call): cache misses, cache expiry, runaway tool results, re-reads, exploring in the main context, compaction churn, idle MCP servers, thinking share, permission waits, long foreground commands, pasted input, chatty turns, rate-limit pacing, subagent model choice, error loops, hook overhead, oversized prefix, missing hand-off.

## How it works

Read-only. No changes to Claude Code. Data comes from what Claude Code already writes:

- `~/.claude/sessions/*.json` — which sessions exist and whether they're busy
- `~/.claude/projects/<cwd>/<session>.jsonl` — the transcript: per-response usage (deduplicated by `message.id`), tool calls and results, turn durations, hook timings, Claude Code's own `cost-state`
- `…/<session>/subagents/` — subagent transcripts
- the status-line JSON (via an optional shim) — context size and rate limits
- hooks (optional) — exact tool timings, permission prompts, compactions
- the process tree — running commands, MCP servers, memory

Everything on screen is defined in a metrics registry that generates `docs/metrics.md`; the README will include it once it exists.

<!-- metrics:start -->
<!-- metrics:end -->

## Ask your session about it

`cctop query … --json` exposes every number, and the bundled `cctop-insights` skill teaches Claude Code to use it — ask *"why is my cache hit ratio low?"* in the session and get numbers plus one change to make. See [`plugin/skills/cctop-insights/SKILL.md`](plugin/skills/cctop-insights/SKILL.md).

## Planned install

```
brew install tomstagl/tap/cctop     # or: cargo install cctop
claude plugin add tomstagl/cctop    # adds /cctop and cctop-insights
/cctop                              # opens the dashboard in a right-hand pane
```

Works in tmux, zellij, WezTerm, Kitty and iTerm2.

## Repository

| Path | What |
|---|---|
| `tasks/prd-cctop.md` | Product requirements, v1.1 |
| `ralph/prd.json` | 43 dependency-ordered implementation stories |
| `plugin/skills/` | Claude Code plugin skills |
| `brand/` | Logo (SVG/PNG), build script, candidates |

## License

[MIT](LICENSE). Brand fonts are IBM Plex under the [SIL OFL](brand/fonts/LICENSE.txt).
