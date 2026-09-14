# Implementation plan: the "Big figures" dashboard (direction B)

**Decision:** 2026-09-14 — direction B from the canvas *cctop Coach Surfaces*, page "Density directions" (artboards `BigTerminal`, `BigPanel`, `TypeLevers`; working files in `tasks/design-coach/directions.mjs`).
**Replaces:** the nine-frame dashboard on both surfaces (TUI `layout::solve` grid; pane Overview's framed blocks).
**Fits into:** `tasks/plan-cctop-coach.md` — this is what Phase 4 ships as *the dashboard*; Phase 2's panel work feeds the ledger rows and the full-screen panels instead of the framed grid.

## 1. What the dashboard becomes

Four levels of type, top to bottom, on one screen without frames:

1. **Header line** — `cctop` bold, the session facts, the phase pill right-aligned.
2. **Four tiles** — the coach's four lights as 3-row block digits in the light's level colour, the unit dim on the baseline, the glyph + name bold beside them, two sub-lines. This is the terminal's only "large type"; it is what a person reads from across the room.
3. **The nudge** — one bold line: `▸ headline — action`, class and fire point dim at the right.
4. **The ledger** — nine borderless rows, `1 Context` … `9 Advisor`: the accent digit, the dim name, the panel's key values in normal weight; a second, dim row with the detail. The digit opens the panel full-screen.

122 columns need 24 rows (46 today). Whitespace rows separate the four levels and are the first thing to go when rows are scarce.

```
 cctop  claude-opus-5 · turn 14 · 1:12:08 · ~/code/cctop · PR #142               ● IMPLEMENTING · 18c +42k · silent 3:50

 █ █ ▄█    ◐ context            █▀▀ ▀▀█   ○ limits             █ █ ▄█    ○ cache              ▀▀█   ● rework
 ▀▀█  █    412k of 1.00M        █▀█ █▀▀   5h · ↻ 2h10          ▀▀█  █    warm · 1h TTL        ▀▀█   fails · gh pr view
   ▀  ▀  % ≈$.21/call           ▀▀▀ ▀▀▀ % agents 3 · 86%         ▀  ▀  m misses 2             ▀▀▀   edits 3 ✓ none 11m

 ▸ 3 fails in a row: gh pr view ×2 (8), git push (128) — Esc, give the missing fact — or run it, paste tail  NOW · call 12

 1 Context   ▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁ prefix 57k · inputs 94k · results 73k · thinking 40k · harness 40k
             +42k/turn · autocompact 967k · 555k left · compactions 0 · since /clear 20:04 · re-reads 2
 2 Tokens    cache read 33.0M · write 597k · output 67k · warm 41:12 · misses 2 (model_changed 427k)
             $18.6 · $6.03/h · $/call ≈$.21 · $/turn ≈$2.6 · next 30c ≈$6.3 · agents $4.10 (22 %) · code-review 31 %
 3 Limits    5h 62 % ↻ 2h10 · 7d 11 % ↻ 4d3h · weight ×5 (opus) · long_context 34 % · other sessions 2
 4 Turn      IMPLEMENTING ×18 · api ≈1:02 · tools 2:48 · steers 1 queued · background 0 · hooks p99 31ms · denials 2
 5 Tools     257 calls · 3 err · explore 41 · gitread 12 (3 ✗) · Edit 11 · Read 18 · Agent 4 · top ctx Read state.rs 12k
 6 Agents    3 run · 86 % · Explore 1:12 71k · fork 0:48 412k · wf:review 0:31 38k · $4.10 · 2 failed · ! auth
 7 Files     9 touched · render.rs E×5 · coach_view.rs W×1 E×2 IDE edit · +412/−87 · commit 42m ago
 8 Events    20:41:05 coach fired failure-cascade NOW · 20:41:05 tool git push ✗ 128 · 20:41:02 tool gh pr view ✗ 8
 9 Advisor   NOW 1 · next verify-gap → turn end · snoozed cache-miss (4 turns) · prefix tip (session)

 ?help  1-9 open a panel full-screen  c coach  a ask  t theme  q
```

The pane draws the same object at 72 body columns: tiles two per row, the ledger's value rows wrapping to a second row where they must (`BigPanel`).

## 2. One object: `cctop query dashboard`

The rule from the coach plan holds — no surface computes what it draws. A new `dashboard::snapshot(&State) -> Dashboard` in `src/dashboard.rs` produces:

```
{
  header: { session, model, version, turn, elapsed, cwd, pr, phase: { glyph, word, tokens[] } },
  tiles: [ { id, level, figure, unit, name, sub1, sub2, source, approx } ×4 ],   // the coach's lights
  nudge: { line1, action, class, fired_at, acted } | null,
  rows:  [ { digit, name, values: Line, detail: Line } ×9 ],
  session_mode
}
```

- `tiles` are the coach object's `lights[4]` (`coach::snapshot`), so the tile figure **is** the light's `number`: context = % of window, limits = 5 h %, cache = minutes warm (or the re-write size in k when cold), rework = the count of open issues (fails, corrections, blocked calls, or unverified source edits). `—` when the source is absent, never 0; `≈` on the unit when estimated.
- `rows` carry tagged segments (`Line` = `[{text, style}]`, styles fg / dim / accent / ok / warn / crit / bold), cut at 118 cells in the snapshot so both surfaces truncate identically; each panel contributes its two lines through a new `Panel::ledger(&State) -> (Line, Line)`.
- `cctop query dashboard [--session]` returns it (`src/query.rs`, `src/main.rs`, `docs/query.md`); `scripts/pane-fixtures.sh` emits `tests/pane/fixtures/dashboard.json` next to the others; an insta snapshot covers fixture B.

`cctop query summary` stays for the numbers; `dashboard` is the drawn form.

## 3. The terminal (TUI)

**Layout.** `src/ui/dashboard.rs` replaces the grid: a fixed vertical composition of header (1) · blank · tiles (3) · blank · nudge (1) · blank · ledger (9 or 18) · footer (1). `layout::solve` and its narrow/wide modes go, with their snapshots; the `w` key and `Config.layout` are removed (a config file that still carries `layout` is read and ignored). The panels keep their `id`, `title`, `summary`, `render`, `render_overlay`, `handle_key`; they lose `min_rows`, `priority`, `placement`, `flexible`.

**Breakpoints** (columns × rows):

| width | tiles | ledger |
|---|---|---|
| ≥ 100 | four in a row, 30 cells each | values + detail rows (18) |
| 60–99 | two per row (2 × 3 rows + 1 blank) | values rows; detail rows only if height allows |
| 40–59 | one per row is too tall: the tiles collapse to the coach's L1 line `◐41% ○62% ○41m ●3 ▸` | values rows, cut |
| < 40 | L2 glyphs | names and the first value |

Rows, in the order they give way: the blank separators, then the detail rows, then the nudge's action half (headline stays), then the tiles collapse to L1. The four tiles and the nine value rows are never dropped; at 40 × 24 the screen is L1 + nine rows + footer.

**Block digits.** `src/ui/widgets.rs` gains `big_digits(t: &Theme, text: &str, style: Style) -> [Line; 3]`, a 3 × 3 font (one cell between digits): 

```
0 █▀█ █ █ ▀▀▀    1 ▄█   █   ▀     2 ▀▀█ █▀▀ ▀▀▀    3 ▀▀█ ▀▀█ ▀▀▀    4 █ █ ▀▀█   ▀
5 █▀▀ ▀▀█ ▀▀▀    6 █▀▀ █▀█ ▀▀▀    7 ▀▀█   █   ▀    8 █▀█ █▀█ ▀▀▀    9 █▀█ ▀▀█ ▀▀▀    —     ▀▀▀
```

The unit sits one cell after the digits on the baseline row, dim. **ASCII fallback** (`Caps.ascii`): the figure as bold text on row 1 (`41 %`), the sub-lines unchanged — the tile keeps its three rows so the layout does not move. Colour: the digits in the light's level colour (quiet = fg, watch = warn, act = crit); the glyph in the same colour; the name bold; sub-lines fg, the parenthetical parts dim. No other colour on the screen except state (errors crit, warm ok, the accent on the ledger digits).

**Full-screen panels.** `1`–`9` open that panel over the whole body (the existing `state.overlay` path; `Esc` returns). Panels that already own an overlay (Context → ledger / prefix, Tools, Events, Advisor) keep it; the others draw their `render` into the full area under a one-line title (`draw_frame` with the panel's title and summary). Inside a full-screen panel its own keys work as today (sort, search, `n`). `Tab` focus cycling is no longer needed on the dashboard (there is nothing to focus but the nudge) and is removed from `BINDINGS`; `a` asks about the panel that is open, else the nudge.

**Keys.** `1-9` open · `Esc` back · `Enter` act on the nudge (the ask popup, as in the coach view) · `c` coach · `a` ask · `t` theme · `L` sessions · `p` pause · `+`/`-` refresh · `?` · `q`. `BINDINGS`, the footer and the help overlay updated; the `w` binding removed.

**Files.** New `src/dashboard.rs`, `src/ui/dashboard.rs`; changed `src/ui/widgets.rs`, `src/ui/panel.rs`, every `src/ui/panels/*.rs` (the `ledger` method; the layout methods removed), `src/app.rs` (draw, keys, bindings), `src/config.rs`, `src/theme.rs` (nothing new: the levels use the existing roles), `src/ui/layout.rs` (deleted), `src/query.rs`, `src/main.rs`, `docs/query.md`, `docs/metrics.md` (the tile figures get metric ids), `site/guide/*` (the dashboard page and screenshots), `scripts/demo.py`.

## 4. The pane

`plugin/hooks/views/overview.tsx` is rewritten to draw `model.query.dashboard`:

- the state line row, then the tiles two per row (`frame.tsx` gains `bigDigits(text, color): Line[3]` with the same font; the pane has no ASCII mode — Claude Code's terminal is always UTF-8), then the nudge (two rows: headline, action), then the ledger.
- Ledger rows are **Buttons** (`plain`, no hotkey): `1 Context   412k / 1.00M · +42k/turn · …`. Rows 5–9 switch to the view of that panel (Tools, Agents, Files, Events, Advisor — the existing tabs); rows 1–4 unfold their block inline beneath the row (the Context / Tokens / Limits / Turn frames the Overview draws today, from `contextBlock` and friends) and fold on the next press. A folded/unfolded set lives in the model and survives a re-render, not a reload.
- Below 50 body columns the tiles collapse to the coach's L1 line, as on the TUI; below 40, L2.
- `model.ts`: `dashboard` joins `QUERY_VERBS` (polled with `summary`), the Overview's engine-first rule stays for the numbers the engine has (`model.usage` for the context tile before the binary answers).
- Tests: `tests/pane/overview.test.ts` rewritten against `fixtures/dashboard.json` at 40 / 50 / 72 / 100 columns; a font test that every glyph is 3 × 3; the inline (classic-renderer) form keeps its flat header + Context + Limits rows and gains the L1 line.

The coach card (state line · four light rows · nudge) stays the header of the **Coach** view; the Overview's tiles are the same four lights drawn large, so the two views never disagree by construction.

## 5. Sequence and size

1. **Font and tiles** (S) — `widgets.rs::big_digits` + ASCII fallback + `frame.tsx::bigDigits`; a unit test per glyph; a snapshot of the four tiles on fixture B in both surfaces.
2. **`dashboard::snapshot` and `cctop query dashboard`** (M) — the header, the four tiles from `coach::snapshot`'s lights (so this step lands with or right after Phase 4's coach object; before it, the tiles read the four lights computed by the same functions), the nine `Panel::ledger` rows, the nudge from the Advisor engine; fixtures regenerated.
3. **TUI dashboard** (M) — `src/ui/dashboard.rs`, full-screen panels, keys, config, the grid and its snapshots deleted; new snapshots at 122 × 24, 100 × 30, 80 × 40, 60 × 51, 40 × 24 and the ASCII form on fixture B.
4. **Pane Overview** (M) — the rewrite above; README table and the guide updated; `docs/verification/pane.md` gains an item for the tiles and the ledger Buttons.
5. **Dogfood** (a week) — the tile figures' thresholds (§6.3 of the PRD) tuned against real sessions; the ledger rows' wording trimmed to what a person actually reads (each row's cut point is an editorial decision — record the choices in `docs/metrics.md`).

Total: about the same work as the framed-panel additions of the coach plan's Phase 2 + the header changes of Phase 4 that this replaces; the grid's deletion pays for the block font.

## 6. Decisions taken, and what to watch

- **One dashboard.** The framed grid is removed, not kept behind a flag: two dashboards would mean two sets of snapshots and two places for every Phase 2 row. Full-screen panels keep everything the grid could show.
- **Digits are the hotkeys.** `1-9` open panels (they toggled visibility before); nothing is hidden any more — a panel with nothing to say shows `—` in its row.
- **The tile figure is the light's number** — one definition, in the coach object. If a light's figure turns out to be the wrong thing to enlarge (cache minutes, say), it changes in `coach::snapshot`, and every surface follows.
- **Block glyphs need a font with the half blocks** `▀ ▄ █` (every monospace font in use has them; the ASCII fallback covers `LANG=C`). Kitty's text-sizing protocol could draw true 2× digits later behind a capability check; not in this plan.
- **Watch the ledger's truncation on 60–99 columns**: the value rows are written for 118 cells and cut with `…`; the dogfood week decides whether the mid-width form needs its own, shorter wording per row (the pane's 72-column rows in `BigPanel` are that shorter wording and can be reused).
