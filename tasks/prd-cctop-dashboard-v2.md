# PRD: dashboard v2 — zones, meters and one colour language

**Status:** v2.0 · 2026-09-16 — proposed, nothing implemented. v1.0 left the layout open between three directions; the dogfood and a pane-contract sweep closed it on **Console** (§3.6, §4).
**Target:** cctop ≥ 0.3.1 attached to Claude Code CLI 2.1.272 on macOS/Linux; the TUI and the function-hooks pane change together, `cctop query dashboard` changes with them.
**Depends on:** `tasks/prd-cctop.md` v1.1 (the nine panels, the metrics registry, the query interface) · `tasks/prd-cctop-coach.md` v1.3 (the four lights, the nudge slot, the urgency model) · `tasks/prd-cctop-pane.md` v1.2 (the second front-end).
**Supersedes:** `tasks/plan-dashboard-big-figures.md` §1 and §3 — the block-digit tiles and the nine-row ledger. The one-object rule of §2 (`dashboard::snapshot`, every surface draws it verbatim) is kept and extended.
**Design:** the canvas *cctop Dashboard Redesign* — pages *Redesign*, *Colour & k9s*, *Directions*, *Console*. Working files in `tasks/design-dashboard-v2/`, with a standalone clickable prototype at `tasks/design-dashboard-v2/prototype.html`.

> **v2.0 changes.** The layout question is settled by use rather than by argument: the current dashboard was dogfooded, the data changed behaviour and the rendering did not survive the pane. So §4 is Console — a fixed header of whole-area targets over one body — and the `layout = tiles | zones` comparison arm v1.0's plan demanded is dropped, because the comparison happened. §3.6 adds three constraints traced out of `claude-code.d.ts` and `docs/claude-code-panels.md` that invalidate parts of v1.0: the pane is 35–85 columns and never 122, it has four colours and no hex, and the engine draws the clickable chrome itself. §6 is rewritten from "navigation and chrome" to the target model, §7 gains four target FRs, and §10 records what closed the question. US-102 and US-103 are rewritten, US-109 is new, US-106 narrows to the rule line; US-101, US-104, US-105, US-107 and US-108 stand unchanged.

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

### 3.6 What the pane actually is

Three constraints, traced out of `plugin/.claude/types/claude-code.d.ts` and `docs/claude-code-panels.md` rather than assumed. Each invalidates something v1.0 drew.

**The dock is 35–85 columns, never 122.** `docs/claude-code-panels.md` §4: `p3(columns) = min(floor(columns × 0.45), 90, columns − 70)`, then `BORDER_COLUMNS = 4` and `DOCK_GRIP_COLUMNS = 1` come out of it. Ninety is a hard cap.

| terminal | dock | body | |
|---|---|---|---|
| 110 | 40 | ~35 | the narrowest dock there is |
| 132 | 59 | ~54 | |
| 162 | 72 | ~67 | **the design centre** |
| 200 | 90 | ~85 | the cap |
| 240 | 90 | ~85 | still the cap |
| < 110 | — | — | no dock: an inline band above the prompt |

Every screen in v1.0 captioned "the pane" at 122 columns was the standalone terminal split. The pane has never been that wide and cannot be.

**The pane has four colours, and they are the person's.** `plugin/hooks/views/frame.tsx`: *"Colours are Claude Code theme keys, not palette names, so the pane follows the person's theme."* `ACCENT = 'suggestion'`, `OK = 'success'`, `WARN = 'warning'`, `CRIT = 'error'`, borders are dimmed default text, and `Color` is `'green' | 'yellow' | 'red' | 'cyan'` — plus `dimColor`, `bold`, `inverse`. **There is no hex and no way to ask for one.** So §5.2's series ramp is a TUI-only affordance; the pane must carry the same meaning through the alternating fill glyph alone, which promotes the `stacked_bar` defect in US-105 from a nicety to a pane correctness bug.

**The engine draws the clickable chrome itself.** `ButtonProps`: `Button` is "every surface's pressable leaf"; the terminal draws `[ label ]`, or `1: label` when `plain` — "the hotkey in the accent color, a colon, the label". Under the pointer it inverts with no hook run. `dimColor` is "dim at rest … and at full strength under the pointer or the focus". `hover` applies label styles "while the nearest enclosing keyed `Box`, or given a `scope` its group, is hovered". A `hotkey` is one digit or one lowercase letter, and a bare digit presses from an empty composer.

None of that is cctop's to draw, and none of it can be restyled. v1.0's hand-drawn digit map was cctop painting an affordance the engine paints better.

## 4. Console

### 4.1 The shape

A header that never moves, and one body that fills every remaining row.

```
 cctop  opus-5 · t1 · 52:11                        ● WORKING 52:11
 1:ctx 35% 616k left             2:5h 4% ↻4h06
 3:cache 59m 1h TTL              4:spend ≈$22.50 ≈$90/h
 5:✓ test 10s · rework 0         6:159 calls · 1 err
 ▸ steer window · 4c 1:56                                 a:advisor
─── events ────────────────────────────────── 0 home  ·  ? keys ───
  05:53  tool  Bash  cargo fmt --all && make check > /tmp/check.log
  05:52  hook        PostToolBatch
  05:52  tool  Bash  ✓ 94 tokens to context
  …
```

- **Row 1 — identity.** `cctop`, the model, turn, elapsed and cwd; the phase cell right-aligned in its level colour. Fixed.
- **Rows 2–4 — the glance.** Six cells, three per row at ≥ 80 columns, two below. Each cell is one whole-area target (§6). Fixed positions, never reordered (FR-4).
- **Row 5 — act.** The coach's slot, wrapped rather than cut, with `a:advisor` right-aligned. The whole line is a target. A pending permission wait or `AskUserQuestion` pre-empts the nudge and paints the line `crit`.
- **Row 6 — the rule.** The open body's name, and the only navigation cctop draws itself: `0 home` and `? keys`.
- **Rows 7+ — the body.** Every remaining row, one thing shown properly.

Twelve rows of chrome-plus-body at 85 columns and thirteen at 54, against the current pane's seven rows of tiles before anything else is drawn.

### 4.2 The bodies

Eight, one per target, sharing one frame. Each is a full-height view of what its cell summarises; §6's table gives the registry ids behind each.

`context` · `limits` · `cache` · `cost` · `work` · `tools` · `advisor` · `events` (the default and the way home).

The nine panels collapse into these eight: Header becomes row 1, Turn folds into the phase cell and `work`, Agents rides `tools`, Files rides `work`, Advisor becomes `advisor`.

### 4.3 The width ladder

Positions never move; cells lose their trailing clause, then their second column.

| body columns | header cells | cell text | act line |
|---|---|---|---|
| ≥ 80 | 3 per row, 2 rows | `ctx 35% · 616k left` | full, with `a:advisor` right-aligned |
| 52–79 | 2 per row, 3 rows | `ctx 35% 616k left` | full at ≥ 72, shortened below |
| 35–51 | 2 per row, 3 rows | `ctx 35%` | shortened, no right-aligned tail |
| inline (< 35) | the one-line strip, not individually addressable | — | the act line only |

The `< 110` case is not a dock at all: `InlinePanes` draws a band above the prompt whose rows come from `pluginPanes.inlineRows`. That form carries the act line and the strip, and nothing else.

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

## 6. Targets

### 6.1 The target is the cell, not the number

A cell is **a keyed `Box` holding two or three `Button`s that share one `scope` and one `onPress`**. The whole area lights together on hover, any part of it clicks — padding included, which is drawn as a Button for exactly that reason — and each part keeps its own colour, which one Button with one label could not do.

The digit is not drawn by cctop: a `plain` Button with `hotkey="1"` renders as `1: label` with the digit in the person's accent colour. Contiguous `1`–`6`, one per cell, plus `a` and `0`.

### 6.2 The eight targets

| key | cell | opens | reads |
|---|---|---|---|
| `1` | `ctx 35% · 616k left` | context | `context_size` `context_window` `turns_until_compaction` `context_anatomy` `context_velocity` `compactions` `file_rereads` |
| `2` | `5h 4% · ↻ 4h06` | limits | `limit_5h` `limit_7d` `limit_reset` `limit_weight` `behaviour_flags` `limit_exhaustion` `other_sessions` |
| `3` | `cache 59m · 1h TTL` | cache | `cache_warm` `cache_expires_in` `cache_ttl` `cache_misses` `cache_recache_if_cold` `cache_hit_ratio` |
| `4` | `spend ≈$22.50 · ≈$90/h` | cost | `cost` `burn_rate` `cost_per_call` `cost_per_turn` `cache_read` `cache_write` `output` `thinking` `agents_cost` |
| `5` | `✓ test 10s · rework 0` | work | `last_check` `coach_rework` `file_touches` `file_rereads` `uncommitted` `rewind_points` |
| `6` | `159 calls · 1 err` | tools | `tool_calls` `tool_errors` `tool_p50` `tokens_to_ctx` `top_ctx` `bash_class` `error_class` |
| `a` | the act line, whole | advisor | the coach object · `advice_saving` |
| `0` | `0 home` in the rule line | events | D2 transcript · D4 hook spool |

### 6.3 The five states

| state | docked pane | standalone terminal |
|---|---|---|
| rest | plain Button: digit in the theme's accent, colon, label; secondary parts `dimColor` | same characters, accent digit, dim tail |
| hover | the keyed Box lights through the shared scope, padding included; the Button under the pointer inverts — both the surface's own doing | **nothing**: no pointer. The one state the surfaces cannot share |
| focus | the engine's focus ring; Enter presses; `dimColor` parts render full strength | no ring — the key is printed in the cell |
| pressed | the body swaps in place, the header does not move; `ui.press` carries the cell's key | identical, handled in `app.rs` |
| current | the open body's cell draws digit and label in `ok`, bold | identical |

### 6.4 What is deferred

`Ctrl-E` collapse, `z` faults-only, `/` filter, `s` sort and `-` previous view stay out of this PRD (US-107). Console needs none of them to work: the body *is* the drill-down, and `0` is the way back.

## 7. Functional requirements

**The object**

- **FR-1** `dashboard::snapshot` remains the only place a surface's contents are decided. The TUI, the pane and `cctop query dashboard` draw it verbatim. There is **no** `cctop_dashboard` MCP tool — `src/mcp.rs` exposes eight and none is the dashboard — and this work does not add one.
- **FR-2** The object is colour-free. A slice carries a *step index*, not a colour; each surface maps it through its own vocabulary. This is what lets the TUI use a ramp the pane cannot express (§3.6).
- **FR-3** No fact is drawn twice on one screen.
- **FR-4** Cells keep fixed positions and never reorder by urgency, value or recency (k9s #3589, §3.5).
- **FR-5** A body with nothing to report says so in one line; it does not draw an empty frame.

**Reading**

- **FR-6** Every figure carries its unit in the same cell run, on the same line.
- **FR-7** The act line is never truncated mid-fact; it wraps. Every other truncation is marked.
- **FR-8** `ok` / `warn` / `crit` are used only for a threshold the registry defines. Composition uses the §5.2 ramp in the TUI and glyph alternation in the pane.
- **FR-9** Every state is legible with `NO_COLOR`, on 16 colours, in a non-UTF-8 locale, and **in the pane, which has none of those fallbacks and cannot detect the font** — through glyph and position alone.
- **FR-10** A bar never carries meaning alone: every bar has its number beside it, so a font that renders `▇` a fraction narrow costs alignment and not meaning.
- **FR-11** A figure whose source is absent prints `—`, never `0`; a figure with no sample yet prints `—`, never a computed zero.
- **FR-12** An estimate a reader could mistake for a hard fact carries a word, not only `≈`.

**Targets**

- **FR-13** A target is an area. Every cell of it presses, padding included, and hovering any part lights all of it.
- **FR-14** cctop draws no hotkey chrome. Digits and their accent colouring are the engine's `plain` Button rendering.
- **FR-15** Hotkeys are contiguous and one-to-one with the cells.
- **FR-16** The pane reads `schema` and, on a mismatch, renders one line naming the cctop it needs — never an indefinite wait.

## 8. User stories

> **correctness — ships on the layout that exists today, changes no schema**

### US-101: Two readers of one session must not disagree about cost
**Description:** As a user, I want every surface reading one session to report one cost, so that a 2250× gap between two windows is impossible.

**Acceptance Criteria:**
- [ ] The §3.2 mechanism is reproduced in a test: one reader that has consumed a `cost-state` line and one that has not, over the same transcript, and the assertion is that they agree
- [ ] `Cost::current()` no longer lets a cleared `since` and a near-zero `total_cost_usd` present as a confident `$0.000` — either the authoritative branch keeps the estimate it displaced, or a near-zero cost on a non-trivial session carries `≈` and a caveat
- [ ] Fixed in `src/metrics/cost.rs`, **not** in a renderer, and not by hiding one of the two values
- [ ] The registry row for `cost` gains the caveat if both readings are legitimate at different points
- [ ] Turn state, separately: panel 4's field is labelled for what it reads (the turn) and the header's for the phase classifier, so the two can differ without reading as a contradiction. A turn-ended-while-tools-run fixture (`scripts/anonymise-transcript.py`, never hand-edited) pins what each says
- [ ] Ships on the layout that exists today and changes no schema

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

> **Console**

### US-102: The Console object
**Description:** As every surface, I want one object that decides what Console shows, so the pane and the terminal cannot disagree.

**Acceptance Criteria:**
- [ ] `src/dashboard.rs` schema `1 → 2` (current value verified at `src/dashboard.rs:203`): `tiles` out; `header`, `act`, `cells: Vec<Cell>`, `body: Body` in
- [ ] `Cell` is `{ key: char, id, label: Line, opens: &'static str, active: bool }` — `label` is a `Line` of segments so a cell can be several Buttons sharing a press (FR-13)
- [ ] A slice carries `step: u8` into `Theme::series(n)`, **not** a colour (FR-2)
- [ ] `Body` is `{ id, title, keys, rows: Vec<Line> }`, one of the eight in §4.2
- [ ] The object is **not** width-aware. `snapshot` stops cutting at `ROW_WIDTH`; each surface cuts once at its own width. This removes the double cut without a `--columns` flag, a resize re-poll, or rows cut for a stale width (v1.0's plan had no caller story for any of it: `src/query.rs:394` is `pub fn dashboard(state)` with no width, `src/main.rs:114` declares `Dashboard` as a bare unit variant, and `poller.ts:135` sends no width)
- [ ] The digit question is settled **before** the schema is written, not after: `key` is a wire field
- [ ] `src/query.rs`, `docs/query.md:47` follow; `src/main.rs:114`'s clap help is rewritten
- [ ] `scripts/pane-fixtures.sh` regenerated for both fixtures

### US-103: Console in the terminal
**Description:** As a user at a terminal, I want the header and one body.

**Acceptance Criteria:**
- [ ] `src/ui/dashboard.rs` draws §4.1; `tile_block`, `tile_rows`, `FOUR_TILES`, `TWO_TILES`, `L1_TILES`, `TILE_WIDTH` are deleted, and `big_digits` + `BIG` + `mod big_tests` from `src/ui/widgets.rs` with them
- [ ] `TWO_TILES` and `L1_TILES` gate **ledger** decisions, not tile ones — `with_detail = width >= TWO_TILES` (`:236`) and the narrow `ledger_row` form (`:230`, `:189–194`). Both need replacements, not deletions
- [ ] Digits `1`–`6`, `a`, `0` swap the body; `Esc` returns to `events`; the header never moves
- [ ] A test asserts every composed row is **≤ `width` cells measured before render** (a `TestBackend` buffer is `width` cells by construction, so measuring after render is vacuous) at 35 / 54 / 67 / 85 / 122
- [ ] `src/theme.rs:532`'s `assert!(text.contains("1 Context"))` is a plain assertion inside `dashboard_no_color_is_monochrome`; `INSTA_UPDATE` will not fix it
- [ ] `src/dashboard.rs:922–1022` asserts `rows.len() == 9`, `digit == i+1`, both `ROW_WIDTH` bounds, `tiles.len() == 4` and `schema == 1` — all of it is rewritten
- [ ] Insta snapshots regenerated at 35 / 54 / 67 / 85 / 122

### US-109: Console in the pane
**Description:** As a user of the docked pane, I want the cells to be real clickable areas in the engine's own chrome.

**Acceptance Criteria:**
- [ ] `plugin/hooks/views/overview.tsx` draws §4.1; `tileRows`, `tileLines`, `engineTiles`, `TILES_MIN`, `L2_MAX`, `type Tile`, `tileOf` are deleted, and `dashboardOf`'s hard `if (!Array.isArray(tiles)) return null` with them
- [ ] `frame.tsx`'s `bigDigits` / `BIG_GLYPHS` is deleted **in the same commit** as `tileLines`, its only caller (`overview.tsx:17`, `:569`; asserted at `tests/pane/overview.test.ts:20,127–139`)
- [ ] A cell is a keyed `Box` of `plain` Buttons sharing a `scope` and one `onPress`; the padding is a Button (FR-13)
- [ ] No cctop-drawn digit chrome (FR-14)
- [ ] The three-way context split survives on `dimColor` and glyph alternation alone — **the pane has no ramp** (§3.6)
- [ ] `TILES_MIN` also gates the pane's detail rows at `overview.tsx:635`; that needs a replacement
- [ ] The pane's digit model changes with it: `Model.unfolded` (`model.ts:113,186`), `overview.toggle` (`:145,301–305`), `viewOfDigit` (`overview.tsx:717`), dispatcher `pane.tsx:253–255`
- [ ] FR-16: read `schema`, render one line on a mismatch. There is no binary pin — `plugin/.claude-plugin/plugin.json` carries only the plugin's version and the pane probes `cctop query --help` (`poller.ts:55–67`, `model.ts:334–342`), so a schema-2 pane on a schema-1 binary otherwise waits forever
- [ ] **A fixture-B row-identity harness is built.** It does not exist: `coach.test.ts` covers the Coach card, `overview.test.ts` uses fixture A and regexes rather than Rust-rendered rows, and the `-b` dashboard fixtures are generated but read by no test
- [ ] Four assumptions go to `docs/verification/pane.md` as `Result: pending` (CLAUDE.md): whether a `plain` Button with no hotkey draws only its label; whether the hover scope covers a Button whose label is only spaces; whether six Buttons in one band can each claim a bare digit; and the real `bodyColumns` at 35 / 54 / 67 / 85

> **after Console**

### US-106: Per-view keys in the rule line
**Description:** As a user, I want each body to name its own keys where Console already draws a rule line.

**Acceptance Criteria:**
- [ ] The rule line's right half is `Body::keys`, already on the object (US-102), so there is no new trait. The three legacy footer strings — `src/ui/dashboard.rs::FOOTER`, `src/ui/coach_view.rs:26`, and the hard-coded one at `src/app.rs:815` — reduce to two, since Console has no footer
- [ ] `?` expands the rule line into the full key map for the open body, and collapses again
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

- **Whether six cells are the right six.** They are the four budgets plus health plus volume, which is what the current four lights and the row ledger between them already say. If the dogfood on Console says one is never pressed, it becomes a line in another body rather than a cell.
- **The inline form below 110 columns.** §4.3 gives it the act line and the strip. Nobody has used cctop inline for a working week, so this is the least evidenced part of the design.
- **Whether `0` or `Esc` is the way home.** Both are wired; which one people reach for is a dogfood question.
- **The deferred chrome** (US-107). It stays deferred until something in Console actually needs it.

## 10. What settled the layout

v1.0 could not answer whether zones beat tiles and said so, and its plan demanded a `layout = tiles | zones` arm on the coach's exposure machinery before the tiles could be deleted. That gate is met, by use rather than by instrumentation:

- the current dashboard was **used**, and the data changed behaviour — so the readings, the thresholds and the registry behind them are evidence-backed and none of them changes here;
- the rendering did not survive the pane — *"it looks just ugly in the panel"* — and §3.6 says why in numbers: seven mandatory tile rows in a viewport that is 35–85 columns wide and short;
- three directions were built as clickable prototypes at the real widths and compared, and Console was chosen.

So this PRD deletes the tiles without an A/B arm, and the plan no longer carries one. What it does carry instead is §3.6's constraints, which is the part v1.0 was actually missing: not a preference between two layouts, but the shape of the surface both were being drawn for.
