# Hand-off: implementing Console (dashboard v2)

Three prompts, for three sessions. They are written to stand alone — a fresh
session has none of the design conversation, only the repo.

**Do them in order.** Part A ships on the layout that exists today and is worth
having whether or not Part B happens; Part B deletes the tiles and cannot start
until A4 has landed (the pane has no colour ramp, so the glyph fix is the only
thing keeping its context bar readable — plan §1).

---

## Prompt 1 — Part A, the correctness half

```
Implement Part A of the cctop dashboard redesign: the correctness half.
Do not change any layout, any schema, or any keybinding.

Read first, in this order:
  tasks/prd-cctop-dashboard-v2.md  — §3.2, §3.3, §3.4, §5, and US-101, US-104, US-105
  tasks/plan-dashboard-v2.md       — §2 (A1–A4), §5
  CLAUDE.md

Baseline: `cargo test` is green — 292 passed, 0 failed, 2 ignored. Every commit
below ends with that still true and `make check` clean.

Four commits, in order.

A1 · Cost is not single-sourced across readers.
  Two readers of one transcript can report costs 2250× apart. `Cost::current()`
  (src/metrics/cost.rs:172–191) has an authoritative branch and an estimate
  branch; `$0.000` without `≈` needs the first with a near-zero total, `≈$22.5`
  needs `any_since` — and src/metrics/cost.rs:138–142 sets `authoritative` while
  clearing `since`. Reproduce it with two readers over one transcript, one past
  the `cost-state` line and one before it, and assert they agree. Fix it in
  src/metrics/cost.rs, NOT in a renderer, and NOT by hiding one of the values.
  Regenerate `docs/metrics.md` and the README block IN THIS COMMIT — see traps.

A2 · Turn state says which question it answers.
  src/ui/panels/turn.rs:34 reads `agg.current_turn()`; `dashboard::header` takes
  the phase word from `coach::state_line` (src/coach.rs:1038). Both are honest;
  only the labelling implies they should agree. Capture a
  turn-ended-while-tools-run fixture with scripts/anonymise-transcript.py and
  name each field for what it reads. There is a bug only if the fixture shows
  the classifier naming a phase with no tool activity and no open turn — then it
  is in src/phase.rs.

A3 · Six honest figures, each with a test.
  - context velocity prints `—` until it has two samples, never `+0/turn`
    (src/metrics/context.rs; the metric returns Option, the renderer prints —)
  - cost is labelled API-equivalent in src/ui/panels/tokens.rs and
    src/dashboard.rs::row_tokens — the registry says a subscription has no
    per-token bill, and `≈` does not carry that
  - `8 stale` gains its noun and the ⚠ the registry already defines at ≥ 3
  - `4c +145` is spelled out — src/coach.rs:1107, inside `pub fn state_line`
    (there is no `state_tokens`)
  - the rework light's figure becomes a state word; `0` must not mean both
    "healthy" and "no data"
  - `159 api calls · 159 tool calls`: INVESTIGATE BEFORE WRITING A CRITERION.
    api_calls counts distinct message.ids, tool_calls counts tool_use blocks. If
    they are genuinely equal here, pin it with a fixture assertion. If one is
    standing in for the other, that is an A1-class bug — say so and fix it there.

A4 · The series ramp and the glyph fix.
  1. FIX stacked_bar FIRST (src/ui/widgets.rs:29–62). It picks the glyph from
     `i % 2` over the INPUT slices and `continue`s a zero-cell slice, so a
     three-slice bar on a session with no extended thinking draws two adjacent
     slices in the SAME glyph. Alternate on EMITTED segments instead. Test the
     zero-middle-slice case under `Caps { mono: true }` — this is the FR-9 test,
     and Part B's pane depends on it entirely because the pane has no colour ramp.
  2. Add `Theme::series(n)` (src/theme.rs): one hue from the theme's own accent,
     lightness carrying the order, the near endpoint solved by bisection against
     `bg` until contrast ≥ 3.2:1. Derive it from the UNREDUCED accent —
     `Theme::for_caps` (src/theme.rs:258–282) consumes self and rewrites each
     field, so a ramp derived afterwards reads a Color::Indexed with no RGB.
     Keep `Caps` or the source accent on `Theme`. There is no colour crate in
     Cargo.toml and no OKLab code in the repo: this is a new module.
  3. Pin the six bundled hexes against the generator — run
     `node tasks/design-dashboard-v2/series-ramp.mjs`, which prints the exact
     table PRD §5.2 carries. Assert the hexes, not just the properties, or the
     Rust ramp drifts from every mockup.
  4. Put series(3) through `to_ansi16` for all six themes and assert three
     distinct results — or declare the 16-colour path glyph-only and let step 1
     carry it. Say which you chose.
  5. Apply the ramp to PANEL 1's existing five-slice bar ONLY. The overview's
     three-slice bar is Part B.

Traps that will cost you a red CI:
  - src/metrics/registry.rs:278 and :289 are ORDINARY TESTS. The moment registry
    text changes, run `cctop metrics --md > docs/metrics.md` and
    `cctop metrics --readme README.md` in the SAME commit.
  - A3's coach changes invalidate eight src/ui/snapshots/…coach_*.snap AND six
    tests/pane/fixtures/coach-<moment>.json, which tests/pane/coach.test.ts
    asserts row-for-row. Regenerate with
    `scripts/pane-fixtures.sh fixtures/session-b.jsonl b` in the same commit.
  - insta: `INSTA_UPDATE=always cargo test`, then strip `assertion_line:` and
    delete every *.snap.new before committing.
  - Never hand-edit a fixture transcript's structure (CLAUDE.md) — regenerate.
  - rustfmt reformats whole files, so `make check` can go red from drift in a
    file you touched lightly. Fold it in.

Do NOT in this part: touch src/ui/dashboard.rs's layout, change
`Dashboard.schema`, delete anything tile-related, or add a keybinding.

Commit subjects are descriptive prose: `Metrics: …`, `Coach: …`, `Theme: …`.
Work on a branch; push when all four are green.
```

---

## Prompt 2 — Part B, the object and both surfaces

Two decisions are the author's and are not in the PRD. Settle them in the first
ten minutes, because `Cell.key` is a wire field and getting it wrong costs a
schema 3:

- **`0` or `Esc` as the way home.** Both are wired in the prototype.
- **Are six cells the right six?** They are the four budgets plus health plus
  volume. If one is never pressed in a dogfood week it becomes a line in another
  body rather than a cell — but it has to be decided before the schema.

```
Implement Part B of the cctop dashboard redesign: Console.

Read first:
  tasks/prd-cctop-dashboard-v2.md  — §3.6, §4, §6, §7, and US-102, US-103, US-109
  tasks/plan-dashboard-v2.md       — §3 (B0–B2), §4, §5
  CLAUDE.md
Then open tasks/design-dashboard-v2/prototype.html and click through the Console
tab at all three widths. Every screen in it is generated by proto/*.js and
measured on the character grid, so the column stops in plan §3 B2 are
transcribed from working code rather than estimated.

Three facts decide this work and are easy to disbelieve (PRD §3.6):
  - The dock is 35–85 columns and NEVER 122. p3(columns) = min(floor(columns ×
    0.45), 90, columns − 70), less BORDER_COLUMNS=4 and the 1-column grip. A
    162-column terminal gives 67 body columns; a 240-column one still gives 85.
  - The pane has four colours and they are the person's:
    `Color = 'green'|'yellow'|'red'|'cyan'` over the engine's theme keys, plus
    dimColor/bold/inverse. No hex. So Theme::series is TUI-only and the pane
    carries the same meaning through glyph alternation alone.
  - The engine draws the clickable chrome. A `plain` Button renders as the
    hotkey in the accent colour, a colon, the label. cctop draws NO hotkey
    chrome. A cell is a keyed Box of Buttons sharing one `scope` and one
    `onPress`, so the whole area clicks — padding included, drawn as a Button
    for exactly that reason.

Order matters:

B0 first, while it is a no-op. scripts/build-site.sh parses the README mockup
with FIVE layout-coupled regexes (:44 :45 :46 :47 :51) plus two silent
re.search fences (:42 :49), and none of them errors when it stops matching.
Make zero matches exit non-zero NOW. pages.yml deploys site/ on every push, so
without this the public site silently documents a UI that no longer exists.

B1 — the object. src/dashboard.rs schema 1 → 2 (current value at :203).
  Out: `tiles`, `Tile`, ROW_WIDTH as a constant.
  In:  `cells: Vec<Cell>`, `act: Act`, `body: Body`, `Slice { label, tokens,
       step: u8 }` — a STEP INDEX, never a colour, so each surface maps it
       through its own vocabulary.
  The object STOPS being width-aware rather than becoming more so: `snapshot`
  emits full-length lines and each surface cuts once at its own width. Do not
  add a --columns flag. No caller can supply a width — src/query.rs:394 is
  `pub fn dashboard(state)`, src/main.rs:114 is a bare unit variant, and
  plugin/hooks/poller.ts:135 sends none while the pane's real width arrives at
  render time (pane.tsx:506) and is null before the first render.
  Land the pane's schema gate HERE (FR-16): read `schema`, render one line
  naming the cctop it needs. There is no binary pin — the pane probes
  `cctop query --help` — so a schema-2 pane on a schema-1 binary otherwise waits
  forever on dashboardOf() → null.
  Follow: src/query.rs, docs/query.md:47, src/main.rs:114's clap help (mirrored
  as a literal in tests/pane/poller.test.ts:26).

B2 — both surfaces, ONE PUSH. B1 leaves tests/pane/overview.test.ts red;
ci.yml runs the pane job on every push, so B1 and B2 must not be two pushes.
  TUI: delete tile_block, tile_rows, FOUR_TILES, TWO_TILES, L1_TILES,
  TILE_WIDTH, and big_digits + BIG + mod big_tests in src/ui/widgets.rs. BUT
  TWO_TILES and L1_TILES gate LEDGER decisions, not tile ones — `with_detail =
  width >= TWO_TILES` (:236) and the narrow ledger_row form (:230, :189–194).
  They need replacements, not deletions.
  Pane: delete tileRows, tileLines, engineTiles, TILES_MIN, L2_MAX, type Tile,
  tileOf, and dashboardOf's hard `if (!Array.isArray(tiles)) return null`.
  TILES_MIN also gates the pane's detail rows at overview.tsx:635.
  frame.tsx's bigDigits/BIG_GLYPHS goes in THIS commit — it is live, not dead:
  overview.tsx:17 and :569, asserted at tests/pane/overview.test.ts:20,127–139.
  The pane's digit model changes with it: Model.unfolded (model.ts:113,186),
  overview.toggle (:145,301–305), viewOfDigit (overview.tsx:717), and the
  dispatcher at pane.tsx:253–255.

  Tests that break and are NOT snapshots, so INSTA_UPDATE will not fix them:
    src/theme.rs:532  assert!(text.contains("1 Context"))
    src/dashboard.rs:922–1022  asserts rows.len()==9, digit==i+1, both
                               ROW_WIDTH bounds, tiles.len()==4, schema==1
    tests/pane/overview.test.ts  nine tests (NOT fixtures.test.ts, which only
                                 JSON-parses and keeps passing)

  BUILD the fixture-B row-identity harness — it does not exist. coach.test.ts
  covers the Coach card; overview.test.ts uses fixture A and regexes, not
  Rust-rendered rows; the -b dashboard fixtures are generated but read by no
  test. Pin it at 54, 67 and 85.

  Regenerate in this order:
    cargo test                       # expect failures; read them
    INSTA_UPDATE=always cargo test   # accept; strip assertion_line:, rm *.snap.new
    scripts/pane-fixtures.sh
    scripts/pane-fixtures.sh fixtures/session-b.jsonl b
    npm run typecheck && npm test
    make check

  Four things cannot be checked by automation (CLAUDE.md). Add them to
  docs/verification/pane.md as `Result: pending`:
    - does a `plain` Button with NO hotkey draw only its label?
    - does the hover scope cover a Button whose label is only spaces?
    - can six Buttons in one band each claim a bare digit?
    - what is the real bodyColumns at 35 / 54 / 67 / 85?
```

---

## Prompt 3 — the downstream surface

```
Finish the cctop Console redesign: bring the docs, the site and the demo assets
to what ships. Read tasks/plan-dashboard-v2.md §3 B3–B4 and US-106, US-108.

B3 — the rule line's key map. `Body::keys` is already on the object from B1, so
there is no new trait and no Panel::keys. `?` expands the rule line into the
open body's full key map and collapses again. Console has no footer, so the
three legacy footer strings reduce to two: src/ui/coach_view.rs:26 and the
hard-coded one at src/app.rs:815.

B4 — text, which can be done anywhere:
  - `make site` — it was in NO phase of the earlier plans. site/index.html,
    site/metrics.html and site/guide/*.html are COMMITTED BUILD OUTPUTS.
  - README.md `## What it looks like`, regenerated from the real renderer:
    `cctop run --once --session fixtures/session-b.jsonl --size 85x24`
  - README.md:66 — the paragraph under the mockup describes the tiles and the
    nine-row ledger and is OUTSIDE every marked block, so
    `cctop metrics --readme` will not touch it.
  - site/index.template.html: §2's heading "Four lights, one nudge, nine rows",
    the tile paragraph, the `tiles` and `▸` list items, Fig. 2's caption, :95's
    "How to read it", :203's "two-tiles-a-row layout", §3's ○ ◐ ● symbol note,
    and Fig. 1's alt text.
  - plugin/README.md:35,44 — marketplace-shipped text.
  - src/query.rs:392 doc comment; module docs at src/dashboard.rs:1–6,
    src/ui/dashboard.rs:1–8, src/ui/widgets.rs:120.
  - scripts/build-guide.py:248, 251–252, 382. The coach_* sample lines at
    :239–242 are the COACH VIEW's and stay — the coach view does not change.
  - og-image.png does NOT show the dashboard (brand/build.ts:134 is the logo on
    a flat rect). Leave it.

  Assets, only on a machine with the tools:
  `make demo` needs the release binary, Chrome, ffmpeg and cwebp. Make it
  runnable in CI first — it is three lines: scripts/demo.py resolves Chrome from
  a hardcoded list plus shutil.which, and ffmpeg from shutil.which("ffmpeg"). A
  Claude Code web container ships both, off PATH, at
  $PLAYWRIGHT_BROWSERS_PATH/chromium-*/chrome-linux/chrome and
  $PLAYWRIGHT_BROWSERS_PATH/ffmpeg-*/ffmpeg-linux. Add those globs and an FFMPEG
  env override beside the existing CCTOP one. cwebp is genuinely absent and
  needs a Pillow fallback or stays local. site/demo.tape's key sequence changes
  with the bodies.

  DO NOT tick the asset acceptance criteria on a container run without cwebp.
```
