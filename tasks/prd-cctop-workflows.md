# PRD: workflow runs — live while they run, a verdict when they end

**Status:** v1.0 · 2026-09-28 — draft, not implemented. Written from `main` at 7aa5dd8 and the two workflow runs on the user's machine (Claude Code 2.1.2xx, session `28c68a61`).
**Target:** cctop after v0.8.0; TUI, `cctop query agents`, the MCP tool and the pane in one change.
**Depends on:** `tasks/prd-cctop-agent-costs.md` (the agent ledger, priced cost per agent, fork skip, `WorkflowNotification`, the agents view and its workflow group row).
**Plan:** `tasks/plan-cctop-workflows.md`.

> Decisions taken with the user on 2026-09-28:
> 1. Two jobs, in this order: **live awareness** (a run is going — which phase, how far, what it costs, is it failing) and a **verdict** when it ends (was it worth it, and how to fix it).
> 2. The verdict leads with **waste** — failed $ as one figure, beside a failed *count* — and shows the **overhead ratio** (run $ ÷ main-thread $ over the same window) next to it. No counterfactual: cctop never claims the main thread would have done the work cheaper.
> 3. **Digging deeper says how to fix it**: every failed agent gets one structural cause; each cause maps to one fixed fix line; the fix line points at the `parallel(` / `pipeline(` call in the workflow script (`path:line`).
> 4. **No new top-level view.** A one-line strip on the dashboard while a run is live; the run detail opens from the agents view's workflow group row with `Enter`. `cctop query agents` carries `workflows[]`.
> 5. The coach stays silent about workflows in v1.

---

## 1. Introduction

A `Workflow` run fans out tens to hundreds of agents from a script. cctop already finds them — agents under `<session>/subagents/workflows/wf_*/` are tagged with their run, `workflow_journals` counts `launched` / `started` / `result` / `failed`, and the agents view folds a run into one group row with a subtotal. What it cannot say is what the *run* is doing: which phase is live, whether it is failing, what the failures are, and what to change in the script.

The two runs on the user's machine show why that matters:

| Run | Status | Starts | Distinct keys | Results | Failed | Failed at first call |
|---|---|---|---|---|---|---|
| `wf_0aa065ff` (`cctop-research-sweep`, 4 script versions) | completed | 300 | 172 | 65 | 234 | 198 |
| `wf_89cf8717` | killed | 35 | 25 | 5 | 20 | 14 |

**Every one of the 254 failures is `error: "rate_limit"`, `apiErrorStatus: 429`.** 212 of them died on their first call (a 10-line transcript), so their priced cost is near zero: the damage is wall-clock and resumes, not dollars. A verdict that shows only failed $ would call that run healthy. The failures sit in one phase — `Verify`, 201 of 234 — which is one `pipeline(canon, …)` call at line 136 of the script.

## 2. Goals

- **Know a run exists and how it is going** without leaving the dashboard: phase, progress, failed count with its dominant cause, spend so far.
- **A per-phase verdict** when the run ends: started, results, failed (count and $), waste %, overhead ratio, cold-start share.
- **A fix line per failure cause**, pointing at the script call that produced the failing agents.
- **Structural evidence only.** No prompt, label, log, summary or result text is kept — see §4.6.

## 3. Findings

### 3.1 What the code does today

| Fact | Where | Consequence |
|---|---|---|
| `WorkflowJournal { run, launched, started, results, failed, failed_ids }` — counts only; `phase`, `label`, `key` dropped | `agents.rs:426`, `workflow_journals` `:437` | Per-phase stats need the parser to keep `phase` + `agentId` per `started` |
| Journals re-read every 5 s by a tick hook; agents by the fs-event `AgentWatcher` | `attach.rs:207-218`; `load.rs:128` once for `query` | Live updates need no new watcher |
| An agent transcript's API-error lines are **skipped** (`Line::Assistant(a) if a.is_api_error() => {}`) | `agents.rs:235`; `is_api_error` `transcript/mod.rs:678` | Nothing records a 429 per agent today; `apiErrorStatus` / `error` are read only for the main session (`events.rs:140-151`) |
| `Agent::state` Failed = `last_was_error_result` + 60 s | `agents.rs:373` | A 429-killed agent is `Failed` only through the journal |
| `WorkflowGroup { run, launched, done, failed, empty_result, agents, running, cost, waste_usd }`; `workflow_groups` sums priced members | `agent_ledger.rs:133`, `:149` | The run-level aggregate exists; phases, causes, overhead and state extend it |
| `WasteReason { Failed, Killed, NoReturn, Idle }`; waste = the agent's whole priced cost; `cold_start` per row | `agent_ledger.rs:23`, `:240`, `:317` | `workflow_waste_pct` and `workflow_cold_start_pct` are sums over existing row fields |
| `WorkflowNotification { agent_count, done, error, skipped, empty_result }`, keyed by run id via `tools.workflow_launches` | `transcript/task_notification.rs:48`; `ui/state.rs:1563`, `:370` | `Done` detection and `empty_result` are already linked to the run |
| Agents view: `Entry::Group`, `Enter` toggles `expanded`; `group_line` at `:221` | `ui/agents_view.rs:58`, `:111`, `:221` | `Enter` on a group changes meaning: it opens the detail (members listed inside it) |
| `query agents` → `workflows[] { run, launched, done, failed, empty_result, agents, cost, waste }` | `query.rs:403-417` | Extended, not replaced — existing keys keep their meaning |
| Dashboard `body_work` has an agents row (label 11 cells, conditional on rows) | `dashboard.rs:1205`, `:1454-1500` | The strip is a sibling row, same label width, same conditional pattern |
| Metrics via `metric!(id, panel, name, unit, formula, [sources], caveats, estimate_when)` | `metrics/registry.rs:21` | Five new entries |
| Pane `agents.tsx` has no workflow handling; `overview.tsx` draws `bodies[].rows` generically | `plugin/hooks/views/` | The strip is free in the pane; the run detail is new pane code |
| Nothing reads `<session>/workflows/<run>.json` | — | New reader |

### 3.2 What Claude Code writes (measured on this machine)

| Where | What | Consequence |
|---|---|---|
| `<session>/subagents/workflows/<run>/journal.jsonl` | One `launched` line, then `started` `{key, agentId, label, phase}`, `result` `{agentId, …return value…}`, `failed` `{key, agentId}` | Phase and agent id per start are free. `result` lines carry the agent's **return value** (prose) — count them, never keep them |
| same, `key` | `v2:<sha256>` per `agent()` call identity; repeats across resumes (300 starts over 172 keys, max 3 per key) | A run is one `runId` across several invocations; retries are visible but v1 does not split them out (decision 2) |
| same, `agent-<id>.jsonl` of a failed agent | Last line: `isApiErrorMessage: true`, `error: "rate_limit"`, `apiErrorStatus: 429`, `quotaLimits` | The cause is on the last line, as tokens, not text |
| `<session>/workflows/<run>.json` | `runId`, `workflowName`, `status` (`completed` / `killed`), `startTime`, `durationMs`, `agentCount`, `totalTokens`, `totalToolCalls`, `defaultModel`, `phases[{title, detail}]`, `workflowProgress[]`, `script` (inline), `scriptPath`, `args`, `result`, `logs`, `summary`, `error` | Describes the **last invocation only** (`agentCount` 27 vs 300 starts in the journal). `detail`, `args`, `result`, `logs`, `summary`, `error` are prose — never read. Appears to be written when an invocation ends — **to verify live** |
| `<session>/workflows/scripts/<name>-<n>.js` | One file per invocation's script | The inline `script` in the run record is the one to scan; label prefixes survive edits, line numbers do not |
| Script | `agent(…, { label: \`verify:${c.id}\`, phase: 'Verify', … })` inside `pipeline(canon, …)` at line 136 | Label prefix before `:` + phase → the `agent(` call → the enclosing `parallel(` / `pipeline(` |
| Main transcript | `toolUseResult.status = "async_launched"`, `taskType: "local_workflow"`, `runId`, `workflowName`, `transcriptDir`; the completion notification carries `<agent_count>`, `<agents_done>`, `<agents_error>`, `<agents_skipped>`, `<agents_empty_result>` | The launch line gives the run's start in the main timeline; the notification ends it |
| Workflow runtime | Concurrency is capped at `min(16, CPUs − 2)` per workflow; there is **no per-call concurrency option**; `agent()` returns `null` after a terminal API error; resume (`resumeFromRunId`) replays finished agents from cache | Fix lines can recommend batching, a cheaper `effort`/`model` for the phase, or resuming after the quota window — never "set concurrency to N" |

Only `rate_limit` / 429 is observed on this machine. Every other cause in §4.3 is unobserved: each ships only behind a synthetic test and is labelled as such in `harness_facts.rs`. The `error` field is a machine token (`rate_limit`), so `Unknown` may show it verbatim.

## 4. Design

### 4.1 Data: extend what exists

**Collected** (`agents.rs`, re-read on the existing 5 s tick):

```
WorkflowJournal {                         // existing counts kept
  …,
  phases: Vec<JournalPhase>,              // first-seen order
  mtime_ms: u64,                          // journal's last change, for Live/Stalled
}
JournalPhase { title, started, results, failed, agent_ids, label_prefixes /* match keys only, never output */ }

Agent { …, api_error: Option<ApiError>, completed_calls: usize }   // set where :235 skips the line today
ApiError { status: Option<u16>, kind: ApiErrorKind /* RateLimit | Overloaded | PromptTooLong | Other */ }

WorkflowRecord {                          // new, from <session>/workflows/<run>.json
  run, name, status, start_ms, duration_ms, script_path,
  pointers: BTreeMap<String /*phase*/, Pointer>,   // computed at read, script text dropped
}
Pointer { line: u32, call: Parallel | Pipeline | PhaseMarker }
```

**Derived** (`agent_ledger.rs`, on read, like every other metric):

```
WorkflowGroup {                           // existing fields kept
  …,
  name: Option<String>,
  state: Live | Stalled | Done(Completed | Killed),
  phases: Vec<PhaseRow>,
  failed_usd, waste_pct, overhead: Option<f64>, cold_start_pct,
  fixes: Vec<Fix>,                        // top two causes, §4.3
}
PhaseRow { title, started, results, failed, failed_usd, waste_pct, causes: BTreeMap<Cause, usize>, pointer: Option<Pointer> }
```

Priced cost comes from the existing `AgentRow`s (fork skip applied); a phase's figures are sums over its `agent_ids`.

**State.** `Live` while no workflow notification has arrived for the run and a journal or agent file changed in the last 60 s; `Stalled` with no notification and no change for 60 s (the strip greys; after 30 min it drops off the dashboard); `Done` when the notification arrives or the run record's `status` is terminal.

### 4.2 Metrics (declared in `metrics/registry.rs`)

| id | unit | formula |
|---|---|---|
| `workflow_failed` | count | failed agents in the run (journal `failed` entries, deduplicated by agent id) |
| `workflow_failed_usd` | usd, estimate | Σ priced cost of those agents |
| `workflow_waste_pct` | percent | Σ `AgentRow.waste.usd` of the run's agents (the existing `WasteReason` classification) ÷ run $ |
| `workflow_overhead` | ratio | run $ ÷ main-thread $ over `[run start, run end or now]` |
| `workflow_cold_start_pct` | percent | Σ first-call cache-write $ of the run's agents ÷ run $ |

Caveat on every one: priced from the transcripts, so `≈`; `workflow_overhead` is `—` when the main thread spent < $0.01 in the window.

### 4.3 Causes and fix lines

Each failed or killed agent gets exactly one `Cause`, from its last transcript line and its line count. Fix lines are fixed strings, cut at the pane's width in `dashboard.rs`/`coach.rs` style; `{n}` fields are numbers.

| Cause | Evidence | Fix line | Observed |
|---|---|---|---|
| `RateLimitFirst` | `apiErrorStatus: 429` / `error: "rate_limit"`, no completed assistant call before it | `{n} agents hit the rate limit on their first call — batch this phase's items, or lower its effort/model` | yes (212) |
| `RateLimitMid` | 429 after ≥ 1 completed call | `{n} agents hit the rate limit mid-task — batch this phase, then resume the run after the window resets` | yes (42) |
| `Overloaded` | `apiErrorStatus: 529` | `{n} agents met an overloaded API — transient; resume the run, finished agents are cached` | no |
| `ContextOverflow` | the API error's `error` token for an over-long prompt, pinned in `harness_facts.rs` when first observed | `{n} agents' input was too large — pass paths, not contents` | no |
| `NoStructuredOutput` | ended without a `StructuredOutput` tool_use and without an API error, while ≥ 1 agent of the same phase ended with one (so the phase used a schema) | `{n} agents never satisfied the schema — loosen it or split the task` | no |
| `Killed` | notification or run record says killed | *(no fix line — reported)* | yes (run-level) |
| `Unknown` | anything else | `{n} agents failed ({error token}) — cctop has no fix for this yet` | — |

`agents_empty_result` from the notification is shown as a count beside the phase table; it has no per-agent cause (the journal cannot say which).

The verdict footer shows the fix lines of the two causes with the most agents, each followed by the pointer of the phase that has most of them.

### 4.4 The pointer

Input: the run record's inline `script` (the last invocation), never the prompts. Scan:

1. For each phase, collect the label prefixes seen in the journal (`verify` from `verify:C017`).
2. Find each `agent(` call in the script whose options contain `phase: '<title>'` (either quote style) and a label template starting with `<prefix>:`; take the nearest enclosing `parallel(` or `pipeline(` by scanning backwards with a bracket counter.
3. No match, or more than one → the line of `phase('<title>')`, kind `PhaseMarker`.

Kept: the line number, the call kind, and `scriptPath`. Nothing else from the script is stored, logged or returned. Shown as `→ <file name>:<line> pipeline()`; in the TUI `o` copies `<scriptPath>:<line>` to the clipboard (cctop opens nothing). If the run record is missing, the pointer is omitted and the fix line stands alone.

### 4.5 Surfaces

**Dashboard strip** (present only while a run is `Live` or `Stalled`, one line, fixed widths):
`wf <name:16> ▸ <phase:10> <results>/<started> ✗<failed> <top cause short:8> $<run$> <overhead>×main`
e.g. `wf research-sweep ▸ Verify    12/246 ✗201 429×201 $41.20 3.4×main`.

**Agents view.** The workflow group row gains the state glyph and `✗n`. `Enter` on it now opens the run detail (replacing the expand toggle; the member rows move into the detail, below the phase table):

```
 wf_0aa065ff  cctop-research-sweep  completed  47m  $X  3.4×main  cold 22 %
 phase        start  res   ✗   ✗$     waste  cause
 Sweep           6    6    0   0.00     0 %
 Candidates      4    4    0   0.00     0 %
 Merge           1    1    0   0.00     0 %
 Verify        246   44  201   0.41    38 %  429×201
 Design         38    9   29   0.07    12 %  429×29
 Critic          5    1    4   0.01     9 %  429×4
 ───
 201 agents hit the rate limit on their first call — batch this phase's items, or lower its effort/model
   → cctop-research-sweep-4.js:136 pipeline()
```

(The start / result / ✗ counts and causes are the run's; the dollar figures, waste % and ratios are illustrative until the fixture is priced.)

**`cctop query agents`** extends each existing `workflows[]` entry (old keys unchanged) with `name`, `state`, `phases[]`, the §4.2 metrics tagged by id, `causes`, `fix[]` (text as rendered) and `pointer`. The dashboard object gains an optional `workflow` line. The MCP `agents` tool returns the same object. The pane renders both verbatim; `tests/pane` asserts them row-identical to the TUI on a fixture.

### 4.6 What is deliberately not read

Journal `result` values, labels beyond the prefix before `:` (kept only as a phase-local match key, never output), the script's prompt strings, the run record's `detail`, `args`, `result`, `logs`, `summary`, `error`, and every agent's message content. The cause is taken from `error`, `apiErrorStatus`, `isApiErrorMessage` and the presence of a `StructuredOutput` tool_use. `transcript/` and `agents.rs` document this beside the parser, as `insights.rs` does.

## 5. User stories

### US-001: Journal phases, per-agent API errors, the run record
- [ ] `WorkflowJournal` keeps per-phase counts, agent ids and label prefixes; `result` lines are counted and their values dropped.
- [ ] `Agent` records `api_error` and `completed_calls` where `agents.rs:235` skips API-error lines today; nothing else about the line is kept.
- [ ] `WorkflowRecord`: the run record is read for `workflowName`, `status`, `startTime`, `durationMs`, `scriptPath`; no other field is deserialised.
- [ ] `WorkflowGroup.state` follows the rule below §4.1, from the notification already linked by run id, the record's `status` and the journal's mtime.
- [ ] Unit tests on synthetic journals (resumed keys, missing record, killed run).

### US-002: Causes
- [ ] `Cause` from an agent's last line and call count, per §4.3; `harness_facts.rs` records which kinds are observed and at which version.
- [ ] Unit tests for every cause on synthetic agent lines; the two 429 variants also on the fixture.

### US-003: The pointer
- [ ] `workflow_script.rs`: the §4.4 scan over a `&str`, returning `(line, kind)` per phase; no text escapes the function.
- [ ] Tests: `parallel`, `pipeline`, nested `parallel` inside a `pipeline` stage, both quote styles, an unmatched phase (→ `PhaseMarker`), a label template with no prefix.

### US-004: Metrics
- [ ] The five §4.2 metrics in the registry; `docs/metrics.md` and the README block regenerated; the staleness test passes.

### US-005: Run detail in the agents view and the dashboard strip
- [ ] `Enter` on a workflow group row opens the detail; `Esc` returns; `o` copies the pointer.
- [ ] Insta snapshots at 120 × 30 and 56 × 20 on the fixture; the strip on a synthetic live state.

### US-006: Query, MCP and the pane
- [ ] `workflows[]` in `cctop query agents` and the MCP tool; `workflow` in the dashboard object.
- [ ] `plugin/hooks/views/agents.tsx` renders the run detail; `overview.tsx` the strip; pane fixtures regenerated with `scripts/pane-fixtures.sh`; row-identical tests.

### US-007: Fixture
- [ ] `fixtures/session-w/`: a composed, anonymised session from `wf_0aa065ff` — journal (labels reduced to `<prefix>:<n>`, `result` values emptied), a sample of 429 agents (first-call and mid-task), a few `result` agents, the run record with `script` kept and every prompt string replaced by `"…"`, and the main-transcript launch and notification lines. Built by `scripts/compose-fixture.py`, never hand-edited.

## 6. Functional requirements

- FR-1: Every figure in the strip, the detail, the query and the pane comes from one `WorkflowGroup`, derived from the collected journal, record and agent rows.
- FR-2: No field listed in §4.6 is read into memory beyond the deserialiser skipping it.
- FR-3: A session with no workflow run renders byte-identically to today (snapshots unchanged).
- FR-4: A run with no run record, no notification, or no script still renders; the missing parts show `—`.
- FR-5: The coach's rules do not read `WorkflowGroup`'s new fields in v1.

## 7. Non-goals

- Cross-session workflow history, per-workflow-name trends (`cctop-insights` territory, later).
- Splitting failed $ into retried and lost.
- Any action on the run (stopping it, resuming it). cctop is read-only.
- Opening the script in an editor.
- A coach rule about a burning run.

## 8. Open questions

1. When is `<session>/workflows/<run>.json` first written — at launch, per phase, or only at the end of an invocation? Decides whether `name` and the pointer exist while a run is live. Check on the next live run; until then the launch `toolUseResult` supplies `workflowName` and the pointer appears when the record does.
2. Does a resumed invocation append a second `launched` line to the journal? Both runs show one; one of them was resumed three times. If not, invocations cannot be told apart from the journal and v1 does not try.
3. The concurrency cap (`min(16, CPUs − 2)`) is from the Workflow authoring reference, not measured. The strip does not show it.
