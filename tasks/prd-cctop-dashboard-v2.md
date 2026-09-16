# PRD: dashboard v2 — zones, meters and one colour language

**Status:** v1.0 · 2026-09-16 — proposed, nothing implemented
**Target:** cctop ≥ 0.3.1 attached to Claude Code CLI 2.1.272 on macOS/Linux; the TUI and the function-hooks pane change together, `cctop query dashboard` changes with them.
**Depends on:** `tasks/prd-cctop.md` v1.1 (the nine panels, the metrics registry, the query interface) · `tasks/prd-cctop-coach.md` v1.3 (the four lights, the nudge slot, the urgency model) · `tasks/prd-cctop-pane.md` v1.2 (the second front-end).
**Supersedes:** `tasks/plan-dashboard-big-figures.md` §1 and §3 — the block-digit tiles and the nine-row ledger. The one-object rule of §2 (`dashboard::snapshot`, every surface draws it verbatim) is kept and extended.
**Design:** the canvas *cctop Dashboard Redesign* — page *Redesign* (Before, Main, Responsive, Anatomy, Audit), page *Colour & k9s* (Color, K9s). Working files in `tasks/design-dashboard-v2/`.

> Assumptions taken without pausing:
> A. The reader is one developer with a live session, glancing at the dashboard every few minutes while working elsewhere. They are not studying it.
> B. Nothing is deleted. Every figure the ledger shows today survives somewhere; the question this document answers is only *where*, and the three answers are first glance, second look, and behind a digit.
> C. The read-only rule and the no-prose rule are untouched. This is a rendering and information-architecture change; no new collector, no new source, no new write.
> D. Numbers below are measured from this repo — the committed insta snapshots at 40/60/80/100/122 columns, `src/dashboard.rs`, `src/ui/dashboard.rs`, `plugin/hooks/views/overview.tsx`, `src/metrics/registry.rs` — or computed with the palette validator. Where a figure comes from the coach PRD's 81-session corpus it says so.

---

## 1. Introduction

The dashboard works and nobody can read it quickly.

It renders four block-digit tiles, one nudge line and eighteen borderless ledger rows. At 122 × 24 that is the whole screen with every blank separator already dropped, and the fixture still loses the end of four rows to an ellipsis. In the pane it is worse: `tileRows()` lays the tiles two per row at half the pane width whatever the width is, so the tile block costs seven rows and carries four numbers and six short strings, the rest padding.

The four numbers are the problem in miniature. `35`, `59`, `4` and `0` are drawn identically, in the same type, at the same size — and they are a percent of a context window, a count of minutes, a percent of a rate limit, and a count of open issues. Their units sit two rows below them, on the baseline row, where `tileLines()` puts them; on the cache tile the unit lands flush against the sub-line and the screen reads `m misses 0`. A figure whose unit is two rows away and whose neighbour means something else entirely is not a figure, it is a decoration.

Underneath, the eighteen ledger rows are ` · `-joined prose. There is no column to scan, nothing right-aligned under anything else, and no way to compare row 2 with row 3. Each row is cut at a fixed 118 cells by `dashboard::cut` and then clipped again, silently and without a marker, by ratatui at the terminal edge — so the most specific fact on a row, which is almost always at its end, is the first to go and goes twice.

And the same fact is printed up to four times. The header's phase cell is character-for-character the value row of `4 Turn`. `warm 59m` is the cache tile's whole subject and appears again in row 2. `≈$.19/call` is on the context tile and in row 2's detail. The context percentage is the tile and the row-1 bar.

This document rebuilds the dashboard around three reading distances — a three-second glance, a fifteen-second second look, and one keypress — and fixes the colour language while it is open, because the context bar currently spends the status palette on categories and says the opposite of what it means.

## 2. Goals

- **Answer the five questions a glance asks**, in fixed positions, at every width: is it working or stopped on me · do I need to act · am I near a wall · what is this costing · is the work converging.
- **Give numbers a grid.** Fixed column stops for label, value, meter and percent, so the eye scans a column instead of reading a sentence. This is the single change that buys the most legibility per row.
- **One colour language, four roles, documented.** `ok` / `warn` / `crit` mean threshold state everywhere and nothing else; composition gets its own sequential ramp derived from each theme's `accent`.
- **Never truncate what must be read.** The act band wraps rather than cutting; anything that can be silently clipped is by definition not first-glance material, and there is a key that un-truncates the rest.
- **Reclaim rows.** The tile block's seven rows go; the rows go to content, and the chrome that remains can be collapsed by the person reading it.
- **Keep every figure.** Nothing is dropped from the product, only moved down a reading distance, and the overview prints the digit that opens the full table.
- **Keep the surfaces identical.** `dashboard::snapshot` stays the single object; the TUI, the pane, `cctop query dashboard` and the MCP tool draw it verbatim as they do today.

### Non-goals

- No new data source, collector, metric or rule. The registry gains no row (§10.2 is a rewording pass, not new readings).
- No change to the coach engine, its classes, TTLs or `acted` predicates. The act band is a new *rendering* of the existing slot.
- No mouse-first interaction. The pane keeps its clickable view chips; the TUI stays keyboard-driven.
- Not a general widget toolkit. Meters, tables and the act band are the vocabulary; a fifth kind of element needs a reason.

## 3. Evidence

### 3.1 What the screen costs today

Measured from the committed fixture-B snapshots and the two renderers.

| Fact | Measurement | Where |
|---|---|---|
| Tile block | 3 rows at ≥ 120 columns; **7 rows** at 60–119 and **always 7 in the pane** (`tileRows` is unconditionally two per row) | `src/ui/dashboard.rs::tile_block`, `plugin/hooks/views/overview.tsx::tileRows` |
| Tile payload | 4 figures + 6 sub-strings; at 61 cells per tile the digits occupy ≤ 7 cells of 61 | `tileLines(t, half)` |
| Unit placement | the unit is emitted on row index 2 only, `dw + 2` cells right of the digits | `tile_rows` / `tileLines` |
| Ledger | 9 rows + 9 detail rows = **18 rows**, drawn whether or not the row has content (`6 Agents —` costs two) | `compose()` |
| Row budget at 122 × 24 | header 1 + tiles 3 + nudge 1 + ledger 18 + footer 1 = 24, every blank separator already dropped | `fixture_b_dashboard_122x24.snap` |
| Truncation | values cut at `ROW_WIDTH = 118` with `…`, then clipped again at the terminal edge with no marker | `dashboard::cut`, ratatui `Paragraph` without `wrap` |
| Dead height | 80 × 40 renders 9 blank rows below the ledger while four rows above are ellipsed; 60 × 51 renders 21 | `fixture_b_dashboard_80x40.snap`, `…_60x51.snap` |
| Duplication | 4 facts printed twice or more (phase cell ≡ row 4 values; `warm 59m`; `≈$/call`; context %) | `dashboard.rs::header` vs `row_turn`, `tile` vs `row_tokens` |
| Footer | one static string in every view, including drill-downs that have their own keys (`s` sort, `Enter` calls, `a` agents) | `ui/dashboard.rs::FOOTER` |
| 40-column form | tiles collapse to `○14% ○— ○59m ●1 ▸` — four lights with nothing naming which is which | `fixture_b_dashboard_40x24.snap` |

### 3.2 Values that disagree — one real, one not

The screenshot that opened this work shows an overview and two expanded panels carrying values that contradict each other. Checked against the code, they are two different situations and only one of them is a defect.

**Cost — not a code defect.** Panel 2 shows `cost $0.000` beside row 2's `≈$22.5`. Both read the same accessor — `state.cost.current()` at `src/ui/panels/tokens.rs:94` and `src/dashboard.rs:450` — and `fmt::usd` emits three decimals only below `$0.01` (`src/ui/fmt.rs:27`). **One `State` cannot render both**, so the two boxes in that image were captured from different states: different moments, a different session, or a composite. There is nothing here to fix in the renderers. It is kept in this document because it is a good argument for FR-3: a screen that prints the same reading in two places invites exactly this confusion, in a bug report and in a user's head.

**Turn state — a real divergence, possibly by design.** Panel 4 shows `state idle`, `elapsed —` beside the header's `● WORKING · silent 1:56`. These genuinely read two sources: `src/ui/panels/turn.rs` reads `agg.current_turn()`, while `dashboard::header` takes the phase word from the classifier via `coach::snapshot`. A turn that has ended while tool activity continues will legitimately produce both answers — they are answers to different questions. The defect, if any, is that both are labelled as the session's state with no way to tell them apart.

CLAUDE.md's invariant is *"One state, many surfaces. Every number comes from the same pipeline, so the TUI, `cctop query`, the MCP tools and the pane never disagree."* The turn case does not break the pipeline rule; it breaks the naming. US-101 therefore verifies first and fixes only what reproduces.

### 3.3 The colour finding, computed

The context bar paints its five anatomy slices with `accent` / `ok` / `warn` / `crit` / `dim` — the status palette. Run through the palette validator against `default-dark`'s own surface, treated as what it is, a categorical palette:

```
Palette (dark, surface #0E1318, categorical): 5 slots
  [FAIL] Lightness band      outside band: #4CC2C2 0.748 · #5FC77E 0.749 · #E2B04A 0.784
  [FAIL] Chroma floor        below floor (reads gray): #5D6B7D 0.033
  [FAIL] CVD separation      worst adjacent #E2B04A↔#5FC77E ΔE 5.6 (protan) · tritan 2.4
  [FAIL] Normal-vision floor worst adjacent #5FC77E↔#4CC2C2 ΔE 9.9 — below 15
  [PASS] Contrast vs surface all 5 ≥ 3:1
```

`results` (green) and `prefix` (teal) are **adjacent segments of the same bar** and separate at ΔE 9.9 for readers with full colour vision — below the hard floor of 15, which secondary encoding does not excuse. `inputs` (amber) and `results` (green) separate at ΔE 5.6 under protanopia, below the 6–8 band where secondary encoding would make them legal at all.

The semantic failure is worse than the numeric one. Four rows above the bar, `crit` means *act now* and `ok` means *nothing to do*. In the bar, `crit` is `thinking` — 42 k of retained reasoning that the next context boundary discards whether or not anyone acts — and `ok` is `results`, the largest slice and the only one the person can meaningfully shrink today. **The colour tells the reader to worry about the one slice that needs no action and to relax about the one that does.**

### 3.4 What the five slices actually are

The ordering the bar should teach, from the registry and the coach PRD's corpus:

| Slice | Fixture | Can the person change it? |
|---|---|---|
| `prefix` | 44 k | Not this session. System prompt, CLAUDE.md and tool schemas, written at the first call and re-read on every call after (22.3 % of all token-calls, coach PRD §3.1). Between sessions: trim CLAUDE.md, drop unused MCP servers. |
| `harness` | ≈ 21 k | No. Reminders, injected files and listings Claude Code adds per turn. |
| `thinking` | 42 k | Nothing to do — dropped at the next context boundary. |
| `inputs` | ≈ 49 k | Yes. The text Claude wrote to call tools; Bash command text alone is 15.7 % of all token-calls. |
| `results` | ≈ 82 k | **Yes, and it is the biggest.** A main-context run of ≥ 8 read-only calls adds a p50 of 48.7 k for good; the same work in an Explore subagent returns a p50 of 71 tokens (coach PRD §3.1). |

### 3.5 What k9s settled

k9s has had a decade and an impatient audience on the same questions. Two of its answers are public mistakes, which are the useful ones.

| Pattern | k9s | cctop today |
|---|---|---|
| Per-view key hints in the chrome | header block, changes per view; `Ctrl-E` hides it | one static footer string everywhere |
| Reclaimable chrome | `Ctrl-E` header, `Ctrl-G` breadcrumb | tile block is mandatory at every size |
| Un-truncate | `Ctrl-W` wide columns | none — rows are cut twice, once silently |
| Faults only | `Ctrl-Z` | none — quiet rows are drawn anyway |
| Jump by name | `:` with aliases | digits 1–9 only, already fewer than the destinations |
| Filter in place | `/` regex, `-l` labels, `-f` fuzzy | none on any table |
| Sort by column | `Shift-O`, `Shift-N/A/P/S` | `s`/`S` on the turn ledger only |
| Navigation stack | breadcrumb, `[` `]`, `-` last view, `Esc` one level | `Esc` one level, no trail |
| Count in the title | `Pods(default)[12]` | already done — panel titles carry a summary figure |
| Inherit the terminal | skin `bg: default` | every theme paints its own background |

**Negative lesson 1 — issue #381, "Docs: Description of status colors", still open.** A user could not work out what k9s's colours meant: orange turned out to be both `Terminating` and `CrashLoopBackOff`, the definitions lived only in `skin.yml`, and the request was simply for documentation. cctop starts ahead of this — four named roles declared once in `themes/*.toml` — and throws the advantage away the moment a fifth meaning appears, which is exactly what §3.3 describes.

**Negative lesson 2 — issue #3589, closed as not planned.** A request to sort failing pods above healthy ones during incidents, with a full severity ordering. The maintainers declined. In a layout whose shape you have learned, a row that moves when something breaks costs more than it saves: you lose the position you navigate by at the moment you can least afford to. **This corrects the redesign**: zones keep fixed positions and never reorder by urgency (FR-4).

## 4. The dashboard

### 4.1 Three reading distances

| Distance | Budget at 122 columns | Contents |
|---|---|---|
| **First glance** — 3 s | 9 rows, fixed positions | status line · act band (3) · three meters + the anatomy legend (4) · spend (1) |
| **Second look** — 15 s | 6 rows, dimmer, may truncate | running tool + turn timings (2) · tools + files + agents (2) · event tail (2) |
| **Dig deeper** — one keypress | full screen behind `1`–`9` | every table, sortable, filterable, no ellipsis |

Twenty-one rows at 122 columns including the footer, against twenty-four today — and it carries a three-row act band instead of one, denominators on every meter, the running tool promoted out of a dim detail line, and the events as a grid instead of one ` · `-joined line.

### 4.2 The zones

```
 cctop  opus-5 · turn 1 · 52:11 · ~/code/cctop · v2.1.272            ● WORKING  52:11  ·  159 calls  1 err

 ▸ OPEN   steer window — 4 tool calls and 1:56 since Claude last spoke to you          type now to redirect
          nothing to fix — no advice queued, none snoozed                                        9 advisor
 ○ ok     rework 0  ·  ✓ cargo test 10s  ·  0 denied  ·  ⚠ 8 stale reads  ·  agents.rs ×3  ·  git +0/−0

 1 context    350k/1.00M   ▇▇▆▆▆▆▇▇▆▆▇▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁ 35 %   616k to autocompact  ·  +0/turn  ·  0 compactions
       fixed 65k   transient 42k   yours ≈131k
 3 limits     5h 4 %       ▇▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁  4 %   ↻ 4h06  ·  7d 22 %  ·  ×5 opus  ·  long ctx 78 %
 2 cache      warm 59m     ▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▁ 98 %   1h TTL  ·  0 misses  ·  re-writes 350k when cold
 2 spend      ≈$22.50      API-equivalent                        ≈$90/h  ·  ≈$.19/call  ·  ≈$29/turn  ·  not a bill

 4 running    Bash  cargo fmt --all && make check > /tmp/check.log                                            0:03
              api ≈21:17  ·  tools 22:14  ·  159 api calls  ·  1 steer  ·  waiting on you —
 5 tools      explore 69  test 39  implement 30 (1 ✗)  read 1  write 2              top ctx  Read …/agents.rs  10k
 7 files      27 touched  ·  agent_ledger.rs W×1 IDE  ·  task_notification.rs W×1 IDE            6 agents  —

 8 05:53  tool  Bash  cargo fmt --all && make check > /tmp/check.log     05:52  hook  PostToolBatch
   05:52  tool  Bash  cargo test harness_facts 2>&1 | grep -c ok         05:52  tool  Bash  ✓ 94 tok

 ?help   1-9 panels   c coach   a ask   t theme   q quit                               read-only · nothing is sent
```

**Status line.** `cctop` in accent, the session facts dim, the phase cell right-aligned in its level colour. The version moves to `?help`.

**Act band — three rows, always drawn, never truncated.** Row 1 is the one thing, prefixed by class (`NOW` crit · `NEXT` warn · `OPEN` ok when the only thing worth saying is that a steer window is open · dim when quiet). Row 2 is the action half, wrapped rather than cut, with `9 advisor` right-aligned. Row 3 is the health ribbon — rework, last check, denials, stale reads, uncommitted — which replaces the rework tile's bare `0` with a state word. A pending permission wait or `AskUserQuestion` pre-empts the nudge and paints the band `crit`: it is the most expensive state on the machine and today it is only in panel 4.

**Meters — one geometry, four rows.** Column stops at 122: digit gutter 0–2, label 3, value right-aligned to 14, meter 27–57, percent right-aligned to 58, detail from 65. `context` carries the stacked anatomy bar and one legend row; `limits` is the 5-hour window with 7-day, weight and the long-context flag as detail; `cache` is the countdown. `spend` has no denominator, so it keeps the label and value columns and leaves the meter lane to the words `API-equivalent` — a bar without a denominator is a lie, and the registry is explicit that a subscription is not billed per token.

**Second look — four rows.** What is running and for how long; where the turn's wall-clock went; the tool mix with the single biggest context consumer; files, with agents right-aligned. Then the event tail as a `TIME · KIND · WHAT` grid in two columns.

**Empty means gone (FR-5).** A zone with nothing to report draws nothing and gives its rows to the zone below. `6 Agents —` and its blank detail row stop existing; agents appears as a right-aligned clause on the files row when there are any.

### 4.3 The responsive ladder

Positions never move; detail columns give way in a declared order.

| Width | Form |
|---|---|
| ≥ 100 | as above; 30-cell meters |
| 60–99 | 16-cell meters; meter detail loses its third and fourth clause; the event tail keeps two rows and one column; label/value/percent stops keep their geometry |
| < 60 | first glance only: status, act band (2 rows), three meters at 8 cells, spend, what is running, and a key map naming every panel that is no longer drawn |

Claude Code un-docks the pane below `MIN_DOCK_COLUMNS = 110`, so the narrow form is the ordinary inline pane, not an edge case.

The give-way order is declared once and applied until the screen fits, instead of the four successive `if need(...) > h` tests in `compose()`: blank separators → meter detail clauses → event tail → tools/files/agents → turn timings → anatomy legend. The act band and the three meters never give way, and the last zone drawn takes the leftover height rather than leaving it blank (§3.1 dead height).

## 5. Colour and encoding

### 5.1 Four roles, and nothing else

| Role | Means | Never means |
|---|---|---|
| `fg` | a value | — |
| `dim` | label, unit, separator, structure | a value that matters |
| `accent` | a key you can press | anything static |
| `ok` / `warn` / `crit` | a threshold defined in the registry, not crossed / crossed / act now | a category, a series, a slice |

### 5.2 The series ramp

Composition gets a **sequential single-hue ramp derived from each theme's own `accent`**: lightness carries the order, the order is how much the person can do about the slice, and the near end is solved against that theme's background until it clears 3.2 : 1. No new key in `themes/*.toml`; a user-authored theme gets a correct ramp for free.

Generated by `node tasks/design-dashboard-v2/series-ramp.mjs` — re-run it if a theme changes and paste the table back.

| Theme | fixed | transient | yours | adjacent ΔE | min contrast |
|---|---|---|---|---|---|
| default-dark | `#516A69` | `#69AAAA` | `#7EF0F0` | 19.4 / 19.7 | 3.2 : 1 |
| default-light | `#768B8D` | `#316569` | `#003F46` | 15.1 / 13.7 | 3.2 : 1 |
| nord | `#768488` | `#95B7C1` | `#B4EEFE` | 15.6 / 15.9 | 3.2 : 1 |
| gruvbox | `#6F7774` | `#92A89F` | `#B7DBCD` | 15.1 / 15.0 | 3.2 : 1 |
| catppuccin-mocha | `#5E7275` | `#83B4BD` | `#A7FBFF` | 20.4 / 20.0 | 3.2 : 1 |
| btop | `#615A71` | `#A591CC` | `#EFCDFF` | 21.9 / 19.8 | 3.2 : 1 |

**Three slices on the overview, five in panel 1.** Five slices sit at ΔE 5.4–9.9 whatever the hue — too close for colour alone. Three clear the ≥ 15 floor on five of the six bundled themes; `default-light`'s tighter pair lands at 13.7, inside the 6–15 band where secondary encoding makes it legal, and `stacked_bar` already supplies it by alternating `▇` and `▆` between adjacent segments. Panel 1 keeps all five on one labelled row each, where identity comes from the label and never from the hue — which is why its ΔE of 5.4 on `default-light` is acceptable there and would not be on the overview.

The overview's three: `fixed` = prefix + harness · `transient` = thinking · `yours` = inputs + results.

### 5.3 The learning this encodes

The bar is not a chart of what happened; it is a list of levers, four of which are bolted down. Ordering the slices by agency and ramping the hue from dim to bright makes the bright block mean *this is the part you can move this afternoon* — without a legend, and without asking the reader to hold a second dictionary. It also keeps working with `NO_COLOR`, on a 16-colour terminal and in a non-UTF-8 locale, because lightness and the alternating glyph carry the order when hue cannot.

## 6. Navigation and chrome

| FR | Change | From |
|---|---|---|
| Per-view footer | the key hints are generated from the active view, so panel 5 advertises `s` sort and `/` filter where they apply | k9s header block |
| `Ctrl-E` | collapse the meter block to its one-line form and give the rows to the tables; the act band never collapses | k9s `Ctrl-E` |
| `w` | wide mode — rows wrap instead of being cut; the direct answer to §3.1's double truncation | k9s `Ctrl-W` |
| `z` | faults only — hide every zone that is quiet | k9s `Ctrl-Z` |
| `/` | filter the tools, files and events tables; `Esc` clears the filter before it leaves the view | k9s `/` |
| `s` | sort by column on **every** table, named in the footer | k9s `Shift-O` |
| `-` | previous view; a breadcrumb in the footer; `Esc` keeps meaning exactly one level up | k9s `[` `]` `-` |
| `bg = "default"` | a theme may inherit the terminal background | k9s skins |

`:` fuzzy jump is **deferred** (§11) — it is the largest of these and the least evidenced.

## 7. Functional requirements

- **FR-1** `dashboard::snapshot` remains the only place a surface's contents are decided. The TUI, the pane, `cctop query dashboard` and `cctop_dashboard` draw it verbatim; a fixture-B test holds the two surfaces row-identical, as `tests/pane` does today.
- **FR-2** Every figure carries its unit in the same cell run, on the same line.
- **FR-3** No fact is drawn twice on one screen.
- **FR-4** Zone order is fixed and never reorders by urgency, value or recency.
- **FR-5** A zone with nothing to report draws nothing and yields its rows.
- **FR-6** The act band is never truncated mid-fact; it wraps. Every other zone may truncate, and every truncation is marked — no silent clip at the terminal edge.
- **FR-7** `ok` / `warn` / `crit` are used only for a registry-defined threshold. Composition uses the §5.2 ramp.
- **FR-8** Every state on the screen is legible with `NO_COLOR`, on 16 colours, and in a non-UTF-8 locale, through glyph and position alone.
- **FR-9** A figure whose source is absent prints `—`, never `0`; a figure with no sample yet prints `—`, never a computed zero (§9, US-104).
- **FR-10** An estimate that a reader could mistake for a hard fact carries a word, not only `≈`: cost is labelled API-equivalent.
- **FR-11** The give-way order is declared once as data and applied in order until the layout fits; the last zone drawn absorbs leftover height.
- **FR-12** The footer is generated from the active view.

## 8. User stories

### US-101: Make turn state say which question it answers
**Description:** As a user, I want to know whether "idle" means the turn ended or the session is quiet, so that two boxes showing different words do not read as a bug.

**Acceptance Criteria:**
- [ ] A test renders panel 2's cost line and the spend zone from one fixture `State` and asserts the same string — a regression guard for the FR-3 case in §3.2, not a fix
- [ ] The turn-ended-while-tools-run case is captured as a fixture (`scripts/anonymise-transcript.py`, never hand-edited) and a test asserts what each surface says for it
- [ ] Panel 4's field is labelled for what it reads (the turn's own state) and the header's for what it reads (the phase classifier's word), so the two can differ without reading as a contradiction
- [ ] If the fixture shows a genuine divergence — the classifier reporting a phase with no tool activity and no open turn — that is a bug in `src/phase.rs` and is fixed there, with the registry row gaining the caveat
- [ ] No renderer papers over a difference by hiding one of the two values

### US-102: The zone layout, TUI
**Description:** As a user, I want status, act, meters, second look and events in fixed positions, so that a glance always lands in the same place.

**Acceptance Criteria:**
- [ ] `src/ui/dashboard.rs` composes the §4.2 zones; `tile_block`, `tile_rows` and `big_digits` are deleted with their tests, and `FOUR_TILES` / `TWO_TILES` / `L1_TILES` / `TILE_WIDTH` go with them
- [ ] Column stops per §4.2; a test asserts every emitted row is exactly `width` cells at 40 / 60 / 80 / 100 / 122
- [ ] FR-5: a zone with no content emits no row; `6 Agents —` is gone and agents rides the files row
- [ ] FR-3: the phase cell appears once; `warm`, `≈$/call` and the context percent appear once each
- [ ] Insta snapshots regenerated at 40 × 24, 60 × 51, 80 × 40, 100 × 30, 122 × 24

### US-103: The zone layout, pane
**Description:** As a user of the docked pane, I want the same screen the terminal draws.

**Acceptance Criteria:**
- [ ] `plugin/hooks/views/overview.tsx` draws the same zones; `tileRows` / `tileLines` / `engineTiles` are deleted
- [ ] `tests/pane` holds the fixture-B moments row-identical between the Rust and TypeScript renderers, as today
- [ ] The narrow form (< `MIN_DOCK_COLUMNS`) is §4.3's, not a truncation of the wide one
- [ ] `scripts/pane-fixtures.sh` regenerated for both fixtures

### US-104: Honest figures
**Description:** As a user, I want a number to say what it knows and to admit what it does not.

**Acceptance Criteria:**
- [ ] Context velocity prints `—` before it has two turns of sample, never `+0/turn` (registry: EMA over per-turn deltas)
- [ ] Cost carries `API-equivalent` on the overview and in panel 2 (registry caveat: subscription plans have no per-token bill)
- [ ] `8 stale` gains its noun and the `⚠` the registry defines at ≥ 3
- [ ] `4c +145` is spelled: tool calls since Claude last wrote, and the context those calls added
- [ ] The rework light's figure becomes a state word; `0` never means both "healthy" and "no data"
- [ ] `159 api calls · 159 tool calls` is confirmed or corrected — distinct `message.id`s and `tool_use` blocks rarely match exactly

### US-105: One colour language
**Description:** As a user, I want a colour to mean the same thing everywhere on the screen.

**Acceptance Criteria:**
- [ ] `Theme::series(n)` derives the §5.2 ramp from `accent`, solving the near endpoint against `bg` for ≥ 3.2 : 1; no new TOML key
- [ ] A test asserts monotone lightness and the contrast floor for all six bundled themes and for a synthetic theme with an extreme accent
- [ ] The overview's context bar carries three slices, panel 1 five; `stacked_bar`'s alternating `▇`/`▆` is kept as secondary encoding
- [ ] `ok` / `warn` / `crit` appear in no composition bar; a grep test in CI would be over-fitting, so this is enforced by review and by the ASCII snapshot
- [ ] `docs/metrics.md` gains the encoding note for `context_anatomy`; the panel guide gains §5.3's sentence in plain words
- [ ] The ASCII / `NO_COLOR` snapshot still distinguishes all three slices

### US-106: Reclaimable chrome and the un-truncate key
**Description:** As a user on a short or narrow terminal, I want to trade chrome for content.

**Acceptance Criteria:**
- [ ] `Ctrl-E` collapses the meter block to the coach's one-line form and back; the act band is unaffected; state persisted in `~/.config/cctop/config.toml`
- [ ] `w` toggles wide mode: rows wrap instead of cutting, in both surfaces
- [ ] `z` hides every quiet zone
- [ ] FR-11: the give-way order is a declared list; a test drives 24 heights at 122 columns and asserts no blank row below a truncated one
- [ ] All three keys appear in the per-view footer and the help overlay

### US-107: Per-view footer, filter, sort, breadcrumb
**Description:** As a user, I want each view to tell me what I can do in it.

**Acceptance Criteria:**
- [ ] `Panel::keys(&State) -> Vec<(key, label)>` feeds the footer; the overview lists its own, each panel its own
- [ ] `/` filters the tools, files and events tables; `Esc` clears the filter before leaving the view
- [ ] `s` sorts every table by the selected column, named in the footer
- [ ] `-` returns to the previous view; the footer carries a breadcrumb; `Esc` is exactly one level up everywhere
- [ ] `bg = "default"` in a theme inherits the terminal background

### US-108: The downstream surface
**Description:** As a reader of the site and the docs, I want them to describe the dashboard that ships.

**Acceptance Criteria:**
- [ ] README's `## What it looks like` mockup and the `panels:start` block regenerated; the four `build-site.sh` regexes that parse the old layout are rewritten (§10.1)
- [ ] `site/index.template.html` §2 prose ("Four lights, one nudge, nine rows", the tile paragraph, the Fig. 2 caption, the key list) rewritten for zones
- [ ] `scripts/build-guide.py`'s `coach_*` entries and the sample lines regenerated; a guide page for the meters
- [ ] `make demo` regenerates `two-pane.*`, the six `theme-*.png`, `demo.gif` / `.webm`; `site/demo.tape` updated for the new keys
- [ ] `og-image.png` regenerated if it shows the dashboard
- [ ] `docs/query.md` updated if the `dashboard` object's shape changes; `cctop metrics --md` and `--readme` re-run

## 9. What this does not answer

- **`:` fuzzy jump** (§6) is deferred. cctop has roughly a dozen destinations, digits reach nine of them, and the evidence that the remaining three need a command grammar is weak. Revisit when the count passes fifteen.
- **Whether the overview should keep a hero number at all.** This document says no — meters with a shared geometry beat one big figure, and four big figures in four units beat nothing. If dogfooding says a single spend or context figure is missed from across a room, it returns as *one* figure with its unit attached, not four.
- **The ledger's `Panel::ledger` contract.** Zones no longer map one-to-one onto panels (`agents` rides `files`, `cache` and `spend` both carry digit `2`). Whether `Panel` keeps a ledger method or the zones become their own table is a plan-level decision (plan §2, Phase 1).
- **Whether §3.2's turn divergence is a defect or two honest answers.** A code question, not a design one. US-101 settles it with a fixture; the redesign does not depend on the outcome.
