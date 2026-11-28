# Context Breakdown Implementation Plan (PRD 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After every main-thread turn (while the pane is loaded) cctop gets /context's breakdown from `$.session.usage({ breakdown: 'summary' })` through a spool file, uses it as a `ContextCapture` for residency calibration, and shows the window's composition in the dashboard's context cell on both surfaces.

**Architecture:** The pane writes `~/.cctop/context/<sessionId>.json` (v1) on `turn.complete` / `session.compact`, debounced in the pure reducer. Rust's new `src/context_spool.rs` watches and parses it into the existing `ContextCapture`, anchored to the turn of the last API call at or before `at`; the newer of {transcript capture, spool capture} wins. `dashboard.rs` emits a composition row both surfaces draw verbatim.

**Tech Stack:** TypeScript (function hooks, `node --test`), Rust (serde, insta).

**Spec:** `tasks/prd-cctop-context-breakdown.md` (issue #27).

## Global Constraints

- Branch `context-breakdown`, cut from `status-fields` after PRD 2 is complete. Commit subjects `Pane: …`, `Context: …`, `Docs: …`, trailer in the body.
- Breakdown is **only ever `'summary'`**; never `'full'` (no API cost). Only on `turn.complete` / `session.compact`, at most once per 15 000 ms.
- The spool drops `gridRows`, `color`, `slashCommands`. Exact key names (snake_case): `v` (= 1), `at`, `tokens`, `window`, `model`, `categories[{name, tokens, kind}]`, `autocompact_threshold`, `is_autocompact_enabled`, `memory_files[{path, tokens}]`, `mcp_tools[{name, tokens}]`, `skills{count, tokens}`, `agents[{name, tokens}]`.
- Branch on `kind` (`used` | `free` | `buffer` | `deferred`), never on a category's name, except for display and the existing residency name mapping.
- Pane rules (CLAUDE.md): `$` only followed into top-level functions; module state in module-scope `let`s; facades for helper modules; no local `h`/`Fragment`; `$.env.get("NAME")` literal; failures in `.catch` are `next.error`. Validator `--strict` clean.
- A failed call or write is `$.ui.log`ged once per session; never retried in a loop. Skip before a session is bound.
- cctop writes only under `~/.cctop`; Claude Code's files untouched.
- The composition row is cut at the context cell's fixed width; after dashboard changes rerun `scripts/pane-fixtures.sh && scripts/pane-fixtures.sh fixtures/session-b.jsonl b`; row-identity tests pass.
- Insta and generated-docs procedures as in CLAUDE.md. Live checks → `docs/verification/pane.md` `Result: pending`.
- Done = `make check`, `npm run typecheck`, `npm test`, validator clean.

## Rulings made while planning

- **Anchor = a turn.** Today's transcript capture is anchored by the turn it ran in (`metrics::context::residency(…, capture: Option<(&ContextCapture, usize)>)`). The spool capture is anchored to the turn of the last API call whose timestamp ≤ `at` — the PRD's "anchored to the last API call" expressed in residency's existing unit.
- **"Newer"** compares the capture's time: the transcript capture's line timestamp vs the spool's `at`.
- **Mapping detail:** `used_tokens = tokens`, `window_tokens = window`, `used_pct = tokens/window·100`, `categories` = the `used` and `deferred` rows `(name, tokens)` in spool order, `free_tokens` = sum of `kind=free`, `autocompact_buffer` = sum of `kind=buffer`, `autocompact_window = autocompact_threshold`, `mcp_tools = (len, sum)`, `skills = (count, tokens)`.

## Review Focus

1. A spool whose `sessionId` file belongs to the previous session after `/clear` rotated the id: the watcher must follow the new id (`attach.rs` rebuild) and never show the old session's composition (Task 3 test via attach).
2. A spool file written half-way (truncated JSON) when Rust reads it: ignored this poll, picked up next poll, no log spam (Task 2 test).
3. A spool with `at` before the session's first API call: anchored to turn 0 / ignored, never panics (Task 2 test).
4. Breakdown call rejected (no session bound, or the engine throws): logged once, the next turn still tries (Task 1 test).
5. A spool with zero `used` categories: no composition row (an empty row would draw a bare label) (Task 3 test).

---

### Task 1: The pane writes the breakdown spool (US-001)

**Files:**
- Modify: `plugin/hooks/model.ts` (action `breakdown.read`, pure debounce), `plugin/hooks/pane.tsx` (hooks on `turn.complete`/`session.compact`, the read + write in a top-level function)
- Create: `plugin/hooks/breakdown.ts` (pure: `SessionUsage` → spool text)
- Test: `tests/pane/breakdown.test.ts`, `tests/pane/reducer.test.ts`

**Interfaces:**
- Produces: `export const BREAKDOWN_DEBOUNCE_MS = 15_000` and `export function breakdownDue(model: Model, now: number): boolean` in `model.ts`; reducer action `{ type: 'breakdown.read'; at: number }` sets `model.lastBreakdownAt`. `export function spoolText(usage: SessionUsage, at: number): string | null` in `breakdown.ts` (null when `usage.context.breakdown` is absent); output is `JSON.stringify` of the §3.1 object plus `\n`. Spool path `${HOME}/.cctop/context/${sessionId}.json` via the existing fs-write facade.

- [ ] **Step 1: Write the failing tests:** `breakdown.test.ts` — `spoolText` on a fake `SessionUsage` with every field (incl. `gridRows`, `color`, `slashCommands`) returns exactly the expected JSON text (key order as in Global Constraints) without those three; null without a breakdown. `grep`-style test: read `plugin/hooks/*.ts*` sources and assert no occurrence of `'full'` / `"full"` next to `breakdown`. `reducer.test.ts` — `breakdownDue` true initially, false 14 999 ms after a `breakdown.read`, true at 15 000.
- [ ] **Step 2: Run** `npm test` → FAIL.
- [ ] **Step 3: Implement.** In `pane.tsx`, after `turn.complete` / `session.compact` apply: if `breakdownDue`, apply `breakdown.read`, then (outside the hook, e.g. via the existing clock deferral pattern the file uses for post-hook work) call `$.session.usage({ breakdown: 'summary' })`, `spoolText`, write. Log once per session on failure (module-scope flag), Review Focus 4: a failed read does not block the next turn's attempt.
- [ ] **Step 4: Run** `npm run typecheck && npm test` → PASS; `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin validate --strict ./plugin 2>&1 | grep -c 'error TS'` → 0 and the "calls:" line lists `$.session.usage`, `$.fs.write` via top-level functions.
- [ ] **Step 5: Commit** — `Pane: write /context's breakdown to a spool after each turn`.

### Task 2: Rust ingests the spool as a context capture (US-002)

**Files:**
- Create: `src/context_spool.rs` (+ `pub mod` in `lib.rs`)
- Modify: `src/ui/state.rs` (field + accessor), `src/attach.rs` (watcher rebuild), `src/load.rs` (read once), `src/app.rs` (poll alongside the status watcher), the residency call sites that pass `capture` (`grep -rn "residency(" src`)

**Interfaces:**
- Produces:
  - `pub fn parse(text: &str) -> Option<SpoolCapture>`; `pub struct SpoolCapture { pub at_ms: i64, pub capture: ContextCapture }` (unknown `v` → None plus one `log` line via the crate's existing logging, or `eprintln!` only if none exists).
  - `pub struct Watcher` with `new(session_id: &str) -> Watcher` and `poll(&mut self) -> Option<SpoolCapture>` — same pattern as `status::Watcher` (`status.rs:271`).
  - `State.context_spool: Option<(ContextCapture, usize /* turn */, i64 /* at_ms */)>`, `State::apply_context_spool(&mut self, s: SpoolCapture)` (anchors; ignores a spool older than the last `compact_boundary`), and `pub enum CaptureSource { Transcript, Spool }` with `State::context_capture(&self) -> Option<(&ContextCapture, usize, CaptureSource, i64 /* at_ms */)>` returning the newer. Residency call sites use `context_capture()`.

- [ ] **Step 1: Write the failing tests** in `context_spool.rs` / `state.rs`: `parses_v1` (mapping per Rulings, every field); `unknown_version_ignored`; `truncated_json_ignored` (Review Focus 2); `anchors_to_previous_call` (three API calls t1<t2<t3, `at` between t2 and t3 → the turn of t2); `before_first_call` (Review Focus 3 → no capture, no panic); `newer_wins_both_orders`; `stale_after_compaction_ignored`.
- [ ] **Step 2: Run** `cargo test context_spool` → FAIL.
- [ ] **Step 3: Implement**; wire the watcher into `app.rs`'s loop and `attach.rs`'s rebuild (session change → new `Watcher::new(new_id)`); `load.rs` reads `~/.cctop/context/<session>.json` once if present.
- [ ] **Step 4: Run** `cargo test context_spool && make check` → PASS, no snapshot changes (no fixture has a spool).
- [ ] **Step 5: Commit** — `Context: the pane's breakdown spool becomes a context capture`.

### Task 3: The composition row on both surfaces (US-003)

**Files:**
- Modify: `src/dashboard.rs` (context cell composition row), `src/ui/dashboard.rs`, `plugin/hooks/views/overview.tsx`, `src/query.rs`, `src/metrics/registry.rs` (`context_composition`), the residency inspector header (`grep -rn "residency" src/ui`), snapshots, docs, pane fixtures, `docs/verification/pane.md`
- Test: `tests/pane/overview.test.ts` (row identity)

**Interfaces:**
- Consumes: `State::context_capture()`.
- Produces: in the dashboard object's context cell detail, `composition: Option<Composition>` where `Composition { source: &'static str /* "per /context" | "per Claude Code" */, items: Vec<(String, u64)> /* used categories in /context's order, then ("free", n), then ("buffer", n) */, line: String /* pre-cut to the cell width */ }`; `cctop query dashboard` → `composition[]` (`{name, tokens}`) and `composition_source`.

- [ ] **Step 1: Write the failing tests:** `dashboard.rs` snapshot with a transcript capture (fixture B if `ContextCapture` parses from it — check `prefix.context_capture` on fixture B; otherwise fixture D, which has the table per `metrics/context.rs:1446`) → row labelled `per /context`; a unit test with a spool capture → `per Claude Code`; `no_used_categories_no_row` (Review Focus 5); no capture → no row and snapshot unchanged. Pane: the row-identity test covers the row for the chosen fixture.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.** Residency inspector header: `calibrated <age> ago` (age from `at_ms` vs `state.clock_ms()`, the codebase's duration formatter). Registry `context_composition` with formula/sources.
- [ ] **Step 4:** `INSTA_UPDATE=always cargo test` (review), regenerate docs, rerun pane fixtures (if the chosen fixture is D, add it to `scripts/pane-fixtures.sh`'s set only if the row-identity harness needs it — say so in the report); `make check && npm run typecheck && npm test` green.
- [ ] **Step 5:** `docs/verification/pane.md` §B item 41: *"After a turn the TUI's context cell shows `per Claude Code` while the pane is loaded"* — Setup: a real session with the pane loaded and `cctop run` in another terminal; finish one turn. Expected: the composition row appears labelled `per Claude Code` within a minute. `Result: pending`.
- [ ] **Step 6: Commit** — `Context: the window's composition in the context cell, from Claude Code's own numbers`.
