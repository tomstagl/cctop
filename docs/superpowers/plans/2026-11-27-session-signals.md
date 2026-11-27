# Session Signals Implementation Plan (PRD 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse and show three transcript signals: `thinking_drop` (earlier thinking no longer sent, and what changed), `budget_usd` (the `--max-budget-usd` cap with a projected halt), and `derived-from` (where a session came from), plus `cctop query lineage` totalling a chain's cost.

**Architecture:** Typed records in `transcript/` (attachment kinds + a new `Line` variant); per-turn aggregation in `metrics/usage.rs`; surfaces read state (events body, ledger column, limits panel/cell, header, query, MCP). Lineage walks transcripts on disk through `discover.rs` and loads relatives with `load.rs`.

**Tech Stack:** Rust (serde, insta), Python 3 (anonymiser).

**Spec:** `tasks/prd-cctop-session-signals.md` (issue #28).

## Global Constraints

- Branch `session-signals`, cut from `context-breakdown` after PRD 3. Commit subjects `Signals: …`, `Fixtures: …`, `Docs: …`, trailer in the body.
- Keep only lengths, counts, ids and machine tokens. `blockHashes`, `newlyDropped.first`/`last`, `clientChange.baseline` are never stored.
- `budget_usd` repeats every turn: keep the latest only. `derived-from` is last-wins per transcript.
- `first_seen`: `THINKING_DROP = Version(2, 1, 308)`, `BUDGET_USD = Version(2, 1, 332)` (upper bounds); `DERIVED_FROM` documented as "seen by 2.1.332" (lines carry no version) — a doc comment, no constant.
- No coach rule for any of the three (non-goal); the outcome rule for `spend_limit` is not extended.
- Lineage walk: same project directory first, then all project directories; depth ≤ 10; cycle guard; stop at first match per id; `--lines` ignored for relatives.
- Header shows `from <id8> · <via>`, first entry only; the badge never shows more than `subprocess` for `via: subprocess` (PRD §8).
- Fixtures never hand-edited; insta and generated-docs procedures per CLAUDE.md; after dashboard/coach changes rerun pane fixtures, `npm test` green.
- Done = `make check` and `npm test` green.

## Rulings made while planning

- **Fixture names:** `fixtures/session-d.jsonl` already exists (teams). The SDK/budget fixture is `fixtures/session-s.jsonl`; the thinking-drop fixture is `fixtures/session-t.jsonl` (PRD 1's `session-r` source has no `thinking_drop`).
- **Burn rate** for the halt ETA: the session cost burn rate the cost panel already uses (find it: `grep -rn "burn" src/metrics src/ui/panels`). ETA shown as `halts in ~N min` only when < 60 min.

## Review Focus

1. `budget_usd` with `total = 0` or `used > total`: no division by zero, bar full, red, no ETA (Task 3 test).
2. A burn rate of 0 (idle session): no ETA rather than `halts in ~∞` (Task 3 test).
3. `derived-from` whose ancestor transcript is the current file itself (a cycle of length 1): guarded, listed once, `chain_cost_usd` counts it once (Task 4 test).
4. A `thinking_drop` with `clientChange` absent (older shape): parses with empty `change_kinds`, row reads `thinking dropped · N blocks / M turns` without the `·` tail (Task 2 test).
5. `cctop query lineage` for a session with neither link: returns the session alone, `chain_cost_usd` = its own cost, `missing: 0` (Task 4 test).

---

### Task 1: Type the three records; fixtures (US-001, US-005)

**Files:**
- Modify: `src/transcript/attachment.rs` (two kinds, `FALLBACK_RATIO` entries), `src/transcript/mod.rs` (`Line::DerivedFrom`), `src/harness_facts.rs`, `scripts/anonymise-transcript.py`, `fixtures/README.md`
- Create: `fixtures/session-s.jsonl`, `fixtures/session-t.jsonl`

**Interfaces:**
- Produces: `AttachmentKind::ThinkingDrop { request_id: Option<String>, blocks_sent: u64, turns_sent: u64, dropped_blocks: u64, dropped_turns: u64, reason: Option<String>, change_kinds: Vec<String>, first_changed_message_index: Option<u64> }` (`change_kinds` from `clientChange.kinds`, split on `,`, `"none"` dropped); `AttachmentKind::BudgetUsd { used: f64, total: f64, remaining: f64 }`; `Line::DerivedFrom(DerivedFrom { sessions: Vec<DerivedSession { kind: String, id: String, via: String }> })`, `kind()` = `"derived-from"`. `FALLBACK_RATIO`: `("thinking_drop", 0.0)`; `budget_usd` measured via `rendered` (add a ratio only if the table requires every subtype — follow the file's convention).

- [ ] **Step 1: Write the failing tests** with the PRD §1 shapes: `thinking_drop_parses` (incl. `clientChange.kinds = "messagesHistoryChanged,systemPromptChanged"` → two kinds; Debug contains no `blockHashes` value), `thinking_drop_without_client_change` (Review Focus 4), `budget_usd_parses`, `derived_from_parses` (`via: "clear"`).
- [ ] **Step 2: Run** `cargo test transcript` → FAIL. **Step 3: Implement**; `first_seen` entries. **Step 4: Run** → PASS.
- [ ] **Step 5: Fixtures.** Anonymiser: `thinking_drop` drops `blockHashes`, keeps counts/kinds; `budget_usd` keeps numbers, `rendered[].content` → same-length filler; `derived-from` ids through the file's session-id map. Sources (the dispatch names the private paths): the smallest main (non-subagent) session with `budget_usd` → `session-s.jsonl`; the smallest main session with `thinking_drop` → `session-t.jsonl` (trim to a contiguous segment containing the drop with the composer if the anonymiser can't cap length; never hand-edit). Test `fixtures_s_and_t_parse`: session-s has ≥ 1 `BudgetUsd`, session-t ≥ 1 `ThinkingDrop`, zero `Unknown` for these types. `fixtures/README.md` entries. Privacy grep (source project names, `/Users/`, prose) clean.
- [ ] **Step 6:** `make check` green (no existing snapshot moves). **Commit** — `Signals: type thinking_drop, budget_usd and derived-from; fixtures s and t`.

### Task 2: Dropped thinking in state and on screen (US-002)

**Files:**
- Modify: `src/metrics/usage.rs` (per-turn fields), `src/metrics/registry.rs` (`thinking_dropped`), `src/dashboard.rs` (events rows), `src/events.rs` if events are built there, `src/ledger.rs` (+ `src/ui/ledger_view.rs`, `query.rs` ledger JSON: `thinking` column), `src/requests.rs` (PRD 1 join), `src/query.rs` (`thinking_dropped`), snapshots, docs, pane fixtures

**Interfaces:**
- Produces: `Turn.thinking_dropped_blocks: u64`, `Turn.thinking_dropped_turns: u64`, `Turn.thinking_change_kinds: Vec<String>`; session totals on `Aggregate`; event text `thinking dropped · 12 blocks / 3 turns · history changed` (kinds mapped: `messagesHistoryChanged`→`history changed`, `systemPromptChanged`→`system prompt changed`, `toolSchemasChanged`→`tool schemas changed`, joined `, `); `cctop query dashboard` → `thinking_dropped {blocks, turns, by_kind{}}`; PRD 1 join: a `LocatedMiss` whose `cause` is `Unlocated` and whose request id matches a `thinking_drop.request_id` becomes `LocatedCause::ThinkingDrop { kinds: Vec<String> }` with `short()` = `history changed (thinking drop)` (first kind's mapped text + ` (thinking drop)`), `kind()` = `"thinking_drop"`.

- [ ] **Step 1: Write the failing tests:** usage aggregation on synthetic lines; events row text; ledger `thinking` = 12 on the drop's turn and empty elsewhere; `unlocated_miss_filled_by_thinking_drop` (synthetic request + drop with the same request id); dashboard JSON key on session-t.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; registry `thinking_dropped` (sources `D2`, caveat "only on 2.1.308+; counts blocks no longer sent, not tokens").
- [ ] **Step 4:** snapshots (only session-t-based or new ones change), docs regenerated, pane fixtures; `make check && npm test` green.
- [ ] **Step 5: Commit** — `Signals: dropped thinking per turn, in events, the ledger and the request ledger`.

### Task 3: The USD budget (US-003)

**Files:**
- Modify: `src/ui/state.rs` (latest budget), `src/ui/panels/limits.rs`, `src/dashboard.rs` (limits cell), `src/metrics/registry.rs` (`budget_usd`, `budget_halt_eta`), `src/query.rs`, snapshots, docs, pane fixtures

**Interfaces:**
- Produces: `State.budget_usd: Option<(f64, f64, f64)>`; `fn budget_text(used: f64, total: f64, burn_usd_per_ms: Option<f64>) -> (String /* "budget $3.20 / $5.00" */, f64 /* ratio 0..=1, clamped */, Option<String> /* "halts in ~12 min" */)` in `src/dashboard.rs`, used by both surfaces; red at ratio ≥ 0.9.

- [ ] **Step 1: Write the failing tests:** `budget_text` normal; `total_zero_or_over` (Review Focus 1); `idle_no_eta` (Review Focus 2); `eta_over_an_hour_omitted`; limits panel render with a budget; `cctop query dashboard --session fixtures/session-s.jsonl` has the budget.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4:** snapshots, docs, pane fixtures; `make check && npm test` green.
- [ ] **Step 5:** In `tasks/prd-cctop-session-signals.md` add a "Follow-ups" line: extending the outcome rule's spend-limit nudge to `budget_usd` (US-003's last criterion). **Commit** — `Signals: the --max-budget-usd budget with a projected halt`.

### Task 4: Session lineage (US-004)

**Files:**
- Create: `src/lineage.rs` (+ `pub mod`)
- Modify: `src/dashboard.rs` (header `from <id8> · <via>`), `src/query.rs` + `src/main.rs` (verb `lineage`), `src/mcp.rs` (tool `lineage`), `docs/query.md`, `docs/mcp.md`, snapshots, pane fixtures

**Interfaces:**
- Produces: `pub struct Member { pub id: String, pub via: Option<String>, pub started_at: Option<String>, pub cost_usd: Option<f64>, pub transcript_found: bool }`; `pub fn chain(session_path: &Path, projects_root: &Path) -> Lineage` where `Lineage { ancestors: Vec<Member>, this: Member, descendants: Vec<Member>, chain_cost_usd: f64, missing: usize }`; JSON exactly those keys. Ancestors via `derived-from` (first entry per transcript), descendants via `continued-in`. `projects_root` defaults to `~/.claude/projects`; tests pass a temp dir.

- [ ] **Step 1: Write the failing tests** (temp dir, synthetic transcripts with `derived-from` / `continued-in` lines and assistant usage so costs are non-zero): a three-session chain (`clear`, `compact`), a missing ancestor (`missing: 1`, `transcript_found: false`), a 2-cycle and the self-cycle (Review Focus 3), a lone session (Review Focus 5); header text `from 1a2b3c4d · clear`.
- [ ] **Step 2: Run** `cargo test lineage` → FAIL. **Step 3: Implement**, reusing `discover.rs`'s project walk; costs via `load.rs` without UI.
- [ ] **Step 4:** `cargo run -q -- query lineage --session fixtures/session-b.jsonl` runs (session-b has a `continued-in`); docs; snapshots/pane fixtures if the header changed for any fixture; `make check && npm test` green.
- [ ] **Step 5: Commit** — `Signals: session lineage in the header, cctop query lineage and the MCP tool`.
