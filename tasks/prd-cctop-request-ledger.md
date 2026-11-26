# PRD 1: the request ledger — which part of the prompt changed when the cache missed

**Status:** v1.0 · 2026-11-26 — ready for implementation. **Issue:** #25.
**Target:** cctop after v0.9.1; Rust only (TUI, `cctop query`, MCP). The pane draws whatever `cctop query` returns, so it needs no change.
**Depends on:** nothing. Independent of PRD 0.
**Source:** 40 recent transcripts on the maintainer's machine (Claude Code 2.1.302 – 2.1.332), read for keys, counts and hashes only.

> Decisions taken with the user on 2026-11-26:
> 1. This is the first of the seven scan PRDs to build, together with PRD 2.
> 2. It feeds the existing A01 `cache-miss` rule and the turn ledger. No new coach rule in v1.

---

## 1. Introduction

When a call misses the prompt cache, cctop can name the miss today in two ways. `diagnostics.cache_miss_reason` on the assistant line gives `tools_changed` / `messages_changed` / `model_changed` (`transcript/mod.rs:716`, aggregated in `metrics/usage.rs:409–414`). The status-line shim gives `prompt_cache.last_miss_cause.causes` (`status.rs:96`). Both say *that* something changed, never *what*. "Tools changed" does not say which tool; "messages changed" does not say where.

Since 2.1.302 Claude Code writes, before every API request, the request's own description to the transcript:

| Line | Keys (values not shown) | What it gives |
|---|---|---|
| `api-request` | `id` (UUID), `querySource`, `shapeHash`, `params{model, betas[], max_tokens, thinking{type,display}, context_management{edits[]}, output_config{effort}}`, `thread`, `keep`, `tail[]`, `cacheControl[{path, value{type, ttl, scope?}}]`, `requestDigest` | One request: its settings, which shape it used, how many messages it reused from the previous request (`keep`), the hashes of the new messages (`tail`), and where the cache breakpoints sit |
| `api-request-shape` | `shapeHash`, `shape{system[{type,text}], tools[{name, description, input_schema, defer_loading?, eager_input_streaming?, model?, type?}]}` | The system blocks and tool schemas a `shapeHash` stands for, written once per new hash |
| `api-request-blob` | `hash`, `message{role, content[]}` | One message's content, keyed by the hash `tail` names. **This is prose: cctop keeps its length and hash only.** |

Measured on the 40 transcripts:

| Fact | Value |
|---|---|
| Transcripts with `api-request` lines | 39 of 40; first seen 2.1.302 (upper bound: the oldest transcript on the machine that has them) |
| `querySource` | `repl_main_thread` 1117, `sdk` 361, `repl_main_thread:outputStyle:Concise` 191 |
| `api-request.id` equals the next assistant line's `requestId` | **0 of 1669**. The join is by order: an `api-request` line precedes the assistant lines of its response |
| Consecutive main-thread request pairs | 1299 |
| …where `keep` < the previous request's `keep + len(tail)` (prefix rewritten) | 10: nine to `keep = 0`, one `50 → 48` |
| …where `shapeHash` changed | 7. Every hash used had its `api-request-shape` line in the same file. |
| …where any `params` key changed | 0 in this sample |
| `cacheControl` breakpoints | `["system",1]` (1h, `scope: global`) and `["system",2]` (1h) on all 1669; one message-content breakpoint per request |
| `context_management.edits` | `clear_thinking_20251015` on all 1669 |
| Resume semantics (from the 2.1.332 binary's line table) | `api-request*` lines are `boundary-cleared`: a compact boundary starts a new chain |

The pairing gives a *located* cause per miss: which tool was added or removed, which system block changed size, which message index the prefix was rewritten from, which setting flipped. Or none of these, with a gap longer than the TTL, which is expiry.

## 2. Goals

- Every cache miss on a transcript ≥ 2.1.302 gets one located cause, from the transcript alone: no shim, no hooks, works on past sessions and fixtures.
- The A01 nudge and the turn ledger say *what* changed, not just the category.
- The prefix inspector shows exact tool and system-block sizes when shape lines exist, instead of estimates from disk.
- Nothing from `system[].text`, tool `description`s or `api-request-blob` content is kept: lengths and hashes only.

## 3. Design

### 3.1 Parsing (`src/transcript/`)

Three new `Line` variants, deserialised into structs that hold only what cctop keeps:

```rust
Line::ApiRequest(ApiRequest)      // query_source, shape_hash, keep, tail: Vec<String> (hashes), params: RequestParams, cache_breakpoints: Vec<Breakpoint>, timestamp
Line::ApiRequestShape(ShapeFacts) // shape_hash, system: Vec<usize> (chars per block), tools: Vec<ToolFacts{name, schema_chars, description_chars, deferred: bool}>
Line::ApiRequestBlob(BlobFacts)   // hash, role, chars (sum of text lengths; images counted as image_tokens elsewhere)
```

`RequestParams { model, effort, betas_hash: u64, max_tokens, thinking_type, edits: Vec<String> }`. Betas are hashed, not listed: only a change matters. A custom `Deserialize` reads `text` / `description` as lengths, so the string is never stored. `Line::Unknown` must not be the fallback for these three types; an unparseable one becomes `Unknown` **minus** its `message` / `shape` fields.

### 3.2 The request chain (`src/requests.rs`, new)

`RequestChain` is fed in transcript order:

- `ApiRequest` with a main-thread `querySource` (prefix `repl_main_thread`, or `sdk`) opens a pending request.
- The next assistant line with a `requestId` closes it: the request gets that `message.id` and its usage.
- A `compact_boundary` or `continued-in` resets the chain (`boundary-cleared`).
- Sidechain / agent transcripts are not chained in v1.

For each closed request after the first, `locate(prev, cur, shapes, usage, ttl_ms)` returns at most one `LocatedCause`, checked in this order:

| Order | Cause | Test | Detail kept |
|---|---|---|---|
| 1 | `ParamsChanged(field)` | any of model, effort, betas_hash, thinking_type, max_tokens, edits differs | field name; model / effort old → new (machine tokens) |
| 2 | `ToolsChanged{added, removed, resized}` | `shape_hash` differs and the tools lists differ | tool names (structural) and size Δ in chars |
| 3 | `SystemChanged{block, delta_chars}` | `shape_hash` differs, tools equal | block index, Δ chars |
| 4 | `PrefixRewritten{from, of}` | `cur.keep < prev.keep + prev.tail.len()` | `from = cur.keep`, `of = prev total` (`from = 0` is a full rewrite) |
| 5 | `Expired{gap_ms}` | none of the above, and the gap since the previous request's timestamp > TTL (from the breakpoint's `ttl`, else `state.cache_ttl_ms()`) | gap |
| 6 | `Unlocated` | none of the above | — |

A request counts as a miss when its usage shows `cache_creation_input_tokens` above the threshold A01 already uses (`token.rs:38`), or its assistant line carries `cache_miss_reason`. `locate` runs only for misses. The API's own `cache_miss_reason`, when present, is kept beside the located cause. If the two disagree (e.g. the API says `tools_changed` but no shape changed), the ledger shows both.

### 3.3 Surfaces

- **A01** (`advisor/rules/token.rs:41–100`): `{what}` becomes the located cause, cut to the coach's fixed width: `tools changed: +mcp__x__search −0`, `system block 2 +1.2k chars`, `prefix re-sent from msg 48/50`, `effort high→max`, `cache expired (6m gap > 5m TTL)`. Fallback to today's text when unlocated.
- **Turn ledger** (`ledger.rs:28` `miss_cause`): the located cause's short form.
- **`cache_misses` metric** (`metrics/registry.rs:85`): `caveats` names the located causes; a new registry entry `cache_miss_located` (count by cause) with formula and sources. `docs/metrics.md` and the README block regenerated.
- **`cctop query`**: `cache.misses[]` gains `located: {kind, detail}` per miss (`query.rs:221–225` neighbourhood); existing keys keep their meaning.
- **Prefix inspector** (`prefix.rs`, `ui/prefix_view.rs`): when the newest `ShapeFacts` exists, `Kind::Tools` rows use `schema_chars + description_chars` per tool and `Kind::SystemPrompt` uses the block sizes, marked *exact (request shape)* instead of *estimated*. `Other` is reconciled as today.

## 4. User stories

### US-001: Parse the three request lines, keeping no prose
**Description:** As a developer, I want `api-request`, `api-request-shape` and `api-request-blob` typed, so that the rest of cctop can use them without ever holding prompt text.

**Acceptance criteria:**
- [ ] `Line::ApiRequest`, `Line::ApiRequestShape`, `Line::ApiRequestBlob` exist with the fields in §3.1. `Line::kind()` and `timestamp()` cover them.
- [ ] A unit test feeds a shape line whose `system[0].text` and a tool `description` are distinctive strings. It asserts that `format!("{:?}", line)` contains neither string, and that the char counts are right.
- [ ] The same for a blob line's `message.content[].text`.
- [ ] A malformed instance of each becomes `Line::Unknown`, with `shape` / `message` removed from the kept value (test).
- [ ] `harness_facts::first_seen::API_REQUEST = Version(2, 1, 302)` with a doc comment saying it is an upper bound and naming the line types.
- [ ] `cargo test transcript` passes; `make check` green.

### US-002: A fixture with request lines
**Description:** As a developer, I need an anonymised fixture from ≥ 2.1.302 so that tests and snapshots exercise the chain.

**Acceptance criteria:**
- [ ] `scripts/anonymise-transcript.py` handles the three line types. In shapes it replaces `system[].text` and tool `description` / `input_schema` strings with same-length filler, keeps tool names whose prefix is a built-in (`Bash`, `Read`, …), and renames `mcp__<server>__<tool>` consistently. Blob `content[].text` becomes same-length filler. `shapeHash` / `hash` / `tail` keep their equality relations (the same input gives the same output).
- [ ] `fixtures/session-c.jsonl` is produced from a real session that has at least one `shapeHash` change and one `keep` drop, via `scripts/anonymise-transcript.py` / `compose-fixture.py`; never hand-edited.
- [ ] A test asserts the fixture parses with zero `Line::Unknown` for the three types.
- [ ] `fixtures/README` (or the scripts' header) lists fixture C's version and what it demonstrates.

### US-003: The request chain and `locate`
**Description:** As a developer, I want a pure function that names the cause of each miss, so that every surface reads one answer.

**Acceptance criteria:**
- [ ] `src/requests.rs` implements `RequestChain::apply(&Line)` and `locate(...)` per §3.2.
- [ ] Unit tests, one per row of the §3.2 table, on synthetic lines; plus the order rule: params and shape changing together report `ParamsChanged`.
- [ ] A test on fixture C asserts at least one `ToolsChanged` or `SystemChanged` and one `PrefixRewritten`, at the line numbers the fixture has.
- [ ] The join is by order; a test with two `api-request` lines before one assistant line pairs the second with it and leaves the first unpaired (an aborted request).
- [ ] `ui::State` holds the chain and applies it in `State::apply`; `load.rs` builds it for `cctop query`.
- [ ] O(1) per line (no rescans): a test feeds 10 000 synthetic requests and asserts linear time loosely (under 200 ms in release).

### US-004: A01, the ledger and the query say what changed
**Description:** As a person using cctop, I want the cache-miss nudge and the turn ledger to name the tool, block, message or setting that changed, so that I know what to stop doing.

**Acceptance criteria:**
- [ ] A01's text uses the located cause (§3.3) with today's text as the fallback. The insta snapshot for A01 is updated and reviewed; `assertion_line:` stripped, `*.snap.new` deleted.
- [ ] `ledger.rs` `miss_cause` carries the short form; `cctop query ledger` shows it.
- [ ] `cctop query dashboard --session fixtures/session-c.jsonl` includes `cache.misses[].located` with the expected kinds.
- [ ] `coach.rs` / `dashboard.rs` cut the new text at the existing fixed widths; `scripts/pane-fixtures.sh` is rerun and `npm test`'s row-identity tests pass.
- [ ] `metrics/registry.rs` has `cache_miss_located`; `cctop metrics --md > docs/metrics.md && cctop metrics --readme README.md` rerun; the freshness test passes.

### US-005: Exact prefix sizes from the request shape
**Description:** As a person reading the prefix inspector, I want tool and system sizes from what was actually sent, so that I trust the biggest rows.

**Acceptance criteria:**
- [ ] With a `ShapeFacts` present, `prefix.rs` reports per-tool and per-system-block sizes from it, tagged exact. Without one (transcripts < 2.1.302), today's estimates are unchanged (existing snapshots unchanged).
- [ ] Deferred tools (`defer_loading: true`) are listed apart and excluded from the prefix total, as `/context` does.
- [ ] The prefix view's snapshot for fixture C shows the exact tag; reviewed and accepted.
- [ ] `make check` green.

## 5. Functional requirements

- FR-1: cctop must parse `api-request`, `api-request-shape`, `api-request-blob` and keep no string from `system[].text`, tool `description`/`input_schema`, or blob `content`.
- FR-2: cctop must pair each main-thread `api-request` with the next assistant `requestId` in transcript order.
- FR-3: For each cache miss on a chained request, cctop must compute exactly one located cause using the §3.2 order.
- FR-4: A01, the ledger, `cctop query` and the MCP tool must show the located cause; when none is found they show today's text.
- FR-5: The prefix inspector must prefer request-shape sizes when present and say so.
- FR-6: Transcripts older than 2.1.302 must behave exactly as today (all existing snapshots unchanged).

## 6. Non-goals

- Chaining subagent or workflow-agent requests (v1 is the main thread and `sdk`).
- Reconstructing or showing message content from blobs.
- A new coach rule. A01 gets better text; whether a "tools churn" rule is worth it is decided after a week of data.
- Using `requestDigest` or `cacheControl` `scope` beyond display.

## 7. Technical considerations

- Shape lines can be large (16 tool schemas). The parser measures and drops; it must not clone the raw `Value` before measuring. Deserialise straight from the line.
- `tail` hashes are 64-hex strings; keep them only until the next request (needed for nothing beyond `len()` in v1, so store the count, not the hashes).
- `keep` counts messages, not tokens. `PrefixRewritten.from` is a message index; the ledger prints it as such.
- The order join assumes Claude Code writes the request line before the response's lines (observed on all 1669). If a future version interleaves, the chain falls back to unpaired requests and A01's old text. A test pins the observed order.

## 8. Success metrics

- On the maintainer's last 40 sessions, ≥ 90 % of misses above A01's threshold get a cause other than `Unlocated`.
- A01 shown with a located cause leads to `acted` (the rule's existing notion) at least as often as the unlocated text, measured with coach-stats after two weeks.

## 9. Open questions

- Do `sdk` sessions (361 requests in the sample) belong in the main ledger, or in their own section? v1 treats them as main-thread.
- Is a `params` change ever seen in the wild? 0 in this sample. Keep the row; verify on a session with `/model` or `/effort` mid-way (verification item 40, `Result: pending`).
