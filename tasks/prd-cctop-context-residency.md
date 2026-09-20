# PRD: context residency — what is in the window right now, and what put it there

**Status:** v1.0 · 2026-09-20 — draft, not implemented. Sits beside the two spend PRDs but is not one of them: this is about *space*, not money.
**Target:** cctop ≥ 0.7.0 attached to Claude Code CLI ≥ 2.1.274 (`READ_FROM`).
**Depends on:** `tasks/prd-cctop.md` v1.1 (collectors, `attach.rs`, `metrics/`); the prefix inspector (`src/prefix.rs`, `src/ui/prefix_view.rs`) whose reconciliation model this PRD copies for the other half of the window.

> Decisions taken with the user on 2026-09-20, after the measurement in §3.2 replaced the original request:
> 1. **Not a files view — a residency view.** The request was "which files consume how much space". The measurement says a file-keyed panel answers under a fifth of the window and, in the session it was measured on, would have rendered empty. The view is keyed on **source**, one row per kind, and a file is one kind with a per-path drill-down.
> 2. **Bash reads are attributed by path, single-path commands only.** `files.rs::bash_read_paths` already resolves `cat` / `head` / `tail` / `sed -n`; when it returns exactly one path the result's `stdout_chars` are that file's, when it returns two or more they stay in the unattributed `bash output` row. Never split, never weight by size on disk.
> 3. **Residency, not cumulation.** Every figure is "since the last context boundary" — the last `compact_boundary` (or the transcript's start). The header says so. A session that just compacted shows small numbers because the window *is* small.
> 4. **Opportunistic calibration.** The raw estimate is `chars / 4`, the divisor `tools.rs::top_ctx` already uses. When the session has a `/context` capture (`ContextCapture`, already parsed in `transcript/local_command.rs`), the total is reconciled against its real `Messages` figure and the rows are scaled; the header says which mode it is in. No persisted per-model ratio in v1.
> 5. **One new coach rule, on the prefix share** — not on files. The lever the measurement found is the fixed prefix, and it is the one a person can act on. Urgency LATER, `acted` when the inspector is opened.
> 6. **Surfaces: the inspector, `cctop query`, `mcp.rs`, the coach rule. Not the pane in v1.** All nine digit slots in `ui/panels/mod.rs` are taken, so this is a full-height inspector like `prefix_view`, not a tenth panel.

---

## 1. Introduction

cctop can already say how full the context is (`metrics/context.rs`: `size`, `window`, `ratio`, `velocity`, `turns_until_compaction`) and what the *fixed* part of it is made of (`prefix.rs`: a row per CLAUDE.md file, tool schema, skill, MCP server, agent type, with `Other` reconciled against the first API call). It cannot say anything about the other half. `ContextView::messages()` is one number — `size − prefix` — and nothing decomposes it.

Meanwhile the collectors already hold almost every input needed. `transcript/tool_result.rs` parses `content_chars` for a file read, `stdout_chars` / `stderr_chars` for a bash result, `result_chars` for an agent return, and `image_tokens()` for an image; `tools.rs` computes `result_tokens_est` per call and ranks the top five (`top_ctx`); `files.rs` keys reads, edits, writes and bash reads by path. What is missing is the join: **no code attributes result bytes to the path or the source kind that produced them**, and nothing reconciles the parts against `messages()`.

This PRD adds that join and the surfaces that show it.

## 2. Goals

- **A residency breakdown of the whole window**: prefix (one row, drilling into the existing inspector) plus one row per message source, each with an estimated token count and a share, summing to `ContextView::size` by construction.
- **Per-file drill-down** inside the `files` row: path, tokens, reads, re-reads, sorted by tokens — the user's original request, in the place where it is truthful.
- **Honest residuals**: conversation text, thinking blocks and anything unattributed appear as a computed remainder, never as a silent shortfall, exactly as `Kind::Other` does in `prefix.rs`.
- **Truth about the boundary**: every figure resets at the last `compact_boundary`; the header states the reference point.
- **Actionable nudge**: one LATER rule on the prefix share, because §3.2 says that is where the tokens are.
- **Read-only, no prose**: only lengths, counts and paths. `content_chars` and `stdout_chars` are already lengths; no result text is retained, matching `transcript/`'s rule.

## 3. Findings

### 3.1 What the code does today

| Fact | Where | Consequence |
|---|---|---|
| `ContextView` holds `size`, `window`, `prefix`, `history`, `velocity`, `compactions`, `threshold`; `messages()` is `size − prefix` | `metrics/context.rs` | The container exists; `messages()` is the number to decompose |
| `prefix.rs` builds `Row { kind, name, bytes, tokens_est, count }` and reconciles `Kind::Other` as "first call minus the rows above" | `prefix.rs`, `ui/prefix_view.rs` (opened with `i`) | The reconciliation model to copy; the prefix row in the new view links here |
| `ContextCapture` parses `/context`'s table into `(category, tokens)` in printed order plus `Free space` | `transcript/local_command.rs:33,70,123` | Ground truth for calibration whenever the person ran `/context`; already on `Prefix::context_capture` |
| `tool_result.rs` parses `content_chars` (Read), `stdout_chars` / `stderr_chars` (Bash), `result_chars` (Agent), `persisted_output_size`, `truncated_by_token_cap`, `image_tokens()` (`w·h/750`, 1 500 unknown) | `transcript/tool_result.rs` | Every per-source length is already parsed |
| `Call { result_tokens_est, truncated, persisted_output_size, name, turn, input_summary }`; `top_ctx(n)` ranks by `result_tokens_est`; `State::reread_tax(call)` prices a result against later calls | `tools.rs:622`, `ui/state.rs:1655` | Per-call tokens exist; they are not grouped by source kind and not reset at a boundary |
| **`content_chars`, `stdout_chars` and `result_chars` have exactly one consumer** — `tools.rs:378` reads `image_tokens()`. Nothing else | `grep` over `src/` | The join is the whole of the new work |
| `files.rs::FileStats` keys `reads`, `edits`, `writes`, `reads_since_edit`, `cheap_reads`, `bash_reads`, `stale`, `lines_added/removed` by path; `reread_warning()` at ≥ 3 | `files.rs` | The per-path container exists and has no size field |
| `bash_read_paths(cmd)` splits on `\|` `;` `\n` `&&` and returns file arguments of `cat` / `head` / `tail` / `sed -n` only — **not `grep`** | `files.rs:333` | Reusable verbatim for decision 2; grep output stays unattributed by construction |
| A ranged `Read` (`is_ranged()`) and a `file_unchanged` result (~30 tokens) are already classed as cheap | `files.rs`, `tool_result.rs:356` | The cheap/expensive split the view needs already exists |
| `metrics/registry.rs` declares each metric once (id, unit, formula, sources, caveats, estimate); `docs/metrics.md` and the README block are generated and a test fails when stale | `metrics/registry.rs:128` | Every new figure needs a registry entry and a regeneration |
| **All nine digit slots are taken**: context, tokens, limits, turn, tools, agents, files, advisor, events | `ui/panels/mod.rs` | A tenth panel is impossible; a full-height inspector is the only shape |
| 36 rules today; the engine keeps one nudge slot and persists fires to `~/.cctop/<session>.advisor.json`; rules fire on structural evidence only | `advisor/mod.rs`, `advisor/rules/` | A 37th rule needs a registry entry, regenerated docs and a `coach-replay` check |

### 3.2 What was measured (2026-09-20)

**A. A live, uncapped transcript** — the session this PRD was written in, 56 API calls, no compaction, `~/.claude/projects/-home-user-cctop/db822b07….jsonl`:

| Figure | Tokens | Share |
|---|---|---|
| Context at the last call | 106,375 | 100 % |
| Prefix (first call's total, cctop's own definition) | 47,196 | **44 %** |
| Messages since | 59,179 | 56 % |
| — of which any tool result at all | 18,103 | 17 % |
| — of which attributable to a **file** | **0** | **0 %** |
| — residual (conversation, thinking, harness turns) | ~41,076 | ~39 % |

The zero is the finding. Every file read in that session went through `Bash` (`cat`, `sed -n`, `grep`), not the `Read` tool, so 15,656 tokens of file content entered the window with no `file_path` anywhere in the transcript. **A view keyed on `Read`/`Edit` inputs would have shown an empty panel for a session that spent 15.6 k tokens on file content.** This is what decision 1 and decision 2 exist for.

**B. Claude Code's own `/context` table**, preserved verbatim by the anonymiser in `fixtures/session-b.jsonl` and `fixtures/session-d.jsonl`:

| Category | Tokens | Share of 1 m window |
|---|---|---|
| System prompt | 10.2 k | 1.0 % |
| System tools | **31.2 k** | 3.1 % |
| Skills (49) | 4.8 k | 0.5 % |
| Messages | 42.9 k | 4.3 % |
| Free space | 877.9 k | 87.8 % |
| Autocompact buffer | 33 k | 3.3 % |
| MCP tools (297) | **0** | 0 % — loaded on demand |

Tool schemas are **3× the system prompt**, and 297 MCP tools cost nothing because they are deferred behind `ToolSearch`. That pair is the whole argument for decision 5: the actionable lever is the prefix, and the mechanism that fixes it already exists in Claude Code.

**C. The anonymiser caps every string at `MAX_STR = 4000`** (`scripts/anonymise-transcript.py:136`; `anon_text` returns `fill(min(len(s), MAX_STR))`). Same-length filler below the cap, truncation above it. Measured on `fixtures/session-d.jsonl`, the longest tool result is exactly 2 000 characters.

Consequence, and it is a hard constraint on §8: **fixtures cannot validate absolute token figures.** A real 30 k-token file read is a 4 k-character lorem-ipsum string in every fixture. Fixtures validate *structure* — row ordering, bash attribution, the boundary reset, that the residual reconciles — and the absolute arithmetic is validated against a live `ContextCapture` (decision 4) and in `docs/verification/`. Raising `MAX_STR` was considered and rejected: `fill()` emits lorem ipsum so there is no privacy cost, but `session-a.jsonl` is already 2.1 MB and the cap is what keeps the corpus committable.

### 3.2 D — Thinking (measured 2026-09-20, live transcript)

| Question | Answer | Evidence |
|---|---|---|
| Does the **main** session's transcript carry `thinking_tokens`? | **Yes**, on every assistant line | 99/99 lines, 42 unique API calls, `usage.output_tokens_details.thinking_tokens`; Σ 16,560 over the session |
| Do thinking **blocks** carry text? | **No** — signature only | 31 blocks of `type: "thinking"`, **0 characters** of `thinking` text between them |
| Is past thinking **resident** in later context? | **Yes**, on the evidence | Fitting Δcontext against `output + tool_result` (thinking resident) vs `output − thinking + tool_result` (dropped) over 36 usable calls: mean \|residual\| **828** resident vs **1,285** dropped. Resident fits better by about the magnitude of thinking itself |

Three consequences for §4:

1. The `Conversation` row **splits into `Thinking` and `Text`**, and `Thinking`
   is the only **exact** row in the view — it comes from Claude Code's own
   counter, not from `chars / 4`. It is marked as such; every other row keeps
   the `≈` mark.
2. It is the one row that **cannot** be estimated from characters, because the
   text is not in the transcript. If a future Claude Code stops writing
   `thinking_tokens`, the row disappears rather than degrading — a
   `harness_facts::first_seen` entry gates it.
3. Both models leave a **positive** residual of ~828 tokens per call. That is
   per-turn harness injection (system reminders re-sent each turn), and it
   belongs in `Text`. It is real context that no tool result explains.

### 3.2 E — The prefix grows mid-session, and `messages()` absorbs it

`ContextView::prefix` is the **first API call's** total. On the live transcript
two jumps are not explained by the previous output or by any plausible tool
result:

| At | Jump | Previous output | Unexplained | Almost certainly |
|---|---|---|---|---|
| call 1 | +20,379 | 641 | ~19,738 | MCP servers connecting — tool schemas arriving |
| call 34 | +8,662 | 882 | ~7,780 | the skills listing re-injected |

Together ~27.5 k, **22 % of all context growth in the session**. Because the
prefix is measured once at call 1, every one of those tokens is inside
`messages()` today, and under §4.1 the `Conversation` row would silently
absorb them and blame them on conversation.

**FR-10 follows from this** (§6). The reconciliation identity still holds — the
rows always sum to `size` — but a row would be lying about *what* it holds.

### 3.2 F — The `LatePrefix` threshold, swept (2026-09-20)

Per-call unexplained residual, `Δcontext − (previous output + tool results
between the two calls)`, over 5 fixtures + 1 live transcript (197 calls).

**Fixture residuals are biased high**: tool results are capped (§3.2 C), so the
subtracted term is understated and the residual absorbs the missing mass
(`session-a` p50 1,917 against the live transcript's 183). The threshold is
therefore taken from the live, uncapped transcript; fixtures only show how
badly the detector misfires on capped data.

Live transcript, sorted tail: `… 3,461 · 3,680 · **7,780** · **19,366**`.
The two known injections sit alone above a ×2.1 gap.

| Threshold | Fires on live | Verdict |
|---|---|---|
| 2,000 | 7 | 5 false |
| 3,000 | 4 | 2 false |
| **4,000 – 7,500** | **2** | **exact — the plateau** |
| 10,000 | 1 | misses the skills re-injection |

**`LATE_PREFIX_MIN = 5_000`**, centred in the plateau with 3,680 below and
7,780 above. On capped fixtures it fires on ~2 % of calls, all artefacts —
so a fixture test may assert that a *known* injection is caught, never that
an ordinary call is not.

### 3.2 G — Full-session simulation, validated against ground truth

A Python port of §4 was run over the whole corpus. `session-d` holds the only
native `/context` capture, so it is the only ground truth available:

| At the turn `/context` ran | Claude Code | The model | Error |
|---|---|---|---|
| **Total context** | 88,700 | **88,745** | **+0.05 %** |
| Prefix (system + tools + skills) | 46,200 | 61,667 | **+33 %** |
| Messages | 42,900 | 27,078 | **−37 %** |

The total is essentially exact. **The split is not**, and the two errors are
equal and opposite: `ContextView::prefix` — the first API call's total —
overstates the true fixed prefix by ~15.5 k, because that first call already
carries the opening user message and its attachments. Those 15.5 k are then
missing from `messages()`.

Consequences:

1. **Calibration is not a nice-to-have for the prefix row.** In `Estimated`
   mode the prefix row is systematically ~⅓ too large and every message row's
   *share* is correspondingly wrong. The header must say so, and the view
   should invite the person to run `/context` once — it is the cheapest
   accuracy the feature can buy.
2. **`session-b`'s capture is borrowed, not native.** Its capture text is
   byte-identical to `session-d`'s (sha1 `57bb2ec8a799f111`, 1 842 chars) while
   its real context at that line is 193,094 against the table's 88,700. The
   fixture composer copied it. Phase 2 of the plan must test calibration on
   **D only**; B would validate against a table that was never its own.

### 3.2 H — Three defects the simulation found

| # | Defect | Evidence | Fix |
|---|---|---|---|
| 1 | **Trailing tool results are counted but are not resident.** Results that arrive after the last API call have not entered any measured context, yet they are attributed — so the rows can exceed `messages()` and the identity breaks | `session-c` over-attributes by 503, `session-d` by 1,549; `Text` clamps to 0 and the total exceeds `size` | **FR-12**: count only results that landed *before* the last API call |
| 2 | **The prefix overstates** (§3.2 G) | +33 % on the only ground truth | Calibrate; mark the row in `Estimated` mode |
| 3 | **`Summary` cannot be a positive bucket.** Set from the boundary reading it already equals all of `messages()`, so anything attributed afterwards overflows — FR-12 alone left `session-d` over by 959 | simulation, after FR-12 | **FR-15**: one remainder row, computed last, named by context: `summary + text` when a boundary is in range, `text` otherwise |
| 4 | **After a compaction the view is nearly empty, and that is correct.** B, C and D have 1–3 calls since their boundary; their remainder is 92–100 % of messages | simulation, all three | Not a defect — the header says how many calls have happened since the boundary |

**After FR-12 and FR-15 the identity holds on all six transcripts with no
clamping** — re-run and verified, which is the only reason they are stated as
requirements rather than guesses.

| Transcript | Remainder | % of messages | Identity |
|---|---|---|---|
| session-a (capped) | text | 66 % | holds |
| session-b (post-compaction) | summary + text | 92 % | holds |
| session-c (post-compaction) | summary + text | 100 % | holds |
| session-d (post-compaction) | summary + text | 99 % | holds |
| session-e (capped) | text | 39 % | holds |
| **live (uncapped)** | text | **34 %** | holds |

Coverage: on the live transcript the model attributes **66 %** of messages
against the **33 %** a naive `chars / 4` over tool results reaches (§3.2 C
control) — it doubles what the obvious implementation would explain. The 34 %
remainder is a minority, which is the bar the view has to clear to be worth
drawing. `session-a`'s 66 % is a cap artefact, not a result.

## 4. Design

### 4.1 The source kinds

A new `src/metrics/residency.rs`, derived on read from `agg`, `state.tools` and `state.files` — no new collector, no new watcher, nothing added to `State::apply`.

```rust
pub enum Source {
    Prefix,          // ContextView::prefix; drills into prefix_view
    Files,           // per-path, see §4.2
    BashOutput,      // stdout/stderr of results with 0 or ≥2 resolved paths
    McpResults,      // calls whose name starts with "mcp__"
    AgentReturns,    // ToolUseDetail::Agent result_chars
    Web,             // WebFetch / WebSearch results
    Images,          // image_tokens()
    Thinking,        // usage.output_tokens_details.thinking_tokens — EXACT, §3.2 D
    LatePrefix,      // schemas and listings that arrived after call 1 — §3.2 E
    Text,            // the reconciled remainder: assistant prose, user turns,
                     // per-turn harness injections (~828 tok/call, §3.2 D)
}
```

`Thinking` is exact and summed from `thinking_tokens` (§3.2 D). `LatePrefix` is the sum of context jumps that no output or tool result explains (§3.2 E), detected as `Δcontext − (previous output + tool results between the two calls)` above a threshold. `Text` is computed, never summed: `messages() − Σ(every row above it)`, floored at zero — assistant prose, user turns and the ~828 tokens per call of re-sent harness reminders.

### 4.2 Attribution rules

| Input | Goes to | Rule |
|---|---|---|
| `ToolUseDetail::Read`, `kind == File` | `Files[path]` | `content_chars / 4`; a ranged read or `file_unchanged` contributes its real (small) size, not zero — it *is* in the window |
| `ToolUseDetail::Read`, `kind == Image` | `Images` | `image_tokens()`, `1500` when unknown |
| `Edit` / `Write` / `NotebookEdit` tool_use input | `Files[path]` | the call's own bytes: an `Edit` carries `old_string` + `new_string` and they ride the window like any other content |
| `Bash` result, `bash_read_paths(cmd).len() == 1` | `Files[path]` | `stdout_chars / 4` |
| `Bash` result, `len() == 0` or `≥ 2` | `BashOutput` | unattributed by decision 2 |
| `Bash` result with `persisted_output_size` | `BashOutput` | the *spilled* size never entered the window; count the result text only, and mark the row `⊘` as `top_ctx` already does |
| `mcp__*` result | `McpResults` | grouped by server (`name.strip_prefix("mcp__").split("__").next()`) |
| `Agent` result | `AgentReturns` | `result_chars / 4` — a subagent's own context never lands here, only its summary |
| everything else | `Conversation` | by remainder |

**Boundary reset.** Only calls whose `finished_at` is newer than the last `compact_boundary` are counted. `ContextView::compactions` already carries the turn; `agg` carries the timestamps. Before the first compaction the reference point is the transcript's start. When `compactions_heuristic` is set (transcripts before 2.1.263, the ≥ 30 % drop rule) the header marks the reference point approximate.

### 4.3 Calibration

Two modes, stated in the header:

- **estimated** (no `/context` capture this session): every row is `chars / 4`, `Conversation` is the remainder. Sound in proportion, off by whatever the real tokenizer does with code.
- **calibrated** (a `ContextCapture` exists): the attributed rows are scaled by `capture.category("Messages") / Σ(attributed + remainder)` and `Conversation` is recomputed against the captured figure. The prefix row takes `System prompt + System tools + Skills` from the capture rather than the first-call estimate.

A capture ages: it is a snapshot from the turn `/context` ran. The header carries its turn number so a capture from 40 turns ago is visibly stale. No persisted per-model ratio in v1 (decision 4).

### 4.4 The inspector

A sibling of `prefix_view`, full height, opened with `m` on the Context panel (`i` stays the prefix inspector; `m` for messages):

```
 Context sources — 106.4k of 1m, since session start  ·  estimated  (Esc back)
 SOURCE            TOKENS   SHARE  NOTE
 prefix            47,196    44%   system + tools + skills + CLAUDE.md   [i]
 ── messages ──────────────────────────────────────────────────────────
 files             12,430    12%   14 files · 3 with re-reads            [→]
 bash output        5,673     5%   unattributed (grep, builds, tests)
 mcp results        3,100     3%   github 2.1k · semrush 1.0k
 agent returns      1,900     2%   4 agents
 web / images         800     1%
 conversation      35,276    33%   assistant text, thinking, user turns
```

`→` on `files` opens the per-path drill-down — path, tokens, reads, re-reads, sorted by tokens descending, which is the user's original request. Panel 7 also gains a `tokens` column and `FileSort::Tokens` in the existing `s` rotation, so the number is visible without opening anything.

### 4.5 The coach rule

`prefix-heavy`, `advisor/rules/token.rs`, urgency **LATER**:

- **Evidence:** `ContextView::prefix / window ≥ 0.30`, with `prefix ≥ 20_000` so a small window does not trip it.
- **Text:** names the two largest prefix rows and what to do — defer MCP servers behind `ToolSearch`, trim the CLAUDE.md it names. Never quotes their content.
- **`acted`:** the prefix inspector or the sources inspector was opened. `State.agents_view_opens` exists and A48 reads it (`advisor/rules/token.rs:892`); the equivalent counter for these two inspectors **does not exist yet** and is part of US-005.
- **TTL / cooldown:** the prefix does not change within a session, so it fires once per session and snoozes long.

This makes 37 rules. `docs/metrics.md`, the README block and the site regenerate; `cctop coach-replay` must show no existing rule's fire count moving on fixtures A–D.

### 4.6 Query and MCP

`cctop query context` gains a `sources` array (one object per row: `kind`, `tokens` with a metric id, `share`, `note`) and a `files` array inside the files row. `mcp.rs` exposes the same object as a tool so a model can ask what is filling its own window. Both are mechanical once `residency.rs` exists — "one state, many surfaces".

## 5. User stories

### US-001: The join and its fixtures
`residency.rs` computes the rows from `agg` + `tools` + `files` with the §4.2 rules and the boundary reset. Tests on fixtures A–D assert row ordering, that the rows plus `Conversation` equal `messages()` exactly, that a single-path `cat` lands on its file and a two-path one does not, and that a `compact_boundary` resets the figures. **No test asserts an absolute token count** (§3.2 C).

### US-002: Calibration
`ContextCapture` reconciliation per §4.3, both modes, with the capture's turn number carried for staleness. Fixture B and D both hold a capture; a fixture without one exercises the estimated path.

### US-003: The inspector and Panel 7
The `m` inspector per §4.4, the `→` drill-down, the `tokens` column and `FileSort::Tokens`. Insta snapshots under `src/ui/snapshots`.

### US-004: Query, MCP, registry
The `sources` array, the MCP tool, the registry entries with `estimate` set and the caveats from §3.2 C, `docs/metrics.md` and the README regenerated.

### US-005: The coach rule
`prefix-heavy` per §4.5, the registry entry, the regenerated guide, and the `coach-replay` check on fixtures A–D.

## 6. Functional requirements

1. Every row is reset at the last context boundary and the header names the reference point.
2. The rows plus the reconciled remainder equal `ContextView::size` by construction; a shortfall is impossible.
3. A bash result is attributed to a path only when `bash_read_paths` resolves exactly one.
4. A result that spilled to disk (`persisted_output_size`) contributes only the text that entered the window, marked `⊘`.
5. The header states `estimated` or `calibrated`, and for `calibrated` the capture's turn.
6. No result text, prompt text or file content is retained — lengths, counts, paths and tool names only.
7. Panel 7 shows per-file tokens and can sort by them.
8. `cctop query context` and the MCP tool carry the same numbers as the inspector, tagged with registry ids.
9. The coach rule fires on the prefix share, not on files, at most once per session.
10. Context that arrived after the first API call and is explained by neither output nor a tool result is reported as `LatePrefix`, never folded into `Text` (§3.2 E).
11. `Thinking` is marked exact; every other row is marked `≈`. When `thinking_tokens` is absent the row is omitted, not zeroed.
12. Only tool results that landed **before the last API call** are attributed; trailing results are not yet resident and must not be counted (§3.2 H.1).
13. `LATE_PREFIX_MIN = 5_000` (§3.2 F).
14. In `Estimated` mode the prefix row carries a mark saying it overstates (§3.2 G); the header invites one `/context` run to make the split exact.
15. There is exactly **one** remainder row, computed last. A compaction summary is never a positive bucket (§3.2 H.3); the row is named `summary + text` when a boundary is in range and `text` otherwise.

## 7. Non-goals

- **The pane.** Not in v1 (decision 6); `tests/pane` and `scripts/pane-fixtures.sh` are untouched.
- **A tenth panel.** Impossible (§3.1); the inspector is the shape.
- **Splitting multi-path bash reads** by any heuristic (decision 2).
- **A persisted per-model chars→tokens ratio** (decision 4).
- **Counting a subagent's internal context.** Only its returned summary is in this window; its own spend is the agent PRDs' subject.
- **Acting on the window.** cctop never compacts, clears, or edits anything — it says what is there.
- **Parsing what a file contains** to judge whether it deserved the space.

## 8. Technical considerations

- **Budget:** no new collector, watcher or file handle. `residency.rs` is derived on read over `state.tools.calls` and `state.files`, both already in memory — O(calls) per render, the same order as `top_ctx`. The stated budget (≤ 2 % CPU idle, ≤ 5 % in a turn, ≤ 50 MB RSS) is unaffected.
- **Fixtures cannot check absolute tokens, and cannot check ranking either** (§3.2 C). Measured 2026-09-20: the same `chars / 4` estimator recovers **5–16 %** of real message tokens on fixtures A–E against **33 %** on an uncapped live transcript, so the cap costs 2–6×. Worse than the shrinkage is the flattening: a 3 k-character file and a 300 k-character one both land at the cap, so "sorted by tokens, descending" is *unfalsifiable* on fixtures — a test can prove the comparator runs, not that the order is real. Structure, reconciliation, bash attribution and the boundary reset are fixture-testable; absolute arithmetic and ranking are checked against a live `/context` capture and recorded in `docs/verification/`.
- **The corpus is not even internally consistent.** `session-a.jsonl` has 195 strings at exactly 4 000 characters; B–E have theirs at exactly 2 000 and none at 4 000. Two different caps were in force when the corpus was built, so cross-fixture comparisons of size are meaningless.
- **`bash_read_paths` is reused verbatim.** If it is ever widened to `grep`, the attribution rule inherits that and FR-3 must be re-read.
- **Registry first.** Adding a figure before its registry entry fails the staleness test; `cctop metrics --md` and `--readme` regenerate, never hand-edited.
- **`coach-replay` on fixtures A–D** before and after US-005; no existing rule's fire count may move.

## 9. Design considerations

The inspector borrows `prefix_view`'s frame, column widths and `Esc` behaviour so the two read as one pair. `coach.rs` / `dashboard.rs` cut text at fixed widths; the sources object follows the same discipline even though the pane does not draw it in v1, so adding the pane later is a renderer and nothing else.

## 10. Open questions

1. ~~Does the inspector key collide?~~ **Answered 2026-09-20.** `c` is taken twice in `app.rs` (l589, l635) plus `ctrl+c`. The bound set is `$ + - / 0 = ? A G L N S a c d e f g i j k l n p q s t x`; `m` is free, which is why §4.4 binds it.
2. ~~Should `Conversation` split into assistant text vs thinking?~~ **Answered 2026-09-20, yes — see §3.2 D.** It is the one row that can be exact.
3. **The prefix row in `calibrated` mode** takes three `/context` categories; if Claude Code renames or adds one, `harness_facts::first_seen` needs an entry. Worth checking against 2.1.274 before US-002.

## 11. Follow-ups

- The pane view (decision 6), once the inspector's rows have settled.
- A persisted chars→tokens ratio per model, if the estimated/calibrated gap proves stable over a dogfood week.
- A second rule on the `bash output` row if the dogfood week shows it routinely dominating — §3.2 A suggests it might, and "pipe it to a file" is an action.
