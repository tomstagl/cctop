# PRD: subagent spend — the combined figure on Panel 2 and the agents view

**Status:** v1.0 · 2026-09-15 — draft, not implemented (decisions taken with the user in the session that produced this file; see the box below)
**Target:** cctop ≥ 0.3.1 attached to Claude Code CLI ≥ 2.1.271; TUI first, then `cctop query` / MCP and the pane.
**Depends on:** `tasks/prd-cctop.md` v1.1 (Panels 2 and 6, the metrics registry), `tasks/prd-cctop-coach.md` v1.3 (US-005 shipped `agents_cost`, the recursive `subagents/**` scan and the dashboard object). The teammate collector is a separate PRD: `tasks/prd-cctop-team-costs.md`.

> Decisions taken with the user on 2026-09-15:
> 1. Panel 2's headline becomes the **combined** figure: main session plus subagents (plus teammates once the second PRD lands). Main-only stays reachable with the existing `a` toggle.
> 2. Panel 6 **stays as it is**; the new agents view opens from it with `Enter`, the way the ledger opens from Panel 2.
> 3. The insight the view is built around is **wasted spend**: money that went to agents whose work did not come back.
> 4. Subagents and teammates ship as **two changes with two PRDs**. This is the first.

---

## 1. Introduction

Every subagent transcript under `<session>/subagents/**` is already parsed: tokens, model, state, elapsed, tool calls per agent (`src/agents.rs`). The money is not. Panel 2's headline cost is `CostTracker::current()`, which is the main transcript's `cost-state` plus a priced estimate of main-session responses newer than that line. The subagents' share appears only as a dim attribution `agents $X (N %)` on the last line, is never added to the headline, the burn rate, the per-turn figure or the baseline multipliers, and `cctop query summary` / `dashboard` export the same main-only `cost`. Panel 6 shows one row per agent with tokens and no dollars; `cctop query agents` has no cost field at all.

At the scale the user runs (20 subagents, or a workflow run of 41 agents that spent $118 against $10 in the main thread, coach PRD §3.1), this means the number a person reads as "what this session cost" can be an order of magnitude low, and the panel that lists the agents gives no way to tell which of them earned its money.

Two facts found while scoping decide the design:

- **Subagent transcripts carry no `cost-state`.** Their cost is always cctop's estimate from `pricing.toml`, so every agent dollar is `≈` and says so.
- **Whether Claude Code's own `cost-state` already includes subagent calls is not established.** On fixture A, `modelUsage["claude-sonnet-5"].outputTokens` (67 234) is *smaller* than the main transcript's deduplicated output (67 061) plus the fork agent's (1 415), which says the ledger excludes the agent; but the fixture's anonymised timestamps do not prove the agent ran before the `cost-state` was written. Adding an estimate on top of a ledger that already contains it would double count. US-001 settles this on live sessions before anything is drawn.

## 2. Goals

- **One number for the session's money**, on Panel 2, the header tile, `cctop query`, the MCP tool and the pane, with the same provenance rules everywhere: exact where Claude Code wrote it, `≈` where cctop priced it, and never double counted.
- **A per-agent ledger** that answers, at 20 or 40 agents, which agents cost what and which of that was wasted.
- **Wasted spend as a first-class metric**, computed only from structural evidence: the agent's state, the journal, the size of what came back through the `Task` result, the cache profile of its calls. No prompt text, no description text, no model calls.
- **No change to the rest of cctop's meaning.** The coach rules, baselines and the cost gradient keep their inputs unless a story below names them.

## 3. Findings

Everything below is from the code on `main` at v0.3.1 and fixture A (`fixtures/session-a.jsonl` with its `subagents/agent-a9a92645226d3a561.jsonl`).

| Fact | Where | Consequence |
|---|---|---|
| Headline cost = `cost-state.totalCostUSD` + estimate of newer main responses | `metrics/cost.rs` `CostTracker::current` | Subagents never enter it |
| `agents_cost()` = Σ `pricing.estimate(a.usage, a.model)` over `state.agents`, share = usd / (main + usd) | `ui/state.rs:1610` | Exists, one figure, not in the headline; `None` when no agent model is priceable |
| `a` on Panel 2 toggles `tokens_include_agents` for the token bars only | `ui/panels/tokens.rs:44` | Cost lines ignore the toggle today |
| Burn rate (`$/h`) and `last turn $` are computed from `agg.turns` | `metrics/cost.rs` `rates`, `tokens.rs` l7 | Main-only; agents have no turn membership |
| Baseline multipliers `×N$` divide `cost.current()` by human turns | `tokens.rs` l7 | Change meaning if the headline changes; the baseline corpus is main-only |
| Panel 6 lists agents newest-first: glyph, type, description, elapsed, tokens, model, `↰` inherited context | `ui/panels/agents.rs:96` | No dollars, no sort by spend, no total; 20 agents overflow a fixed-height panel |
| Dashboard row 6 = `N run · share % · top 3 by tokens · $agents · N failed` | `dashboard.rs:704` | Top 3 by tokens, not by dollars or waste |
| `cctop query agents` per agent: id, type, description, model, state, elapsed, tokens, workflow, inherited_context, depth | `query.rs:285` | No `cost`, no waste fields |
| `Meta.tool_use_id` links an agent to the `Task` tool_use in the main transcript | `agents.rs` `Meta` | The `tool_result` for that id in the main transcript *is* what came back; its length is already the kind of number cctop keeps |
| Workflow journals: launched / started / result / failed per run | `agents.rs` `workflow_journals` | A `failed` journal entry is structural evidence, independent of the 60 s `FAILED_AFTER_MS` heuristic |
| Hook spool attributes `PostToolUse` / `PostToolUseFailure` per `agent_id` | `agents.rs` `note_hook` | Exact tool timings and error counts per agent, before the transcript exists |
| Fork agents (`isFork`) carry `fork-context-ref.contextLength` | `agents.rs` | Their first call re-reads the parent's cache; not a cold start |
| `cost-state` vs main + agent sums on fixture A | see §1 | Inclusion of subagents in Claude Code's ledger is unverified |
| Registry already has `agents_cost` (Panel 2, D6 + D9, `≈`) and `agent_tokens` (Panel 6, D2a) | `metrics/registry.rs:87,131` | New ids go beside them; `docs/metrics.md` regenerates |

## 4. Design

### 4.1 Provenance model

Every dollar figure on every surface carries one of three sources, and the surfaces show them the same way:

| Source | Meaning | Mark |
|---|---|---|
| `ledger` | Claude Code's own `cost-state` (`totalCostUSD`, `modelUsage[*].costUSD`) | none |
| `priced` | cctop's estimate: usage × `pricing.toml` | `≈` |
| `unpriced` | usage on a model the table does not know | `—` and the token count |

The combined figure's mark is the worst of its parts. `cctop query` already tags every value with `approx`; this PRD adds a `source` string to the cost values it introduces so the pane and the MCP tool can say *why* a number is approximate (FR-3).

### 4.2 Panel 2 — the combined headline

Line 6 today: `cache hit 98 % · $9.90 ($3.10/h) · in 12k/min`. It becomes:

```
 cache hit 98 %  ·  ≈$10.05 ($3.10/h)  ·  in 12k/min
 main $9.90 · agents ≈$0.15 (2 %)
```

- The headline is `main + agents` (+ `team` once PRD 2 lands). It is `≈` whenever the agents part is priced, which is always, so it is `≈` whenever any agent has usage. With no agents it is exactly today's figure and today's mark.
- The breakdown line replaces the `agents $X (N %)` fragment on line 10; the `where:` attribution and the `weight ×N` stay on line 10.
- `a` toggles agents out of both the token bars and the cost lines; the existing `main only` marker on line 7 covers both. The toast text is unchanged.
- `$/h` stays main-only and says so with a dim `main` suffix when agents are present; giving agents a turn membership is out of scope (§9). `last turn $` is unchanged.
- The `×N$` baseline multiplier keeps dividing the **main** figure by turns, because the 7-day baseline was computed main-only; its label gains nothing. Re-basing the corpus is a follow-up (§11).
- The header tile that shows cost (dashboard direction B) shows the combined figure with the same mark.

If US-001 finds that Claude Code's ledger already contains subagent calls, the headline stays `cost-state` (exact), the breakdown line becomes `main ≈$9.75 · agents ≈$0.15 (2 %)` with main derived as `ledger − agents`, and nothing is added. The two branches share every other story.

### 4.3 The agents view (from Panel 6, `Enter`)

A full-screen view in the pattern of `ui/ledger_view.rs`: `State.context_view` gains an `Agents` variant, Panel 6's `handle_key` opens it on `Enter`, `Esc`/`q` returns. It renders at 60 × 51 and 56 × 20 like the other views, scrolls with `j`/`k`, and sorts with `s` (spend, waste, elapsed, started). 60 columns:

```
 Agents ─ 12 · 3 running · ≈$4.82 · wasted ≈$1.94 (40 %)   [s]ort
 ─────────────────────────────────────────────────────────
   type      model    time    tok   ≈$     ret   waste
 ✗ Explore   haiku    1m 02  212k  0.19     —   0.19 failed
 ◐ claude    opus    12m 40  1.9M  1.41   ...   1.11 idle 6m
 ✓ Explore   haiku      44s  180k  0.16    71   0.00
 ✓ general   opus     3m 10  620k  0.92   2.1k  0.00
 ✓ Explore   haiku      51s  201k  0.18     0   0.18 no ret
 ✓ fork      sonnet     42s  548k  0.15   310   0.00  ↰32
 ─────────────────────────────────────────────────────────
 wf wf_abc-123   9 launched · 7 done · 2 failed   ≈$2.40
 ─────────────────────────────────────────────────────────
 waste: failed 0.19 · no return 0.18 · idle 1.11 · dup 0.46
 cold starts 4 (≈$0.61 of cache writes) · 0.8 % of output returned
```

Columns: state glyph (as Panel 6), agent type, model family, elapsed, tokens, priced cost, **ret** (tokens of the `Task` result that came back to the main context, `—` while running, `0` for an empty result), **waste** (dollars, with the reason word). Workflow runs fold into one group row with a subtotal, expandable with `Enter` on the row. The footer sums waste by reason and gives the two ratios that the coach PRD measured across the corpus (cold starts, share of output returned), for this session.

### 4.4 Wasted spend — definitions

All are computed per agent from `State` and are deterministic. A dollar of spend is counted under at most one reason, tested in this order:

| Reason | Evidence | Amount |
|---|---|---|
| `failed` | `state == Failed`, or the workflow journal has a `failed` entry for the agent id, or the `Task` tool_result in the main transcript has `is_error` | the agent's whole priced cost |
| `no return` | `state == Done` and the `Task` tool_result for `Meta.tool_use_id` is absent or has zero content length | the whole priced cost |
| `idle` | `state == Running` and `last_line_at` older than `IDLE_MS` (5 min, Panel 6's own constant) with no pending tool result attributed by the hook spool | the whole priced cost so far, labelled `idle <age>` and re-classified when the agent moves again |
| `dup` | a second agent with the same `agent_type` **and** the same `description` hash (`u64`, computed at parse time; the text itself is already kept for display) launched within the session while the first is still running or done | the later agent's priced cost |
| — | everything else | 0 |

Two more figures are shown but not counted as waste, because they are costs of the design rather than of a failure: **cold start** (an agent whose first call has `cache_write > cache_read` and is not a fork: the cache-write dollars of that call) and **return ratio** (Σ `ret` / Σ agent output tokens).

Nothing in this table reads prompt or description text as a signal; `dup` compares hashes of a field cctop already stores and displays.

### 4.5 `cctop query agents`, dashboard row 6, pane

- Each agent entry gains `cost` (USD, `approx`, `source`), `returned_tokens`, `waste` (`{usd, reason}` or `null`), `cold_start` (bool). The object gains `totals` (`cost`, `waste`, `waste_by_reason`, `returned_ratio`, `cold_starts`).
- `cctop query summary` and `dashboard` gain `cost_combined` beside `cost` (which keeps its main-only meaning for one release; the README notes the rename plan). The header tile reads `cost_combined`.
- Dashboard row 6 sorts its top 3 by waste, then spend, and appends `wasted ≈$X` when non-zero; it stays inside its fixed width, so the pane's Overview and the TUI stay row-identical (`tests/pane`).
- The pane's Agents view draws the new `query agents` fields in the same column order as §4.3; no new engine calls.

## 5. User stories

### US-001: Establish what Claude Code's ledger contains
**Description:** As the implementer, I need to know whether `cost-state` includes subagent calls, so that the combined figure is never double counted.

**Acceptance Criteria:**
- [ ] A script (`scripts/ledger-vs-agents.py`, read-only, counts only) compares, for every session under `~/.claude/projects` with a `cost-state` and at least one subagent transcript, `modelUsage[*]` token counts against the deduplicated main sums and main + subagent sums, and reports which of the two the ledger matches within 1 %; run on the user's machine and the answer recorded in `harness_facts.rs` with the Claude Code version (`COST_STATE_INCLUDES_SUBAGENTS: Option<bool>` and `READ_FROM`).
- [ ] The same check on fixture A and B is a `#[test]` that documents the fixture result without asserting the live answer.
- [ ] `CostTracker` gains `combined(&agents) -> Option<Cost>` implementing both branches of §4.2 behind that constant; unit tests cover both.

### US-002: Combined headline on Panel 2, the tile, query and the pane
**Description:** As a user, I want the cost I see first to be the session's whole spend, so that 20 subagents cannot hide $100.

**Acceptance Criteria:**
- [ ] Panel 2 line 6 shows the combined figure with the mark of §4.1; line 7's `main only` covers cost when `a` is off; the breakdown line of §4.2 renders under it; the `agents $X (N %)` fragment leaves line 10.
- [ ] `$/h` carries a dim `main` suffix when agents have usage; `×N$` is unchanged and its docstring says why.
- [ ] `cctop query summary` / `dashboard` export `cost_combined` with `metric_id`, `approx`, `source`; the header tile reads it; `docs/metrics.md` regenerates with `cost_combined` (Panel 2, D2 + D9 + D11, `≈ when any agent has usage`).
- [ ] Insta snapshots for Panel 2 and the dashboard on fixtures A and B are updated and reviewed; `tests/pane` fixtures regenerated via `scripts/pane-fixtures.sh`.
- [ ] With no subagents, the rendered characters of Panel 2 are identical to v0.3.1.

### US-003: Per-agent cost and waste in `State`
**Description:** As every surface, I need one place that prices an agent and classifies its waste, so that the TUI, query and pane cannot disagree.

**Acceptance Criteria:**
- [ ] `agents.rs` `Agent` gains `returned_tokens: Option<u64>` (from the main transcript's `tool_result` for `Meta.tool_use_id`, set by `State::apply`), `task_result_is_error: bool`, `description_hash: u64`, `first_call_cache: (u64, u64)` (cache write / read of the first API call).
- [ ] `ui/state.rs` gains `agent_ledger(&self) -> Vec<AgentLedgerRow>` (id, cost `Option<Cost>`, waste `Option<Waste>`, cold start, returned tokens) and `agent_totals(&self)`; §4.4's order of reasons is a unit test per reason on synthetic lines, plus fixture A (the fork returns 310 tokens, waste 0, not a cold start).
- [ ] `AgentWatcher::poll` and `agents::load` populate the new fields identically (the `load.rs` path and the live path stay in sync: a test loads fixture A both ways and compares).
- [ ] Registry ids added: `agent_cost` (Panel 6, D2a + D9, `≈`), `agent_returned` (D2), `agent_waste` (D2a + D6 + D9, `≈`), `agents_waste` (Panel 6 total), `agents_cold_starts`, `agents_return_ratio`.

### US-004: The agents view
**Description:** As a user with 20 agents, I want a sortable ledger of them with dollars and waste, so that I can see which ones I should not have launched.

**Acceptance Criteria:**
- [ ] `ContextView::Agents`; Panel 6 opens it on `Enter`; `Esc`/`q` close; `j`/`k` scroll; `s` cycles spend › waste › elapsed › started; `BINDINGS` and the help overlay updated (the help test passes).
- [ ] Renders §4.3 at 60 × 51 and 56 × 20 with fixed column widths cut by `fmt::clip`; workflow runs fold into a group row with a subtotal, `Enter` on it expands; the footer shows waste by reason, cold starts and the return ratio.
- [ ] Insta snapshots on fixture A (1 fork) and on a synthetic 25-agent state (built in the test from `Agent::from_lines` with two failed, one idle, one duplicate, three cold starts) so the overflow and sort paths are covered.
- [ ] Panel 6's own rendering is unchanged.

### US-005: `cctop query agents`, dashboard row 6, the pane
**Description:** As the pane and the MCP tool, I want the same rows the TUI draws.

**Acceptance Criteria:**
- [ ] `query::agents` exports the fields of §4.5; `tests/pane/fixtures/agents.json` regenerated; the pane's Agents view shows type, model, time, tokens, `≈$`, ret, waste in that order and its test asserts the columns against the fixture.
- [ ] Dashboard row 6 sorts by waste then spend and appends `wasted ≈$X`; `tests/pane` still asserts row-identical text on fixture B's moments.
- [ ] MCP tool `agents` (via `mcp.rs`) carries the same object; its schema text names the new fields.
- [ ] README's metrics block regenerated (`cctop metrics --readme`).

### US-006: The coach hears about waste
**Description:** As a user, I want the coach to say so once when agent waste crosses a threshold, so that the view is not the only place it is visible.

**Acceptance Criteria:**
- [ ] One LATER rule `agents-waste` in `advisor/rules/token.rs`: fires when `agents_waste.usd ≥ max($1, 25 % of agent spend)` and at least two agents are classified; nudge text names the top reason and the key to open the view (`6` then `Enter`); TTL three human turns; acted when the view was opened or the waste ratio drops below 10 %; cooldown 10 human turns; never in bot loops (FR-4 of the coach PRD).
- [ ] Replay on fixture B: fires zero times (B has no waste), asserted in `advisor` tests; the rule count in CLAUDE.md's architecture line and `docs/metrics.md` updates.

## 6. Functional requirements

- FR-1: The combined cost must never count a subagent call twice; the branch is chosen by `harness_facts::COST_STATE_INCLUDES_SUBAGENTS`, and an `Option::None` there must fall back to the *exclusive* assumption with the figure marked `≈`.
- FR-2: Every cost value on every surface must carry `approx` and the `source` of §4.1; the mark of a sum is the worst of its parts.
- FR-3: `cctop query` values introduced here must carry `metric_id`, `approx` and `source`; existing values keep their shape.
- FR-4: Waste classification must use only the evidence listed in §4.4; no rule may read the description or prompt text, only its hash and length.
- FR-5: An agent's `returned_tokens` must be a count of the `tool_result` content in the main transcript; the content itself is never stored (cctop's read-only, no-prose rule).
- FR-6: With no subagents, Panel 2, the dashboard and `cctop query summary` must render character-identical to v0.3.1 except for the added `cost_combined` key.
- FR-7: The view must be usable at 20 rows with 25 agents (scroll, sort, group rows) and must not allocate per tick beyond the existing 2 s agent scan.
- FR-8: New metrics must exist in the registry with sources and caveats before they are drawn; `docs/metrics.md` and the README block regenerate; CI fails on drift.

## 7. Non-goals

- No turn membership for agents, so `$/h`, `last turn $` and the cost gradient stay main-only in this PRD.
- No re-basing of the 7-day baseline to combined cost (follow-up in §11).
- No reading of teammates' transcripts; that is `tasks/prd-cctop-team-costs.md`.
- No semantic judgement of whether an agent's *result* was useful; `ret` is a size.
- No changes to Panel 6's inline list beyond opening the view.

## 8. Design considerations

- **One figure first.** The person reads the headline while typing; the breakdown line is there for the 2 % of the time they wonder where the money went.
- **Waste has a reason word.** A dollar figure without `failed` / `no ret` / `idle` / `dup` next to it would just be a second cost column.
- **Same glyphs, same clip widths** as Panel 6 and the ledger view, so the pane can mirror the rows.
- **Honest marks.** Every agent dollar is `≈` and the combined headline inherits it; the view header says `≈` once rather than per cell.

## 9. Technical considerations

- **Linking result to agent.** `State::apply` already sees every `tool_result` on the main transcript; it needs an index `tool_use_id → agent id` built from `Meta.tool_use_id` when an agent is registered (from the watcher, `load`, or `SubagentStart`), and a pending map for results that arrive before the meta file does (the meta can land late; `AgentWatcher::poll` already re-reads empty metas).
- **Fixture coverage.** Fixture A has one fork; the synthetic 25-agent state in US-004 lives in the test, not in `fixtures/`. If a real multi-agent session is anonymised into `fixtures/session-c.jsonl` later, its `subagents/` directory ships beside it like A's.
- **Cost of pricing.** `pricing.estimate` is a prefix match over a small table; 40 agents × 2 s tick is negligible. Cache the per-agent `Cost` on the `Agent` and recompute on `push`.
- **Rustfmt / clippy.** `agents.rs` and `state.rs` are large; fold formatting drift in.

## 10. Open questions

1. **Ledger inclusion** (US-001): the whole of §4.2 forks on it; run the script before writing the view.
2. **`dup` threshold**: same type + same description hash is strict; a looser rule (same type within 30 s) would catch fan-outs that are intentional. Ship strict, measure on the user's corpus.
3. **Idle**: an agent waiting on a slow tool (a 10-minute build) is not wasted; the hook spool's pending `PreToolUse` without `PostToolUse` should suppress `idle` if that pairing is reliable enough (check on live sessions).

## 11. Follow-ups

- Re-base `baseline.rs` (the 7-day medians) on combined cost once the combined figure has been live for a week of sessions.
- Turn membership for agents (attribute an agent's spend to the human turn that launched it) so `$/h` and the gradient become combined too.
- `tasks/prd-cctop-team-costs.md`: teammates, whose numbers are exact.
