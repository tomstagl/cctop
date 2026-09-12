# PRD: cctop pane — the dashboard inside Claude Code's own TUI

**Status:** Draft v1.1 · 2026-09-12 (v1 reviewed against the generated `plugin/.claude/types/claude-code.d.ts` of 2.1.269 and the binary; corrections marked *v1.1*)
**Target:** Claude Code CLI ≥ 2.1.269 on macOS and Linux, fullscreen renderer (`/tui fullscreen`) with graceful behaviour in the classic renderer. Windows is out of scope.
**Depends on:** Claude Code *function hooks* (early access, `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1`, tracked in [anthropics/claude-code#91870](https://github.com/anthropics/claude-code/issues/91870)).

> Decisions taken with the user on 2026-09-12:
> 1. Data source is **hybrid**: engine-native for live core metrics, the `cctop` binary (`cctop query …`) for deep metrics and the Advisor.
> 2. v1 scope is a **full mirror** of the TUI's nine panels, compacted for a docked pane.
> 3. `cctop split` (multiplexer pane / new terminal window) **stays as the fallback** when function hooks are unavailable, and this PRD lists the changes it needs.
> 4. Ship as the **default `/cctop` path now**, with automatic fallback, despite the API being early access.

> **v1.1 review findings applied** (each would have blocked the goal as v1 wrote it):
> - `skill.prompt` only replaces the text the model reads; it **cannot short-circuit a model turn**. The no-model-turn path is the native command (US-001); the skill hook is a graceful degrade (US-006, FR-12).
> - `$` exposes **no engine version**; the compatibility pin is compile-time from the d.ts header, not a runtime read (US-008).
> - `$.fs` has **no delete**; the pane marker file uses a heartbeat + `open:false` instead of removal (§8).
> - `session.detach` is "a client left the session's roster", not module unload; unload is `ui.close` with `e.origin` = `unload`.
> - `tool.call` results carry `isError` (camelCase); `$.session.usage()` is async.
> - **Bun is not on this machine** (Node 22 is) and nothing toolchain-related may live under `plugin/` (the marketplace copies that directory). Tests run under Node; the TS toolchain lives at the repo root.
> - `/plugin-types` writes a second file, `claude-code-mcp.d.ts`, from the *developer's* MCP servers — it is machine-specific and gitignored.
> - The sandbox limits quoted in v1 (8 MiB / 512 files) are not in the binary; what is: `$.fs` reads/writes over 4 MiB and a `$.store` over 4 MiB of JSON are refused.
> - US-007's "already implemented, uncommitted" was wrong: window positioning is committed (`adabf68`); the tab-vs-window fix is new work.
> - Live-session checks cannot be run by an autonomous agent; every story now has **automated** criteria (typecheck, Node tests, a headless render harness, `claude plugin validate --strict`) and the live checks are collected in one manual checklist story (US-010).

---

## 1. Introduction

Today `/cctop` opens the Rust TUI in a *separate* pane or window: it needs tmux, zellij, WezTerm, Kitty or iTerm2, and on a plain terminal (Apple Terminal, GNOME Terminal, …) the best it can do is a second OS window. Meanwhile Claude Code's own `/diff` shows a live panel *inside* the Claude Code screen with no multiplexer at all.

`/diff` is not extensible, but Claude Code ships (behind a flag) a plugin API — *function hooks* — that lets a plugin open a pane docked beside the transcript, draw into it with a JSX element tree, and read session data straight from the engine. This PRD adds a **hooks module to the cctop plugin** that renders the cctop dashboard in such a pane, so the user keeps working in the session on the left and sees cctop's numbers and advice on the right, on any terminal, on macOS and Linux, and — for free — in Claude Code Desktop.

The Rust binary stays: it remains the analysis engine (transcript parsing, cache analysis, per-tool context cost, the 18-rule Advisor) and the standalone TUI. The pane is a second front-end for it.

## 2. Goals

- **No multiplexer required:** `/cctop` shows the dashboard beside the transcript in the fullscreen renderer of a bare terminal (Apple Terminal, GNOME Terminal, Konsole, Alacritty, Ghostty) on macOS and Linux.
- **Panel parity:** every panel of the TUI (Context, Tokens & Cost, Limits, Turn, Tools, Agents & MCP, Files, Events, Advisor) is reachable in the pane, with the same metric definitions (`docs/metrics.md`) and the same `≈` estimate marking.
- **Works without `cctop install`:** context, cost, rate limits, tool timings and turn state come from the engine; the status-line shim and command hooks become optional refinements.
- **Live:** engine-native metrics update within 1 s of the event; binary-backed metrics within the poll interval (2 s while a turn runs, 10 s idle).
- **Invisible cost:** the hooks module adds no perceptible latency to a turn (render < 16 ms, event hooks < 1 ms of own work before `next(e)`, no `$.process.run` inside `tool.call`).
- **Never regress:** when function hooks are off, `/cctop` behaves exactly as before (multiplexer split, then new-window fallback, then the manual hint).
- **Verifiable without a human at the keyboard:** every render function is testable against fixture JSON in a headless harness; the live checks are a short, explicit checklist.

## 3. Background: the function-hooks API (verified against 2.1.269)

Extracted from `plugin/.claude/types/claude-code.d.ts`, written by `/plugin-types` (header: *EARLY ACCESS: may change between releases without notice*; the first line names the Claude Code version that wrote it). What we rely on:

| Need | API |
|---|---|
| Declare the module | `plugin/hooks/hooks.json` → `"modules": ["./pane.tsx"]` (accepted keys: `description`, `hooks`, `modules`, `surface`); `.ts/.tsx/.js/.jsx`, transpiled by Claude Code's embedded Bun in a sandbox; no DOM, no Node |
| Entry point | `export const register: Register = (on, options) => { … }`; hooks are `on(event, matcher?, ($, e, next) => …)`; `on(...).catch(handler)` receives a hook's throw/overrun (`e.budget`, `e.timeout`) |
| Open / close the pane | `$.ui.open({ id: "cctop", title: "cctop", focus? })` (Promise; an open id is retitled), `$.ui.close({ id })`; the engine event `ui.close` carries `e.origin` (`plugin` / `person` / `unload`) — *v1.1:* `ui.open` is an op, not an engine event |
| Draw the body | `on("ui.render", { component: "Pane" }, …)` returning a tree of `Box`, `Text`, `Button`, `Input`, `Select`, `Link`, `Code` from `$.ui.resolve(e)`; `e.requestId` is the pane id; props: `e.props.title`, `isFocused`, `bodyColumns`, `placement: "dock" \| "inline"`, `scroll`; `e.viewport?.columns/rows`; `Text wrap` accepts `wrap \| end \| middle \| truncate \| truncate-start \| truncate-middle \| truncate-end`; `Button hotkey` fires only while the pane is focused |
| Placement rules | fullscreen: docked beside the transcript from **110 columns** (144 unless `focus` was requested; judged at each open); classic renderer: inline above the prompt; several panes = tabs; user keys: `ctrl+x` arrows resize (persisted to settings as `pluginPanes: { dockColumns, inlineRows }`), `ctrl+x x` closes, `ctrl+x tab` focuses/cycles |
| Redraw | `$.ui.invalidate("ui.render")`; `$.clock.every(ms, fn)` / `$.clock.after` return `{ cancel() }`; `$.clock.now()` |
| Engine data | `await $.session.usage()` → `{ context: { tokens?, window, percent? }, rateLimits: [{ kind, percentUsed, resetsAt? }], cost?: { usd } }` — the status line's own figures; `$.session.id() / model() / turns() / cwd() / repo()` (all async); events `turn.start` (`turnId`, `text`), `turn.step`, `turn.complete` (`durationMs`, `reason`), `tool.call` (wrap `next` to time it; result `{ result, text, isError? }`), `tool.check`, `session.compact`, `session.start` (`cwd`, `surface`, `isInteractive`), `agent.spawn`, `$.agent.list()` |
| Binary | `$.process.run(["cctop", "query", "summary", "--session", id], { timeoutMs })` → `{ exitCode, stdout, stderr }`; argv, no shell; 30 s default timeout, 10 min max; whole output read before it resolves |
| Native command | `$.command.register({ name: "cctop", description, argumentHint, immediate: true })` + `on("command.run", { command: "cctop" }, …)` returning `{ text }` — no model turn; "a built-in's name is refused" (a skill is not a built-in); `skill.prompt` replaces the *text the model reads* for a skill, nothing more |
| State | `$.store.get/set/delete/keys` (JSON, per plugin, survives sessions, ≤ 4 MiB); `$.config` rows in `/config` |
| Files | `$.fs.read/write/list/exists/stat` (relative to the session cwd or absolute; > 4 MiB refused) — *v1.1:* **no delete** |
| Diagnostics | `$.ui.log`, `$.ui.toast(text, { timeoutMs })`, `$.ui.status`; `claude --debug` shows refused/skipped hooks |
| Dev loop | `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --plugin-dir ./plugin` (hot reload), `/plugin-types ./.claude/types` (also runs as `claude -p '/plugin-types …'` **only with the flag set**; writes `claude-code.d.ts` and a machine-specific `claude-code-mcp.d.ts`), `/reload-plugins`, `claude plugin validate --strict ./plugin` |
| Gate | on when `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` or the GrowthBook flag `tengu_plugin_hooks_modules` (default off); a module must be reviewed/accepted in `/plugin` before it runs |

Not available: embedding a PTY / the ratatui TUI as-is; an engine version at runtime; deleting files. The pane body is a declarative element tree, so the dashboard is re-implemented as TSX render functions over cctop's JSON.

**Documentation status (checked 2026-09-12):** the official docs (`plugins.md`, `plugins-reference.md`, `fullscreen.md`, changelog) describe no plugin pane API; `/diff` is documented as a built-in view and the only documented split-pane mechanism is agent teams' `teammateMode` (tmux / iTerm2, else in-process). Everything in the table above comes from the binary's own `claude-code.d.ts` and [issue #91870](https://github.com/anthropics/claude-code/issues/91870). Treat it as a preview contract: regenerate the types after every Claude Code update and report breakage upstream via `/feedback` or the issue.

## 4. User stories

Automated criteria are what an agent checks; **live** criteria are collected in US-010 and run by a person.

### US-001: Toolchain, types and hooks-module skeleton
**Description:** As a developer, I want a typechecked, testable hooks module skeleton that Claude Code loads, so that every later story lands on a green base.

**Acceptance Criteria:**
- [ ] `plugin/.claude/types/claude-code.d.ts` (written by `/plugin-types`, 2.1.269) is checked in; `plugin/.claude/types/claude-code-mcp.d.ts` is gitignored
- [ ] Repo-root `package.json` (devDependencies: `typescript` ≥ 5.4 only) and `tsconfig.json` as the d.ts header prescribes (`lib: ["es2023"]`, `types: []`, `jsx: "react"`, `jsxFactory: "h"`, `jsxFragmentFactory: "Fragment"`, `include: ["plugin/.claude/types", "plugin/hooks", "tests/pane"]`); `npm run typecheck` = `tsc --noEmit`; **nothing under `plugin/` besides the module and the types** (no `node_modules`, no tests)
- [ ] `npm test` compiles `plugin/hooks` + `tests/pane` with `tsc` to `.test-build/` (gitignored) and runs `node --test .test-build/tests/pane`; a first test passes
- [ ] `plugin/hooks/hooks.json` declares `modules: ["./pane.tsx"]`; `plugin/hooks/pane.tsx` exports `register`; the module registers `ui.render` for `{ component: "Pane" }` drawing one `Text` line, `command.run` for `cctop`, and calls `$.command.register({ name: "cctop", description, argumentHint: "[view|close]", immediate: true })` on load
- [ ] `claude plugin validate --strict ./plugin` passes
- [ ] `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p --plugin-dir ./plugin --debug 'say ok' --max-turns 1` shows the module loaded and no refused `$` calls (grep the debug output; document the exact grep in `docs/verification/pane.md`)
- [ ] CI (`.github/workflows/ci.yml`) runs `npm ci`, `npm run typecheck`, `npm test`, `claude plugin validate --strict ./plugin` (validate step skipped when `claude` is not on the runner)
- [ ] Typecheck passes; tests pass
- [ ] *Live (US-010):* `/cctop` appears in the slash-command list; `/cctop` opens the pane docked at 144 and 110 columns, inline in `/tui default`; `/cctop` again closes it; `/cctop close` closes it; `/cctop:cctop` still resolves the skill (open question 1)

### US-002: Headless render harness
**Description:** As a developer, I want to render the pane's element tree to plain text without Claude Code, so that every view can be asserted against fixture data in `npm test`.

**Acceptance Criteria:**
- [ ] `tests/pane/harness.ts`: a fake `$` (`ui.resolve` returning a fake element table, `ui.invalidate` counting calls, `clock` with a manual tick, `store` in memory, `process.run` scripted per argv, `session.usage/id/model/turns` scripted, `ui.log` captured) and a fake `on` that records registrations and can dispatch an event through them with a working `next`
- [ ] `tests/pane/render.ts`: lays out `Box` (flexDirection row/column, width, flexGrow) and `Text` (`wrap` truncate/wrap, `dimColor`, `bold`, `color` ignored) into a `columns × rows` character grid; `Button` renders as `[hotkey label]`; unknown elements render their children
- [ ] `tests/pane/fixtures/`: JSON captured from `cctop query summary|tools|files|agents|advice|events --session fixtures/session-a.jsonl` plus a `usage.json` in `SessionUsage` shape; `scripts/pane-fixtures.sh` regenerates them
- [ ] A test renders the skeleton pane at 50 and 80 columns and asserts no row exceeds the width
- [ ] Typecheck passes; tests pass

### US-003: Model, reducer and engine-native core metrics
**Description:** As a user without `cctop install`, I want context, cost, rate limits and the current turn tracked live from the engine, so that the pane is useful with zero setup.

**Acceptance Criteria:**
- [ ] `plugin/hooks/model.ts`: a plain `Model` object (`usage`, `turn`, `tools`, `compactions`, `binary`, `view`, `open`, `stale`) and `reduce(model, event) → model` (pure, no `$`)
- [ ] Hooks: `session.start`, `turn.start`, `turn.complete` (`durationMs`, `reason`), `turn.step` (tool starts), `tool.call` wrapping `next` (per tool: calls, errors via `isError`, duration, result text length → `tokens_to_ctx` ≈ len/4, running tool + elapsed), `session.compact` (count)
- [ ] `$.session.usage()` is read from a `$.clock.every(1000)` timer while a turn runs and once after `turn.complete`/`session.start`; **never awaited before `next(e)`**
- [ ] Every hook returns `next(e)` unchanged; own work before `next` < 1 ms (timestamps and counters only); every registration has `.catch` logging via `$.ui.log`
- [ ] Reducer tests: a scripted sequence (start → tool.call ×3 with one `isError` → compact → complete) yields the expected counts, p50 of durations, `tokens_to_ctx`, `reason`; usage rows show `tokens / window (percent %)`, `five_hour` / `seven_day` `percentUsed` with a `resetsAt` countdown from a fixed `now`, `cost.usd`
- [ ] Typecheck passes; tests pass

### US-004: Binary detection and poller
**Description:** As a user with the `cctop` binary installed, I want the pane to pull deep metrics from `cctop query`, so that it matches the TUI.

**Acceptance Criteria:**
- [ ] On load, `$.process.run(["cctop", "--version"], { timeoutMs: 3000 })` sets `model.binary` to `present | missing`; missing shows a one-line hint in the header (`brew install tomstagl/tap/cctop`) and disables binary-backed sections only
- [ ] `plugin/hooks/poller.ts`: runs `cctop query summary|tools|files|agents|advice|events --session <id>` (`$.session.id()`), one command at a time with a single in-flight promise, every 2 s while a turn runs and 10 s idle; each call `timeoutMs: 5000`; a failure keeps the last good data and sets `stale` after 30 s
- [ ] Only `cctop query` verbs the current binary has are called (`cctop query --help` parsed once); an unknown verb marks its section `unsupported`
- [ ] Precedence: engine-native wins for Context and Limits when both exist; binary values fill Tokens & Cost breakdown (cache read/write/fresh/output/thinking, hit ratio, TTL), burn rate, input rate, agents, files, advice, events
- [ ] Poller tests with the scripted `process.run` and manual clock: cadence switches with turn state, no overlap when a call is slow, stale after 30 s, missing binary never spawns `query`
- [ ] Typecheck passes; tests pass

### US-005: Overview view (Context, Tokens & Cost, Limits, Turn)
**Description:** As a user, I want the four core panels in one compact Overview, so that the default pane is the TUI's top half.

**Acceptance Criteria:**
- [ ] `plugin/hooks/views/overview.tsx`: header row (status busy/idle/waiting, turn number, elapsed, model, effort; binary/shim/hooks badges) then Context (size / window / %, velocity, turns until autocompact, compactions), Tokens & Cost (five classes, hit ratio, TTL, cost, burn rate), Limits (5 h, 7 d, resets in, projected exhaustion), Turn (state, elapsed, API vs tool time, waiting on, permission waits, queued prompts)
- [ ] Layout adapts to `bodyColumns`: ≥ 60 two-column rows, < 60 single column; numbers right-aligned in fixed-width `Box`es; every `Text` uses `wrap="truncate"`; colours are palette names only (no hex / truecolour)
- [ ] `≈` marker on every value whose source is `approx: true`; each row has a `key` equal to the metric id in `docs/metrics.md`
- [ ] When an Advisor item's severity is high, its headline is the last Overview row
- [ ] Harness tests at 50, 60 and 80 columns against the fixtures: expected values present, no row wider than the width, `≈` where the fixture says `approx`
- [ ] Typecheck passes; tests pass

### US-006: Detail views (Tools, Agents & MCP, Files, Events, Advisor)
**Description:** As a user, I want the five remaining panels reachable in the pane, so that nothing from the TUI is lost.

**Acceptance Criteria:**
- [ ] `views/tools.tsx` (calls, errors, p50/p95, tokens → context, top consumers), `views/agents.tsx` (subagents with state and tokens, MCP servers with RSS and calls, background tasks), `views/files.tsx` (touches, lines ±, re-reads), `views/events.tsx` (last 50, newest at bottom), `views/advisor.tsx` (ranked list; top item expanded with evidence, explanation and action)
- [ ] Same units and colour semantics as the TUI (green ≥ 0.8 hit ratio, amber ≥ 0.5, red below); `≈` marking; `key` = metric id
- [ ] Each view caps its tree at 400 rows; long views rely on `e.props.scroll`
- [ ] Binary missing: each view shows the one line "needs the cctop binary"
- [ ] Harness tests per view against the fixtures at 50 and 80 columns
- [ ] Typecheck passes; tests pass

### US-007: Navigation, `/cctop` arguments and classic-renderer placement
**Description:** As a user, I want to switch views by hotkey or command and get a sensible pane in the classic renderer.

**Acceptance Criteria:**
- [ ] View bar of `Button`s (`1 Overview · 2 Tools · 3 Agents · 4 Files · 5 Events · 6 Advisor`) with `hotkey`; `onPress` sets `model.view` and invalidates
- [ ] `command.run` for `cctop`: no args toggles the pane; `<view>` opens it on that view; `close` closes it; unknown arg answers `{ text }` with the usage line; the pane is opened without `focus`
- [ ] `placement === "inline"` renders only the header and the Context/Limits lines
- [ ] Harness tests: dispatching `command.run` through the fake `on` opens/toggles/switches; inline placement hides detail views
- [ ] Typecheck passes; tests pass
- [ ] *Live (US-010):* hotkeys fire only while the pane is focused (`ctrl+x tab`); verified at 110, 144 and 200 columns

### US-008: Refresh, lifecycle and marker file
**Description:** As a user, I want the pane to be live without slowing the session down, and to survive reloads.

**Acceptance Criteria:**
- [ ] `$.ui.invalidate("ui.render")` after each snapshot change, coalesced to ≤ 4/s (own throttle; the engine folds further)
- [ ] Timers and the poller stop on `ui.close` (any origin) and start on open; while closed only the `turn.*`/`tool.call` bookkeeping runs
- [ ] `{ open, view }` persisted in `$.store`; on `session.start` with `open: true` and `isInteractive`, the pane is reopened
- [ ] A throw inside `ui.render` is caught (`.catch` and a try/catch in the hook) and renders one error line above the last good snapshot
- [ ] Marker `~/.cctop/pane/<session>.json` = `{ version, sessionId, openedAt, heartbeatAt, open }` written on open, refreshed by the poller tick, rewritten with `open: false` on `ui.close`; readers treat `open: false` or `heartbeatAt` older than 30 s as absent (*v1.1:* `$.fs` cannot delete)
- [ ] Tests with the manual clock: invalidate count under a burst, timers cancelled on close, store round-trip, marker content after open/close
- [ ] Typecheck passes; tests pass
- [ ] *Live (US-010):* `/reload-plugins` reopens the pane; Claude Code CPU idle with the pane open < 2 %

### US-009: Skill, fallback and packaging integration
**Description:** As a user on any Claude Code build, I want `/cctop` to do the best thing available, and installing the plugin to be enough.

**Acceptance Criteria:**
- [ ] `skill.prompt` hook for the `cctop` skill: opens the pane, then returns `{ text: "The cctop pane is open beside the transcript. Reply with exactly one line: \"cctop is open in the side pane.\" Run no tools." }` (*v1.1:* the model turn still happens; only the native command avoids it)
- [ ] `plugin/skills/cctop/SKILL.md` step 3: "if `~/.cctop/pane/<session>.json` exists with `open: true` and a fresh heartbeat, say the pane is already open; else run `cctop split`"; the fallback text explains `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude` and that docking needs the fullscreen renderer
- [ ] `cctop split` refuses (exit 0, message "cctop pane is already open in this session") when a fresh marker with `open: true` exists for the session (Rust, `src/split.rs`, unit-tested with a temp dir); exit codes for every other path unchanged
- [ ] `plugin/.claude-plugin/plugin.json` version bumped to `0.2.0`; `plugin/README.md` documents the pane, the flag, the trust prompt in `/plugin`, the fallback; root `README.md` install section adds `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1`
- [ ] `scripts/check-plugin-types.sh` compares the version on line 1 of `claude-code.d.ts` with `claude --version` and exits 1 with a "run /plugin-types" hint on mismatch; wired into `make check` (skipped when `claude` is absent); the module carries `const TESTED_WITH = "2.1.269"` shown in the header badge
- [ ] `cctop-insights` skill untouched
- [ ] Typecheck passes; tests pass; `cargo test` and clippy clean
- [ ] *Live (US-010):* flag off → `/cctop` in tmux still splits, in Apple Terminal still opens the new window

### US-010: Live verification checklist
**Description:** As the maintainer, I want one checklist for everything only a person at a real terminal can verify, so that "done" is explicit.

**Acceptance Criteria:**
- [ ] `docs/verification/pane.md` lists every *Live* item of US-001, US-007, US-008, US-009 plus: Advisor top item equals `cctop query advice`; engine-native values visible within 1 s; `/cctop` opens in < 500 ms; turn latency with vs. without the module differs < 1 % over 20 turns on the fixture; each with the exact command/keys, the expected result, a `Result:` line (`pass` / `fail` / `pending`) and the Claude Code version
- [ ] The checklist's `pending` items are listed in `plugin/README.md` under "Verification status"
- [ ] Typecheck passes

### US-011: `cctop split` fallback — Apple Terminal real window
**Description:** As a user without function hooks on macOS, I want the Terminal.app fallback to open a real window, not a tab, so that the dashboard sits beside the session.

**Acceptance Criteria:**
- [ ] Root cause confirmed (macOS "Prefer tabs when opening documents"); `src/split.rs` uses `open -na Terminal <generated .command file>` (file in `~/.cctop/run/`, executable, runs `cctop run --session <id>`) or, if that still yields a tab, System Events "Move Tab to New Window"; the existing bounds positioning (`adabf68`) is kept
- [ ] Unit test for the generated command shape and `.command` file content
- [ ] `docs/hosts.md` row updated with the verification date
- [ ] `cargo test`; clippy clean

### US-012: `cctop split` fallback — Linux terminals and idempotency
**Description:** As a user without function hooks on Linux, I want a new-window fallback and no duplicate dashboards.

**Acceptance Criteria:**
- [ ] `src/split.rs` new-window order on Linux: `$TERMINAL`, `x-terminal-emulator -e`, `gnome-terminal --`, `konsole -e`, `alacritty -e`, `kitty` (`kitten @ launch` when `$KITTY_LISTEN_ON`, else `kitty -e`), `xterm -e`; each runs `cctop run --session <id>`; `Host::LinuxWindow(program)` variant
- [ ] Idempotent: `cctop run` writes `~/.cctop/run/<session>.pid`; a second `split` with a live pid prints "cctop already running for this session (pid N)" and exits 0
- [ ] iTerm2 new-window path (`TERM_PROGRAM=iTerm.app` without `ITERM_SESSION_ID`) kept; `manual_hint` names the Linux fallbacks
- [ ] `docs/hosts.md` lists every host with detection env, command and verification status
- [ ] Unit tests for detection order and command shape per host (env mocked, `which` mocked); `cargo test`; clippy clean

## 5. Functional requirements

- FR-1: The plugin must ship a function-hooks module at `plugin/hooks/pane.tsx`, declared in `plugin/hooks/hooks.json`; `plugin/` contains no toolchain files.
- FR-2: The module must register a native `/cctop` command with `argumentHint: "[view|close]"`; `/cctop` toggles the pane, `/cctop <view>` opens it on that view, `/cctop close` closes it — all without a model turn.
- FR-3: The module must open exactly one pane, id `cctop`, title `cctop`; it must never request `focus` on open.
- FR-4: The pane body must be produced by a `ui.render` hook matched on `{ component: "Pane" }` and must only draw when `e.requestId === "cctop"`.
- FR-5: The Overview view must show Context (size / window / %, velocity, turns until autocompact, compactions), Tokens & Cost (cache read/write/fresh/output/thinking, hit ratio, TTL, cost, burn rate), Limits (5 h, 7 d, resets in, projected exhaustion), Turn (state, elapsed, API vs tool time, waiting on, permission waits, queued prompts).
- FR-6: Detail views must show Tools, Agents & MCP, Files, Events (last 50, newest at bottom), Advisor (ranked; top item expanded with evidence, explanation and action).
- FR-7: Each metric must carry the `≈` marker when the source marks it `approx`, and the metric id must be the one in `docs/metrics.md`.
- FR-8: Engine-native metrics must refresh within 1 s of the underlying event; binary-backed metrics within 2 s while a turn runs and 10 s when idle.
- FR-9: The module must degrade: without the binary, Overview (engine-native parts) and Turn still work, and every binary-backed section shows "needs the cctop binary" in one line.
- FR-10: The module must never call `$.process.run` from inside `tool.call`, `turn.step` or `ui.render`; only from timers.
- FR-11: The module must persist `{ open, view }` in `$.store` and restore the pane on the next interactive `session.start` when `open` was true.
- FR-12 (*v1.1*): When the module is loaded, invoking the `cctop` skill must open the pane and replace the skill prompt with a one-line instruction so the model answers in one line and runs nothing (`skill.prompt` cannot skip the turn; FR-2 is the no-turn path).
- FR-13: When the module is not loaded, the `cctop:cctop` skill must behave as today, with the added fallbacks of US-011/US-012 and an explanation of how to enable function hooks.
- FR-14: The `cctop-insights` skill must remain untouched.
- FR-15: All hooks must call `next(e)` and pass events through unchanged; the module must never rewrite tool inputs, prompts or model choices.
- FR-16 (*v1.1*): Every view must be renderable in the headless harness from fixture JSON, and `npm test` must cover every view at 50 and 80 columns.

## 6. Non-goals

- No embedding of the ratatui TUI, no PTY in the pane, no mouse support beyond what `Button` gives.
- No Windows support.
- No writes to the session: the pane is read-only (no `$.tool.call`, no prompt injection, no `prompt.context` sections).
- No replacement of the standalone TUI (`cctop run`) or its themes; the pane uses Claude Code's palette only.
- No desktop/mobile-specific layouts in v1 (the `terminal` tree is what remote surfaces get; `Svg` sparklines are a follow-up).
- No attempt to reach the built-in `/diff` panel's internals or Claude Code's app state.
- No auto-installation of the status-line shim or command hooks from the module (still `cctop install`, still the user's choice).
- No runtime engine-version detection (the API offers none); compatibility is pinned at build time.

## 7. Design considerations

- **Pane width:** docked panes start narrow; design Overview for 50 body columns (the TUI's narrow layout) and let two-column rows kick in at ≥ 60. `Text wrap="truncate"` everywhere; numbers right-aligned with fixed-width `Box`.
- **View switching:** a top row of `Button`s with `hotkey`; they only fire while the pane has focus (`ctrl+x tab`), which keeps hotkeys away from the composer. `/cctop <view>` is the keyboard-free route.
- **Advisor first:** when an Advisor item's severity is high, the Overview shows its headline as the last row so the recommendation is visible without switching views.
- **Consistency with the TUI:** reuse the TUI's colour semantics and section order; reuse `docs/metrics.md` ids in `key` props for tests.
- **Classic renderer:** inline placement above the prompt has few rows; show only the header and the Context/Limits lines there (`e.props.placement === "inline"`).

## 8. Technical considerations

- **Module shape:** `register(on, options)` builds a `Model` (plain object), `reduce(event)`, a `Poller`, and `views/*.tsx` render functions taking `(model, elements, columns, placement)`; all unit-testable without Claude Code through `tests/pane/harness.ts`.
- **Toolchain (*v1.1*):** repo-root `package.json` + `tsconfig.json`; `typescript` is the only dev dependency; tests are `node --test` over `tsc` output in `.test-build/` (Node ≥ 22; Bun is not required — Claude Code transpiles the module itself at load). `plugin/` ships only `hooks/`, `skills/`, `.claude-plugin/`, `.claude/types/claude-code.d.ts`, `README.md`.
- **Snapshot sources merged by precedence:** engine-native > status-line shim (via binary) > transcript estimate; the binary already applies the last two, so the module only overrides Context and Limits with `$.session.usage()`.
- **Session id:** `$.session.id()`; pass `--session <id>` to every `cctop query` so the pane never attaches to a neighbouring session.
- **Sandbox (*v1.1*):** no Node, no DOM; JSX compiles against `h`/`Fragment`; elements come only from `$.ui.resolve(e)`; `$.fs` transfers and the `$.store` are capped at 4 MiB.
- **Performance budget:** `ui.render` builds ≤ 400 rows; render hook < 16 ms on the fixture; `tool.call` wrapper stores timestamps only; poller commands serialised with a single in-flight promise; `$.session.usage()` only from timers or after `next`.
- **Trust flow:** users must accept the module in `/plugin` details once; document it. Marketplace updates that change the module trigger a new review.
- **Version drift:** the API is early access; `claude-code.d.ts` is checked in and its header version compared with `claude --version` by `scripts/check-plugin-types.sh`; CI runs `tsc` so a breaking change fails visibly. Keep the split fallback as the safety net.
- **Marker file (*v1.1*):** `~/.cctop/pane/<session>.json` = `{ version, sessionId, openedAt, heartbeatAt, open }` via `$.fs.write`; refreshed on every poller tick, `open: false` on `ui.close` (incl. origin `unload`); a reader ignores `open: false` and heartbeats older than 30 s. Written with an absolute path (`$.fs` paths are relative to the session cwd otherwise); the home directory comes from `$.env` (`HOME`).
- **Rust side changes (US-009, US-011, US-012):** `src/split.rs` marker check, Terminal.app real-window fix, Linux terminals, pid file, `docs/hosts.md`. No changes to the query JSON needed; add `cctop query pane` (one combined, trimmed document) only if the poller's payload proves too large (open question 3).

## 9. Success metrics

- `/cctop` opens the pane in < 500 ms in a fullscreen session with function hooks on, on Apple Terminal and GNOME Terminal, with no multiplexer.
- Panel parity checklist: 9/9 TUI panels represented; every metric id in `docs/metrics.md` appears in a view (asserted by a test over the `key` props).
- Engine-native metrics visible within 1 s of the event; binary-backed within the poll interval; measured on the fixture session.
- Turn latency with the module loaded vs. unloaded differs by < 1 % (measured over 20 turns on the fixture).
- Zero-setup value: with neither `cctop install` nor the binary, the pane still shows context %, cost, both rate limits and the current tool.
- No regression: `cctop split` test matrix (tmux, zellij shape, WezTerm shape, Kitty shape, iTerm2, Terminal.app, Linux fallbacks) passes.
- `npm test` covers every view at two widths; `docs/verification/pane.md` has no `pending` item at release.

## 10. Open questions

1. **Command name vs. skill name:** with `$.command.register({ name: "cctop" })`, does `/cctop` run the native command while `/cctop:cctop` still resolves the skill (expected: plugin skills are namespaced), or does the engine fold them? Resolve in US-001's live check; if they clash, name the native command `cctop-pane` and rely on the `skill.prompt` degrade.
2. **Does `command.run` fire for a skill invocation?** If `/cctop:cctop` raises `command.run` with `command: "cctop:cctop"`, answering `{ text }` there *would* skip the model turn — a better FR-12. Check in US-001's debug log.
3. **Poller payload:** is `summary` + `tools` + `advice` + `events` every 2 s cheap enough (JSON size, process spawn)? Fallback: a single `cctop query pane` subcommand.
4. **Permission waits without the shim:** does `tool.check` fire on the permission prompt so the module can time waits natively? If not, keep them binary-backed (needs `cctop install`).
5. **Apple Terminal tab merge:** confirm the root cause and pick between `open -na Terminal <file.command>` and the System Events "Move Tab to New Window" approach (US-011).
6. **Ghostty:** no scripting API for splits; is a new-window fallback (`ghostty +new-window`?) available, or does Ghostty stay "manual hint" only?
7. **GA timing:** Anthropic says function hooks ship "on the scale of weeks"; if the flag disappears or the API changes before then, which parts of this PRD need a re-check (US-001, placement rules, the trust flow)?

*Answered in v1.1:* dock width — the engine persists `pluginPanes: { dockColumns, inlineRows }` to settings when the user resizes with `ctrl+x` arrows; no default needs setting.
