# PRD 7: OTel traces — who decided each permission, retries, hook overhead

**Status:** v1.0 · 2026-11-26 — ready for implementation.
**Target:** cctop after v0.9.1; Rust (`otel.rs`, `ui/state.rs`, TUI). Like today's OTel data, it is live in `cctop run --otlp` only.
**Depends on:** nothing. Independent of PRD 0.
**Source:** the span writers in the Claude Code 2.1.332 binary (span names, start/end attributes, the enablement gate). No trace was captured live yet (§8).

---

## 1. Introduction

cctop hosts a loopback OTLP/JSON receiver (`src/otel.rs`, `cctop run --otlp`, default `127.0.0.1:4318`). It decodes `/v1/metrics` (`claude_code.token.usage`, `cost.usage`, `lines_of_code.count`, `active_time.total`) and `/v1/logs` (`api_request` with `duration_ms` / `time_to_first_token_ms` / `cost_usd`, `api_error`, `tool_result`), at `otel.rs:304–305`. `/v1/traces` answers 404 (`otel.rs:395`). `cctop install --otel` prints the `env` block a person adds to settings (`otel.rs:338–356`). cctop never writes it.

Claude Code 2.1.332 also exports **traces** when `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1` (or `ENABLE_ENHANCED_TELEMETRY_BETA`) and `OTEL_TRACES_EXPORTER=otlp` are set. The binary's own help text: *"Enables Claude Code's session tracing (`CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1` + `OTEL_TRACES_EXPORTER=otlp`)… Each user interaction exports a trace."*

| Span | Start attributes | End attributes | New to cctop |
|---|---|---|---|
| `claude_code.interaction` | `interaction.sequence`, `parent.source`, `queued_sends`; the prompt is `<REDACTED>` unless `OTEL_LOG_USER_PROMPTS` is set | — | the root: one trace per prompt |
| `claude_code.llm_request` | `model`, `gen_ai.request.model`, `llm_request.context` (`tool` / `interaction` / `standalone`), `speed` (`fast` / `normal`), `query_source` | `duration_ms`, `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_creation_tokens`, `success`, `status_code`, `ttft_ms`, `first_content_ms`, `effort`, `stop_reason`, `gen_ai.response.id`, `client_request_id`; events `gen_ai.request.attempt {attempt}` | **retry attempts per request**; `first_content_ms` beside TTFT |
| `claude_code.tool` | `tool_name`, `gen_ai.tool.name`, `anthropic.mcp.server.name`, `tool_use_id` | — | parent of the two below |
| `claude_code.tool.blocked_on_user` | — | `duration_ms`, **`decision`** (accept / reject / cancelled), **`source`** (config, classifier, hook, user, …) | **who decided each permission**, and how long it blocked |
| `claude_code.tool.execution` | — | `duration_ms`, `success`, `error.type` | execution time apart from permission time |
| `claude_code.hook` | `hook_event`, `hook_name`, `num_hooks`; `hook_definitions` under a content flag | `duration_ms`, `num_success`, `num_blocking`, `num_non_blocking_error` | **what hooks cost per turn**, cctop's own included |
| `claude_code.compaction` | `trigger` (auto / manual), `message_count` | — | compaction duration |
| `claude_code.subagent.spawn` | `agent_id`, `agent_type`, `parent_agent_id` | `success` | **disabled in 2.1.332** (its gate returns false) |
| `claude_code.mcp.rpc`, `claude_code.bash.subprocess` | not read yet | not read yet | verify live (§8) |

cctop's permission waits today come from hooks: `PermissionRequest` → next `PostToolUse` (`ui/state.rs:741`). They give the duration, not who decided. That matters for the coach: a classifier allow, a config rule and a person's click are different signals.

## 2. Goals

- Decode `/v1/traces` (OTLP/JSON) in the existing receiver.
- Permission waits gain `decision` and `source` when traces arrive.
- Requests show retry counts; the turn panel shows retry time.
- Hook overhead per turn is visible, by hook event.
- Only attributes on an allowlist are kept. Content-bearing attributes (prompts, hook definitions, tool inputs) are dropped even if a person enabled them.

## 3. User stories

### US-001: `/v1/traces` in the receiver
**Description:** As a developer, I want the receiver to accept OTLP/JSON traces, so that span data reaches `State`.

**Acceptance criteria:**
- [ ] `handle` routes `/v1/traces` to `ingest_traces` (`otel.rs:304` neighbourhood). Protobuf gets the same 415 hint as metrics and logs.
- [ ] `ingest_traces` walks `resourceSpans[].scopeSpans[].spans[]` and keeps spans whose `name` is in the §1 table. For each, it keeps only allowlisted attributes: those named in §1, minus `hook_definitions` and any `user_prompt*` / `*.content` / `tool_input*` key. `session.id` comes from the resource or span attributes, as for metrics.
- [ ] Spans are stored per session as typed records: `LlmRequestSpan`, `PermissionSpan`, `ToolExecSpan`, `HookSpan`, `CompactionSpan`, keeping `trace_id` / `span_id` / `parent_span_id` and start/end ns. Unknown span names are counted, not kept.
- [ ] The store is bounded: the last 2000 spans per session (ring).
- [ ] Tests with OTLP/JSON bodies built from §1's attributes, including one with `hook_definitions` and `user_prompt` set: they are absent from the store (`format!("{:?}")` check). The 404 test for `/v1/other` is unchanged.

### US-002: Who decided each permission
**Description:** As a person using cctop, I want each permission wait to say whether I, a rule, a hook or the classifier decided it, so that I can see which waits a rule would remove.

**Acceptance criteria:**
- [ ] `apply_otel` joins `PermissionSpan`s to cctop's permission records by `tool_use_id`, taken from the parent `claude_code.tool` span.
- [ ] The permissions part of the turn panel (`ui/panels/turn.rs:200`) shows the split, e.g. `waits 6 · you 4 · rule 1 · classifier 1`, when traces exist; unchanged otherwise. Snapshot both ways.
- [ ] `metrics/registry.rs`: `permission_decisions` (counts by `source` × `decision`, source "OTel traces"); docs regenerated.
- [ ] The advisor's permission-rule nudge (the `PermissionRequest`-driven rule in `advisor/rules/events.rs`) counts only waits whose `source` is the person, when traces exist. Test: three classifier-approved waits do not trigger it.

### US-003: Retries and first content
**Description:** As a person using cctop, I want to see when requests were retried, so that slow turns caused by API retries are told apart from slow models.

**Acceptance criteria:**
- [ ] Each `LlmRequestSpan` keeps `attempts` (the count of `gen_ai.request.attempt` events), `ttft_ms`, `first_content_ms`, `status_code`, `success`. It joins to the transcript's assistant message via `gen_ai.response.id` = `requestId`.
- [ ] The turn panel shows `retries 2 (+14 s)` for a turn whose requests had attempts > 1; the added time is the sum over retried requests of (end − the last attempt event's time).
- [ ] `metrics/registry.rs`: `api_retries`; docs regenerated.

### US-004: Hook overhead
**Description:** As a person using cctop, I want to see how much time hooks add per turn, cctop's own included, so that a slow hook is found.

**Acceptance criteria:**
- [ ] `HookSpan`s are summed per turn and per `hook_event`. The events view gets a `hooks` row per turn when the sum exceeds 500 ms: `hooks +1.2 s · PreToolUse 0.9 s · Stop 0.3 s`.
- [ ] `cctop query dashboard` carries `hook_overhead{total_ms, by_event}` when traces exist.
- [ ] `metrics/registry.rs`: `hook_overhead`; docs regenerated.

### US-005: Turning it on
**Description:** As a person setting up cctop, I want `cctop install --otel` to tell me how to turn traces on, and what it costs, so that I opt in knowingly.

**Acceptance criteria:**
- [ ] `env_block` adds `"OTEL_TRACES_EXPORTER": "otlp"` and `"CLAUDE_CODE_ENHANCED_TELEMETRY_BETA": "1"`, with a comment line saying traces are a Claude Code beta. It also says that cctop drops prompt and content attributes, and that `OTEL_LOG_USER_PROMPTS` should stay unset.
- [ ] The sources view shows `otel: metrics · logs · traces`, each lit when data for it arrived.
- [ ] `docs/` (the OTel section of the README and `docs/metrics.md` via the registry) is updated; `make site` rerun.

## 4. Functional requirements

- FR-1: The receiver must accept OTLP/JSON on `/v1/traces` and keep only allowlisted attributes of the §1 spans.
- FR-2: Permission waits must carry decision and source when a matching span exists.
- FR-3: Requests must carry attempt counts; turns must show retry time.
- FR-4: Hook time must be summed per turn and per event.
- FR-5: Without traces, every surface must be exactly as today.

## 5. Non-goals

- Protobuf OTLP.
- Persisting spans for `cctop query` from another process, or for the pane. That is a spool, a separate decision: today's metrics and logs are not persisted either.
- A trace waterfall view.
- `subagent.spawn` (disabled upstream in 2.1.332).

## 6. Technical considerations

- Traces are larger than metrics: one interaction is tens of spans. The receiver already reads bodies fully; cap the body at 4 MB and answer 413 above it.
- `tool_use_id` sits on the `claude_code.tool` span (via `Pt(span, tool_use_id)`). The permission and execution spans are its children, so the join goes through `parent_span_id`.
- `source` values beyond `config`, `classifier`, `hook`, `user` may exist (the binary text lists them as a prefix). Keep unknown values verbatim as machine tokens, and show them as `other`.

## 7. Success metrics

- On one week of the maintainer's sessions with traces on, ≥ 95 % of permission waits get a `source`.
- The hook-overhead row finds cctop's own hook cost per turn (expected: under 50 ms each).

## 8. Open questions

- No trace has been captured live. First task of US-001: run one session with the env set against `cctop run --otlp --dump-otel` (add `--dump-otel` if missing, writing raw bodies to the scratch dir). Confirm the span names, the `blocked_on_user` `source` values, and whether `mcp.rpc` / `bash.subprocess` appear. Record the result as item 44 in `docs/verification/pane.md`, `Result: pending` until a person confirms.
