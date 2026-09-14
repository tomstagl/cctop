# Design canvas sources — cctop Coach Surfaces

The working files behind the canvas https://claude.ai/code/artifact/d8dbec1d-2d13-4dbf-830f-26c512dc88f9
(the pane Overview and the TUI dashboard in the "Big figures" form chosen on
2026-09-14, the coach view on both surfaces, the one-line forms, the nav
options, the terminal after the nav move, the type levers, and the sync sheet).

- `mock.mjs` — a tagged-text terminal mockup toolkit (`{ok b:● BUSY}`), frame
  helpers that mirror `src/ui/panel.rs` / `plugin/hooks/views/frame.tsx`, and
  the HTML emitter in the default-dark theme. Every row is measured in cells
  and the build fails on a row that does not fit.
- `screens.mjs` — the coach card, the coach views, the nav options, the
  terminal board; one scenario throughout (turn 14, opus-5, 412k / 41 %, warm
  41m, 5h 62 %, three gh/git failures, three unverified edits).
- `dashboard.mjs` — the block-digit font, the four tiles, the dashboard for
  the terminal and the pane (`tasks/plan-dashboard-big-figures.md`).
- `build.mjs` — writes one `<Name>.dc.html` per artboard, `canvas.json` and
  the sync sheet; `node build.mjs` regenerates them (they are not committed).

The generated artboards are seeded into the canvas with the `/design` skill's
helper and republished to the URL above.
