# PRD: context residency — what is in the window right now, and what put it there

**Status:** v1.0 · 2026-09-20 — **implemented**, Phases 1–5 one commit each (`86d2b86` `1e4fd8c` `db2c5f6` `633ebbd` `3e1fe32`), A17's threshold settled in `730c1a3` (§10.6). Sits beside the two spend PRDs but is not one of them: this is about *space*, not money.
**Target:** cctop ≥ 0.7.0 attached to Claude Code CLI ≥ 2.1.274 (`READ_FROM`).
**Depends on:** `tasks/prd-cctop.md` v1.1 (collectors, `attach.rs`, `metrics/`); the prefix inspector (`src/prefix.rs`, `src/ui/prefix_view.rs`) whose reconciliation model this PRD copies for the other half of the window.

> Decisions taken with the user on 2026-09-20, after the measurement in §3.2 replaced the original request:
> 1. **Not a files view — a residency view.** The request was "which files consume how much space". The measurement says a file-keyed panel answers under a fifth of the window and, in the session it was measured on, would have rendered empty. The view is keyed on **source**, one row per kind, and a file is one kind with a per-path drill-down.
> 2. **Bash reads are attributed by path, single-path commands only.** `files.rs::bash_read_paths` already resolves `cat` / `head` / `tail` / `sed -n`; when it returns exactly one path the result's `stdout_chars` are that file's, when it returns two or more they stay in the unattributed `bash output` row. Never split, never weight by size on disk.
> 3. **Residency, not cumulation.** Every figure is "since the last context boundary" — the last `compact_boundary` (or the transcript's start). The header says so. A session that just compacted shows small numbers because the window *is* small.
> 4. **Opportunistic calibration.** The raw estimate is `chars / 4`, the divisor `tools.rs::top_ctx` already uses. When the session has a `/context` capture (`ContextCapture`, already parsed in `transcript/local_command.rs`), the total is reconciled against its real `Messages` figure and the rows are scaled; the header says which mode it is in. No persisted per-model ratio in v1.
> 5. **One new coach rule, on the prefix share** — not on files. The lever the measurement found is the fixed prefix, and it is the one a person can act on. Urgency LATER, `acted` when the inspector is opened.
> 6. **Surfaces: the inspector, `cctop query`, `mcp.rs`, the coach rule. Not the pane in v1.** All nine digit slots in `ui/panels/mod.rs` are taken, so this is a full-height inspector like `prefix_view`, not a tenth panel.
>
> Added after the corpus sweep (§3.2 I, 2026-09-20):
> 7. **Per-step reconciliation against the exact Δcontext** (FR-16) replaces "the identity is the test". The three over-attributing transcripts are explained (§3.2 I, FR-18/19/20). **A `ModelSwitch` resets the rows only when its Δ is negative**; a switch that does not shrink keeps the rows and the header notes it (FR-17; corpus 6/4).
> 9. **The coach rule fires on an absolute prefix: `prefix ≥ 50_000` tokens**, not a share of the window or the size (§4.5). The prefix is paid on every request; its cost is absolute. **Amended after the replay (§10.6, `730c1a3`): the token arm reads the 50 k only against a calibrated prefix — a `/context` ran — because the raw first-call figure overstates by a third; the $/turn arm is unchanged.**
> 10. **Phases 1–5 run without a stop** — the user reviews the finished thing.
> 11. **Corrected before code:** `anatomy()` and A17 already exist (§3.1). Phase 1 upgrades `anatomy()` in place; Phase 5 retunes A17. Same goal, less new code, one anatomy.

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
| ~~`content_chars` … have exactly one consumer~~ **Corrected 2026-09-20:** nobody reads the raw lengths, but `tools.rs` folds them into `Call.result_tokens_est` / `input_chars`, and **`metrics/context.rs::anatomy()` already joins them** into a seven-slice `Anatomy` (prefix · inputs · results · thinking · harness · prose · other) drawn as the Context panel's stacked bar and sent to the pane as `Body.slices` (`dashboard.rs::body_context`, registry `context_anatomy`) | `metrics/context.rs:488–570`, `ui/panels/context.rs:72`, `dashboard.rs:693` | **This PRD upgrades `anatomy()`; it does not add a sibling.** Two anatomies of one window would be the disagreement the architecture forbids |
| `anatomy()` has every blind spot §3.2 I found: its reference is `since_boundary_turn()` = the last *explicit* `agg.boundaries` entry (no heuristic drop, no shrink, no model switch); it counts the in-flight call's own thinking / inputs / prose; prose is `prose_chars / 4` although prose sits *inside the exact `output_tokens`*; on overshoot it scales **prefix** down with the estimates | `metrics/context.rs:525–570`, `ui/state.rs:1454` | FR-12/16/18/19/20 apply to it; prose becomes exact (`out − thinking − inputs`) |
| **A17 `BigPrefix` (family `prefix-tip`) already nudges on the prefix**: ≥ $0.25/turn at the cache-read price or ≥ 100 k tokens; LATER; no `acted` | `advisor/rules/token.rs:906` | Decision 9 **retunes A17** (50 k, `acted` on opening an inspector) — no 37th rule |
| `Turn.harness_tokens` / `harness_approx` measure reminders, listings and injected files from `rendered[]` attachments (2.1.266+, ratios before) | `metrics/usage.rs:167,579` | The structural source for §3.2 E's "late prefix"; the residual heuristic is what is left *after* it |
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

### 3.2 I — The real corpus (139 transcripts, the user's machine, 2026-09-20)

A privacy-safe sweep script (numbers only, never content or paths) ported the
§4 model and was run over `~/.claude/projects` on the user's machine: 165
files, 139 usable, 8,823 API calls, Claude Code 2.1.247–2.1.278, four runs as
the script was corrected. What follows is what survived.

**Confirmed.**

| Claim | Corpus result |
|---|---|
| Coverage: the model attributes a majority of messages | **median 65 %**, IQR 58–72 %, over 119 transcripts — the single-session 66 % sits at the median |
| `output_tokens` includes `thinking_tokens` | Regressing the per-step residual on `think_prev`: slope **−0.07** (n = 136 clean steps). The cap `out − think` in §4.2 is right |
| Thinking is resident across **human-turn** boundaries | On every local transcript, 2.1.247–2.1.278: the "stripped at a new human message" model leaves a residual of +3 k to +30 k where the resident model leaves a few hundred to ~3 k. Not stripped |
| chars/4 on tool **results** does not overshoot | 31 steps with ≥ 500 estimated tokens injected, uncapped live transcript: estimate/exact **median 0.62**, overshoot on **0/31**. It undershoots code by ~40 %; it is not an over-attribution source |
| The prefix/messages split bias | The one real `/context` capture found reproduces +0.05 % total, **+33 % prefix** — but it is the same session `session-d.jsonl` was anonymised from (identical token figures). Still **n = 1** |

**Refined — `LatePrefix` is real, common, one-off, early; it is not bimodal.**

| Fires at 5 000 per transcript | Transcripts |
|---|---|
| 0 | 69 (50 %) |
| 1 | 41 |
| 2 | 13 |
| 3–4 | 12 |
| 5+ | 4 |

Position of the *first* fire: **median 8 %** through the session, p75 25 %.
Half of sessions never trip it; of the half that do, most trip it once, early.
That is the §3.2 E story (tools and skills landing at the start) — but the
pooled residual distribution is continuous (p95 2,621, p99 6,812, max
66,472), not two clusters with a gap, so **no single threshold is a clean
cut**. `LATE_PREFIX_MIN = 5_000` stays as an approximation of a continuous
phenomenon; the row is framed as "this session's one-time tool/skill cost",
not as an anomaly flag.

**Falsified — the identity check is a weak test, and the model missed a
whole class of event.**

Four hypotheses for three transcripts that over-attributed (`Σ rows > size`
by 309, 3,630 and 13,168) were tested and refuted by counters that read zero
on all three: a duplicated `tool_use`/`tool_result` line (retry, resume); an
orphan result; `len(json.dumps(tool_use.input))` re-escaping a large Edit
(+13 % measured — real, and fixed by measuring decoded string lengths like
every other bucket, but not the cause); and thinking stripped at human
turns (above). The two structural findings that came out instead:

1. **The identity `Σ rows == size` only fails when over-attribution exceeds
   `Text`'s headroom.** The live transcript carries a 28,700-token error
   and *holds*, because `Text` has ~50 k of slack to hide it in. Passing
   the identity is not evidence the rows are right.
2. **The measuring stick moves.** At exactly the `/model claude-sonnet-5`
   switch (line 445 → first sonnet call at line 460), the cache rebuilt
   from zero (`cache_read 219,492 → 0`, `cache_creation 214 → 191,006`)
   and the *same* conversation re-measured **28,700 tokens (13 %) smaller**.
   A different tokenizer, a different per-model prefix, or both — it cannot
   be told from one number. Every estimate is in model-independent chars/4;
   `msgs` is in whatever the current model's tokens are. 3 of 5 fixtures
   also switch model mid-session, all with non-negative Δ, so a switch does
   not always shrink — it depends on the pair.

**Resolved by the v4 per-call dump — three transcripts, three different
causes, each fixed and each reproduced on a synthetic case:**

| Transcript | What the numbers showed | Cause | Fix |
|---|---|---|---|
| #104 (33 calls, over by 13,168) | call 25: `Δ −22,748`, 83,199 → 60,451 — a **27.3 % drop**, under the 30 % heuristic; no boundary declared, buckets kept everything, `msgs` fell to 3,646 | content left the window unmarked | **FR-18**: any negative Δ is a boundary. cctop's own `COMPACTION_DROP_RATIO = 0.30` (`metrics/context.rs`) has the same blind spot — §11 |
| #60 (8 calls, over by 3,630) | calls 2/5/6/7: estimate over the exact budget by 8 %, 15 %, **42 %**, 5 % on large results | content that tokenizes better than 4 chars/token | **FR-16** per-step reconciliation removed exactly 5,768; corpus-wide it engages on 45 of 9,580 steps (0.5 %) |
| #129 (2 calls, over by 309) | `est 436` **at call 0** against `msgs 280` | content seen before the first API call was attributed to a message bucket — but the first call's context *carries* it, so it is inside `prefix` | **FR-19**: discard everything pending at the first commit |

A fourth defect surfaced while reproducing #104 synthetically: a boundary
that lands **below the first call's context** makes `msgs` negative
(`prefix 62,000 > size 60,700`), and then any attribution overflows. The
first call overstates the fixed prefix (§3.2 G, +33 %) because it carries
the opening message; the true prefix survives every compaction, so the
smallest context seen right after a boundary is a tighter upper bound on
it — **FR-20**, `prefix = min(prefix, ctx_after_boundary)`.

**v5 confirmation (same corpus, 140 transcripts):** `overflow_raw > 0` on
**1/140** — #60 only, `per-step 5,768 / global 0`, the estimate-variance
case FR-16 exists for. #104 and #129 are gone. Model changes on 9/140,
Δ at the switch negative on **6** (median −19,405) and non-negative on 4.

**The price of FR-18.** Making every negative Δ a boundary resets the rows
more often, so content before a shrink or a model switch is no longer
attributed: coverage moved from median 65 % (IQR 58–72) to **61 % (IQR
53–67)**. Four points of coverage for no phantom attribution of content
that has left the window. Still roughly double the naive 33 %.

**Two more, found while replacing `anatomy()` (Phase 1):**

- **The first turn's attachments are inside `prefix`.** `ContextView::prefix`
  is the first call's `cache_read + cache_write`, and Claude Code caches the
  whole first request — system prompt, tools, *and* the opening message's
  reminders and listings. On fixture A that is 6,095 tokens of harness the
  old bar counted a second time on top of the prefix (harness 9.1 k → 3.0 k;
  step 0's exact budget is the 2 uncached tokens). FR-19 as stated in
  cctop's own terms: step 0 reconciles against `ctx₀ − prefix`.
- **Fixture B's window starts at a model switch, not its compaction.** The
  compaction at call 109 (720 842 → 193 094) is followed one call later by a
  model change that re-measured the window 30 % smaller (193 094 →
  134 464). The reference is the later boundary (FR-17); turn 6's 7,759
  attachment tokens and its 355 of thinking all landed before it and are
  gone — the old bar showed both.

**Calibration, corpus-wide** (what per-step reconciliation had to remove):

| Source | Removed | Kept | Removed % |
|---|---|---|---|
| Images (flat 1 500/image in the prototype) | 19,726 | 341,774 | **5.5 %** |
| Web | 1,914 | 78,142 | 2.4 % |
| Files | 4,110 | 2,046,517 | 0.2 % |
| BashOutput | 2,941 | 2,368,634 | 0.1 % |
| McpResults | 195 | 139,387 | 0.1 % |

chars/4 is well calibrated for code; the prototype's flat 1 500 per image
is not — Phase 1 uses `tool_result.rs::image_tokens()` (`w·h/750`), which
already exists. Model changes: **9/140** transcripts, Δ at the switch
negative on 5 (median −15,372) and non-negative on 5.

**Consequences for §4 — FR-16, FR-17, FR-18.** Reconcile every step's
estimate against that step's *exact* growth, treat a model change as a
boundary kind of its own, and stop treating the identity as the test.

## 4. Design

### 4.1 The source kinds

**Not a new module — an upgrade of `metrics/context.rs::anatomy()`** (§3.1, corrected). A per-call walk `residency(agg, tools, …) -> Residency` carries the reference, the reconciliation and the source split; `Anatomy` is derived from it (`From<&Residency>`), so the Context panel's bar and the pane's `Body.slices` keep their seven labels and steps and simply become right. Derived on read; no new collector, nothing added to `State::apply`.

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

**Prose is exact, not estimated.** A call's `output_tokens` is text + thinking + tool-use JSON, and thinking is reported exactly — so `prose = out − thinking − inputs` per call, where `inputs` is the call's tool_use bytes capped at `out − thinking`. The old `prose_chars / 4` is dropped.

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

**A17 `BigPrefix` retuned** (`advisor/rules/token.rs:906`, family `prefix-tip`, urgency **LATER**) — it already exists (§3.1, corrected); no 37th rule:

- **Evidence:** the residency model's `prefix ≥ 50_000` tokens **in `Calibrated` mode** (a `/context` ran — §4.2), or the existing price arm, ≥ $0.25 per turn at the cache-read price on either figure — **decision 9 as amended in §10.6**. The prefix is paid on every request, so its cost is absolute (tokens × calls), not a share of the window. A share-of-window rule (the first draft's 0.30) is inert on a 1 m window however large the prefix; a share-of-size rule fires on half of all sessions (corpus median 32 %). The corpus did not collect windows, so neither ratio was validated; the absolute figure needs none. The token arm waits for the calibration because the raw first call overstates the prefix by a third (§3.2 G) and runs 47–62 k on a plugin-heavy setup: an `Estimated` figure over 50 k is not evidence, and the sources inspector's footer already invites the one `/context` that makes it so.
- **Text:** names the two largest prefix rows and what to do — defer MCP servers behind `ToolSearch`, trim the CLAUDE.md it names. Never quotes their content.
- **`acted`:** the prefix inspector or the sources inspector was opened — a new `State.inspector_opens`, bumped by both views, read the way A48 reads `agents_view_opens` (`token.rs:892`). A17 has no `acted` today.
- **TTL / cooldown:** fires once per session and snoozes long. (The prefix *does* change within a session — §3.2 E grows it, a model switch re-measures it — but the person's lever, which servers and files are always on, does not; once is enough.)

36 rules stay 36. A17's fire count on fixtures A–D may move (its threshold did); no other rule's may. `docs/metrics.md`, the README block and the site regenerate.

### 4.6 Query and MCP

`cctop query sources` is the inspector's object: size, window, prefix (share, tightened, mode), the reference, the rows with shares, the per-file list, `reconciled`, `overflow_raw`, a model switch kept. **On MCP there is no tenth tool**: `mcp.rs` keeps its schemas under 800 tokens (`descriptions_are_short_and_schema_is_small`) because they ride in every request — the very cost this PRD measures — and a tenth did not fit (887). `cctop_prefix` already answers "what is in the window" for the fixed half; it now returns the other half too, as an additive `sources` key, and its description says so. Mechanical once `residency()` exists — "one state, many surfaces".

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
16. **Per-step reconciliation.** At each API call, `Δcontext − previous output_tokens` is the exact number of tokens everything injected between the two calls could have cost. Estimates for that step are scaled down to fit it, never up; the amount removed per source is kept and reported (§3.2 I). `Σ rows == size` holds by construction and is **not** the correctness test — the pre-reconciliation overflow is.
17. **A model change is a boundary of its own kind** (`Reference::ModelSwitch`). Estimates before it are in a different model's tokens than `size`; the header names the switch and the Δ it produced. How the pre-switch rows are re-based (uniform scale by the observed ratio, or shown as one "before the switch" row) is decision 7, taken at Phase 1 with the v4 numbers in hand.
18. **Any negative Δcontext is a boundary.** Within a session the window never shrinks except by removal or re-measurement; the 30 % rule distinguished a compaction from anything else, but for residency both invalidate what was attributed before. Kinds: `Compaction` (explicit `compact_boundary`), `Heuristic` (≥ 30 % drop, unmarked), `Shrink` (a smaller drop, unmarked — #104's 27 %), `ModelSwitch` (the model changed on that call). All reset the rows; the header names the kind and the Δ.
19. **Content seen before the first API call is inside `prefix`**, never in a message bucket (#129).
20. **`prefix` is a running minimum.** It starts as the first call's context and is lowered to the context right after any boundary that lands below it. The first call overstates the fixed prefix (§3.2 G); each compaction tightens the bound.

## 7. Non-goals

- **The pane.** Not in v1 (decision 6); `tests/pane` and `scripts/pane-fixtures.sh` are untouched.
- **A tenth panel.** Impossible (§3.1); the inspector is the shape.
- **Splitting multi-path bash reads** by any heuristic (decision 2).
- **A persisted per-model chars→tokens ratio** (decision 4).
- **Counting a subagent's internal context.** Only its returned summary is in this window; its own spend is the agent PRDs' subject.
- **Acting on the window.** cctop never compacts, clears, or edits anything — it says what is there.
- **Parsing what a file contains** to judge whether it deserved the space.

## 8. Technical considerations

- **Budget:** no new collector, watcher or file handle. `residency.rs` is derived on read over `state.tools.calls` and `state.files`, both already in memory — O((calls + results + attachments) · log calls) per read: the call that carried each result, input and attachment is a binary search over the calls' timestamps, and a turn's first call a map lookup. The first cut scanned linearly and a 6 400-call transcript made `cctop coach-replay` thirty times slower (80 s against 2.7 s) and `cctop query dashboard` twice as slow; the review of PR #10 measured and fixed it (plan §3). The stated budget (≤ 2 % CPU idle, ≤ 5 % in a turn, ≤ 50 MB RSS) is unaffected.
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
4. ~~What over-attributed on the three corpus transcripts~~ **Answered 2026-09-20** — §3.2 I: a 27 % drop under the heuristic, chars/4 overshoot on large results, and pre-first-call content attributed as messages. FR-16, FR-18, FR-19, FR-20.
6. ~~A17's threshold (decision 9) — re-opened with evidence.~~ **Answered 2026-09-20 (`730c1a3`): the token arm fires only in `Calibrated` mode; 50 k stays; the $/turn arm is unchanged.** Shipped at 50 k as decided, `cctop coach-replay` on fixtures A–D moved A17 from 2 fires in 1 session to **5 fires in 4 of 4**, and the one-slot engine then displaced A01 (2 → 1 fires) and A10 (1 → 0 snoozed) — the plan's gate "no other rule's count moves" failed by consequence. The raw first-call prefix on this machine runs 47–62 k (A 60,582 · B 49,267 · D 61,667 raw, 46,200 calibrated · this session 47,196); §3.2 G says the raw figure overstates by a third; the pre-existing test's own comment had rejected 60 k as "every session's on a plugin-heavy setup". Of the options (keep 50 k and accept a nudge on most sessions; fire the token arm only in `Calibrated` mode; 75 k raw; back to 100 k) the user chose the calibrated gate: the figure is then Claude Code's own, D stays quiet at 46,200, and a raw 60 k session gets the `Estimated` footer's invitation to run `/context` instead of a nudge — "the cost is absolute" without nagging on an overstated number. Replay after: A17 2 fires in 1 session (the price arm, as before Phase 5), A01 2, A10 snoozed 1 — the gate holds again. Fixture A's pane fixtures lose the A17 row.
5. ~~Model-switch re-basing~~ **Decided 2026-09-20 (decision 7):** a switch whose Δ is negative is a `ModelSwitch` boundary (rows reset; 6 of the corpus's 9 switches); a switch whose Δ is non-negative keeps the rows, in the old model's tokens, and the header notes the switch (the other 4). Uniform rescaling by the observed ratio was rejected: one number cannot separate a tokenizer change from a per-model prefix change.

## 11. Follow-ups

- **`COMPACTION_DROP_RATIO = 0.30` in `metrics/context.rs` misses real drops** — the corpus has a 27.3 % one (#104) and 9/140 transcripts shrink below the heuristic. That constant feeds `ContextView::compactions` and the `threshold_learned` logic today; it should be re-examined against the corpus independently of this PRD.
- The pane view (decision 6), once the inspector's rows have settled.
- A persisted chars→tokens ratio per model, if the estimated/calibrated gap proves stable over a dogfood week.
- A second rule on the `bash output` row if the dogfood week shows it routinely dominating — §3.2 A suggests it might, and "pipe it to a file" is an action.
