# Implementation plan: cctop coach

**For:** `tasks/prd-cctop-coach.md` v1.1 (13 user stories, 34 rules, the "Lights" view)
**Design:** the canvas *cctop Coach Surfaces* (working files in `tasks/design-coach/`, rebuilt with `node build.mjs`) — pane Overview and Coach, TUI dashboard and coach view, the one-line forms, the sync sheet.
**Written:** 2026-09-14 · **Amended** the same day: the dashboard itself is direction B (`tasks/plan-dashboard-big-figures.md`) — the framed nine-panel grid is replaced by four block-digit tiles, the nudge and a nine-row ledger; Phase 2 and Phase 4 below are read with that plan.

## 1. The shape of the work

The PRD is three things stacked: **evidence** (read what Claude Code already writes), **numbers** (make every panel agree with Claude Code), and **advice** (engine v2, 34 rules, the coach view). Each layer is only as good as the one below it, so the order is fixed: parsers → numbers → engine → view → rules → measurement. The view comes *before* the new rules on purpose: the four lights need only the numbers, the slot is a frame that A01–A18 (fixed) fill from day one, and every later rule lands into a view that is already dogfooded on both surfaces.

Seven phases, each shippable, each ending with both surfaces in sync (§3). Phase 0 shipped today.

| Phase | Stories | Ships | Size |
|---|---|---|---|
| 0 | — | nav bar inside the pane, frame digits = TUI panel ids, the design canvas | done |
| 1 | US-001, US-002, US-003 | every field the PRD names is parsed; fixture B; `promptId` turns; `phase.rs`; `harness_facts.rs` | L |
| 2 | US-004, US-005, US-013 (numbers + alerts) | panels agree with `/context`, `/usage`, the footer; cost gradient; agents found; error lines never a compaction — the rows land in the ledger and the full-screen panels of plan B, not in framed blocks | L |
| 3 | US-006 | engine v2: classes, TTLs, cooldowns, acted, persistence, session_mode; A01–A18 recalibrated | M |
| 4 | US-009, US-010 | `coach::snapshot` → `cctop query coach` → TUI `c` view, pane Coach view, `$.ui.status`, MCP tool, skill; **and the B dashboard** (`dashboard::snapshot` → `cctop query dashboard` → TUI dashboard + pane Overview, plan B §5 steps 1–4) | L |
| 5 | US-007 | token-axis rules A19–A31 in their verified forms; history.jsonl tailer | M |
| 6 | US-008 | outcome/rework rules A32–A47; `Turn` gains its rework fields | L |
| 7 | US-011, US-012 | cross-session sources, session-start line, `coach-replay`, `coach-stats`, exposure alternation | M |

Dependencies: 2 needs 1 (fields), 3 needs 1 (turn identity) and 2 (thresholds), 4 needs 3 (the slot is the engine's), 5 and 6 need 4 (they fill the slot) and 1 (their triggers are the parsed fields), 7 needs 4 (exposure is "a surface was visible").

## 2. Phases

### Phase 1 — Evidence (US-001, US-002, US-003)

Goal: nothing downstream reads a heuristic where Claude Code wrote the fact.

- `src/transcript.rs` (593 lines today): the user-line keys (`promptId`, `promptSource`, `origin.kind`, `toolDenialKind`, `userFeedback` length, `interruptedMessageId`, `isCompactSummary`, `turnCompanion`, `sourceToolAssistantUUID`), the assistant-line keys (`perTurnEffort`, `attribution*`, `isApiErrorMessage` + `error` + `apiErrorStatus` + `quotaLimits`, `gitBranch`, `message.diagnostics.cache_miss_reason`), the line types dropped as `Unknown` today (`compact_boundary` with `compactMetadata`, `local_command` with the `/context` table parsed, `scheduled_task_fire`, `pr-link`, `custom-title`, `file-history-delta`), and `toolUseResult` parsed per tool (Bash, Edit/Write, Read, Agent, AskUserQuestion, TaskCreate/Update). Attachments: all 45 subtypes counted by `rendered[].content` with the per-subtype fallbacks. Gate every parser on the transcript `version` (first-seen map 2.1.220 … 2.1.269).
- `src/metrics/usage.rs::Turn` / `Aggregate::from_lines`: turns grouped by `promptId`; a human turn is `promptSource ∈ {typed, suggestion_accepted, queued}` or `origin.kind == human`; interrupts, slash commands, task notifications, teammate messages and the compaction summary are not turns. `isApiErrorMessage` / `<synthetic>` lines leave model detection, velocity and compaction math and become `api_error` events.
- New `src/phase.rs`: `classify_bash` / `assign_phases` ported from the research `phases.py` as pure functions, `Call.phase`, `Phase::current(&[Call]) -> (Word, run_len)` over the last five calls; the test class confirmed post hoc from stdout. Regression CSVs from the research trail as fixtures.
- New `src/harness_facts.rs`: the binary constants with the Claude Code version they were read from (effective-window table 967 k / 187 k, 13 000 / 20 000 / 3 000 / 0.8, `/usage` tier weights, `effort_cost_index`, `/context` thresholds). `scripts/check-plugin-types.sh` warns when `claude --version` is newer than the table.
- `src/status.rs::Sample`: `prompt_cache.*` (14 keys), `effort.level`, `thinking.enabled`, `fast_mode`, `exceeds_200k_tokens`, `context_window.*`, `rate_limits.spend_limit`, `session_name`, `prompt_id`, `version`, `pr.*`, `worktree.*`; drop the never-populated `plan`. `State` exposes `cache_expires_at_ms`, `cache_ttl_source`, `recache_tokens_if_cold`, `misses`, `expected_rebuilds`, `last_miss_cause`; the countdown is clock-driven between rewrites; the shim-less fallback is marked `≈`.
- `src/hooks.rs::apply_hook`: `duration_ms` (drop `≈`), `is_interrupt`, `error`, `Stop.{background_tasks, session_crons, last_assistant_message}`, `SessionStart.*` resume fields, `Notification.notification_type`, `permission_mode`, `effort.level`, `prompt_id`, `agent_id/agent_type` routed to the agent's stats. `cctop hook` keeps `permission_suggestions` and Agent `tool_response`. `src/install.rs` registers the ten missing events observe-only, shows the diff, uninstall restores. `src/tasks.rs` stops listing dotfiles; background tasks come from `Stop.background_tasks`.
- Fixture B: `fixtures/session-b.jsonl` (anonymised with `scripts/anonymise-transcript.py`) containing every moment the PRD lists (explore run, Edit → cargo test → commit, unverified commit, `AskUserQuestion` + 1 h gap, `/clear`, `TaskUpdate(completed)`, `<synthetic>` error, `compact_boundary`, interrupt, three `toolDenialKind` values, `edited_text_file`).

Tests: unit tests per parser with real (anonymised) payload shapes; the "13 % over-count is gone" fixture; snapshot of `cctop query summary` on fixture B; `cargo test`, clippy clean. Pane: `scripts/pane-fixtures.sh` regenerates `tests/pane/fixtures/*.json` from fixture A (numbers change: header turn count −20 %, ledger rows) — regenerate and review the diff, do not hand-edit.

**Number changes land here, before any coach family** (US-013): the turn count, A12's denominator, the compaction count, the header model, the ledger, the query snapshots and the pane Overview all move together in one story with regenerated fixtures.

### Phase 2 — Numbers that agree with Claude Code (US-004, US-005, US-013)

Goal: the footer, `/context`, `/usage` and cctop show the same figure for the same thing, on both surfaces. With plan B, every panel below has two homes: its ledger row (two lines, `Panel::ledger`) and its full-screen view (`render` / `render_overlay`); the framed-block layouts named here are the full-screen forms.

- Context (`src/metrics/context.rs`, `src/ui/panels/context.rs`, pane `contextBlock`): threshold = effective window − 13 000 honouring the six overrides read from settings and the claude process environment (`ps eww` / procfs); bands ok / warn (−20 000) / blocked (window − 3 000); "precompute armed" at 80 %; footer wording mirrored. Compactions from `compact_boundary` (the ≥ 30 % drop heuristic stays only as a labelled fallback before 2.1.263). The stacked bar (prefix / tool inputs / tool results / thinking / harness / prose / unattributed) — monochrome segments alternating fg and dim, the band colour on the figure, as drawn on the canvas. `harness N k/turn` row. Boundary markers for `/clear`, resume, compact, fork reset the re-read, exploration and churn counters. Defensive branch for `microcompact_boundary` / `[Old tool result content cleared]`.
- Tokens & Cost (`src/metrics/cost.rs`, `panels/tokens.rs`, pane `tokensBlock`): `$ per call` and `$ per turn` at the current context vs at 100 k, `next 30 calls`, the cache line (`warm · 41:12 · TTL 1h · misses 2 (model_changed 427k)`), `where the tokens went` (skill / plugin / agent / MCP / idle turns), `agents $X (N %)` from the recursive `subagents/**` scan, `cost-state` gap at session end; limit weight per turn with the five `/usage` behaviour flags and Claude Code's tip text.
- Limits (`src/metrics/limits.rs`): `quotaLimits` from 429 lines, spend limit window, personal ceiling from P90 of past 5-hour blocks for API-key users, other sessions' idle-since.
- Turn (`panels/turn.rs`, pane `turnBlock`): phase word + run length from `phase.rs`, `edits N ✓ last check`, steers vs task notifications, `AskUserQuestion` / question-ended WAITING with elapsed, interruptions with wasted output tokens, hook time by command, background tasks, goal progress, exact durations.
- Tools (`src/tools.rs`, `panels/tools.rs`, pane `views/tools.tsx`): `IN→CTX`, error class, Bash rows by phase class, image tokens (`w·h/750`), offloaded bytes with the cap marker, re-read tax on the top-ctx line.
- Agents & MCP (`src/agents.rs`, pane `views/agents.tsx`): recursive scan, registration from `SubagentStart`, workflow journal failures, forks' inherited context, teammates, MCP needing auth, `ToolSearch` loads per server, `N/20 · depth d/3`.
- Files (`src/files.rs`, pane `views/files.tsx`): edits per turn, checkpoint version, uncommitted-since-last-commit from `gitOperation`, stale markers, rewind points; the re-read column counts Bash reads and excludes ranged reads and `file_unchanged`.
- Events (`src/events.rs`, pane `views/events.tsx`): `api_error` rows replace `Compact ctx → 0`; named cache misses; exact compactions; denials by kind; interrupts; destructive git; `/model`, `/effort`, `/clear` from `<command-name>` lines.
- Prefix inspector (`src/prefix.rs`, `ui/prefix_view.rs`) and Ledger (`src/ledger.rs`, `ui/ledger_view.rs`) gain their §4 rows; `src/metrics/registry.rs` gets one row per new metric; `docs/metrics.md` regenerated; the CI drift check passes.
- Alerts (`src/alerts.rs`): `ContextHigh` re-thresholded to the warn band; `CompactionSoon` removed; `CacheHitLow` and `PermissionWaitLong` retired in favour of A20 / A41 (Phase 5/6 — leave a tombstone until then); `ToolRunningLong` kept; one toast budget shared with the coach.

Tests: insta snapshots of every panel at 60 × 51 and 40 × 24 on fixture B (`src/ui/snapshots/`); pane `tests/pane/fixtures/` regenerated from fixture B as well as A (`scripts/pane-fixtures.sh` takes the session as an argument); `tests/pane/overview.test.ts` and `views.test.ts` assert the new rows. The "agreement" test: `/context`'s captured table on fixture B equals the Context panel's prefix rows.

### Phase 3 — Engine v2 (US-006)

- `src/advisor/mod.rs`: `Advice` gains `urgency ∈ {NOW, NEXT, LATER}`, `since_turn`, `window_turns`, `acted: fn(&State) -> bool`, `cooldown_turns`, `family`, `action_kind` (prompt / slash / allow-rule / setting / key); `Saving` distinguishes one-off from per-turn and ranks by tokens × expected remaining calls. `Engine` gains the class scheduler (NOW › NEXT › LATER, the published intra-class order of §6.4), hard TTLs, per-rule cooldowns, three-strikes self-snooze, the one-new-nudge-per-turn budget (≤ 3 per 10, ≤ 1 LATER per 2), the suppression list (loop, machine, subagent, first two turns, away > 2 h), and `session_mode` derived from `hook_blocking_error`, `promptSource`, `entrypoint`, `bridge-session`. The occupant changes only at a human-turn boundary, on a NOW event or on retirement.
- Split `src/advisor/rules.rs` (1 214 lines) into `rules/token.rs`, `rules/outcome.rs`, `rules/events.rs`; recalibrate A01–A18 per §5.1 with a fixture pair each; retire A15 and A18 (A38 / A42 replace them in Phase 6), rename A05 → A25.
- Persistence: `~/.cctop/<session>.advisor.json` (dismissals, `first_fired`, per-fire records for Phase 7); single-writer lock (the TUI owns it while running; CLI / MCP / pane write only without the lock).
- `cctop query advice` gains a schema version and returns the primary nudge first; the pane's Advisor view and the TUI Advisor panel read the same list.

Tests: the purity test extended (no rule reads prompt text); fixture pairs (fires / must-not-fire) per rule; a scheduler test that replays ten human turns and asserts the budget; `cctop advise --session fixtures/session-b.jsonl` lists the expected ranked set.

### Phase 4 — The coach object and its surfaces (US-009, US-010)

One implementation, three surfaces, in this order:

1. **`src/coach.rs`** — `coach::snapshot(&State) -> Coach` producing the §6.6 object: `state` (the state line), `lights[4] {id, level, number, detail, source, approx}`, `agents`, `nudge {id, family, class, line1, line2, evidence, action_text, action_kind, since_turn, retires_on, acted, acting}`, `next` with its promotion condition, `snoozed[]`, `recent[]`, `suppressed[]`, `session_mode`. Lines cut at 52 cells here, so every surface shows the same text. The four lights are pure functions of `State` (§6.3 thresholds); the slot is the engine's occupant. The one-line forms (L0 ≥ 80 cols, L1, L2) are methods on `Coach`.
2. **`cctop query coach [--line] [--snooze <id>]`** (`src/query.rs`, `src/main.rs`); insta snapshot on fixture B at the six moments; `docs/query.md`. MCP tool `cctop_coach` (`src/mcp.rs`, schema ≤ 120 tokens; `docs/mcp.md`). `plugin/skills/cctop-insights/SKILL.md` maps "what should I do now / next" to it.
3. **TUI** (`src/ui/coach_view.rs`, `src/app.rs`, `src/ui/state.rs`): `State.view: View::{Dashboard, Coach}` checked at the top of `App::draw` (the dashboard is plan B's `src/ui/dashboard.rs`; `layout::solve` is gone); `c` toggles, persisted in `~/.config/cctop/config.toml` (`src/config.rs`), `--view coach`; `BINDINGS` and the help overlay updated. The view renders the canvas's 56-column card; at ≥ 100 columns the why / lifecycle column sits beside it (the canvas's "TUI · Coach, wide") instead of overlays; at ≤ 40 columns the 2 × 2 form; at height < 20 empty rows go first, then `snoozed`, then `next`. Keys per §6.5 (`Enter` reuses `src/ask.rs`, `x`/`X`, `e`, `n`/`N`, `1`–`4`, `l`, `$`, `Esc`). On the dashboard the four lights are the B tiles and the nudge is the bold line under them (plan B §1), so no separate lights row is needed; the Advisor panel's full-screen view and its ledger row show the slot occupant first; the footer gains `c coach`. Glyphs ○ ◐ ● ◆ ▸ ↻ ✓ get ASCII fallbacks in `src/theme.rs` (`-` `+` `!` `?` `>` `~` `v`).
4. **Pane** (`plugin/hooks/`): `model.ts` gains the `coach` and `dashboard` verbs (the poller swaps `advice` for `coach` every 2 s while a turn runs; `QUERY_VERBS` order), `views/coach.tsx` draws the same card with `[ fill ]` (`$.prompt.fill` for prompt/slash kinds — never `$.prompt.submit`, `$.command.run`, `$.turn.abort`), `[ snooze ]` (`cctop query coach --snooze`), `[ why ]`, and the detail frame that follows the highest light with four buttons; `views/overview.tsx` becomes plan B's Overview (tiles, nudge, ledger Buttons); `$.ui.status` shows the L0/L1/L2 line by `bodyColumns` and changes only when a light's level or the occupant changes; NOW-class promotions go to `$.ui.toast` once per fire, ≤ 1 per human turn; `Coach` joins `VIEWS` first. Below 50 body columns each light drops its third figure (the canvas's "Pane · narrow").
5. **Desktop notifications** (`src/alerts.rs::critical()`, opt-in `--notify`): exactly the three cases of §6.6.

Tests: the same `coach.json` fixtures at the six moments of fixture B (`scripts/pane-fixtures.sh` emits `tests/pane/fixtures/coach-<moment>.json` from `cctop query coach`), consumed by both the TUI insta snapshots (`cctop run --once --view coach --keys … --size 56x20`) and the pane tests (`tests/pane/coach.test.ts`) — one object, two renderings, checked against each other's text. A `sync` test asserts that the state line, the four light rows and the slot's two lines are byte-identical between `cctop query coach --session fixtures/session-b.jsonl` and what each surface draws at 56 columns.

Dogfood starts here: open the coach on both surfaces for a working week with A01–A18 only; log every `x` (US-012 counts them).

### Phase 5 — Token-axis rules (US-007)

`src/advisor/rules/token.rs`: A19 cache-warm countdown, A20 named cache miss (absorbs A22), A21 switch window (from `PreModelSwitch` when hooks are installed, else the history.jsonl watcher), A23 cost of continuing (one family with A42, escalates only at a clean turn end), A25 exploration run (replaces A05, Bash-aware allowlist, "queue" wording), A27 idle-triggered turns, A30 cold resume. As metrics, indicators and events only: A24 harness overhead, A26 tool inputs (`IN→CTX`), A28 churn token, A29 serial round-trips, A31 TTL flip. New collector: the history.jsonl tailer filtered by `sessionId`, read from the last offset (also feeds `/clear` and `/compact` habit baselines and `pastedContents` for A11).

Tests: fixture pair per rule; replay precision over the local transcripts recorded beside the fixture (Phase 7's `coach-replay` is the tool — build a minimal version of it here, the full one in Phase 7).

### Phase 6 — Outcome and rework rules (US-008)

`src/metrics/usage.rs::Turn` gains `first_edit_at`, `last_verify_at`, `edits_since_verify`, `files_edited`, `verify_cmd_detected`, `ended_with_question`, `pending_question`, `interrupt`, `steers`, `denials_by_kind`. `src/advisor/rules/outcome.rs`: A32 verification gap, A33 commit without a check, A36 correction streak (structural markers only), A38 failure cascade (replaces A15), A40 review before merge, A41 waiting on you (+ the two notification cases), A42 natural-boundary checkpoint (replaces A18), A45 denial streak / allow rule (verbatim `permission_suggestions[].ruleContent`, never bare `Bash`), A47 turn died. Next-row only: A34 plan-first, A46 long-context drift. Events with a one-line nudge: A37, A39, A43, A44. A35 stays out until its tightened trigger replays ≤ 1-in-5 wrong.

Tests: fixture pairs on fixture B; `cctop advise` on the empty session fires nothing; A45's rule text is never bare `Bash` and never a rule already in `permissions.allow`.

### Phase 7 — Cross-session sources and measurement (US-011, US-012)

- Readers for `usage-data/session-meta/*.json` and `facets/*.json` (never the never-display fields; a privacy test), `~/.claude.json` (keys only), the plugin catalog cache, `stats-cache.json`; `src/baseline.rs` gains per-turn medians, interruption rate, error categories, commit-without-check ratio, model mix; the one dim session-start line when a `/insights` report is < 30 days old.
- `~/.cctop/<session>.advisor.json` records per fire: rule, class, shown_at, surface_visible, human_idle_ms, time-to-x, acted delay, expired, snoozed, versions, project, session_mode.
- `cctop coach-replay <jsonl…>` (per rule: fires per session, precision against the verdict labels, acted-anyway-within-TTL rate) and `cctop coach-stats [--since 4w]` (the §12 columns and the control row per family; the coach's own cost).
- `cctop run --coach on|off|auto` alternates exposure by session, stratified by project, model family and Claude Code version; the demotion rule (false-positive rate > 20 % after N exposed fires → next-row or removed; precision collapse on a new version → LATER).

## 3. Keeping the two surfaces in sync

The design canvas's sync sheet is the contract; these are the mechanics that enforce it.

- **One object.** No surface computes coach state. The TUI draws `coach::snapshot(&State)`; the pane draws `cctop query coach`; both are the same function. Lines are cut at 52 cells inside `snapshot`, never in a view.
- **One fixture, two renderings.** `scripts/pane-fixtures.sh <session>` regenerates every pane fixture from the binary; the TUI's insta snapshots run on the same session file. A change to a number changes both sides in one commit, or the fixtures test fails on the side that was forgotten.
- **One numbering.** Frame digits are the TUI's panel ids on both surfaces (Phase 0). New panels (the coach card is unnumbered; `c` in the TUI, `Coach` in the pane) follow the sheet.
- **One vocabulary.** Glyphs (○ ◐ ● ◆ ▸ ↻ ✓ ≈ —), the phase pill, colour roles (fg · dim · accent · ok · warn · crit · border) and their pane theme keys (`text` · `dimColor` · `suggestion` · `success` · `warning` · `error`), the three one-line forms. Additions go on the sheet first (`tasks/design-coach/build.mjs`, `syncSheet()`), then into `src/theme.rs` and `plugin/hooks/views/frame.tsx` in the same change.
- **Same words.** Wording is Claude Code's own where it has a word for the thing (footer phrases, error taxonomy, `/usage` flag tips, `/context` thresholds) — `harness_facts.rs` carries the strings with the version they came from.
- **Per-phase checklist** (the definition of done for every phase above): the TUI snapshot and the pane fixture test both changed; the sync sheet updated if a glyph, colour, number or key changed; `plugin/README.md`'s view table and `site/guide/*` mention nothing the other surface lacks; `docs/metrics.md` regenerated; the verification checklist (`docs/verification/pane.md`) has an item when the pane's behaviour changed.

## 4. Risks and decisions to take early

1. **Rendering the pane's tabs.** Verified live on 2.1.270: a `plain` Button without a hotkey draws as its bare label (`docs/verification/pane.md`, third pass). If a later version draws otherwise, `viewBar` in `pane.tsx` drops `plain` for the `[ Label ]` chrome — one line.
2. **The engine's rule count.** 34 rules is the PRD's ceiling, not a target: Phases 5 and 6 admit a rule only with its fixture pair and its replay precision. Ship the P0 set first (A20, A23, A25, A32, A42, A45), then P1, then P2 behind the measurement of Phase 7.
3. **Human presence** (open question 1): every "idle" is transcript silence until a `human_idle_ms` collector exists. Phase 4 should ship the collector's *slot* (a `—` when unavailable) so A19 / A41 can gate on it later without a schema change.
4. **The pane's poll cadence.** `coach` every 2 s while a turn runs replaces `advice`; the CPU budget (≤ 5 % during a turn) should be measured in Phase 4's dogfood week before Phase 5 adds the history.jsonl tailer.
5. **Number changes are user-visible** (the turn count drops ~20 %, the compaction count drops, the header model changes on error lines). Phase 1 lands them in one story with a note in the README, before any coach family, so nobody attributes them to the coach.
6. **Function hooks GA** (open question 13): A19–A21, A41 and A45 gain exact live triggers if the module ships; the TUI path must not depend on it — every rule has a transcript-only trigger first.

## 5. Where things live after the plan

```
src/coach.rs              the coach object (snapshot, lights, forms)
src/dashboard.rs          the dashboard object (header, tiles, nudge, nine ledger rows) — plan B
src/ui/dashboard.rs       the TUI dashboard (replaces ui/layout.rs)
src/phase.rs              the Bash classifier and the phase word
src/harness_facts.rs      Claude Code's constants, by version
src/advisor/{mod,rules/{token,outcome,events}}.rs
src/ui/coach_view.rs      the TUI's c view
plugin/hooks/views/coach.tsx
tests/pane/fixtures/coach-<moment>.json   one object, both renderings
tasks/design-coach/       the canvas's working files and the sync sheet
```
