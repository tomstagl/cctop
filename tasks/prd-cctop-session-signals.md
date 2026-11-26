# PRD 4: three transcript signals — dropped thinking, the USD budget, session lineage

**Status:** v1.0 · 2026-11-26 — ready for implementation.
**Target:** cctop after v0.9.1; Rust only (TUI, `cctop query`, MCP). The pane draws the dashboard object.
**Depends on:** nothing. Pairs well with PRD 1: `thinking_drop.clientChange` is a second witness for the request ledger's located cause.
**Source:** the newest 400 transcripts on the maintainer's machine (keys and machine tokens only), and the writers in the Claude Code 2.1.332 binary.

---

## 1. Introduction

Three record types Claude Code now writes carry facts cctop has no other source for:

| Record | Shape (measured) | Writer (2.1.332 binary) | Seen |
|---|---|---|---|
| `attachment` `thinking_drop` | `requestId`, `model`, `querySource`, `requestedBehavior?`, `thinkingBlocksSent`, `thinkingTurnsSent`, `newlyDropped{blockCount, turnCount, reason, reasonCounts{}, first{messageIndex, blockIndex}, last{…}}`, `blockHashes[]`, `firstReportForThreadInProcess`, `clientChange{kinds, firstChangedMessageIndex, baseline, callNumber}` | emitted when earlier thinking blocks stop being sent | 15 instances, first 2.1.308. `reason`: `prefix_mismatch` 15/15. `clientChange.kinds`: `messagesHistoryChanged` 12, `none` 2, `messagesHistoryChanged,systemPromptChanged,toolSchemasChanged` 1 |
| `attachment` `budget_usd` | `used`, `total`, `remaining` (USD) | `t2i(maxBudgetUsd)`: only when the session runs with `--max-budget-usd` (print/SDK mode). In print mode Claude Code stops the run when `used` reaches `total`: *"print budget halt: this run's spend reached --max-budget-usd"* | 317 instances, all 2.1.332, all in `sdk` sessions |
| line `derived-from` | `v`, `sessions[{type, id, via}]` | `last-wins` per transcript (binary's line table) | `type` always `claude_code_session`; `via`: `subprocess` 14, `clear` 8, `compact` 7 |

What each gives cctop:

- **`thinking_drop`**: when Claude Code stops sending earlier thinking, the next call re-reads context it had cached, and the model loses its prior reasoning. cctop cannot see this today. `clientChange.kinds` names what changed client-side (history / system prompt / tool schemas), the same question PRD 1's request ledger answers from the request lines.
- **`budget_usd`**: a hard spend cap with a halt. cctop shows rate limits and the monthly spend limit (`ui/panels/limits.rs`), not this.
- **`derived-from`**: which session this one came from, and how. cctop follows `/clear` forward via `continued-in` (`transcript/mod.rs:1094`, `attach.rs`). `derived-from` is the backward link, and also covers `compact` and `subprocess` (a session started by another session).

## 2. Goals

- Dropped thinking is visible per turn and counted per session, with what changed.
- A budgeted session shows used / total / remaining and a projected halt time.
- A session shows where it came from, and `cctop query` can total the cost of a chain.
- Lengths, counts, ids and machine tokens only. `blockHashes` are not kept.

## 3. User stories

### US-001: Type the three records
**Description:** As a developer, I want the records typed, so that state and surfaces can use them.

**Acceptance criteria:**
- [ ] `AttachmentKind::ThinkingDrop { request_id, blocks_sent, turns_sent, dropped_blocks, dropped_turns, reason, change_kinds: Vec<String>, first_changed_message_index: Option<u64> }` in `transcript/attachment.rs`, parsed from the raw `attachment` value. `blockHashes`, `first`/`last` and `baseline` are not kept.
- [ ] `AttachmentKind::BudgetUsd { used, total, remaining }`.
- [ ] `Line::DerivedFrom(DerivedFrom { sessions: Vec<DerivedSession{ kind, id, via }> })` in `transcript/mod.rs`, `last-wins` (a later line replaces the earlier).
- [ ] Both new attachment kinds get an entry in the token-share table near `attachment.rs:48–76` (what each costs in context). `budget_usd` has `rendered[]`, so the existing `rendered_chars` path measures it. `thinking_drop` is not sent to the model: share 0.
- [ ] `harness_facts::first_seen`: `THINKING_DROP = 2.1.308`, `BUDGET_USD = 2.1.332`, both upper bounds; `DERIVED_FROM` unknown (lines carry no `version`), documented as "seen by 2.1.332".
- [ ] Unit tests per record with the measured shapes; `cargo test transcript` passes.

### US-002: Dropped thinking in state and on screen
**Description:** As a person using cctop, I want to see when Claude Code stopped sending earlier thinking and why, so that I connect a cache re-write or a change in the model's behaviour to its cause.

**Acceptance criteria:**
- [ ] `metrics/usage.rs` aggregates per turn: `thinking_dropped_blocks`, `thinking_dropped_turns`, and the change kinds. The session total is a new registry metric `thinking_dropped` (formula, sources `D2`, caveat: "only on 2.1.308+; counts blocks no longer sent, not tokens"). Docs regenerated.
- [ ] Events view (`dashboard::EVENTS_BODY`): one row per drop, `thinking dropped · 12 blocks / 3 turns · history changed`. Snapshot.
- [ ] Turn ledger (`ledger.rs`): a `thinking` column showing dropped blocks for the turn when > 0.
- [ ] When PRD 1 is present: if a `thinking_drop` names a `requestId` that the request ledger located as `Unlocated`, `clientChange.kinds` fills the located cause (`history changed (thinking drop)`). Test on synthetic lines. If PRD 1 is not merged yet, this criterion moves to PRD 1's follow-up.
- [ ] `cctop query dashboard` carries `thinking_dropped{blocks, turns, by_kind}`.

### US-003: The USD budget
**Description:** As a person running `claude -p --max-budget-usd`, I want cctop to show how much of the budget is left and when it will run out, so that I am not surprised by the halt.

**Acceptance criteria:**
- [ ] `State` keeps the latest `BudgetUsd`. The limits panel and the dashboard limits cell show `budget $3.20 / $5.00` with a bar, red at ≥ 90 %.
- [ ] Projected halt: `remaining / burn_rate`, using the session cost burn rate cctop already computes for the cost panel. Shown as `halts in ~12 min` when under 1 h, else omitted.
- [ ] `metrics/registry.rs`: `budget_usd` (used/total/remaining) and `budget_halt_eta`; docs regenerated.
- [ ] `cctop query dashboard --session <sdk fixture>` shows the budget. If no anonymised SDK fixture exists, US-005 adds one.
- [ ] The existing limits nudge (the outcome rule that handles `spend_limit`, `advisor/rules/outcome.rs`) is **not** extended in v1. Note it in the PRD's follow-ups.

### US-004: Session lineage
**Description:** As a person using cctop, I want to see that this session came from another (after `/clear`, a compaction handover, or as a subprocess of another session), and the chain's total cost, so that the cost of one piece of work is not split across sessions I cannot connect.

**Acceptance criteria:**
- [ ] The session header shows `from <id8> · clear` (or `compact`, `subprocess`) when `derived-from` is present. Only the first entry is shown; the full list is in `cctop query`.
- [ ] `cctop query lineage --session <id>` returns the chain of ancestors (via `derived-from`) and descendants (via `continued-in`), each with `id`, `via`, `started_at`, `cost_usd`, `transcript_found: bool`. Ancestors are found by id in the same project directory, then in all project directories; depth capped at 10; cycles guarded.
- [ ] Costs come from loading each transcript with `load.rs` (no UI). `--lines` is ignored for relatives.
- [ ] The query's total `chain_cost_usd` sums found members and says how many were missing.
- [ ] `mcp.rs` exposes `lineage` as a tool with the same JSON. `docs/query.md` and `docs/mcp.md` updated.
- [ ] Tests: a synthetic three-session chain on disk (temp dir) covering `clear`, `compact`, a missing ancestor and a cycle.

### US-005: Fixtures
**Description:** As a developer, I need fixtures that contain these records, so that snapshots cover them.

**Acceptance criteria:**
- [ ] `scripts/anonymise-transcript.py` handles `thinking_drop` (drops `blockHashes`, keeps counts and kinds), `budget_usd` (keeps numbers, replaces `rendered[].content` with same-length filler) and `derived-from` (maps ids through the same id map as the rest of the file).
- [ ] `fixtures/session-d.jsonl` from a real `sdk` session with `budget_usd`. Fixture C from PRD 1 gains a `thinking_drop` if the source session had one; otherwise a second small fixture.
- [ ] Neither is hand-edited.

## 4. Functional requirements

- FR-1: cctop must parse `thinking_drop`, `budget_usd` and `derived-from`, keeping only the fields named in US-001.
- FR-2: Dropped thinking must be shown per turn (events, ledger) and counted per session (metric, query).
- FR-3: A budget must be shown with used / total and, when under an hour away, a projected halt.
- FR-4: The header must show the session's origin; `cctop query lineage` must total a chain's cost and say what it could not find.

## 5. Non-goals

- A coach rule for any of the three in v1.
- `data-flow-records`, `session-owner`, `credential_org`: account and org ids, not session health.
- Lineage in the session picker (a follow-up).

## 6. Technical considerations

- `derived-from` ids are session ids; finding their transcripts reuses `discover.rs`'s project-directory walk. Bound the walk (depth 10, stop at first match per id).
- `thinking_drop.requestId` is the API request id (the assistant line's `requestId`), so it joins to assistant lines directly.
- `budget_usd` repeats on every turn; keep the latest only.

## 7. Success metrics

- On the maintainer's machine, every session with a `thinking_drop` shows it in the events view, and `cctop query lineage` resolves ≥ 90 % of `derived-from` ancestors.

## 8. Open questions

- Does `via: subprocess` mean a session started by a `claude -p` call from another session's Bash? It is plausible from the name; verify on one case before the header says more than `subprocess`.
