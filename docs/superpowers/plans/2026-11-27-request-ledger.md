# Request Ledger Implementation Plan (PRD 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every cache miss on a ≥ 2.1.302 transcript gets one located cause (which setting, tool, system block or message changed, or expiry), shown by A01, the turn ledger, `cctop query`, MCP; and the prefix inspector shows exact sizes from the request shape.

**Architecture:** Three new typed `Line` variants measure and drop all prose at parse time. A pure `RequestChain` (new `src/requests.rs`) pairs each main-thread `api-request` with the next assistant `requestId` by order and runs `locate()` on misses. `ui::State` owns the chain; surfaces read `state.requests`.

**Tech Stack:** Rust (serde, insta), Python 3 (anonymiser).

**Spec:** `tasks/prd-cctop-request-ledger.md` (issue #25).

## Global Constraints

- Branch `request-ledger`, cut from `contract-2-1-332` after PRD 0 is complete. Commit subjects: `Requests: …`, `Coach: …`, `Fixtures: …`, `Docs: …`, with the session trailer.
- **No prose kept** (FR-1): no string from `system[].text`, tool `description` / `input_schema`, or blob `content` is stored anywhere — lengths and hashes only. Debug output of every new struct must not contain them.
- Parse from the borrowed `Value` (`T::deserialize(&raw)`), never `raw.clone()` before measuring.
- Main-thread `querySource`: starts with `repl_main_thread`, or equals `sdk`. Sidechains are not chained.
- Located-cause order (§3.2): ParamsChanged › ToolsChanged › SystemChanged › PrefixRewritten › Expired › Unlocated. Exactly one per miss.
- Transcripts < 2.1.302: every existing snapshot and test unchanged (FR-6).
- `coach.rs` / `dashboard.rs` cut text at their existing fixed widths; after any coach/dashboard change rerun `scripts/pane-fixtures.sh` and `scripts/pane-fixtures.sh fixtures/session-b.jsonl b`; `npm test` row-identity passes.
- Insta: `INSTA_UPDATE=always cargo test`, review each `.snap` diff, strip `assertion_line:`, delete `*.snap.new`.
- Generated docs: `cctop metrics --md > docs/metrics.md && cctop metrics --readme README.md`, never hand-edited.
- Fixtures are never hand-edited: anonymiser / composer only.
- Done = `make check` and `npm test` green.

## Rulings made while planning (spec silent or contradicted by the code)

- **Fixture name:** `fixtures/session-c.jsonl` already exists (the agents fixture). The new fixture is `fixtures/session-r.jsonl` ("requests"). Every "fixture C" in the PRD means session-r.
- **Miss test:** a request is a miss when its assistant line carries `cache_miss_reason`, **or** the cached prefix shrank by at least `MISS_TOKENS` (20 000, `advisor/rules/token.rs`): `(prev.cache_read + prev.cache_creation) − cur.cache_read ≥ MISS_TOKENS`. The PRD's bare "creation ≥ 20k" would count every large new tool result as a miss.
- **A01's trigger is unchanged** (named misses in `state.agg.cache_misses`); only its `{what}` text uses the located cause, joined by `message_id`. PRD decision 2: no new rule.
- **Query key:** `cache.misses` is a scalar count today. The per-miss list is a new key `cache.located_misses` (array); `misses` keeps its meaning.

## Review Focus

1. A request whose response never arrives (aborted, `[Request interrupted]`), followed by another request: the first stays unpaired and is not compared against (Task 3 test).
2. Two consecutive misses with no shape lines at all in the file (shape line missing for a hash): ToolsChanged/SystemChanged cannot be computed → fall through to PrefixRewritten/Expired, never panic (Task 3 test `missing_shape_falls_through`).
3. A `compact_boundary` between two requests: the second is a chain start, never `PrefixRewritten{from: 0}` (Task 3 test).
4. A shape line with zero tools or an empty `system` (e.g. `sdk` sessions): sizes are 0, prefix rows not emitted, no division by zero (Task 5 test).
5. A01 text with a long MCP tool name (`mcp__some_long_server__some_long_tool`): cut at the coach's fixed width, never overflowing the pane row (Task 4 test).

---

### Task 1: Parse the three request lines, keeping no prose (US-001)

**Files:**
- Create: `src/transcript/request.rs`
- Modify: `src/transcript/mod.rs` (variants, `from_value`, `kind()`, `timestamp()`), `src/harness_facts.rs` (`first_seen::API_REQUEST`)

**Interfaces:**
- Produces (in `src/transcript/request.rs`, re-exported from `transcript`):
  - `pub struct ApiRequest { pub query_source: String, pub shape_hash: String, pub keep: usize, pub tail_len: usize, pub params: RequestParams, pub cache_breakpoints: Vec<Breakpoint>, pub timestamp: Option<String> }`
  - `pub struct RequestParams { pub model: String, pub effort: Option<String>, pub betas_hash: u64, pub max_tokens: Option<u64>, pub thinking_type: Option<String>, pub edits: Vec<String> }` (`#[derive(PartialEq)]`)
  - `pub struct Breakpoint { pub path: String /* e.g. "system/1" */, pub ttl: Option<String>, pub scope: Option<String> }`
  - `pub struct ShapeFacts { pub shape_hash: String, pub system: Vec<usize>, pub tools: Vec<ToolFacts>, pub timestamp: Option<String> }`
  - `pub struct ToolFacts { pub name: String, pub schema_chars: usize, pub description_chars: usize, pub deferred: bool }`
  - `pub struct BlobFacts { pub hash: String, pub role: String, pub chars: usize, pub timestamp: Option<String> }`
  - `Line::ApiRequest(ApiRequest)`, `Line::ApiRequestShape(ShapeFacts)`, `Line::ApiRequestBlob(BlobFacts)`; `kind()` returns `"api-request"`, `"api-request-shape"`, `"api-request-blob"`.
  - `harness_facts::first_seen::API_REQUEST: Version = Version(2, 1, 302)`.
- `tail` is stored as its length only (PRD §7). `schema_chars` = length of the `input_schema` serialised compactly (`serde_json::to_string(&v).len()`), measured from the borrowed value.

- [ ] **Step 1: Write the failing tests** in `src/transcript/request.rs` `#[cfg(test)]`, on hand-written JSON strings shaped like the PRD §1 table:
  - `shape_keeps_no_prose`: `system[0].text = "SYSTEM-SECRET-xyz"` (17 chars), a tool `description = "DESC-SECRET-abc"` (15). Assert `Line::parse` gives `ApiRequestShape`, `format!("{:?}", line)` contains neither secret, `system == vec![17]`, the tool's `description_chars == 15`, `deferred` read from `defer_loading`.
  - `blob_keeps_no_prose`: `message.content[0].text = "BLOB-SECRET"` → Debug has no secret, `chars == 11`, `role == "user"`.
  - `request_fields`: `keep: 48`, `tail` of 3 hashes, `params.output_config.effort = "high"`, two betas → `tail_len == 3`, `effort == Some("high")`; `betas_hash` is computed over the **sorted** betas, so two requests whose betas differ only in order have equal `betas_hash` (assert), and a request with one extra beta has a different one (assert).
  - `malformed_becomes_unknown_without_prose`: an `api-request-shape` with `system: "notalist"` and a `shape.tools[0].description` secret → `Line::Unknown(v)` where `v.get("shape").is_none()` and Debug has no secret; same for a malformed blob (`message` removed).
- [ ] **Step 2: Run** `cargo test transcript::request` → FAIL (module missing).
- [ ] **Step 3: Implement.** Custom `Deserialize` (or `fn from_raw(&Value) -> Option<Self>`) per struct that reads only the kept fields; text fields measured with `.chars().count()`. In `from_value`, on parse failure for these three types remove the `shape` and `message` keys before wrapping in `Unknown`. Add `API_REQUEST` with a doc comment: upper bound (oldest transcript seen with them), names the three line types.
- [ ] **Step 4: Run** `cargo test transcript` → PASS; `make check` green.
- [ ] **Step 5: Commit** — `Requests: parse api-request, -shape and -blob lines, keeping no prose`.

### Task 2: Fixture `session-r` with request lines (US-002)

**Files:**
- Modify: `scripts/anonymise-transcript.py`
- Create: `fixtures/session-r.jsonl`
- Modify: `fixtures/README.md`
- Test: `src/transcript/request.rs` (fixture test)

**Interfaces:**
- Produces: `fixtures/session-r.jsonl`, ≥ 2.1.302, containing ≥ 1 main-thread `shapeHash` change and ≥ 1 `keep` drop. Later tasks assert on its line numbers.

- [ ] **Step 1:** Anonymiser: for `api-request-shape`, `system[].text` and every tool `description` / string inside `input_schema` → same-length filler (existing `fill(n)`); tool names kept when built-in (no `mcp__` prefix), `mcp__<server>__<tool>` renamed consistently (reuse the existing MCP renaming if there is one, else `mcp__server<N>__tool<M>` stable per input). `api-request-blob` `message.content[].text` → filler. `shapeHash`, `hash`, `tail[]`, `requestDigest` → a stable keyed hash of the input (same in → same out, e.g. existing `hid()`), so equality relations survive. Add the three types' keys to the keep lists as needed.
- [ ] **Step 2:** Pick the source: the smallest transcript under `~/.claude/projects/*/*.jsonl` with ≥ 1 main-thread `shapeHash` change and ≥ 1 `keep` drop (the controller found a 431-line one; the dispatch names it). Produce `fixtures/session-r.jsonl` with the anonymiser (add `--max-str` as the other fixtures do). Grep the output for any non-filler English from the source's prompts — none may remain.
- [ ] **Step 3: Write the test** `fixture_r_parses_request_lines`: every line of `fixtures/session-r.jsonl` whose `type` is one of the three parses to its typed variant (zero `Unknown` for them), and there is ≥ 1 of each.
- [ ] **Step 4: Run** `cargo test fixture_r` → PASS; `make check` green (existing snapshots unchanged — the fixture is new).
- [ ] **Step 5:** `fixtures/README.md`: a `session-r.jsonl` entry — line count, Claude Code version, what it demonstrates (the shape change and keep drop, with their line numbers), how it was made.
- [ ] **Step 6: Commit** — `Fixtures: session-r, a 2.1.3xx session with request lines`.

### Task 3: The request chain and `locate` (US-003)

**Files:**
- Create: `src/requests.rs` (+ `pub mod requests;` in `src/lib.rs`)
- Modify: `src/ui/state.rs` (field + `apply`), `src/load.rs` (if `State::apply` is not already what load uses)

**Interfaces:**
- Consumes: Task 1 types; `crate::advisor::rules::token::MISS_TOKENS`.
- Produces (in `src/requests.rs`):
  - `pub enum LocatedCause { ParamsChanged { field: &'static str, from: Option<String>, to: Option<String> }, ToolsChanged { added: Vec<String>, removed: Vec<String>, resized: Vec<(String, i64)> }, SystemChanged { block: usize, delta_chars: i64 }, PrefixRewritten { from: usize, of: usize }, Expired { gap_ms: u64, ttl_ms: u64 }, Unlocated }`
  - `impl LocatedCause { pub fn kind(&self) -> &'static str /* "params_changed" | "tools_changed" | "system_changed" | "prefix_rewritten" | "expired" | "unlocated" */; pub fn short(&self) -> String /* §3.3 forms */ }`
  - `pub struct LocatedMiss { pub message_id: String, pub turn: Option<usize>, pub at: Option<String>, pub tokens: u64 /* the miss's cache write */, pub cause: LocatedCause, pub api_reason: Option<String> }`
  - `pub struct RequestChain { … }` with `pub fn apply(&mut self, line: &Line, turn: Option<usize>, default_ttl_ms: u64)`, `pub fn misses(&self) -> &[LocatedMiss]`, `pub fn latest_shape(&self) -> Option<&ShapeFacts>`, `pub fn miss_for(&self, message_id: &str) -> Option<&LocatedMiss>`.
  - `pub fn locate(prev: &Closed, cur: &Closed, shapes: &HashMap<String, ShapeFacts>, default_ttl_ms: u64) -> LocatedCause` where `Closed` is the paired request (request + its usage + timestamp).
  - `ui::State.requests: crate::requests::RequestChain`.
- `short()` forms (exact): `tools changed: +<added,> −<removed,>` (counts `0` when none, e.g. `tools changed: +mcp__x__search −0`), `system block <n> <±k> chars` (`fmt` style `+1.2k`), `prefix re-sent from msg <from>/<of>`, `<field> <from>→<to>` (e.g. `effort high→max`; `betas changed` for betas), `cache expired (<gap> gap > <ttl> TTL)` (e.g. `6m gap > 5m TTL`), `unlocated`.
- Chain state per line: an `ApiRequest` with a main-thread source opens a pending request (replacing an older pending one — that one is dropped unpaired); the next `Assistant` line with a `requestId` closes it with that line's `message.id` and usage (later assistant lines of the same `message.id` are ignored); `System` `compact_boundary` and `ContinuedIn` reset (no previous). `ApiRequestShape` lines are stored by hash (only the facts). Store `tail_len`, never hashes. O(1) per line.
- Miss test and TTL: see Rulings. TTL = the request's message-content breakpoint `ttl` (`"1h"` → 3 600 000, `"5m"` → 300 000), else `default_ttl_ms` (callers pass `state.cache_ttl_ms()`).

- [ ] **Step 1: Write the failing tests** (synthetic `Line`s built from JSON strings via a test helper): `params_changed`, `tools_changed` (added `mcp__x__search`, `short() == "tools changed: +mcp__x__search −0"`), `system_changed` (block 2 +1200 → `"system block 2 +1.2k chars"`), `prefix_rewritten` (`keep` 50→48 with prev tail 2 → `from 48, of 52`), `expired` (6-min gap, 5m TTL → `"cache expired (6m gap > 5m TTL)"`), `unlocated`; `order_params_beat_shape` (both change → `ParamsChanged`); `pairs_by_order` (two requests, one assistant → the second is paired, first dropped); `compact_resets_chain`; `missing_shape_falls_through`; `api_reason_kept_beside` (`cache_miss_reason: "tools_changed"` with no shape change → cause from the table, `api_reason == Some("tools_changed")`); `not_a_miss_when_prefix_held` (creation 30k but read grew → no `LocatedMiss`); `linear_time` (`#[cfg(not(debug_assertions))]` or `#[ignore]`-free but loose: 10 000 synthetic requests in < 200 ms under `--release`; in debug just assert it completes).
- [ ] **Step 2: Run** `cargo test requests` → FAIL.
- [ ] **Step 3: Implement** `src/requests.rs` per the interfaces; wire `State.requests.apply(line, current_turn, self.cache_ttl_ms())` into `State::apply` after `self.agg.push(line)`.
- [ ] **Step 4: Fixture test** `fixture_r_located`: load `fixtures/session-r.jsonl` through `load.rs`'s loader; assert at least one `ToolsChanged` or `SystemChanged` and one `PrefixRewritten` among `state.requests.misses()`, and pin their `message_id`s (read them from the run, write them into the test).
- [ ] **Step 5: Run** `cargo test requests && make check` → PASS (existing snapshots unchanged).
- [ ] **Step 6: Commit** — `Requests: the request chain names the cause of each cache miss`.

### Task 4: A01, the ledger, the query, the metric (US-004)

**Files:**
- Modify: `src/advisor/rules/token.rs` (A01 `{what}`), `src/ledger.rs` (`miss_cause`), `src/query.rs` (`cache_object`: `located_misses`), `src/mcp.rs` (if the cache tool builds its own object rather than reusing `cache_object`), `src/metrics/registry.rs` (`cache_misses` caveat, new `cache_miss_located`), `src/coach.rs` (cache light "last" text), snapshots, `docs/metrics.md`, `README.md`, `tests/pane/fixtures/*` (regenerated)

**Interfaces:**
- Consumes: `state.requests.miss_for(message_id) -> Option<&LocatedMiss>`, `state.requests.misses()`, `LocatedCause::{kind, short}`.
- Produces: `cache.located_misses: [{ "turn", "at", "message_id", "tokens", "api_reason", "located": { "kind", "detail" } }]` where `detail = short()`; ledger row `miss_cause` = the turn's first located `short()`, else today's value.

- [ ] **Step 1: Write the failing tests:** in `token.rs`, `a01_names_the_located_tool`: a state built from synthetic lines (named miss `tools_changed` ≥ 20k plus request/shape lines adding `mcp__x__search`) → headline contains `tools changed: +mcp__x__search −0`; `a01_falls_back_when_unlocated` → today's `tool list changed`; `a01_cuts_long_names` (Review Focus 5) → the headline's char count ≤ `coach::WIDTH` (52). In `query.rs`, `located_misses_on_fixture_r`: `summary`/dashboard JSON for session-r has `cache.located_misses[*].located.kind` including the kinds Task 3 pinned. In `ledger.rs`, a row for the miss turn of session-r carries the short form.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.** A01 keeps its trigger; `what` = `miss_for(&miss.message_id).filter(|m| !matches!(m.cause, Unlocated)).map(|m| m.cause.short())` else today's text; action text unchanged except: `PrefixRewritten` uses the existing "an earlier message changed" action, `Expired` uses "the cache expired between turns — keep turns closer than the TTL, or accept the rebuild", `ParamsChanged` reuses the model-switch action. Evidence line adds `· located: <kind>` when located. Registry: `cache_misses` caveat lists the located kinds; new `metric!(cache_miss_located, "Tokens & Cost", "Located cache misses", "count", "<count by located cause, from api-request lines (2.1.302+)>", [...sources as the file's convention], "<caveat: order join; main thread and sdk only>", "")`.
- [ ] **Step 4:** Regenerate: `INSTA_UPDATE=always cargo test` (review every changed `.snap`: only A01/ledger/cache text may change, and only for session-r — fixture A/B snapshots must not move; strip `assertion_line:`, delete `*.snap.new`); `cargo run -q -- metrics --md > docs/metrics.md && cargo run -q -- metrics --readme README.md`; `scripts/pane-fixtures.sh && scripts/pane-fixtures.sh fixtures/session-b.jsonl b`.
- [ ] **Step 5: Run** `make check && npm test` → green; `cargo run -q -- query dashboard --session fixtures/session-r.jsonl | grep -c located` ≥ 1.
- [ ] **Step 6: Commit** — `Coach: A01, the ledger and cctop query name what changed when the cache missed`.

### Task 5: Exact prefix sizes from the request shape (US-005)

**Files:**
- Modify: `src/prefix.rs` (`Row` gains `exact: bool`; `rows()` uses the shape), `src/ui/prefix_view.rs` (the tag), the call sites of `Prefix::rows` that need the shape, snapshots

**Interfaces:**
- Consumes: `state.requests.latest_shape() -> Option<&ShapeFacts>`.
- Produces: `Prefix::rows(&self, first_call_tokens: u64, shape: Option<&ShapeFacts>) -> Vec<Row>`; `Row.exact: bool`; deferred tools as a separate `Kind::Tools` row named `deferred tools` with `exact: true`, **excluded from the prefix total**.

- [ ] **Step 1: Write the failing tests** in `prefix.rs`: `shape_gives_exact_tool_rows` (shape with `Bash` 1000+200 chars and `mcp__x__search` 300+100 → the built-in row `bytes == 1200`, the `x` server row `bytes == 400`, both `exact`); `deferred_listed_apart` (a `deferred: true` tool is in its own row and not in the total); `empty_shape_no_rows` (Review Focus 4: zero tools, empty system → no tool/system rows, no panic); `no_shape_unchanged` (`rows(n, None)` equals today's output for an existing input).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.** With a shape: tool rows grouped as today (built-in vs each MCP server) but sized `schema_chars + description_chars`; `SystemPrompt` rows from `shape.system` block sizes; `tokens_est = bytes / 4` as today; `exact: true`. `Other` reconciled as today. The view shows *exact (request shape)* for exact rows instead of *estimated*.
- [ ] **Step 4:** `INSTA_UPDATE=always cargo test`; only a new session-r prefix snapshot (add one in the prefix view's snapshot tests) may appear — existing snapshots unchanged. Review, strip `assertion_line:`, delete `*.snap.new`.
- [ ] **Step 5: Run** `make check && npm test` → green.
- [ ] **Step 6:** `docs/verification/pane.md` §B: item 40 (PRD 1 §9) — "A `/model` or `/effort` switch mid-session shows `ParamsChanged` in A01" — Setup: a real session on ≥ 2.1.302, switch effort mid-way, then `cctop query dashboard`; Expected: `cache.located_misses` has `params_changed`. `Result: pending`.
- [ ] **Step 7: Commit** — `Requests: exact prefix sizes from the request shape`.
