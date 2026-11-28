# PRD 3: `/context`'s breakdown on every turn, from the pane

**Status:** v1.0 · 2026-11-26 — ready for implementation. **Issue:** #27.
**Target:** cctop v0.10.x / plugin 0.11.x; the pane (TypeScript) writes, Rust reads, both surfaces draw.
**Depends on:** PRD 0, #24 (`prd-cctop-contract-2-1-332.md`), which needs the 2.1.332 d.ts in place. The call exists in 2.1.284 too, but the pane's types must match the engine it runs on.
**Source:** the 2.1.332 d.ts (`SessionUsageArgs`, `SessionContextBreakdown`, `ContextCategory`, `ContextCategoryKind`, `ContextBreakdownDetail`) and `tasks/prd-cctop-context-residency.md`.

> Decisions taken with the user on 2026-11-26:
> 1. One state, many surfaces still holds: the pane does not draw a breakdown of its own. It hands the breakdown to the Rust side, and the dashboard object carries it to both surfaces.

---

## 1. Introduction

The context-residency feature (`tasks/prd-cctop-context-residency.md`, decision 4) estimates what is in the window from `chars / 4`. It calibrates "opportunistically": when the person happens to run `/context`, the captured output (`ContextCapture`, `transcript/local_command.rs:64`) reconciles the totals (`metrics/context.rs:957`). Most sessions never run `/context`, so most residency figures stay uncalibrated.

The function-hooks API answers the same breakdown on demand:

```ts
$.session.usage({ breakdown: 'summary' })
// → { startedAt, context: { tokens, window, percent, breakdown: SessionContextBreakdown }, rateLimits, cost }
```

| `SessionContextBreakdown` field | Meaning (d.ts) |
|---|---|
| `categories[] {name, tokens, color, isDeferred, kind}` | /context's rows; `kind` ∈ `used` \| `free` \| `buffer` \| `deferred`. *"Branch on `kind`, never on the row's `name`."* |
| `totalTokens`, `maxTokens`, `rawMaxTokens`, `percentage` | the totals /context prints |
| `autoCompactThreshold?`, `isAutoCompactEnabled`, `autocompactSource` | the compaction line |
| `memoryFiles[]`, `mcpTools[]`, `agents[]`, `skills?`, `slashCommands?` | per-item rows: names/paths and tokens |
| `gridRows` | the coloured grid; not used |
| `model` | the model the breakdown was computed for |

`'summary'` is computed *"from the last response's usage and local estimates"*: no request, no cost. `'full'` calls the token-count API per category. Not used (non-goals).

The pane already calls `$.session.usage()` (no breakdown) once a second while a turn runs (`pane.tsx:463`, `USAGE_POLL_MS`). This PRD adds one breakdown read per finished turn and hands it to Rust as a spool file. Every session with the pane loaded then gets a calibration point per turn instead of only when `/context` is typed.

## 2. Goals

- A calibration point for context residency after every main-thread turn while the pane is loaded.
- The dashboard's context cell shows the composition (system / tools / memory / skills / messages / free / buffer) from Claude Code's own numbers when available, identically in the TUI and the pane.
- No extra API cost: `'summary'` only.

## 3. Design

### 3.1 The pane writes a spool (`plugin/hooks/`)

- On `turn.complete` and `session.compact`, debounced to at most one read per 15 s, the pane calls `$.session.usage({ breakdown: 'summary' })` and writes `~/.cctop/context/<sessionId>.json`:

```json
{ "v": 1, "at": 1764170000000, "tokens": 88700, "window": 1000000, "model": "claude-opus-5-5",
  "categories": [{"name": "System prompt", "tokens": 4100, "kind": "used"}, …],
  "autocompact_threshold": 967000, "is_autocompact_enabled": true,
  "memory_files": [{"path": "…/CLAUDE.md", "tokens": 2100}], "mcp_tools": [{"name": "mcp__x__y", "tokens": 900}],
  "skills": {"count": 12, "tokens": 1400}, "agents": [{"name": "Explore", "tokens": 300}] }
```

- `gridRows`, `color` and `slashCommands` are dropped. The write goes through the existing `fs.write` facade (`pane.tsx:373`), so the validator sees `$.fs.write` from a top-level function.
- The reducer (`model.ts`) gets an action `breakdown.read` (time only) so the debounce lives in pure code and is testable.
- A failed call or write is `$.ui.log`ged once per session, never retried in a loop.

### 3.2 Rust reads it (`src/`)

- A new collector `context_spool.rs` watches `~/.cctop/context/<session>.json`, using the same file-watch pattern as the status shim (`status.rs` `Watcher`), and parses it into `ContextCapture`. Same type, so residency needs no second code path: `categories` keep the printed names, `free_tokens` comes from `kind = free`, `autocompact_buffer` from `kind = buffer`, `mcp_tools` from the list's count and sum.
- The capture is anchored to the last API call at or before `at`, matched by timestamp, mirroring how a transcript `/context` capture is anchored by line index today. The newer of {transcript capture, spool capture} wins.
- `attach.rs` rebuilds the watcher when the session changes. `load.rs` reads the file once for `cctop query`.
- Stale rule: a spool older than the last compaction boundary is ignored.

### 3.3 Surfaces

- `dashboard.rs`'s context cell gains an optional composition row (`used` categories in /context's order, then free, then buffer). It is drawn by `ui/dashboard.rs` and by the pane's `overview.tsx` from the dashboard object verbatim. The row is present only when a capture exists, labelled `per /context` (transcript) or `per Claude Code` (spool), and cut at the cell's fixed width.
- The residency inspector's header says `calibrated <age> ago` from whichever capture it used.
- `cctop query context` (or `dashboard`) carries `composition[]` and `composition_source`.

## 4. User stories

### US-001: The pane reads the breakdown once per turn
**Description:** As a pane user, I want the pane to take /context's breakdown at the end of each turn, at no API cost, so that cctop's context figures are calibrated without me typing `/context`.

**Acceptance criteria:**
- [ ] `turn.complete` and `session.compact` trigger `$.session.usage({ breakdown: 'summary' })` at most once per 15 s; the debounce is a pure function in `model.ts` with tests.
- [ ] The call is never made with `'full'` (grep test in `tests/pane/`).
- [ ] The spool JSON matches §3.1 exactly; `tests/pane/breakdown.test.ts` feeds a fake `SessionUsage` with every field and asserts the written text, `gridRows`/`color` absent.
- [ ] `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin validate --strict ./plugin` clean; `npm run typecheck` and `npm test` green.

### US-002: Rust ingests the spool as a context capture
**Description:** As a developer, I want the spool to become a `ContextCapture`, so that residency calibration uses it with no new reconciliation code.

**Acceptance criteria:**
- [ ] `src/context_spool.rs` parses v1 into `ContextCapture` (mapping in §3.2); unknown `v` is ignored with a log line.
- [ ] Anchoring by timestamp: a unit test with three API calls and a spool `at` between the second and third anchors to the second.
- [ ] Newer-wins between a transcript capture and a spool capture (test both orders).
- [ ] A spool older than the last `compact_boundary` is ignored (test).
- [ ] `attach.rs` rebuilds the watcher on session change; `load.rs` reads it for `query`.
- [ ] `cargo test context_spool` passes; `make check` green.

### US-003: The composition row on both surfaces
**Description:** As a person using cctop, I want to see what fills the window in Claude Code's own numbers, so that I know which lever to pull.

**Acceptance criteria:**
- [ ] `dashboard.rs` emits the composition row only when a capture exists, with the source label; insta snapshot with and without.
- [ ] `ui/dashboard.rs` and `plugin/hooks/views/overview.tsx` draw it from the object; `scripts/pane-fixtures.sh` regenerated; the row-identity test covers it (a fixture with a `/context` capture is enough: fixture B has one if `ContextCapture` parses from it, else compose one).
- [ ] `cctop query dashboard` carries `composition[]`, `composition_source`; `metrics/registry.rs` gains `context_composition`; docs regenerated.
- [ ] `docs/verification/pane.md` item 41: *"After a turn the TUI's context cell shows `per Claude Code` while the pane is loaded"*, `Result: pending`.

## 5. Functional requirements

- FR-1: The pane must call `$.session.usage` with `breakdown: 'summary'` at most once per 15 s, only on `turn.complete` / `session.compact`.
- FR-2: The pane must write the §3.1 spool and nothing else from the breakdown.
- FR-3: Rust must treat the spool as a `ContextCapture` anchored by time, newest capture winning, ignoring captures from before the last compaction.
- FR-4: The dashboard object must carry the composition row with its source; both surfaces draw it identically.

## 6. Non-goals

- `'full'` breakdowns: they call the token-count API, which is outside cctop's no-cost promise.
- Drawing `gridRows`.
- A pane-only composition view. Breaking "one state, many surfaces" is the thing decision 1 rules out.
- Subagent windows.

## 7. Technical considerations

- `$.session.usage({ breakdown })` resolves *"only with a session bound"* (d.ts). Before `session.start` binds, skip.
- The pane writes to `~/.cctop`, cctop's own directory; Claude Code's files are untouched (read-only rule holds).
- Category names are /context's labels; residency already maps those names (`categories: Vec<(String, u64)>`). The d.ts asks consumers to branch on `kind`. Use `kind` for free/buffer/deferred and the name only for display and for the existing residency mapping.
- `memory_files[].path` are the person's CLAUDE.md paths, which the prefix inspector already shows.

## 8. Success metrics

- With the pane loaded, ≥ 1 calibration point per completed turn (debounce permitting), visible as `calibrated <age>` under 1 min after a turn.
- Residency's per-step reconciliation error (context-residency PRD FR-16) is lower on calibrated sessions than uncalibrated ones over a week of the maintainer's sessions.

## 9. Open questions

- Does `'summary'` stay cheap on very large sessions? Measure the call's duration in the pane log in the first week. If it exceeds 200 ms, raise the debounce.
