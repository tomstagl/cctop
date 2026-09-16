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
- **Keep the surfaces identical.** `dashboard::snapshot` stays the single object; the TUI, the pane and `cctop query dashboard` draw it verbatim. (There is no dashboard MCP tool today and this work does not add one.)

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
| Truncation | cut **twice at two different widths**: `ROW_WIDTH = 118` in the object, then `width − LEDGER_GUTTER` in the renderer. Both mark with `…`, so nothing is lost silently — but the object cuts to a width the terminal may not have, and the second cut always lands on the useful end of the line | `dashboard::cut` at `src/dashboard.rs:197`, `spans()` at `src/ui/dashboard.rs:53` |
| Dead height | 80 × 40 renders 9 blank rows below the ledger while four rows above are ellipsed; 60 × 51 renders 21 | `fixture_b_dashboard_80x40.snap`, `…_60x51.snap` |
| Duplication | 4 facts printed twice or more (phase cell ≡ row 4 values; `warm 59m`; `≈$/call`; context %) | `dashboard.rs::header` vs `row_turn`, `tile` vs `row_tokens` |
| Footer | one static string in every view, including drill-downs that have their own keys (`s` sort, `Enter` calls, `a` agents) | `ui/dashboard.rs::FOOTER` |
| 40-column form | tiles collapse to `○14% ○— ○59m ●1 ▸` — four lights with nothing naming which is which | `fixture_b_dashboard_40x24.snap` |

### 3.2 Two readers of one session, disagreeing by 2250×

The screenshot that opened this work shows an overview and two expanded panels carrying values that contradict each other. My first reading was that the cost pair could not be a code defect, because both sides call `state.cost.current()` and `fmt::usd` (`src/ui/fmt.rs:27`) emits three decimals only below `$0.01`, so one `State` cannot render both. **That inference runs backwards.** Both readings were on one screen. If one `State` cannot produce both, then the two surfaces were not reading one `State` — which is precisely the invariant CLAUDE.md states.

`src/metrics/cost.rs:172–191` says how. `current()` has two branches:

- **authoritative** — `Some(c) => { usd: c.total_cost_usd + since, approx: any_since }`
- **estimate** — `None if any_since => { usd: since, approx: true }`

`$0.000` printed *without* `≈` is reachable only through the authoritative branch with a near-zero total and no un-consumed estimates. `≈$22.5` *with* `≈` needs `any_since`. And `src/metrics/cost.rs:138–142`: a `Line::CostState` sets `authoritative` **and clears `self.since`**. So the screenshot is one reader that had consumed a `cost-state` line reporting ≈ 0 and one that had not — two processes at different points in the same transcript, or two sessions.

A 2250× disagreement between two live surfaces, located in `src/metrics/`. **This is the highest-value finding in this document, and it is not a layout problem at all.**

**Turn state is a second, separate divergence.** `src/ui/panels/turn.rs:34` reads `state.agg.current_turn()`; `dashboard::header` and `row_turn` read `c.state` from `coach::state_line` (`src/coach.rs:1038`). A turn that has ended while tool activity continues legitimately produces both answers — they answer different questions under one label.

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
| Filter in place | `/` regex, `-l` labels, `-f` fuzzy | exists but is inconsistent: `f` with an inline field on tools (`tools.rs:118`), `/` with `n`/`N` jump on events (`events.rs:107`), nothing on files or the ledger |
| Sort by column | `Shift-O`, `Shift-N/A/P/S` | exists but is undiscoverable: `s`/`S` on tools (`tools.rs:113`) and the ledger (`ledger_view.rs:39`), `s` on files (`files.rs:76`), nothing in any footer |
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

- **FR-1** `dashboard::snapshot` remains the only place a surface's contents are decided. The TUI, the pane and `cctop query dashboard` draw it verbatim. (There is **no** `cctop_dashboard` MCP tool — `src/mcp.rs` exposes eight and none of them is the dashboard. Adding one is out of scope.) A fixture-B test holds the two surfaces row-identical; **that harness does not exist yet** and US-103 builds it.
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

### US-101: Two readers of one session must not disagree about cost
**Description:** As a user, I want every surface reading one session to report one cost, so that a 2250× gap between two windows is impossible.

**Acceptance Criteria:**
- [ ] The §3.2 mechanism is reproduced in a test: one reader that has consumed a `cost-state` line and one that has not, over the same transcript, and the assertion is that they agree
- [ ] `Cost::current()` no longer lets a cleared `since` and a near-zero `total_cost_usd` present as a confident `$0.000` — either the authoritative branch keeps the estimate it displaced, or a near-zero cost on a non-trivial session carries `≈` and a caveat
- [ ] Fixed in `src/metrics/cost.rs`, **not** in a renderer, and not by hiding one of the two values
- [ ] The registry row for `cost` gains the caveat if both readings are legitimate at different points
- [ ] Turn state, separately: panel 4's field is labelled for what it reads (the turn) and the header's for the phase classifier, so the two can differ without reading as a contradiction. A turn-ended-while-tools-run fixture (`scripts/anonymise-transcript.py`, never hand-edited) pins what each says
- [ ] Ships on the layout that exists today and changes no schema

### US-102: The zone layout, TUI
**Description:** As a user, I want status, act, meters, second look and events in fixed positions, so that a glance always lands in the same place.

**Acceptance Criteria:**
- [ ] `src/ui/dashboard.rs` composes the §4.2 zones; `tile_block`, `tile_rows` and `big_digits` are deleted with their tests, and `FOUR_TILES` / `TWO_TILES` / `L1_TILES` / `TILE_WIDTH` go with them
- [ ] Column stops per §4.2; a test asserts every composed row is **≤ `width` cells measured before render** (a ratatui `TestBackend` buffer is `width` cells by construction, so measuring post-render is vacuous) at 40 / 60 / 80 / 100 / 122, on both surfaces
- [ ] FR-5: a zone with no content emits no row; `6 Agents —` is gone and agents rides the files row
- [ ] FR-3: the phase cell appears once; `warm`, `≈$/call` and the context percent appear once each
- [ ] Insta snapshots regenerated at 40 × 24, 60 × 51, 80 × 40, 100 × 30, 122 × 24

### US-103: The zone layout, pane
**Description:** As a user of the docked pane, I want the same screen the terminal draws.

**Acceptance Criteria:**
- [ ] `plugin/hooks/views/overview.tsx` draws the same zones; `tileRows`, `tileLines`, `engineTiles`, `TILES_MIN`, `L2_MAX`, `type Tile` and `tileOf` are deleted, and `dashboardOf`'s hard `tiles` requirement with them
- [ ] `frame.tsx`'s `bigDigits` / `BIG_GLYPHS` is deleted **in the same commit** as `tileLines`, its only caller
- [ ] **A fixture-B row-identity harness is built.** It does not exist today: `coach.test.ts` covers the Coach card, `overview.test.ts` uses fixture A and regexes rather than Rust-rendered rows, and the `-b` dashboard fixtures are generated but read by no test
- [ ] The pane reads `schema` and renders one line naming the cctop it needs, instead of waiting forever on `dashboardOf → null`. There is no binary pin; the pane probes `cctop query --help`
- [ ] The digit model is resolved with the schema, not after: `Model.unfolded`, `overview.toggle`, `viewOfDigit` and `pane.tsx:253–255` all hang off it
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
- [ ] `Theme::series(n)` derives the §5.2 ramp from `accent`, solving the near endpoint against `bg` for ≥ 3.2 : 1; no new TOML key. It is a **new module** — the repo has no colour crate in `Cargo.toml` and no OKLab code today
- [ ] The ramp is computed from the **unreduced** accent. `Theme::for_caps` (`src/theme.rs:258–282`) consumes `self` and rewrites each fixed field, so a `series(n)` derived afterwards reads an already-reduced colour that may be `Color::Indexed` with no RGB to convert
- [ ] A test asserts monotone lightness and the contrast floor for all six bundled themes and two synthetic extremes, **and pins the six bundled hexes** so the Rust ramp cannot drift from `series-ramp.mjs`, PRD §5.2 and the artboards
- [ ] A test puts `series(3)` through `to_ansi16` for all six themes and asserts three distinct results — or the 16-colour path is declared glyph-only and the glyph fix below carries it alone
- [ ] **`stacked_bar` is fixed, not reused as-is.** `src/ui/widgets.rs:31` picks the glyph from `i % 2` over `parts` and `continue`s a zero-cell slice, so a three-slice bar on a session with no extended thinking draws `fixed` and `yours` as adjacent runs of the *same* glyph. Alternate on **emitted** segments, not on the input index
- [ ] A test drives the zero-middle-slice case under `Caps { mono: true }` and asserts the two remaining slices are distinguishable by glyph alone. This is the FR-8 test; `dashboard_ascii` carries no colour and cannot enforce it
- [ ] A rendered-buffer assertion that the context bar's spans carry only colours from `Theme::series(n)` — five lines over the buffer, not a grep, and not "enforced by review"
- [ ] `docs/metrics.md` gains the encoding note for `context_anatomy`; the panel guide gains §5.3's sentence in plain words

### US-106: Per-view footer and the un-truncate key
**Description:** As a user, I want each view to name its own keys, and a way to see what a row cut off.

**Acceptance Criteria:**
- [ ] `Panel::keys(&State) -> Vec<(&str, &str)>` on the trait, default empty; the footer is generated from the active view. There are **three** footer sites: `src/ui/dashboard.rs::FOOTER`, `src/ui/coach_view.rs:26`, and a hard-coded string at `src/app.rs:815`
- [ ] `w` toggles wide mode — rows wrap instead of cutting — on both surfaces, with a rule for a zone taller than the screen
- [ ] Both appear in the footer and the help overlay
- [ ] Live-terminal verification goes to `docs/verification/pane.md` as `Result: pending` (CLAUDE.md). That file already carries 28 pending items, which is itself the argument for keeping this story to two

### US-107: Deferred — the rest of the chrome
**Description:** `Ctrl-E` collapse, `z` faults-only, `/` filter, `s` sort, `-` previous view and `bg = "default"` are **not** in this PRD.

**Why:**
- `/`, `s` and `f` already exist and disagree with each other: `tools.rs:113,117,118`, `files.rs:76`, `events.rs:107,111–112`, `ledger_view.rs:39–44` (the sort itself is in `src/ledger.rs`). Consolidating them is a nine-panel navigation project with an `f`-vs-`/` decision and an `n`/`N` conflict with `advisor.rs:43–44`
- `-` is already bound to the refresh interval (`src/app.rs:569`)
- `bg = "default"` collides with US-105: the ramp solves its near endpoint against `bg`, so removing the background makes the contrast guarantee unenforceable for the users who opted in. It also breaks `build-site.sh:38`'s CSS injection, and `Theme::parse` discards a whole theme file when any colour fails to parse
- Every item is live-terminal only, so all of it converts acceptance criteria into pending lines

### US-108: The downstream surface
**Description:** As a reader of the site and the docs, I want them to describe the dashboard that ships.

**Acceptance Criteria:**
- [ ] README's `## What it looks like` mockup and the `panels:start` block regenerated; the four `build-site.sh` regexes that parse the old layout are rewritten (§10.1)
- [ ] `site/index.template.html` §2 prose ("Four lights, one nudge, nine rows", the tile paragraph, the Fig. 2 caption, the key list) rewritten for zones
- [ ] `scripts/build-guide.py`'s `coach_*` entries and the sample lines regenerated; a guide page for the meters
- [ ] `make demo` regenerates `two-pane.*`, the six `theme-*.png`, `demo.gif` / `.webm`; `site/demo.tape` updated for the new keys
- [ ] `make site` re-run — `site/index.html`, `site/metrics.html` and `site/guide/*.html` are committed build outputs, not inputs
- [ ] The layout descriptions **outside** every marked block are updated by hand: `README.md:66`, `plugin/README.md:35,44` (marketplace text), `src/main.rs:114` clap help (mirrored in `tests/pane/poller.test.ts:26`), `src/query.rs:392` and the module docs
- [ ] `docs/query.md` updated if the `dashboard` object's shape changes; `cctop metrics --md` and `--readme` re-run

## 9. What this does not answer

- **`:` fuzzy jump** (§6) is deferred. cctop has roughly a dozen destinations, digits reach nine of them, and the evidence that the remaining three need a command grammar is weak. Revisit when the count passes fifteen.
- **Whether zones actually read better than tiles.** This document asserts it and does not measure it. The predecessor's own plan committed to a dogfood week that never happened, so the design being replaced was never evaluated either. **The layout change ships behind `layout = tiles | zones` on the exposure machinery coach Phase 7 already built, and the tiles are deleted only when the arm says so** (plan §1.2). Everything measured in §3 justifies US-101, US-104 and US-105; none of it justifies deleting the tiles.
- **Whether the overview should keep a hero number at all.** This document says no — meters with a shared geometry beat one big figure, and four big figures in four units beat nothing. If the dogfood says a single spend or context figure is missed from across a room, it returns as *one* figure with its unit attached.
- **The ledger's `Panel::ledger` contract.** Zones no longer map one-to-one onto panels (`agents` rides `files`, `cache` and `spend` both carry digit `2`). Whether `Panel` keeps a ledger method or the zones become their own table is a plan-level decision (plan §2, Phase 1).
- **Whether §3.2's turn divergence is a defect or two honest answers.** A code question, not a design one. US-101 settles it with a fixture; the redesign does not depend on the outcome.
