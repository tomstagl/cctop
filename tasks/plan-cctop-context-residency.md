# Implementation plan: context residency

For `tasks/prd-cctop-context-residency.md` v1.0. Same shape as
`tasks/plan-cctop-coach.md`: phases map to commits, each phase leaves
`make check && npm test` green, and §1 gains a "shipped in" column as
commits land.

## 1. The shape of the work

Five phases, roughly in the PRD's story order. The first is the only one
that thinks; the rest are the surfaces, which are mechanical once the
state exists ("one state, many surfaces").

| Phase | Stories | Touches | Shipped in |
|---|---|---|---|
| 1 — The join | US-001 | `src/metrics/residency.rs` (new), `metrics/mod.rs`, `files.rs` | |
| 2 — Calibration | US-002 | `residency.rs`, `prefix.rs` | |
| 3 — The inspector | US-003 | `ui/sources_view.rs` (new), `ui/panels/context.rs`, `ui/panels/files.rs`, `ui/state.rs` | |
| 4 — Query, MCP, registry | US-004 | `query.rs`, `mcp.rs`, `metrics/registry.rs`, `docs/metrics.md`, `README.md` | |
| 5 — The coach rule | US-005 | `advisor/rules/token.rs`, `ui/state.rs`, registry, site | |

Nothing in `plugin/` is touched (PRD §7). Nothing is added to
`State::apply` — `residency.rs` is derived on read, like `metrics/`.

## 2. Phases

### Phase 1 — The join (US-001)

`src/metrics/residency.rs`, pure, no I/O:

```rust
pub struct Row { pub source: Source, pub tokens_est: u64, pub note: String }
pub struct Residency {
    pub rows: Vec<Row>,          // Prefix first, then messages, Conversation last
    pub files: Vec<(String, u64)>,   // path → tokens, sorted descending
    pub since: Reference,        // SessionStart | Compaction { turn, heuristic: bool }
    pub mode: Mode,              // Estimated (Phase 2 adds Calibrated)
}
pub fn view(state: &State) -> Residency;
```

Order of work:

1. `Reference`: find the last `compact_boundary` from `ContextView::compactions`
   and map it to a timestamp via `agg`; carry `compactions_heuristic` into
   `Reference::Compaction.heuristic`. Everything after filters on it.
2. Classify each `tools::Call` newer than the reference by the PRD §4.2
   table. The `Bash` arm calls `files::bash_read_paths` — make it `pub(crate)`
   (it is private today, `files.rs:333`) rather than duplicating it.
3. `Thinking`, exact, summed from `usage.output_tokens_details.thinking_tokens`
   over the calls after the reference point (§3.2 D) — gate it on a
   `harness_facts::first_seen` entry so an older transcript omits the row
   rather than zeroing it.
4. `LatePrefix` (§3.2 E): per call, `Δcontext − (previous output + tool
   results in between)`; anything above the threshold is schema or listing
   injection, not conversation. Pick the threshold from the corpus, not by
   taste — **swept, PRD §3.2 F: `LATE_PREFIX_MIN = 5_000`**, the centre of a
   4 k–7.5 k plateau that detects both known injections and nothing else.
5. **One** remainder row, computed last (PRD FR-15), named `summary + text`
   when a boundary is in range and `text` otherwise. A compaction summary is
   never a positive bucket: set from the boundary reading it already equals
   all of `messages()` and everything attributed afterwards overflows.
   If the subtraction ever clamps, the attribution is wrong — see the next item.
6. **Count only results that landed before the last API call** (PRD FR-12).
   Without this the rows exceed `messages()` and the identity breaks:
   measured, `session-c` over-attributes by 503 and `session-d` by 1,549.
7. `files` map: `Read`/`Edit`/`Write` by `file_path`, single-path bash by
   its resolved path.

Tests on fixtures A–D, none asserting an absolute token count (PRD §3.2 C):

- rows are in `Source` order and `Text` is last;
- `Thinking` is exact and marked so; it is omitted, not zero, on a transcript
  without `thinking_tokens`;
- a synthetic call with a large unexplained Δcontext lands in `LatePrefix`
  and not in `Text`;
- `Σ rows == ContextView::size` exactly, on every fixture, **with no clamping**
  — a clamped `Text` is the bug of PRD §3.2 H.1, not a rounding detail;
- a single-path `cat` lands on its file; a two-path `cat a b` lands in
  `BashOutput` and on neither file;
- a fixture with a `compact_boundary` reports `Reference::Compaction` and
  excludes calls older than it;
- a `persisted_output_size` result contributes only its result text.

Verified on the prototype before writing the Rust: with FR-12 and FR-15 the
identity holds with no clamping on all five fixtures and the live transcript.
Without them, `session-c` over-attributes by 503 and `session-d` by 1,549.

Commit: `Context: residency — what is in the window, by source`.

### Phase 2 — Calibration (US-002)

`Mode::Calibrated { capture_turn }` per PRD §4.3. `Prefix::context_capture`
already holds the `ContextCapture`; read `Messages` and scale, and take the
prefix row from `System prompt + System tools + Skills`.

Open question 3 of the PRD is settled here, not deferred: read the three
category names off Claude Code 2.1.274 before writing the lookup, and if
any is new or renamed add a `harness_facts::first_seen` entry so older
transcripts fall back to `Mode::Estimated` rather than mis-scaling.

**Fixture D only.** `session-b`'s capture is byte-identical to D's and does not
match B's own context at that line (PRD §3.2 G.2) — the composer copied it.
Testing calibration on B would validate against a borrowed table. A, B, C and E
exercise the estimated path.

Commit: `Context: calibrate residency against /context when it was run`.

### Phase 3 — The inspector and Panel 7 (US-003)

`ui/sources_view.rs` is `prefix_view.rs` with different rows — same block,
same column widths, same `Esc`. Bind `m` on the Context panel beside `i`
(`ui/panels/context.rs:45`; `m` is free, PRD §10.1). `→` on the `files`
row opens the per-path list.

Panel 7 gains a `tokens` column and `FileSort::Tokens` in the existing `s`
rotation (`ui/state.rs:598`) — the cheap half of the original request,
visible without opening anything.

Insta snapshots under `src/ui/snapshots`: the inspector at 110 and 162
columns, the drill-down, Panel 7 with the new column.
`INSTA_UPDATE=always cargo test`, then strip `assertion_line:` and delete
`*.snap.new` (CLAUDE.md).

Commit: `Context: the sources inspector on m, tokens on Panel 7`.

### Phase 4 — Query, MCP, registry (US-004)

Registry entries **first** — adding a figure before its entry fails the
staleness test. One per source row plus `file_tokens`, each with
`estimate` set and the §3.2 C caveat ("chars ÷ 4; fixtures cannot validate
absolute tokens"). Then:

- `query.rs`: a `sources` array on the `context` verb, each value tagged
  with its registry id through `m(…)`, and `files` inside the files row;
- `mcp.rs`: the same object as a tool;
- regenerate: `cctop metrics --md > docs/metrics.md && cctop metrics
  --readme README.md`, then `make site`.

Commit: `Context: residency in query, MCP and the registry`.

### Phase 5 — The coach rule (US-005)

`prefix-heavy` in `advisor/rules/token.rs`, urgency LATER, per PRD §4.5.
Add the inspector-opens counter to `State` (it does not exist yet — only
`agents_view_opens` does) and bump it from both `prefix_view` and
`sources_view`; `acted` compares against the fired mark, the pattern at
`advisor/rules/token.rs:892`.

Before and after: `cctop coach-replay` on fixtures A–D, and no existing
rule's fire count may move. 37 rules; the registry, `docs/metrics.md` and
the guide regenerate.

Commit: `Coach: a LATER nudge when the fixed prefix dominates the window`.

## 3. Risks and decisions taken early

- **The residual is the biggest row.** ~39 % on the measured session.
  That is correct and must read as correct — the note column says what it
  holds, and Phase 1's reconciliation test is what keeps it honest.
- **`bash_read_paths` is shared, not copied.** Making it `pub(crate)`
  couples `residency.rs` to `files.rs`'s definition of a read. That is
  intended: if it ever learns `grep`, both should change together, and
  PRD FR-3 gets re-read.
- **Fixtures cap strings** (`scripts/anonymise-transcript.py:136`) — at 4 000
  in session A, at 2 000 in B–E. Measured: `chars / 4` recovers 5–16 % of real
  message tokens on fixtures against 33 % live. No test may assert an absolute
  token figure **or a ranking between two files** — the cap flattens both to
  the same value. Reviewers should push back on any test that does.
- **`Thinking` is the one exact row** and comes from a different mechanism
  (Claude Code's counter, not characters). Do not let it drift into the
  `chars / 4` path during review.
- **A stale capture mis-scales.** A `/context` from turn 4 read at turn 90
  describes a different window. Phase 2 carries the turn number and the
  header shows it; if dogfooding says that is not enough, expiring the
  capture after N turns is the fallback.
- **Snapshot churn.** Phase 3 touches Panel 7, which has snapshots and a
  fixture test asserting a literal summary string
  (`ui/panels/files.rs:250`). Expect to regenerate.

## 4. What is deliberately not in the plan

The pane (PRD §7), a persisted chars→tokens ratio, a second rule on the
`bash output` row, and any splitting of multi-path bash reads. The first
and third are PRD §11 follow-ups to revisit after a dogfood week.
