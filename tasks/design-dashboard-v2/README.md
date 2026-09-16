# Design canvas sources — cctop Dashboard Redesign

The working files behind the canvas https://claude.ai/artifact/1WvbQBFa3mKqyVPZ8YuWmy
for `tasks/prd-cctop-dashboard-v2.md` and `tasks/plan-dashboard-v2.md`.

Unlike `tasks/design-coach/`, these `.dc.html` files **are** the sources — there is
no `build.mjs` between them and the canvas — so they are committed.

Page *Redesign*:

- `Before.dc.html` — the pane at 122 columns as it renders today, rebuilt from
  `src/dashboard.rs` and `plugin/hooks/views/overview.tsx` with the session from
  the screenshot that started this work. Reproduces both truncation stages: the
  `cut()` ellipsis at `ROW_WIDTH` and ratatui's silent clip at the terminal edge.
- `Main.dc.html` — the redesign, interactive: chips switch 122 / 80 / 40 columns
  and open four drill-downs. Tweaks switch theme (all six bundled) and the act
  band's state (quiet / nudge / blocked).
- `Responsive.dc.html` — the three widths side by side and the give-way order.
- `Anatomy.dc.html` — the row budget, the three reading distances, the layout
  rules, the colour roles and the ASCII fallbacks.
- `Audit.dc.html` — 57 figures from today's overview, each kept / demoted /
  moved / fixed / cut, checked against `src/metrics/registry.rs`.

Page *Colour & k9s*:

- `Color.dc.html` — why the context bar's five slices are wrong today, the
  validator output, and the replacement ramp per theme (PRD §5).
- `K9s.dc.html` — what k9s settled, including the two it got wrong in public.

Page *Directions* (added after the dogfood):

- `Directions.dc.html` — the decision sheet: what the dogfood settled, what k9s
  actually comes down to once the keybindings are stripped away, why the colour
  work does not wait for this decision, and the three axes.
- `Instrument.dc.html` — direction 1, *restraint*. No bars, no block digits;
  numbers right-aligned in one column with a rule between groups.
- `Ledger.dc.html` — direction 2, *uniformity*. One row type per reading,
  sortable and filterable, with the count in the title.
- `Console.dc.html` — direction 3, *focus*. Three fixed header rows plus one
  body that fills the rest; a digit swaps the body.

All three are clickable prototypes carrying the same session and the same
readings — the dogfood validated the data, so only the rendering varies. Each
one makes its *characteristic* interaction real rather than all of them:
Instrument opens panels from row labels, Ledger sorts and filters in place,
Console swaps its body. Underlined text inside a terminal is a click target.

`check-grid.mjs` measures `Main.dc.html` only. The three prototypes are checked
by driving every click handler at both widths and re-measuring after each state
change — see the commit that added them for the harness.

## The clickable prototype

`prototype.html` is a standalone page — open it from a clone (`open
tasks/design-dashboard-v2/prototype.html`) or from the published link in the
commit that added it. It needs no build step and no canvas runtime, unlike the
`.dc.html` files above, which are canvas sources and render only inside the
published artifact.

Tabs switch direction, chips switch 122 / 78 columns, underlined text inside
the terminal is a click target, and the digit keys, `Esc`, `w` and `s` work.

Rebuild it after editing a renderer:

```
python3 build-prototype.py     # proto/*.js + prototype.shell.html -> prototype.html
node drive-protos.mjs          # drives every click target at both widths
```

- `proto/terminal.js` — the character-grid renderer: segments, column stops,
  bars, and the cut that mirrors `dashboard::cut`.
- `proto/views.js` — the five view bodies (context, cost, tools, files,
  events) that all three directions share, so what you compare is the chrome.
- `proto/directions.js` — one screen function per direction.

## Scripts

- `node series-ramp.mjs` — derives the sequential series ramp from each bundled
  theme's `accent`, solving the near endpoint against that theme's background
  until it clears 3.2 : 1, and prints the monotonicity, contrast and adjacent-ΔE
  checks. This is what generates the table in PRD §5.2; re-run it if a theme
  changes and update the table.
- `node check-grid.mjs` — evaluates `Main.dc.html`'s renderer and asserts every
  generated terminal row fits its column budget at 40 / 80 / 122 for every view
  and every act-band state. A terminal mockup that overflows is a lie; this
  caught 35 overflows in the first draft.

## Re-seeding the canvas

The artboards are seeded into the published canvas with the `/design` skill's
helper (`seed-canvas.mjs --artboard … --canvas canvas.json`), then published to
the URL above. Edit these files and re-seed; never edit the seeded output.
