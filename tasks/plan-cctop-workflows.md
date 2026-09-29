# Workflow runs — live strip and verdict: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** cctop shows a running `Workflow` run on the dashboard and, per run, a phase table with failed count/$, waste %, overhead ratio, cold-start share, a cause per failure and a fix line pointing at the script's `parallel(` / `pipeline(` call — in the TUI, `cctop query agents`, the MCP tool and the pane.

**Architecture:** Collectors in `agents.rs` gain per-phase journal stats and a per-agent API error; a new `workflow_runs.rs` reads `<session>/workflows/<run>.json` (keeping only identifiers and the pointer scan's result from `workflow_script.rs`) and derives causes, fixes and the run's figures; `agent_ledger::WorkflowGroup` carries the result to `query.rs`, `dashboard.rs`, `ui/agents_view.rs` and, through the query JSON, `plugin/hooks/views/agents.tsx`.

**Tech Stack:** Rust (ratatui, serde_json, insta), TypeScript pane (node --test), Python fixture scripts.

**Spec:** `tasks/prd-cctop-workflows.md` — read it first; §4.3's fix-line strings and §4.6's never-read list are normative.

## Global Constraints

- Read-only: cctop never writes under `~/.claude` and never opens an editor; `o` only copies `path:line` via `ask::copy_to_clipboard`.
- No prose kept (spec §4.6): journal `result` values, labels beyond their prefix, the script's string literals, the run record's `detail`/`args`/`result`/`logs`/`summary`/`error`, message content. Deserialise only named fields; never `Value`-clone a whole record into state.
- Fixed-width text is cut with `fmt::clip` in Rust and drawn verbatim by the pane; the pane never recomposes a string the query already rendered.
- Every new figure has a `metric!` entry; `docs/metrics.md` and the README block are regenerated with `cctop metrics --md > docs/metrics.md && cctop metrics --readme README.md`, never hand-edited.
- Fixtures are composed and anonymised by scripts, never hand-edited; pane fixtures come from `scripts/pane-fixtures.sh`.
- A session without a workflow run renders byte-identically (existing snapshots unchanged).
- `make check && npm test` green at every commit; commit subjects are prose (`Workflows: …`), ending with the `Co-Authored-By` line.

## Review Focus

1. **The run record is absent while the run is live** (it may only be written at invocation end, spec §8 Q1): the strip and the detail must render with `name` from the launch result and no pointer — pinned in Task 5.
2. **A prompt template that contains the text `phase: 'Verify'` or `parallel(`** must not produce a pointer: the scan skips string and template-literal contents — pinned in Task 3.
3. **A truncated last journal line** (the run is writing it) must be skipped, not abort the parse — pinned in Task 1.
4. **Main-thread spend below $0.01 in the run's window** gives overhead `—`, never `inf` or a huge ratio — pinned in Task 5.
5. **Journal agent ids with no transcript file** (queued, never started, or a resumed run's replayed ids) count in `started`/`failed` but add $0 and get cause `Unknown` only if failed — pinned in Task 5.
6. **A resumed run** keeps its `runId`, so the previous invocation's notification and `completed` record are still there while it runs: a later launch of the run, or activity > 10 s after the marker, makes it `Live` again — pinned in Task 5. Its window starts at the first invocation, not at the record's `startTime` (the record describes the last invocation only).
7. **Cause order:** an API error first, then a killed run, then the missing schema — an agent cut off by a kill in a schema phase is `Killed` — pinned in Task 5.
8. **Fix lines wrap, never clip:** at 56 columns (52 inside the frame) the advice after the dash must still be on screen — pinned in Task 7.
9. **The journal's mtime is not evidence of time:** its lines carry no timestamps and a copied or checked-out session (fixture W) has a fresh mtime; only launches and member lines supersede a terminal marker — pinned in Task 5 and on fixture W in Task 9.
10. **A pointer into a script that moved on:** the record holds the last invocation's script only; when a phase's failures all predate it, the pointer is marked stale — pinned in Task 5, and the real run is this case (Task 9).

---

## File map

| File | Responsibility |
|---|---|
| `src/agents.rs` (modify) | `WorkflowJournal.phases`, `mtime_ms`; a typed journal line; `Agent.api_error`, `Agent.completed_calls_before_error` (structured output is read from the existing `tools_by_name`) |
| `src/workflow_script.rs` (create) | Pure lexical scan: `(script, phase, label prefixes) → Option<Pointer>` |
| `src/workflow_runs.rs` (create) | `WorkflowRecord` reader; `Cause`, `Fix`; `derive(state, rows, run) → RunVerdict` |
| `src/agent_ledger.rs` (modify) | `WorkflowGroup` gains `name`, `state`, `verdict` |
| `src/tools.rs`, `src/transcript/tool_result.rs` (modify) | `WorkflowLaunch` struct (with `name`, `at_ms`) replaces the launch tuple; the parser keeps `workflowName` |
| `src/ui/state.rs`, `src/attach.rs`, `src/load.rs` (modify) | `State.workflow_records`, re-read on the 5 s tick and at load |
| `src/metrics/registry.rs` (modify) | Five `workflow_*` metrics |
| `src/query.rs` (modify) | `workflows[]` extended |
| `src/dashboard.rs` (modify) | The live strip row |
| `src/ui/agents_view.rs` (modify) | Group row glyph/✗; `Enter` opens `RunDetail`; `o` copies the pointer |
| `src/harness_facts.rs` (modify) | `mod workflow_run` facts: journal/record fields, observed error tokens, concurrency cap |
| `scripts/compose-workflow-fixture.py` (create), `fixtures/session-w*` | The anonymised workflow fixture |
| `plugin/hooks/views/agents.tsx`, `plugin/hooks/model.ts`, `plugin/hooks/pane.tsx`, `tests/pane/views.test.ts`, `tests/pane/reducer.test.ts` (modify) | Pane run rows as Buttons, `openRun` reducer field, run detail, row-identical tests |
| `docs/query.md` (modify) | `workflows[]` fields |

---

### Task 1: Journal phases and liveness

**Files:**
- Modify: `src/agents.rs:423-475` (`WorkflowJournal`, `workflow_journals`), tests in `mod workflow_tests` (`:673`); `src/ui/agents_view.rs:631` (its `WorkflowJournal` literal has no `..Default::default()` — add it, or the test build breaks)

**Interfaces:**
- Produces:
  ```rust
  pub struct JournalPhase { pub title: String, pub started: usize, pub results: usize, pub failed: usize,
                            pub agent_ids: Vec<String>, pub result_ids: Vec<String>, pub label_prefixes: Vec<String> }
  // WorkflowJournal gains:
  pub phases: Vec<JournalPhase>,   // first-seen order
  pub mtime_ms: Option<i64>,       // journal.jsonl mtime, epoch ms
  ```
  `label_prefixes`: distinct text before the first `:` of each `started.label`, used only by Task 3's matcher; never serialised.

- [ ] **Step 1: Write the failing test** (add to `mod workflow_tests`)

```rust
#[test]
fn journal_keeps_phases_in_order_and_skips_a_torn_line() {
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().join("workflows").join("wf_t");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(
        run.join("journal.jsonl"),
        concat!(
            r#"{"type":"launched"}"#, "\n",
            r#"{"type":"started","key":"k1","agentId":"a1","label":"sweep:x","phase":"Sweep"}"#, "\n",
            r#"{"type":"started","key":"k2","agentId":"a2","label":"verify:C1","phase":"Verify"}"#, "\n",
            r#"{"type":"started","key":"k3","agentId":"a3","label":"verify:C2","phase":"Verify"}"#, "\n",
            r#"{"type":"result","agentId":"a1","value":"prose that must not be kept"}"#, "\n",
            r#"{"type":"failed","key":"k2","agentId":"a2"}"#, "\n",
            r#"{"type":"started","key":"k4","agentId":"a4","lab"#,
        ),
    )
    .unwrap();
    let j = &workflow_journals(dir.path())[0];
    assert_eq!(j.started, 3);
    let t: Vec<_> = j.phases.iter().map(|p| (p.title.as_str(), p.started, p.results, p.failed)).collect();
    assert_eq!(t, vec![("Sweep", 1, 1, 0), ("Verify", 2, 0, 1)]);
    assert_eq!(j.phases[1].agent_ids, vec!["a2", "a3"]);
    assert_eq!(j.phases[0].result_ids, vec!["a1"]);
    assert_eq!(j.phases[1].label_prefixes, vec!["verify"]);
    assert!(j.mtime_ms.is_some());
    assert!(!format!("{j:?}").contains("prose"));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test agents::workflow_tests::journal_keeps_phases`
Expected: compile error, `no field phases on WorkflowJournal`.

- [ ] **Step 3: Implement**

Read each line into a private typed struct — `#[derive(Deserialize)] struct JournalLine { #[serde(rename = "type")] kind: Option<String>, #[serde(rename = "agentId")] agent_id: Option<String>, phase: Option<String>, label: Option<String> }` — instead of today's `serde_json::Value`, so a `result` line's return value is skipped by the deserialiser and never held in memory (spec FR-2); the snippet below is written against `Value` for brevity, map it to the struct's fields. `result` pushes the id onto its phase's `result_ids`. In `WorkflowJournal` add `phases` and `mtime_ms` (derive `Default` already covers them; `PartialEq, Eq` stay — `Option<i64>` is `Eq`). In `workflow_journals`, keep a `HashMap<String, usize>` agent id → phase index while scanning:

```rust
let mut phase_of: std::collections::HashMap<String, usize> = Default::default();
// in the loop, after parsing `v`:
Some("started") => {
    j.started += 1;
    let title = v.get("phase").and_then(|p| p.as_str()).unwrap_or("").to_string();
    let i = match j.phases.iter().position(|p| p.title == title) {
        Some(i) => i,
        None => { j.phases.push(JournalPhase { title, ..Default::default() }); j.phases.len() - 1 }
    };
    let p = &mut j.phases[i];
    p.started += 1;
    if let Some(id) = v.get("agentId").and_then(|a| a.as_str()) {
        p.agent_ids.push(id.to_string());
        phase_of.insert(id.to_string(), i);
    }
    if let Some(pre) = v.get("label").and_then(|l| l.as_str()).and_then(|l| l.split_once(':')).map(|(a, _)| a) {
        if !p.label_prefixes.iter().any(|x| x == pre) { p.label_prefixes.push(pre.to_string()); }
    }
}
Some("result") => {
    j.results += 1;
    if let Some(i) = v.get("agentId").and_then(|a| a.as_str()).and_then(|id| phase_of.get(id)) { j.phases[*i].results += 1; }
}
Some("failed") => {
    j.failed += 1;
    if let Some(id) = v.get("agentId").and_then(|a| a.as_str()) {
        j.failed_ids.push(id.to_string());
        if let Some(i) = phase_of.get(id) { j.phases[*i].failed += 1; }
    }
}
```

A start without `phase` goes to the phase titled `""`; renderers show that title as `—` (Task 7). A torn line already fails `serde_json::from_str` and hits `continue`. Set `mtime_ms` from `std::fs::metadata(run.join("journal.jsonl")).and_then(|m| m.modified())` → epoch ms. Derive `Debug, Clone, Default, PartialEq, Eq` on `JournalPhase`. Update the existing `workflow_agents_journals_and_teammates_are_found` expectation (`:704`) to include `phases` and ignore `mtime_ms` (compare fields, not the whole struct).

- [ ] **Step 4: Run tests**

Run: `cargo test agents::` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/agents.rs src/ui/agents_view.rs
git commit -m "Workflows: the journal keeps its phases, their agents and its mtime"
```

---

### Task 2: An agent's API error, completed calls and structured output

**Files:**
- Modify: `src/agents.rs` — `Agent` struct (`:60-130`), `Agent::new`, `push` (`:235`)
- Test: `mod tests` in `src/agents.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct ApiError { pub status: Option<u16>, pub token: Option<String> } // `apiErrorStatus`, `error` (a machine token)
  // Agent gains:
  pub api_error: Option<ApiError>,          // the last API-error line; cleared by a later real response
  pub completed_calls_before_error: usize,  // api_calls when api_error was set
  ```
  `Agent.tools_by_name` already records `StructuredOutput`; Task 5 reads `tools_by_name.contains_key("StructuredOutput")`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_429_on_the_first_call_is_recorded_without_its_text() {
    let err = Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","message":{"id":"e","model":"<synthetic>","content":[{"type":"text","text":"API Error: secret words"}],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429}"#).unwrap();
    let a = Agent::from_lines("a1", Meta::default(), [&err]);
    assert_eq!(a.api_error, Some(ApiError { status: Some(429), token: Some("rate_limit".into()) }));
    assert_eq!(a.completed_calls_before_error, 0);
    assert_eq!(a.api_calls, 0);
    assert!(!format!("{a:?}").contains("secret"));
}

#[test]
fn a_real_response_after_an_error_clears_it() {
    let err = Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","message":{"id":"e","model":"<synthetic>","content":[],"usage":{"input_tokens":0}},"isApiErrorMessage":true,"error":"overloaded","apiErrorStatus":529}"#).unwrap();
    let ok = Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"m1","model":"claude-haiku-4-5-20251001","content":[{"type":"text","text":"x"}],"stop_reason":"end_turn","usage":{"input_tokens":5,"output_tokens":1}}}"#).unwrap();
    let a = Agent::from_lines("a1", Meta::default(), [&err, &ok]);
    assert_eq!(a.api_error, None);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test agents::tests::a_429` — Expected: compile error, no field `api_error`.

- [ ] **Step 3: Implement**

Replace `Line::Assistant(a) if a.is_api_error() => {}` with:

```rust
Line::Assistant(a) if a.is_api_error() => {
    // `is_api_error` is also true for any `<synthetic>` line; only a real
    // API error (the flag, a status or a token) is recorded.
    if a.is_api_error_message || a.api_error_status.is_some() || a.error.is_some() {
        self.api_error = Some(ApiError { status: a.api_error_status, token: a.error.clone() });
        self.completed_calls_before_error = self.api_calls;
    }
}
```

Add a third test, `a_synthetic_line_without_an_error_is_not_an_api_error`: a `<synthetic>`-model assistant line with no `isApiErrorMessage`, status or token leaves `api_error == None`.

and at the top of the `Line::Assistant(a) =>` (real response) arm, `self.api_error = None;`. Initialise both fields in `Agent::new`. `a.error` is the enum-like token (`rate_limit`); the message text lives in `message.content` and is not touched.

- [ ] **Step 4: Run tests** — `cargo test agents::` — PASS.

- [ ] **Step 5: Commit**

```bash
git add src/agents.rs
git commit -m "Workflows: an agent keeps its last API error's status and token"
```

---

### Task 3: The pointer scan

**Files:**
- Create: `src/workflow_script.rs`; register `pub mod workflow_script;` in `src/lib.rs` (alphabetical, after `ui`)
- Test: in-file `mod tests`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
  #[serde(rename_all = "snake_case")]
  pub enum Call { Parallel, Pipeline, PhaseMarker }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
  pub struct Pointer { pub line: u32, pub call: Call }   // 1-based line
  pub fn pointer(script: &str, phase: &str, label_prefixes: &[String]) -> Option<Pointer>;
  ```

Algorithm (spec §4.4), lexical, one pass building a token list of `(kind, text_if_needed, line, depth)`:

1. Tokenise: identifiers, `(`, `)`, `{`, `}`, `,`, `:`; string literals `'…'`/`"…"`; template literals `` `…` `` (with `${…}` nesting); `//` and `/* */` comments. Newlines inside skipped literals still advance the line counter. Keep a literal's text only when it is ≤ 64 chars and directly follows `phase` `:` or is the first argument of `phase(`, or is a `label` value (template head up to the first `${` or the literal).
2. For each identifier `agent` followed by `(`: find its matching `)`; inside it, look for `phase` `:` `<lit == phase>` and `label` `:` `<lit starting with "<prefix>:" for some prefix>`. Record the `agent` token's depth-stack of enclosing call identifiers.
3. Candidates = matching `agent(` calls. For each, walk its enclosing calls outward; the first `parallel` or `pipeline` gives `(line of that identifier, kind)`.
4. Exactly one distinct `(line, kind)` among candidates → return it. Zero or several → the line of `phase(` whose first argument equals `phase` → `PhaseMarker`; none → `None`.

- [ ] **Step 1: Write the failing tests**

```rust
const SCRIPT: &str = r#"
phase('Sweep')
const sweep = await parallel(READERS.map(r => () =>
  agent(r.prompt, { label: `sweep:${r.key}`, phase: 'Sweep' })))
phase('Verify')
const v = await pipeline(canon,
  c => agent(`Check:
this line says phase: 'Verify' and parallel( inside a prompt
${JSON.stringify(c)}`, { label: `verify:${c.id}`, phase: "Verify" }))
phase('Lonely')
"#;

#[test]
fn parallel_and_pipeline_are_found_by_phase_and_label_prefix() {
    assert_eq!(pointer(SCRIPT, "Sweep", &["sweep".into()]), Some(Pointer { line: 3, call: Call::Parallel }));
    assert_eq!(pointer(SCRIPT, "Verify", &["verify".into()]), Some(Pointer { line: 6, call: Call::Pipeline }));
}

#[test]
fn text_inside_a_prompt_is_never_a_match() {
    // Review Focus 2: the prompt's `phase: 'Verify'` / `parallel(` must not count.
    let only_prompt = "const x = `phase: 'Verify' parallel( agent(`\nphase('Verify')\n";
    assert_eq!(pointer(only_prompt, "Verify", &["verify".into()]), Some(Pointer { line: 2, call: Call::PhaseMarker }));
}

#[test]
fn an_unmatched_phase_falls_back_to_its_marker_or_nothing() {
    // Line 1 is the empty line after `r#"`; the template literal spans lines 7–9.
    assert_eq!(pointer(SCRIPT, "Lonely", &["x".into()]), Some(Pointer { line: 10, call: Call::PhaseMarker }));
    assert_eq!(pointer(SCRIPT, "Absent", &["x".into()]), None);
}

#[test]
fn nested_parallel_inside_a_pipeline_stage_is_the_nearest() {
    let s = "pipeline(xs,\n x => parallel(L.map(l => () =>\n  agent(p, {label: `judge:${l}`, phase: 'Design'}))))\n";
    assert_eq!(pointer(s, "Design", &["judge".into()]), Some(Pointer { line: 2, call: Call::Parallel }));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test workflow_script` — compile error (module missing).

- [ ] **Step 3: Implement** the tokenizer and matcher in `src/workflow_script.rs` per the algorithm above. The function returns only `Pointer`; no `String` from the script leaves it (the kept literals live in a local `Vec` dropped on return). Module doc comment states this.

- [ ] **Step 4: Run tests** — `cargo test workflow_script` — PASS. Task 9's fixture test re-checks the scan on a real (anonymised) script.

- [ ] **Step 5: Commit**

```bash
git add src/workflow_script.rs src/lib.rs
git commit -m "Workflows: find a phase's parallel() or pipeline() call in the script"
```

---

### Task 4: The run record in `State`

**Files:**
- Create: `src/workflow_runs.rs` (record half); `pub mod workflow_runs;` in `src/lib.rs`
- Modify: `src/ui/state.rs:373` (add `workflow_records`), `src/attach.rs:207-218`, `src/load.rs:128`

**Interfaces:**
- Consumes: `workflow_script::{pointer, Call, Pointer}`; `agents::WorkflowJournal` (Task 1).
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub struct WorkflowRecord {
      pub run: String, pub name: Option<String>, pub status: Option<String>, // "completed" | "killed" | …
      pub start_ms: Option<i64>, pub duration_ms: Option<u64>, pub script_path: Option<String>,
      pub pointers: std::collections::BTreeMap<String, Pointer>,             // phase title → pointer
      pub mtime_ms: Option<i64>,                                             // cache key
      pub scanned_with: Vec<(String, Vec<String>)>,                          // cache key: (phase, label prefixes) the pointers were computed from
  }
  /// `<session dir>/workflows/wf_*.json`; `journals` supplies each phase's label prefixes;
  /// `prev` is the last read, reused per file while its mtime and the prefixes are unchanged.
  pub fn read_records(session_dir: &std::path::Path, journals: &[WorkflowJournal], prev: &[WorkflowRecord]) -> Vec<WorkflowRecord>;
  // State gains: pub workflow_records: Vec<crate::workflow_runs::WorkflowRecord>,
  ```

- [ ] **Step 1: Failing test** (in `workflow_runs.rs`)

```rust
#[test]
fn a_record_keeps_identifiers_and_pointers_only() {
    let dir = tempfile::tempdir().unwrap();
    let wf = dir.path().join("workflows");
    std::fs::create_dir_all(&wf).unwrap();
    std::fs::write(wf.join("wf_t.json"), serde_json::json!({
        "runId": "wf_t", "workflowName": "sweep", "status": "completed", "startTime": 1000,
        "durationMs": 50, "scriptPath": "/x/sweep-2.js",
        "script": "phase('Verify')\nconst v = await pipeline(xs,\n c => agent(p, {label: `verify:${c}`, phase: 'Verify'}))\n",
        "result": "PROSE", "logs": ["PROSE"], "summary": "PROSE", "args": {"q": "PROSE"},
        "phases": [{"title": "Verify", "detail": "PROSE"}]
    }).to_string()).unwrap();
    let j = crate::agents::WorkflowJournal { run: "wf_t".into(), phases: vec![crate::agents::JournalPhase {
        title: "Verify".into(), label_prefixes: vec!["verify".into()], ..Default::default() }], ..Default::default() };
    let r = &read_records(dir.path(), &[j], &[])[0];
    assert_eq!(r.name.as_deref(), Some("sweep"));
    assert_eq!(r.pointers["Verify"], Pointer { line: 2, call: Call::Pipeline });
    assert!(!format!("{r:?}").contains("PROSE"));
}
```

- [ ] **Step 2: Run** — `cargo test workflow_runs` — FAIL (module missing).

- [ ] **Step 3: Implement.** Deserialise into a private struct with only `run_id`, `workflow_name`, `status`, `start_time`, `duration_ms`, `script_path`, `script` (`#[serde(rename_all = "camelCase")]`, no `deny_unknown_fields`, so skipped fields are never materialised). Compute pointers, drop `script`. Skip unreadable/invalid files and anything that is not `wf_*.json` (the `scripts/` directory lives beside the records). The tick re-reads records every 5 s: keep the previous `Vec<WorkflowRecord>` and reuse an entry whose file mtime and journals' label prefixes are unchanged, so a script is tokenised once per change, not once per tick (`mtime_ms` and `scanned_with` are the cache key; neither is rendered or serialised). Add a second test, `a_record_is_rescanned_only_when_it_changes`: read once; set a sentinel pointer (`line: 999`) on the result and pass it back as `prev` with the file untouched — the sentinel survives (reused); then pass a journal whose phase has a new label prefix — the sentinel is gone (re-scanned). In `attach.rs`'s tick hook, after the journals line: `state.workflow_records = crate::workflow_runs::read_records(&session_dir, &state.workflow_journals, &state.workflow_records);` where `session_dir` is the `subagents_dir`'s parent (derive it the same way `subagents_dir` is derived there). Same in `load.rs` with `transcript.with_extension("")` and `&[]`.

- [ ] **Step 4: Run** — `cargo test` — PASS (no snapshot changes: nothing renders records yet).

- [ ] **Step 5: Commit**

```bash
git add src/workflow_runs.rs src/lib.rs src/ui/state.rs src/attach.rs src/load.rs
git commit -m "Workflows: read the run record — name, status, times and each phase's pointer"
```

---

### Task 5: Causes, fixes and the run verdict

**Files:**
- Modify: `src/workflow_runs.rs` (derivation half), `src/agent_ledger.rs:133-210` (`WorkflowGroup`, `workflow_groups`), `src/tools.rs:311,393` (`WorkflowLaunch`), `src/transcript/tool_result.rs:23,168` (`ToolUseDetail::Workflow` gains `name`), `src/ui/state.rs:1563` (`note_task_notification` reads the struct and records `workflow_notified_at`), `:2300` (the existing `workflow_notifications_are_keyed_by_run` test), `src/harness_facts.rs`
- Test: `src/workflow_runs.rs` `mod tests`; existing `agent_ledger` and `ui::state` tests updated for the new fields and the launch struct

**Interfaces:**
- Consumes: Tasks 1, 2, 4; `agent_ledger::AgentRow` (`cost`, `waste`, `cold_start`, `workflow`, `id`); `State.agg.calls` (`CallRecord { at_ms, model, usage }`); `State.cost.pricing().estimate(&usage, &model)`; `State.workflow_notifications`; `State.now_ms`.
- Produces:
  ```rust
  // src/tools.rs — replaces `Vec<(String, Option<String>, Option<String>)>`; one entry per launch, so a resume adds one
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct WorkflowLaunch { pub tool_use_id: String, pub task_id: Option<String>, pub run_id: Option<String>,
                              pub name: Option<String> /* toolUseResult.workflowName */, pub at_ms: Option<i64> }
  // src/ui/state.rs — State gains, set in note_task_notification from the notification line's timestamp:
  pub workflow_notified_at: std::collections::BTreeMap<String, i64>,   // run → latest notification's line time
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
  #[serde(rename_all = "snake_case")]
  pub enum Cause { RateLimitFirst, RateLimitMid, Overloaded, ContextOverflow, NoStructuredOutput, Killed, Unknown }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
  #[serde(rename_all = "snake_case")]
  pub enum RunState { Live, Stalled, Completed, Killed, Failed }   // Failed: the notification said the run itself failed
  #[derive(Debug, Clone, PartialEq, serde::Serialize)]
  pub struct PhaseRow { pub title: String, pub started: usize, pub results: usize, pub failed: usize,
      pub failed_usd: f64, pub waste_pct: Option<f64>, pub causes: std::collections::BTreeMap<Cause, usize>,
      pub pointer: Option<Pointer>, pub pointer_stale: bool }
  #[derive(Debug, Clone, PartialEq, serde::Serialize)]
  pub struct Fix { pub cause: Cause, pub agents: usize, pub text: String, pub phase: String,
      pub pointer: Option<Pointer>, pub pointer_stale: bool, pub script_path: Option<String> }
  #[derive(Debug, Clone, PartialEq, serde::Serialize)]
  pub struct RunVerdict { pub name: Option<String>, pub state: RunState, pub phases: Vec<PhaseRow>,
      pub failed: usize, pub failed_usd: f64, pub waste_pct: Option<f64>, pub overhead: Option<f64>,
      pub cold_start_pct: Option<f64>, pub started_ms: Option<i64>, pub ended_ms: Option<i64>, pub fixes: Vec<Fix> }
  pub fn cause(a: Option<&crate::agents::Agent>, phase_used_schema: bool, killed: bool) -> Cause;
  pub fn fix_text(cause: Cause, n: usize, token: Option<&str>) -> String;   // `None` for Killed callers never reach; Unknown's token or `no error`
  pub fn verdict(state: &crate::ui::State, rows: &[crate::agent_ledger::AgentRow], run: &str) -> RunVerdict;
  pub const LIVE_MS: i64 = 60_000;
  pub const RESUME_SLACK_MS: i64 = 10_000;   // the journal's last write may land just after the notification
  pub const STALLED_DROP_MS: i64 = 30 * 60_000;
  // WorkflowGroup gains: pub verdict: RunVerdict
  ```

Rules:
- Which agents get a cause: the journal's `failed` ids, and — when the run is `Killed` — every started id of the run with neither a `result` nor a `failed` entry (the phase's `agent_ids` minus those). These are the agents counted in `PhaseRow.causes`; `PhaseRow.failed` stays the journal's count.
- `cause`, tested **in this order** (Review Focus 7):
  1. an API error: `status == Some(429) || token == Some("rate_limit")` → `RateLimitFirst` if `completed_calls_before_error == 0` else `RateLimitMid`; `529` or `"overloaded"`/`"overloaded_error"` → `Overloaded`; token in `harness_facts::workflow_run::PROMPT_TOO_LONG_TOKENS` → `ContextOverflow`; any other token → `Unknown`;
  2. `killed` → `Killed`;
  3. `phase_used_schema` and no `StructuredOutput` in `tools_by_name` → `NoStructuredOutput`;
  4. else (including `a == None`) → `Unknown`.
- `phase_used_schema`: any agent of the phase has `StructuredOutput` in `tools_by_name`.
- Use `state.clock_ms()` for "now" throughout, as `agent_ledger::row` does (not `now_ms`).
- `line_activity` = max(members' `last_line_at`) — line-time evidence. `last_activity` = max(`line_activity`, journal `mtime_ms` capped at `clock_ms()`). Journal lines carry no timestamps and a copied or checked-out session has a fresh mtime, so the mtime may make a marker-less run `Live`/`Stalled` but never supersedes a marker.
- `state` (spec §4.1, Review Focus 6): the *terminal marker* is the later of `workflow_notified_at[run]` (status from the notification) and the record's `start_ms + duration_ms` (status from the record's `status`, when `completed` or `killed`). It **holds** unless a `WorkflowLaunch` of the run has `at_ms` after it, or `line_activity > marker + RESUME_SLACK_MS`. Holding → `Killed` / `Failed` / `Completed` by the notification's status (else the record's `status`). The notification and its time are looked up with the same lone-notification fallback `workflow_groups` uses for `empty_result` (`agent_ledger.rs:165-170`): `workflow_notified_at` is keyed like `workflow_notifications` (run id, else task id). Not holding (or none) → `Live` if `now − last_activity < LIVE_MS`, else `Stalled`.
- `started_ms` = min(first `WorkflowLaunch.at_ms` of the run, record `start_ms`, members' `started_at`) — the record alone covers the last invocation only. `ended_ms` = the holding marker's time; else `None` while `Live` (the window runs to now); else `last_activity`.
- `pointer_stale` (spec §4.4): the phase has failed agents and none of them has `started_at >= record.start_ms`. A `Fix` copies its phase's flag.
- `overhead`: `run $ ÷ Σ estimate(call) for call in state.agg.calls with at_ms in [started_ms, ended_ms or now]`; `None` when that sum < 0.01 (Review Focus 4).
- `waste_pct` = Σ member `waste.usd` ÷ run $ (`None` when run $ is 0); `cold_start_pct` = Σ first-call cache-write $ of members with `cold_start` ÷ run $ (reuse the helper behind `Totals.cold_start_usd`).
- Fix text, exactly (spec §4.3). `{n agents}` is `agents_n(n)` = `"1 agent"` / `"{n} agents"`, and `{n agents'}` is `"1 agent's"` / `"{n} agents'"` — one helper in `workflow_runs.rs`, never an `(s)`:
  - `RateLimitFirst`: `"{n agents} hit the rate limit on their first call — batch this phase's items, or lower its effort/model"`
  - `RateLimitMid`: `"{n agents} hit the rate limit mid-task — batch this phase, then resume the run after the window resets"`
  - `Overloaded`: `"{n agents} met an overloaded API — transient; resume the run, finished agents are cached"`
  - `ContextOverflow`: `"{n agents'} input was too large — pass paths, not contents"`
  - `NoStructuredOutput`: `"{n agents} never satisfied the schema — loosen it or split the task"`
  - `Unknown`: `"{n agents} failed ({token}) — cctop has no fix for this yet"` (`{token}` is the api error token; `no transcript` when the agent has none; else `no error`; with several tokens among the cause's agents, the most frequent)
  - `Killed`: no fix.
  `fixes` = the two causes (excluding `Killed`) with the most agents over the run, each with the phase holding most of them and that phase's pointer.

- [ ] **Step 1: Test support, then the failing tests.** Add to `src/workflow_runs.rs` (Tasks 7 and 8 reuse it):

```rust
#[cfg(test)]
pub mod test_support {
    use crate::agents::{Agent, JournalPhase, Meta, WorkflowJournal};
    use crate::metrics::cost::parse_ts_ms;
    use crate::transcript::Line;
    use crate::ui::State;
    use crate::workflow_script::{Call, Pointer};

    pub const NOW: &str = "2026-01-01T00:10:00Z";

    pub fn err(ts: &str, status: u16, token: &str) -> Line {
        Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"e{ts}","model":"<synthetic>","content":[],"usage":{{"input_tokens":0}}}},"isApiErrorMessage":true,"error":"{token}","apiErrorStatus":{status}}}"#)).unwrap()
    }
    /// A priced haiku response; `tool` adds a tool_use block of that name.
    pub fn ok(id: &str, ts: &str, tool: Option<&str>) -> Line {
        let block = match tool {
            Some(t) => format!(r#"{{"type":"tool_use","id":"tu{id}","name":"{t}","input":{{}}}}"#),
            None => r#"{"type":"text","text":"x"}"#.to_string(),
        };
        Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-haiku-4-5-20251001","content":[{block}],"stop_reason":"end_turn","usage":{{"input_tokens":1000,"cache_creation_input_tokens":20000,"output_tokens":200}}}}}}"#)).unwrap()
    }
    pub fn agent(id: &str, run: &str, lines: &[Line]) -> Agent {
        let mut a = Agent::from_lines(id, Meta::default(), lines.iter());
        a.workflow = Some(run.to_string());
        a
    }
    /// `(phase, [(agent id, "ok" | "failed" | "open")])`.
    pub fn journal(run: &str, phases: &[(&str, &[(&str, &str)])], mtime: &str) -> WorkflowJournal {
        let mut j = WorkflowJournal { run: run.into(), launched: 1, mtime_ms: parse_ts_ms(mtime), ..Default::default() };
        for (title, ids) in phases {
            let mut p = JournalPhase { title: title.to_string(), label_prefixes: vec![title.to_lowercase()], ..Default::default() };
            for (id, what) in *ids {
                p.started += 1; j.started += 1;
                p.agent_ids.push(id.to_string());
                match *what { "ok" => { p.results += 1; j.results += 1; p.result_ids.push(id.to_string()) } "failed" => { p.failed += 1; j.failed += 1; j.failed_ids.push(id.to_string()) } _ => {} }
            }
            j.phases.push(p);
        }
        j
    }
    pub fn state(agents: Vec<Agent>, journals: Vec<WorkflowJournal>) -> State {
        let mut s = State::new(crate::metrics::Pricing::bundled());
        s.now_ms = parse_ts_ms(NOW).unwrap();
        s.clock_override = true;
        for a in agents { s.agents.insert(a.id.clone(), a); }
        s.workflow_journals = journals;
        s
    }
    pub fn launch(s: &mut State, run: &str, ts: &str) {
        s.tools.workflow_launches.push(crate::tools::WorkflowLaunch {
            tool_use_id: format!("tu{ts}"), task_id: None, run_id: Some(run.into()),
            name: Some("sweep".into()), at_ms: parse_ts_ms(ts),
        });
    }
    pub fn record(run: &str, status: &str) -> super::WorkflowRecord {
        super::WorkflowRecord {
            run: run.into(), name: Some("sweep".into()), status: Some(status.into()),
            start_ms: parse_ts_ms("2026-01-01T00:00:00Z"), duration_ms: Some(300_000),
            script_path: Some("/p/.claude/workflows/sweep.js".into()),
            pointers: [("Verify".to_string(), Pointer { line: 6, call: Call::Pipeline })].into_iter().collect(),
            mtime_ms: None, scanned_with: vec![],
        }
    }
    /// Verify: v1–v3 429 on the first call, v4 returned through StructuredOutput;
    /// Design: d1 429 after one call. Completed, with a record.
    pub fn state_with_run() -> State {
        let t = "2026-01-01T00:01:00Z";
        let agents = vec![
            agent("v1", "wf_t", &[err(t, 429, "rate_limit")]),
            agent("v2", "wf_t", &[err(t, 429, "rate_limit")]),
            agent("v3", "wf_t", &[err(t, 429, "rate_limit")]),
            agent("v4", "wf_t", &[ok("m4", t, Some("StructuredOutput"))]),
            agent("d1", "wf_t", &[ok("m5", t, None), err("2026-01-01T00:02:00Z", 429, "rate_limit")]),
        ];
        let j = journal("wf_t", &[
            ("Verify", &[("v1", "failed"), ("v2", "failed"), ("v3", "failed"), ("v4", "ok")]),
            ("Design", &[("d1", "failed")]),
        ], "2026-01-01T00:05:00Z");
        let mut s = state(agents, vec![j]);
        s.workflow_records = vec![record("wf_t", "completed")];
        s
    }
}
```

Tests (in `mod tests`, with `use super::*; use super::test_support::*; use crate::metrics::cost::parse_ts_ms; use crate::ui::State;` — the support module's own `use`s are not re-exported):

```rust
fn v(s: &State) -> RunVerdict {
    let rows = crate::agent_ledger::rows(s, crate::agent_ledger::Sort::Waste, false);
    verdict(s, &rows, "wf_t")
}

#[test]
fn first_call_and_mid_task_429s_are_told_apart() {
    let s = state_with_run();
    assert_eq!(cause(s.agents.get("v1"), true, false), Cause::RateLimitFirst);
    assert_eq!(cause(s.agents.get("d1"), false, false), Cause::RateLimitMid);
}

#[test]
fn a_529_is_overloaded() {
    let a = agent("o", "wf_t", &[err("2026-01-01T00:01:00Z", 529, "overloaded")]);
    assert_eq!(cause(Some(&a), false, false), Cause::Overloaded);
}

#[test]
fn no_structured_output_needs_a_sibling_that_had_one() {
    let a = agent("n", "wf_t", &[ok("mn", "2026-01-01T00:01:00Z", None)]);
    assert_eq!(cause(Some(&a), false, false), Cause::Unknown);
    assert_eq!(cause(Some(&a), true, false), Cause::NoStructuredOutput);
}

#[test]
fn a_journal_id_without_a_transcript_is_unknown_and_free() {
    // Review Focus 5
    let j = journal("wf_t", &[("Verify", &[("ghost", "failed")])], "2026-01-01T00:05:00Z");
    let s = state(vec![], vec![j]);
    let r = v(&s);
    assert_eq!(r.phases[0].causes.get(&Cause::Unknown), Some(&1));
    assert_eq!(r.failed, 1);
    assert_eq!(r.failed_usd, 0.0);
}

#[test]
fn overhead_is_none_when_the_main_thread_spent_nothing() {
    // Review Focus 4: state_with_run has no main-transcript calls
    assert_eq!(v(&state_with_run()).overhead, None);
}

#[test]
fn a_live_run_without_a_record_has_no_pointer() {
    // Review Focus 1
    let mut s = state_with_run();
    s.workflow_records.clear();
    s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
    let r = v(&s);
    assert_eq!(r.state, RunState::Live);
    assert!(r.fixes.iter().all(|f| f.pointer.is_none()));
}

#[test]
fn a_quiet_run_without_an_end_is_stalled() {
    let mut s = state_with_run();
    s.workflow_records.clear();
    s.workflow_journals[0].mtime_ms = Some(s.now_ms - 120_000);
    assert_eq!(v(&s).state, RunState::Stalled);
}

#[test]
fn prompt_too_long_is_context_overflow_and_other_tokens_are_unknown() {
    let t = "2026-01-01T00:01:00Z";
    let a = agent("c", "wf_t", &[ok("mc", t, None), err("2026-01-01T00:02:00Z", 400, "prompt_too_long")]);
    assert_eq!(cause(Some(&a), true, true), Cause::ContextOverflow, "an API error wins over killed and schema");
    let u = agent("u", "wf_t", &[err(t, 401, "authentication_failed")]);
    assert_eq!(cause(Some(&u), false, false), Cause::Unknown);
}

#[test]
fn an_agent_cut_off_by_a_kill_is_killed_not_schema() {
    // Review Focus 7: x is open (no result, no failed entry) in a schema phase when the run is killed.
    let t = "2026-01-01T00:01:00Z";
    let x = agent("x", "wf_t", &[ok("mx", t, None)]);
    assert_eq!(cause(Some(&x), true, true), Cause::Killed);
    let v4 = agent("v4", "wf_t", &[ok("m4", t, Some("StructuredOutput"))]);
    let j = journal("wf_t", &[("Verify", &[("v4", "ok"), ("x", "open")])], "2026-01-01T00:05:00Z");
    let mut s = state(vec![v4, x], vec![j]);
    s.workflow_records = vec![record("wf_t", "killed")];
    let r = v(&s);
    assert_eq!(r.state, RunState::Killed);
    assert_eq!(r.phases[0].causes.get(&Cause::Killed), Some(&1));
    assert!(r.fixes.is_empty(), "Killed has no fix line");
}

#[test]
fn a_resumed_run_is_live_again() {
    // Review Focus 6: the previous invocation's `completed` record ends at 00:05.
    let mut s = state_with_run();
    launch(&mut s, "wf_t", "2026-01-01T00:07:00Z");
    s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
    assert_eq!(v(&s).state, RunState::Live, "a later launch supersedes the record");
    let mut s = state_with_run();
    s.agents.insert("n1".into(), agent("n1", "wf_t", &[ok("mn1", "2026-01-01T00:09:55Z", None)]));
    assert_eq!(v(&s).state, RunState::Live, "a member line past the marker supersedes it too");
    let mut s = state_with_run();
    s.agents.insert("n1".into(), agent("n1", "wf_t", &[ok("mn1", "2026-01-01T00:05:05Z", None)]));
    assert_eq!(v(&s).state, RunState::Completed, "a line within the slack does not");
    let mut s = state_with_run();
    s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
    assert_eq!(v(&s).state, RunState::Completed, "a fresh journal mtime alone (a copied session) does not");
}

#[test]
fn a_pointer_is_stale_when_its_failures_predate_the_last_script() {
    // The Verify failures started at 00:01; the last invocation began at 00:04.
    let mut s = state_with_run();
    s.workflow_records[0].start_ms = parse_ts_ms("2026-01-01T00:04:00Z");
    s.workflow_records[0].duration_ms = Some(60_000);
    let r = v(&s);
    assert!(r.phases[0].pointer_stale);
    assert!(r.fixes[0].pointer_stale);
    assert!(!v(&state_with_run()).fixes[0].pointer_stale);
}

#[test]
fn the_window_starts_at_the_first_invocation() {
    // The record describes the last invocation only (00:04 + 60 s); the members started at 00:01.
    let mut s = state_with_run();
    s.workflow_records[0].start_ms = parse_ts_ms("2026-01-01T00:04:00Z");
    s.workflow_records[0].duration_ms = Some(60_000);
    let r = v(&s);
    assert_eq!(r.started_ms, parse_ts_ms("2026-01-01T00:01:00Z"));
    assert_eq!(r.ended_ms, parse_ts_ms("2026-01-01T00:05:00Z"));
}

#[test]
fn the_count_agrees_with_its_noun() {
    assert_eq!(fix_text(Cause::ContextOverflow, 1, None), "1 agent's input was too large — pass paths, not contents");
    assert_eq!(fix_text(Cause::ContextOverflow, 3, None), "3 agents' input was too large — pass paths, not contents");
    assert_eq!(fix_text(Cause::Unknown, 1, Some("authentication_failed")), "1 agent failed (authentication_failed) — cctop has no fix for this yet");
}

#[test]
fn fixes_are_the_top_two_causes_with_their_phase_pointer() {
    let r = v(&state_with_run());
    assert_eq!(r.state, RunState::Completed);
    assert_eq!(r.fixes.len(), 2);
    assert_eq!(r.fixes[0].cause, Cause::RateLimitFirst);
    assert_eq!(r.fixes[0].phase, "Verify");
    assert_eq!(r.fixes[0].pointer, Some(Pointer { line: 6, call: Call::Pipeline }));
    assert_eq!(r.fixes[0].text, "3 agents hit the rate limit on their first call — batch this phase's items, or lower its effort/model");
    assert_eq!(r.fixes[1].cause, Cause::RateLimitMid);
    assert_eq!(r.fixes[1].text, "1 agent hit the rate limit mid-task — batch this phase, then resume the run after the window resets", "singular");
    assert_eq!(r.fixes[1].pointer, None, "Design has no pointer in the record");
}

#[test]
fn waste_counts_the_failed_agents_money() {
    let r = v(&state_with_run());
    assert!(r.failed_usd > 0.0, "d1 made one priced call before its 429");
    assert!(r.waste_pct.unwrap() > 0.0 && r.waste_pct.unwrap() < 1.0);
}
```

- [ ] **Step 2: Run** — `cargo test workflow_runs` — FAIL.

- [ ] **Step 3: Implement** `cause`, `verdict`; in `agent_ledger::workflow_groups` set `verdict: crate::workflow_runs::verdict(state, rows, &run)` and `name` from the run's latest `WorkflowLaunch.name`, falling back to the record's name. Replace the launch tuple with `WorkflowLaunch` (`ToolUseDetail::Workflow` gains `name: s("workflowName")`; `at_ms` is the result line's `at`, already in scope at `tools.rs:393`); update `note_task_notification`'s `find` to the struct's fields and insert `workflow_notified_at` from the line's timestamp (pass it in from the `apply` call site at `:1411`); update the `workflow_notifications_are_keyed_by_run` expectation. Add `pub mod workflow_run` to `src/harness_facts.rs` with `PROMPT_TOO_LONG_TOKENS: &[&str] = &["prompt_too_long"]` (the token Claude Code writes on the main session, which the A47 rule already maps; not yet seen from a workflow agent — say so), `OBSERVED_ERROR_TOKENS: &[&str] = &["rate_limit"]`, and doc comments listing the journal and record fields read (spec §3.2).

- [ ] **Step 4: Run** — `cargo test` — PASS; existing snapshots unchanged.

- [ ] **Step 5: Commit**

```bash
git add src/workflow_runs.rs src/agent_ledger.rs src/tools.rs src/transcript/tool_result.rs src/ui/state.rs src/harness_facts.rs
git commit -m "Workflows: a cause per failed agent, the run's verdict and its fix lines"
```

---

### Task 6: Metrics

**Files:**
- Modify: `src/metrics/registry.rs` (after `agents_return_ratio`, `:143`), `docs/metrics.md`, `README.md` (generated)

- [ ] **Step 1: Add the entries**

```rust
metric!(workflow_failed, "Agents & MCP", "Workflow failed", "count", "Agents of a workflow run with a `failed` journal entry, by phase, each with one cause from its last API-error line (`apiErrorStatus`, `error` token) and its call count", ["D2a"], "A 429 on the first call costs ≈$0: read it with the count, not the dollars", ""),
metric!(workflow_failed_usd, "Agents & MCP", "Workflow failed $", "USD", "Σ `agent_cost` of the run's failed agents", ["D2a", "D9"], "", "≈ always"),
metric!(workflow_waste_pct, "Agents & MCP", "Workflow waste", "percent", "Σ `agent_waste` of the run's agents ÷ the run's priced cost", ["D2", "D2a", "D9"], "", "≈ always"),
metric!(workflow_overhead, "Agents & MCP", "Workflow overhead", "ratio", "The run's priced cost ÷ the main thread's priced responses between the run's start and its end (or now)", ["D2", "D2a", "D9"], "A ratio, not a counterfactual: the main thread would not necessarily have done the work cheaper; `—` under $0.01 of main spend", "≈ always"),
metric!(workflow_cold_start_pct, "Agents & MCP", "Workflow cold starts", "percent", "Cache-write $ of the first call of the run's cold-started agents ÷ the run's priced cost", ["D2a", "D9"], "A cost of the design, not waste", "≈ always"),
```

(Sources, from `registry.rs:180`: `D2` the main transcript, `D2a` subagent transcripts — the workflow journal and the run record live beside them under the session directory and count as `D2a` — `D9` the price table. `D1` is the session registry and is not used here.)

- [ ] **Step 2: Regenerate** — `cargo run -q -- metrics --md > docs/metrics.md && cargo run -q -- metrics --readme README.md`
- [ ] **Step 3: Run** — `cargo test registry` — PASS (staleness test).
- [ ] **Step 4: Commit** — `git add src/metrics/registry.rs docs/metrics.md README.md && git commit -m "Workflows: five metrics in the registry"`

---

### Task 7: `cctop query agents` and the dashboard strip

**Files:**
- Modify: `src/query.rs:405-420`, `src/dashboard.rs:1460` (in `body_tools`, body 6, before the agents row), `docs/query.md`
- Test: `src/query.rs` tests; `src/dashboard.rs` tests

**Interfaces:**
- Consumes: `WorkflowGroup.verdict` (Task 5).
- Produces (JSON, added to each `workflows[]` entry; existing keys unchanged):
  ```json
  { "name": "…", "state": "live|stalled|completed|killed",
    "failed_usd": <metric workflow_failed_usd>, "waste_pct": <metric>, "overhead": <metric>, "cold_start_pct": <metric>,
    "phases": [{ "title", "started", "results", "failed", "failed_usd", "waste_pct", "causes": {"rate_limit_first": 201}, "pointer": {"line": 144, "call": "parallel"}, "pointer_stale": true }],
    "fixes": [{ "cause", "agents", "text", "phase", "pointer", "script_path" }],
    "row": "<group row>", "detail": ["<row>", …], "detail_narrow": ["<row>", …] }
  ```
  Rendered text lives in `workflow_runs.rs`, so the TUI and the pane draw identical characters:
  - `group_text(&WorkflowGroup) -> String` — the agents view's group row: state glyph (`▶` live, `‖` stalled, `✓` completed, `✗` killed), run, name, `✗{failed}` when > 0, then today's `group_line` fields. Task 8 makes `agents_view::group_line` call it.
  - `detail_lines(&WorkflowGroup, width: usize) -> Vec<String>` — `DETAIL_WIDE = 116` and `DETAIL_NARROW = 52` are the only widths used: the rows inside a 120- and a 56-column frame (the pane's `innerWidth` is columns − 4, `frame.tsx:161`; the TUI's bordered `Block` leaves width − 2). `detail` is the wide layout, `detail_narrow` the narrow one; a surface uses the wide layout when its inner width is ≥ 116. No row carries trailing spaces (the pane's test `body()` right-trims, `tests/pane/render.ts:48`).
  - `strip_text(&WorkflowGroup) -> String` — the dashboard strip.

  Dashboard: a row `"  workflow "` (11 cells) + `strip_text(g)` for each group whose state is `Live` or (`Stalled` and `now − last activity < STALLED_DROP_MS`).

`strip_text` format (spec §4.5), fields in order of importance so a narrow clip drops the least useful: `"▸ {phase:10} {results}/{started} ✗{failed} {cause:8} ${usd} {overhead}×main  {name}"` where `phase` is the last phase in journal order with a start (`—` for the untitled phase), `cause` is the short form (`429×n`, `529×n`, `ctx×n`, `schema×n`, `kill×n`, `?×n`) of that phase's top cause, `overhead` is `—` when `None`, `name` is the run id when there is no name. Tone: `Crit` when `failed > 0`, `Dim` when `Stalled`.

`detail_lines` rows, in order: header (`run name state elapsed $ overhead×main cold n %`, clipped), column heads, one row per phase (`title:12 start:5 res:5 ✗:4 ✗$:7 waste:6 cause`; the narrow layout drops `✗$` and `waste`; the untitled phase shows `—`), a rule, then for each fix its `text` **word-wrapped** at `width` with a two-cell hanging indent (never clipped — Review Focus 8), and, when it has a pointer, `"  → {file name}:{line} {call}()"`, followed by `" · script changed since"` when `pointer_stale` (wrapped like the fix text).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn workflows_carry_the_verdict_and_old_keys_keep_their_meaning() {
    let state = crate::workflow_runs::test_support::state_with_run(); // Task 9 switches this to fixture W
    let v = agents(&state);
    let w = &v["workflows"][0];
    for k in ["run", "launched", "done", "failed", "empty_result", "agents", "cost", "waste"] { assert!(w.get(k).is_some(), "{k}"); }
    assert_eq!(w["phases"][0]["title"], "Verify");
    assert_eq!(w["phases"][0]["pointer"]["call"], "pipeline");
    assert!(w["detail"].as_array().unwrap().len() >= 4);
    assert!(w["detail_narrow"].as_array().unwrap().len() >= 4);
    assert!(w["row"].as_str().unwrap().contains("✗4"));
}
#[test]
fn fix_lines_wrap_at_56_and_keep_the_advice() {
    // Review Focus 8
    let s = crate::workflow_runs::test_support::state_with_run();
    let rows = crate::agent_ledger::rows(&s, crate::agent_ledger::Sort::Waste, false);
    let g = &crate::agent_ledger::workflow_groups(&s, &rows)[0];
    let lines = crate::workflow_runs::detail_lines(g, crate::workflow_runs::DETAIL_NARROW);
    assert!(lines.iter().all(|l| l.chars().count() <= 52 && !l.ends_with(' ')), "{lines:#?}");
    let flat = lines.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("batch this phase's items, or lower its effort/model"));
}
#[test]
fn the_strip_shows_only_while_a_run_is_live() {
    use crate::workflow_runs::test_support::*;
    let label = |s: &State| snapshot(s, &crate::advisor::Engine::for_state(s)).bodies.iter().flat_map(|b| &b.rows)
        .filter(|r| r.first().is_some_and(|seg| seg.text == "  workflow ")).count();
    let mut s = state_with_run();
    assert_eq!(label(&s), 0, "completed");
    s.workflow_records.clear();
    s.workflow_journals[0].mtime_ms = Some(s.now_ms - 5_000);
    assert_eq!(label(&s), 1, "live");
    assert_eq!(label(&state(vec![], vec![])), 0, "no run");
}
```

(`snapshot` is `src/dashboard.rs:270`; `Engine::for_state` is how `fixture_at` at `:1711` builds the engine; `Seg.text` is `:39`.)

`cctop_agents` in `src/mcp.rs:81` already returns `query::agents`, so the MCP tool needs no change.

- [ ] **Step 2: Run** — `cargo test query:: dashboard::` — FAIL.
- [ ] **Step 3: Implement** the JSON fields with `m(value, unit, metric_id, estimate)` as the existing `waste` does; `group_text`, `detail_lines` and `strip_text` in `workflow_runs.rs`; the dashboard row before the `agents` row using `dim("  workflow ")` + `seg(strip_text(g), tone)`. Document the fields in `docs/query.md` under `agents`.
- [ ] **Step 4: Run** — `cargo test` — PASS; `INSTA_UPDATE=no` shows no changed snapshots for fixtures A–D.
- [ ] **Step 5: Commit** — `git add src/query.rs src/dashboard.rs src/workflow_runs.rs docs/query.md && git commit -m "Workflows: the verdict in cctop query agents, and a dashboard strip while a run is live"`

---

### Task 8: The run detail in the agents view

**Files:**
- Modify: `src/ui/agents_view.rs` — `AgentsUi` (`:31`), `handle_key` (`:111`), `group_line` (`:221`), render (`:407`); tests (`:486`, `:662`). `synthetic_25_agents_scroll_sort_and_group` (`:657-690`) hard-asserts today's group row (`wf wf_abc-123`, `9 launched · 6 done · 2 failed`), `Enter` expanding to 26 entries and `▾ wf_abc-123`: rewrite those assertions for the new row and for `Enter` opening the detail (the member count moves into the detail) — this is a deliberate behaviour change, not a snapshot drift.

**Interfaces:**
- Consumes: `workflow_runs::detail_lines`, `Fix.script_path`, `Fix.pointer`, `ask::copy_to_clipboard(&str) -> Result<&'static str, String>`.
- Produces: `AgentsUi.detail: Option<String>` (the run shown); `AgentsUi.status: Option<String>` (the "copied" / "no pointer" line, cleared on the next key).

Behaviour: `group_line` becomes `workflow_runs::group_text(g)` (the `expanded` argument goes). `Enter` on `Entry::Group(g)` sets `detail = Some(g.run)` (replacing the expand toggle; `expanded` and the member rows under a group are removed — the detail lists the members below the fixes, one `agent_line` each). In the detail: `Esc` → `detail = None` (return `Handled::Yes` so App does not close the view); `o` → copy `"{script_path}:{line}"` of `fixes[0]`, `status = "copied …:144"` or `"no pointer for this run"`; `j/k` scroll. The group row's `✗{failed}` is drawn in `Crit` when > 0.

- [ ] **Step 1: Failing tests** — insta snapshots on the Task 5 synthetic run: `agents_workflow_detail_120x30`, `agents_workflow_detail_56x20`, and a key test:

```rust
#[test]
fn enter_opens_the_detail_and_esc_returns_to_the_list() {
    let mut s = crate::workflow_runs::test_support::state_with_run();
    s.agents_ui.selected = entries(&s, &s.agents_ui).iter().position(|e| matches!(e, Entry::Group(_))).unwrap();
    // `key` in this module takes a char (`:476`); build the special keys directly.
    let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
    assert_eq!(handle_key(k(KeyCode::Enter), &mut s), Handled::Yes);
    assert!(s.agents_ui.detail.is_some());
    assert_eq!(handle_key(k(KeyCode::Esc), &mut s), Handled::Yes);
    assert!(s.agents_ui.detail.is_none());
}
```

- [ ] **Step 2: Run** — `cargo test agents_view` — FAIL.
- [ ] **Step 3: Implement.** Render the detail with `detail_lines(g, if inner.width >= DETAIL_WIDE as u16 { DETAIL_WIDE } else { DETAIL_NARROW })` verbatim (`inner` = the bordered block's inner area), the rule and fix rows in `Warn`, the pointer rows in `Dim`, then members.
- [ ] **Step 4: Accept snapshots** — `INSTA_UPDATE=always cargo test agents_view`, strip `assertion_line:` from the new `.snap` files, delete `*.snap.new`; review that the `agents_synthetic_*` snapshots changed only in the group row. `cargo test` — PASS.
- [ ] **Step 5: Commit** — `git add src/ui/agents_view.rs src/ui/snapshots && git commit -m "Workflows: Enter on a run opens its phases, causes and fix; o copies the pointer"`

---

### Task 9: Fixture W

**Files:**
- Create: `scripts/compose-workflow-fixture.py`; `fixtures/session-w.jsonl`, `fixtures/session-w/subagents/workflows/<run>/…`, `fixtures/session-w/workflows/<run>.json`
- Modify: `fixtures/README.md` (a `session-w` entry and the rebuild command)

Script behaviour (`compose-workflow-fixture.py <session.jsonl> <run> <out-stem> [--mid N] [--ok N] [--ghosts N]`):
1. Main transcript: keep the `Workflow` tool_use(s), their `async_launched` results (one per invocation — keeps the resume launches), the workflow `<task-notification>`, and the assistant responses whose timestamps fall inside the run's window (for overhead); anonymise with `--max-str 2000`.
2. Agents: **every** first-call 429 agent (last line `apiErrorStatus == 429`, no completed call before it — ≤ 10 lines each, so copying all keeps the real per-phase majority), `--mid` mid-task 429 agents (default 6), `--ok` `result` agents (default 6), spread across phases; each through `anonymise-transcript.py`; meta files copied (`agentType`, `model` only).
3. Journal: every `launched`/`started`/`result` line, and the `failed` lines of copied agents plus `--ghosts` (default 2) failed ids chosen on purpose without a transcript (Review Focus 5); the `failed` lines of other uncopied agents are dropped, so their causes cannot turn into `Unknown` and outvote the real 429s. `label` → `<prefix>:<n>` (n = ordinal), `result` lines' value fields emptied (keep `type`, `agentId`, `key`).
4. Run record: keep `runId`, `workflowName` (**rewritten to `sweep-<n>`** from the real `cctop-research-sweep-<n>`; the launch `toolUseResult.workflowName` likewise), `status`, `startTime`, `durationMs`, `scriptPath` (rewritten to `/home/user/project/.claude/workflows/scripts/sweep-<n>.js`), and `script` with (a) every string/template literal's content replaced by same-newline-count filler **except** `phase:` values, `phase(` arguments and `label` template heads, (b) every comment's text replaced by same-length filler, and (c) identifiers in a denylist (`--rename word=replacement`, at least every identifier containing `cctop` or `research`, e.g. the bare object key on line 37) renamed — so line numbers and the matcher survive; every other key dropped.
5. Every transcript goes through `scripts/anonymise-transcript.py [--max-str N] <src> <dst>` (file arguments, not a pipe).

- [ ] **Step 1:** Write the script. Run: `scripts/compose-workflow-fixture.py ~/.claude/projects/-Users-tom-code-cctop/28c68a61-3744-474d-83a5-58141249e54f.jsonl wf_0aa065ff-0a0 fixtures/session-w`
- [ ] **Step 2:** Verify no prose: `grep -rIl "cctop\|Users/tom\|research" fixtures/session-w* ; echo $?` — Expected: `1` (no matches).
- [ ] **Step 3:** `cargo run -q -- query agents --session fixtures/session-w.jsonl | jq '.workflows[0] | {state, failed, phases: [.phases[] | {title, failed, causes, pointer}]}'` — Expected: `state` is `completed`; `Verify`'s top cause is `rate_limit_first`, `Unknown` is at most `--ghosts`; its pointer is `{"line": 144, "call": "parallel"}` with `pointer_stale: true` (the real run: Verify failed under script v2's `pipeline(` at 136, and v4 — the record's script — had already batched it). Verify by hand once against the real record before committing: `phase('Verify')` at 137, `parallel(` at 144.
- [ ] **Step 4:** Add insta snapshots `agents_w_120x30`, `agents_w_detail_120x30`, `agents_w_detail_56x20` on the fixture; accept as in Task 8. Add a sibling test `workflows_on_fixture_w` loading `fixtures/session-w.jsonl` the way `agent_ledger.rs:1026-1029` loads fixture C (`SessionInfo::from_fixture` + `load::state_from`), asserting `Verify`'s top cause is `rate_limit_first`, its pointer is line 144 `parallel` and `pointer_stale`, and the run is `Completed` whatever the fixture files' mtimes (they are checkout times). `cargo test` — PASS.
- [ ] **Step 5: Commit** — `git add scripts/compose-workflow-fixture.py fixtures/session-w* fixtures/README.md src && git commit -m "Workflows: fixture W, composed from a real run and anonymised"`

---

### Task 10: The pane

**Files:**
- Modify: `plugin/hooks/views/agents.tsx` (`renderAgents`, `:170`), `plugin/hooks/views/index.ts` (`ViewActions`, the `agents` case at `:32`), `tests/pane/fixture.ts` (`QUERY_FIXTURES`), `plugin/hooks/model.ts` (`openRun`, action `agents.run`), `plugin/hooks/pane.tsx` (the action wiring, beside `why` at `:284`), `scripts/pane-fixtures.sh` (add `fixtures/session-w.jsonl w` to the documented runs), `tests/pane/views.test.ts`, `tests/pane/reducer.test.ts`, `tests/pane/overview.test.ts`
- Create (generated): `tests/pane/fixtures/agents-w.json`, `dashboard-w.json`

Behaviour: the pane reads no hotkeys (`pane.tsx:597`, `docs/claude-code-panels.md` §5.5), so opening a run is a `Button`, the way the coach's `why` is (`views/coach.tsx:237` → `pane.tsx:284` → `coach.why`). For each `workflows[]` entry the agents view draws its `row` verbatim as a `Button` (`key` = the run id) whose press dispatches `{ type: 'agents.run', run }`; the reducer sets `openRun: string | null` (a pure field, `null` initially, reset to `null` on `session.id`). While `openRun` names a run in the data, the view draws a `Button` labelled `back` (dispatching `{ type: 'agents.run', run: null }`) and then that run's `detail` (≥ 120 columns) or `detail_narrow` verbatim, instead of the list; a run no longer in the data falls back to the list. `renderAgents(model, el, columns, now, actions?)` takes an optional `{ el, open(run: string | null) }`: `views/index.ts:32` passes it from `ViewActions` (add an `agents` member beside `coach`/`overview`, built in `pane.tsx` like `why`), so `$` stays in `pane.tsx`. Without actions (tests, the inline placement) the rows are plain `Text` and the detail still draws when `openRun` is set. The detail is `detail` when `innerWidth(columns) >= 116`, else `detail_narrow`. The strip reaches the pane through `dashboard.bodies[].rows` with no pane change.

- [ ] **Step 1: Regenerate fixtures** — `scripts/pane-fixtures.sh fixtures/session-w.jsonl w`
- [ ] **Step 2: Failing test** (`tests/pane/views.test.ts`)

```ts
test('agents-w: the run detail is the query rows, row for row', () => {
  const data = fixture<Record<string, unknown>>('agents-w');
  const wf = (data.workflows as Array<Record<string, unknown>>)[0];
  const model = reduce(build({ query: { agents: data } }), { type: 'agents.run', run: wf.run as string });
  const text = rows('agents', model, 120);
  const at = text.indexOf((wf.detail as string[])[0]);
  assert.ok(at >= 0, 'detail drawn');
  assert.deepEqual(text.slice(at, at + (wf.detail as string[]).length), wf.detail);
  const narrow = rows('agents', model, 56);
  const n = narrow.indexOf((wf.detail_narrow as string[])[0]);
  assert.deepEqual(narrow.slice(n, n + (wf.detail_narrow as string[]).length), wf.detail_narrow);
});
```

And in `tests/pane/reducer.test.ts`: `agents.run` sets and clears `openRun`; `session.id` resets it.

(`build`, `rows` and `fixture` are the helpers of `tests/pane/views.test.ts:30-58`, used by the `agents-c` test at `:239`; import `reduce` from `model.ts`. Add `'agents-w'` and `'dashboard-w'` to `QUERY_FIXTURES` in `tests/pane/fixture.ts:8`; `pane-fixtures.sh … w` writes all eight verbs, as it does for b–d — commit them all, as those runs do.)

- [ ] **Step 3: Run** — `npm test` — FAIL.
- [ ] **Step 4: Implement** in `agents.tsx`, `model.ts` and `pane.tsx`; `npm run typecheck && npm test` — PASS; `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin validate --strict ./plugin` — passes (no new `$` use outside `pane.tsx`).
- [ ] **Step 5: Commit** — `git add plugin/hooks tests/pane scripts/pane-fixtures.sh && git commit -m "Workflows: the pane draws the run detail from the query rows"`

---

### Task 11: Verification record and full check

**Files:**
- Modify: `docs/verification/pane.md`, `tasks/prd-cctop-workflows.md` (status line)

- [ ] **Step 1:** Add live checks to `docs/verification/pane.md`, each `Result: pending` (automation never marks them): (a) start a small workflow; the strip appears within 5 s and shows the live phase; (b) when it ends, the strip goes and the run detail shows `completed`; (c) note in the entry when `<session>/workflows/<run>.json` first appeared (answers spec §8 Q1); (d) whether a resumed run appends a second `launched` line (§8 Q2); (e) resume a completed run: the strip comes back within 5 s and the detail shows `live`, then `completed` again at its end; (f) in the pane at < 120 columns, open a run with its button and read a whole fix line.
- [ ] **Step 2:** `make check && npm test` — all green. `make check-contract` if `claude` is installed.
- [ ] **Step 3:** Set the PRD status to `v1.1 — implemented on <branch>, live checks pending`.
- [ ] **Step 4: Commit** — `git add docs/verification/pane.md tasks/prd-cctop-workflows.md && git commit -m "Workflows: live checks recorded as pending; PRD status"`
