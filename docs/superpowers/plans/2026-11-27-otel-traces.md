# OTel Traces Implementation Plan (PRD 7)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The loopback OTLP receiver accepts `/v1/traces` (JSON), keeps allowlisted attributes of seven span kinds, and cctop shows who decided each permission, request retries, and hook overhead per turn; `cctop install --otel` explains how to turn traces on.

**Architecture:** `otel.rs` gains `ingest_traces` and typed span records in a bounded per-session ring; `State::apply_otel` joins them (permission → tool via `parent_span_id` → `tool_use_id`; request → assistant via `gen_ai.response.id` = `requestId`); surfaces read state. Live in `cctop run --otlp` only, like today's OTel data.

**Tech Stack:** Rust (serde_json, insta); Python 3 stdlib for a one-off capture.

**Spec:** `tasks/prd-cctop-otel-traces.md` (issue #31).

## Global Constraints

- Branch `otel-traces`, cut from `pane-apis` after PRD 6. Commit subjects `OTel: …`, `Docs: …`, trailer in the body.
- Kept span names: `claude_code.interaction`, `claude_code.llm_request`, `claude_code.tool`, `claude_code.tool.blocked_on_user`, `claude_code.tool.execution`, `claude_code.hook`, `claude_code.compaction`. Others are counted, not kept.
- Attribute allowlist = the attributes PRD §1 names for those spans, **minus** `hook_definitions` and any key matching `user_prompt*`, `*.content`, `tool_input*` — dropped even when present.
- Ring: last 2000 spans per session. Body cap 4 MB → 413. Protobuf → the same 415 hint as metrics/logs. `/v1/other` → 404 unchanged.
- Unknown `source` values kept verbatim, displayed as `other`.
- FR-5: without traces every surface is exactly as today (no snapshot moves).
- `cctop run` is never run in the foreground of a tool call; the capture uses a throwaway stdlib receiver and `claude -p --max-turns 1`.
- Insta, generated docs, `make site` per CLAUDE.md. Done = `make check` and `npm test` green.

## Rulings made while planning

- **Live capture now, headless:** PRD §8 asks for a captured trace before relying on §1. Task 1 captures one with a stdlib Python OTLP/JSON receiver on a free loopback port and one `claude -p --max-turns 1` run (one model call). Raw bodies stay in the session scratchpad (never committed); tests use synthetic bodies shaped like them with neutral values. Item 44 still goes in `docs/verification/pane.md` as `Result: pending` for the person's interactive confirmation (permission spans need a real permission prompt, which `-p` won't show). `--dump-otel` is added to `cctop run` as the PRD asks.

## Review Focus

1. A `blocked_on_user` span whose parent `claude_code.tool` span arrives in a **later** export batch: the join resolves when the parent arrives (Task 3 test).
2. Spans for a session cctop isn't attached to: stored under their own session id, never mixed in (Task 2 test).
3. An `llm_request` with zero `gen_ai.request.attempt` events: `attempts = 1`, not 0; no retry row (Task 4 test).
4. Body exactly at 4 MB vs 4 MB + 1: accepted vs 413 (Task 2 test).
5. A hook span with `num_hooks: 0` or `duration_ms` missing: counted as 0 ms, never NaN (Task 5 test).

---

### Task 1: Capture one live trace; `--dump-otel` (PRD §8)

**Files:**
- Modify: `src/main.rs` / `src/app.rs` (flag `--dump-otel <dir>` on `cctop run`), `src/otel.rs` (write raw bodies when set), `docs/verification/pane.md`
- Scratch (not committed): the capture script and bodies under the session scratchpad the dispatch names

**Interfaces:**
- Produces: `otel::serve(addr, dump: Option<PathBuf>)` (or a setter on the shared store) writing each raw body to `<dir>/<unix-ms>-<path-tail>.json`; a short written summary in the task report of observed span names, attribute keys per span (keys only), `source` values seen.

- [ ] **Step 1:** Write a stdlib Python receiver (scratchpad) that accepts POST `/v1/traces`, `/v1/metrics`, `/v1/logs` and saves bodies. Run it on a free 127.0.0.1 port in the background; run `CLAUDE_CODE_ENABLE_TELEMETRY=1 CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1 OTEL_TRACES_EXPORTER=otlp OTEL_EXPORTER_OTLP_PROTOCOL=http/json OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<port> claude -p --max-turns 2 "run ls in the current directory, then reply ok" </dev/null` in a temp dir (so a tool span appears); stop the receiver. Record span names and attribute keys (keys only) in the report. If no traces arrive, record that and continue from PRD §1.
- [ ] **Step 2: Failing test** in `otel.rs`: with a dump dir set, posting a body writes one file with that body.
- [ ] **Step 3: Implement** `--dump-otel`. **Step 4:** `make check` green.
- [ ] **Step 5:** `docs/verification/pane.md` §B item 44: *"A session with traces on: span names, `blocked_on_user` `source` values, and whether `mcp.rpc` / `bash.subprocess` appear"* — Setup: the env from `cctop install --otel` (after Task 6) and `cctop run --otlp --dump-otel /tmp/otel` in a real terminal; accept one permission prompt. Expected: `blocked_on_user` spans with `source` `user`. Include what the headless capture showed. `Result: pending`.
- [ ] **Step 6: Commit** — `OTel: --dump-otel, and what a headless 2.1.332 trace contains`.

### Task 2: `/v1/traces` in the receiver (US-001)

**Files:**
- Modify: `src/otel.rs` (`handle` route, `ingest_traces`, span types, ring, body cap)

**Interfaces:**
- Produces: `pub struct SpanIds { pub trace_id: String, pub span_id: String, pub parent_span_id: Option<String>, pub start_ns: u64, pub end_ns: u64 }`; `pub enum Span { Interaction { ids, sequence: Option<u64> }, LlmRequest { ids, model: Option<String>, query_source: Option<String>, response_id: Option<String>, attempts: u32, attempt_times_ns: Vec<u64>, ttft_ms: Option<u64>, first_content_ms: Option<u64>, status_code: Option<u64>, success: Option<bool>, duration_ms: Option<u64> }, Tool { ids, tool_name: Option<String>, mcp_server: Option<String>, tool_use_id: Option<String> }, Permission { ids, decision: Option<String>, source: Option<String>, duration_ms: Option<u64> }, ToolExec { ids, success: Option<bool>, error_type: Option<String>, duration_ms: Option<u64> }, Hook { ids, hook_event: Option<String>, hook_name: Option<String>, num_hooks: Option<u64>, duration_ms: Option<u64>, num_blocking: Option<u64> }, Compaction { ids, trigger: Option<String>, message_count: Option<u64> } }`; `SessionData.spans: VecDeque<Span>` (cap 2000), `SessionData.unknown_spans: u64`; `pub fn ingest_traces(store: &mut Store, v: &Value)`.

- [ ] **Step 1: Failing tests** (OTLP/JSON bodies built from PRD §1, shaped like Task 1's capture): each kind decodes; a span carrying `hook_definitions`, `user_prompt`, `tool_input` and `x.content` → none in `format!("{:?}", store)`; unknown names counted; ring keeps 2000; protobuf → 415 hint; 4 MB boundary (Review Focus 4); spans keyed by their own `session.id` (Review Focus 2); `/v1/other` 404 unchanged.
- [ ] **Step 2: Run** `cargo test otel` → FAIL. **Step 3: Implement.** **Step 4:** `make check` green.
- [ ] **Step 5: Commit** — `OTel: the receiver decodes /v1/traces, keeping allowlisted span facts only`.

### Task 3: Who decided each permission (US-002)

**Files:**
- Modify: `src/ui/state.rs` (`apply_otel` join), `src/ui/panels/turn.rs` (~200), `src/advisor/rules/events.rs` (permission-rule nudge counts only `source = user` when traces exist), `src/metrics/registry.rs` (`permission_decisions`), docs, snapshots

**Interfaces:**
- Consumes: Task 2's `Span::{Tool, Permission}`.
- Produces: permission records gain `decision: Option<String>`, `source: Option<String>`; turn-panel text `waits 6 · you 4 · rule 1 · classifier 1` (`user`→`you`, `config`→`rule`, `classifier`, `hook`, others→`other`; zero groups omitted).

- [ ] **Step 1: Failing tests:** join through `parent_span_id`; Review Focus 1 (parent in a later batch); turn-panel snapshot with and without traces; `classifier_waits_do_not_nudge` (three classifier-approved waits → the rule does not fire); without traces the rule behaves as today.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4:** docs, snapshots (only with-trace ones new); `make check && npm test`.
- [ ] **Step 5: Commit** — `OTel: each permission wait says who decided it`.

### Task 4: Retries and first content (US-003)

**Files:**
- Modify: `src/ui/state.rs` (join request spans to assistant messages by `requestId`), `src/ui/panels/turn.rs`, `src/metrics/registry.rs` (`api_retries`), docs, snapshots

**Interfaces:**
- Produces: per turn `retries: u32`, `retry_ms: u64` (sum over retried requests of `end_ns − last attempt event time`, in ms); turn-panel text `retries 2 (+14 s)`.

- [ ] **Step 1: Failing tests:** attempts counted; Review Focus 3; retry time arithmetic; turn-panel snapshot; no traces → unchanged.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4:** docs, snapshots; green.
- [ ] **Step 5: Commit** — `OTel: request retries and the time they added`.

### Task 5: Hook overhead (US-004)

**Files:**
- Modify: `src/ui/state.rs`, events (`src/events.rs` / `src/dashboard.rs`), `src/query.rs` (`hook_overhead`), `src/metrics/registry.rs` (`hook_overhead`), docs, snapshots, pane fixtures if the dashboard object changes for a fixture

**Interfaces:**
- Produces: per turn `hook_ms_by_event: BTreeMap<String, u64>`; events row when the turn's sum > 500 ms: `hooks +1.2 s · PreToolUse 0.9 s · Stop 0.3 s` (events by descending time); `cctop query dashboard` → `hook_overhead { total_ms, by_event }` only when traces exist.

- [ ] **Step 1: Failing tests:** the row at 501 ms, none at 500; ordering; Review Focus 5; query key present/absent.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4:** docs, snapshots; green.
- [ ] **Step 5: Commit** — `OTel: hook overhead per turn and per event`.

### Task 6: Turning it on (US-005)

**Files:**
- Modify: `src/otel.rs` (`env_block`), `src/ui/sources_view.rs` (`otel: metrics · logs · traces`, each lit when data arrived), `README.md` OTel section, `make site` output

**Interfaces:**
- Produces: `env_block` adds `"OTEL_TRACES_EXPORTER": "otlp"`, `"CLAUDE_CODE_ENHANCED_TELEMETRY_BETA": "1"` and comment lines: traces are a Claude Code beta; cctop drops prompt and content attributes; keep `OTEL_LOG_USER_PROMPTS` unset.

- [ ] **Step 1: Failing tests:** `env_block` contains both keys and the comment lines; sources view snapshot with metrics+logs lit, traces dim, and all three lit.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4:** README OTel section; `make site`; `make check && npm test`.
- [ ] **Step 5: Commit** — `OTel: cctop install --otel turns traces on; the sources view shows which signals arrive`.
