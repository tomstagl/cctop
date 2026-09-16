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
