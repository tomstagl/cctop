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
3. `Conversation` last, as `messages().saturating_sub(sum of the rest)`.
4. `files` map: `Read`/`Edit`/`Write` by `file_path`, single-path bash by
   its resolved path.

Tests on fixtures A–D, none asserting an absolute token count (PRD §3.2 C):

- rows are in `Source` order and `Conversation` is last;
- `Σ rows == ContextView::size` exactly, on every fixture;
- a single-path `cat` lands on its file; a two-path `cat a b` lands in
  `BashOutput` and on neither file;
- a fixture with a `compact_boundary` reports `Reference::Compaction` and
  excludes calls older than it;
- a `persisted_output_size` result contributes only its result text.

Commit: `Context: residency — what is in the window, by source`.

### Phase 2 — Calibration (US-002)

`Mode::Calibrated { capture_turn }` per PRD §4.3. `Prefix::context_capture`
already holds the `ContextCapture`; read `Messages` and scale, and take the
prefix row from `System prompt + System tools + Skills`.

Open question 3 of the PRD is settled here, not deferred: read the three
category names off Claude Code 2.1.274 before writing the lookup, and if
any is new or renamed add a `harness_facts::first_seen` entry so older
transcripts fall back to `Mode::Estimated` rather than mis-scaling.

Fixtures B and D hold captures; A and C exercise the estimated path.

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
- **Fixtures cap strings at 4 000 chars** (`scripts/anonymise-transcript.py:136`).
  No test may assert an absolute token figure. Reviewers should push back
  on any test that does.
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
