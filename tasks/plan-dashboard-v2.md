# Implementation plan: dashboard v2

**For:** `tasks/prd-cctop-dashboard-v2.md` v1.0 — eight user stories (US-101 … US-108).
**Design:** the canvas *cctop Dashboard Redesign*, page *Redesign* and page *Colour & k9s*. Working files in `tasks/design-dashboard-v2/`; the character grid in every artboard is generated and measured, so the mockups are exact cell counts, not sketches.
**Replaces:** `tasks/plan-dashboard-big-figures.md` §1 and §3. §2's one-object rule survives and is extended with a schema bump.
**Written:** 2026-09-16.

## 1. The shape of the work

Three layers stacked, and the order is forced:

1. **Truth.** Establish what the screenshot's contradictions actually are (PRD §3.2) before redrawing on top of them. The cost pair turns out not to be a code defect — both sides read `state.cost.current()` and `fmt::usd` cannot produce both strings from one `State` — so Phase 0 is smaller than it first looked: a fixture, a regression guard, and a labelling fix on turn state.
2. **The object.** `dashboard::snapshot` decides what every surface shows. `Dashboard.tiles` is in the public `cctop query dashboard` schema and in the MCP tool, so replacing it is a schema change, not a rendering change — it lands once, with a version bump, before either renderer moves.
3. **The surfaces.** TUI and pane change in the same phase or the fixture-B row-identity tests fail by design. Chrome and keys come after, because they are additive and can ship late.

The site, the guide and the demo assets are a phase, not a footnote: `scripts/build-site.sh` parses the current layout with four regexes that a redesign breaks silently — the build keeps succeeding and emits an unmarked mockup.

**The precedent to copy is already in the repo.** `src/ui/coach_view.rs` draws the same four lights as `○ context  720k ▇▇▇▇▇▇▇▁▁▁ 72% · ≈$.37/call` — a labelled meter with its unit attached. The dashboard is the outlier, not the coach view, and the coach view does not change in this work.

| Phase | Stories | Ships | Size |
|---|---|---|---|
| 0 | US-101 | the turn-state labels, a turn-ended-while-tools-run fixture, and the cost regression guard | S |
| 1 | US-105 (ramp only) | `Theme::series`, the contrast solve, the six-theme test | S |
| 2 | US-102/103 (object half) | `Dashboard` schema 2: `meters`, `act`, `zones`; `tiles` removed; `query`/MCP/`docs/query.md` follow | M |
| 3 | US-102, US-103, US-105 (render half) | both renderers draw zones; block digits deleted; every snapshot and pane fixture regenerated | L |
| 4 | US-104 | honest figures — the six corrections, each with a test | M |
| 5 | US-106, US-107 | `Ctrl-E`, `w`, `z`, `/`, `s`, `-`, per-view footer, `bg = "default"` | M |
| 6 | US-108 | README, site template, build regexes, guide, `make demo`, `docs/query.md` | M |

Dependencies: 2 needs 1 (the ramp is a field on the object's slices); 3 needs 2; 4 is independent of 3 but its tests are cheaper after 3; 5 needs 3; 6 needs 3 and 5 (the footer text is on the screenshots).

## 2. Phases

### Phase 0 — Establish the baseline (US-101)

Nothing here is a design change, and it is smaller than the screenshot suggested.

- **Cost is already single-sourced.** `src/ui/panels/tokens.rs:94` and `src/dashboard.rs:450` both call `state.cost.current()`, and `fmt::usd` (`src/ui/fmt.rs:27`) emits `$0.000` only below `$0.01`, so no single `State` renders both `$0.000` and `≈$22.5`. Add the regression test that pins this — render both from one fixture `State`, assert the same string — and move on. Do not go looking for a bug here.
- **Turn state is two sources answering two questions.** `src/ui/panels/turn.rs` reads `agg.current_turn()`; `dashboard::header` takes the phase word from `coach::snapshot`. Capture a turn-ended-while-tools-run fixture with `scripts/anonymise-transcript.py` (never hand-edit structure — CLAUDE.md), assert what each says, and fix the *labels* so the difference reads as intended rather than as a contradiction.
- Only if the fixture shows the classifier naming a phase with no tool activity and no open turn is there a bug, and it is in `src/phase.rs`.

**Done when:** the fixture exists, both tests pass, and the two turn readings are named for what they read.

### Phase 1 — The series ramp (US-105, first half)

- `src/theme.rs`: `Theme::series(&self, n: usize) -> Vec<Color>`. OKLab conversion, hue from `accent`, chroma `0.28 → 1.0 × accent`, lightness from a near endpoint solved by bisection against `bg` until contrast ≥ 3.2 : 1, to a far endpoint of `max(near ± 0.30, accent.L ± 0.14)` clamped to `[0.30, 0.94]`. Direction flips on `bg.L < 0.5`.
- No new key in `themes/*.toml`. `Caps` reduction applies afterwards exactly as it does to the role colours, so 256-colour and 16-colour terminals degrade through the existing path.
- Test: for all six bundled themes plus two synthetic extremes (near-black accent, near-white accent), assert monotone lightness in the surface-relative direction and min contrast ≥ 3.2 : 1. The expected hexes for the six are in PRD §5.2 — assert the properties, not the literals, so a theme tweak does not break the test spuriously.
- Nothing renders with it yet.

### Phase 2 — The object (US-102/103, object half)

`src/dashboard.rs`, schema `1 → 2`.

- Out: `tiles: Vec<Tile>`, `Tile`.
- In: `meters: Vec<Meter>` — `{ id, label, value, pct: Option<f64>, bar: Option<Bar>, detail: Line, digit, level }`, where `Bar` is either `Gauge { ratio }` or `Stacked { slices: [{ label, tokens, step }] }` and `step` is the index into `Theme::series`, not a colour. The object stays colour-free; a surface maps `step` through its own theme, which is what keeps the pane and the TUI identical.
- In: `act: Act` — `{ class, headline, action, evidence, health: Line, blocking: bool }`. Built from the same `coach::snapshot` nudge the ledger uses today; `blocking` is true for a pending permission wait or `AskUserQuestion`, which pre-empts the nudge (PRD §4.2).
- In: `zones: Vec<Zone>` — `{ id, digit, rows: Vec<Line>, weight }`, replacing `rows: Vec<Row>`. `weight` is the give-way order (FR-11).
- Kept: `header`, `nudge` (the coach's own object, still used by the coach view), `session_mode`, `lines`, `start_line`, `exposed`.
- `ROW_WIDTH` stops being a constant: a zone's rows are cut at the width the surface asks for, so the object gains `fn snapshot(state, engine, width: usize)`. This is the fix for the double truncation — the object no longer cuts at 118 and hands a 131-cell row to a 122-cell terminal.
- `src/query.rs` `dashboard_json`, `src/mcp.rs`'s `cctop_dashboard`, `docs/query.md` line 47.
- `plugin/hooks/model.ts`, `views/overview.tsx`, `views/frame.tsx` gain the types but keep drawing the old shape until Phase 3 — except that `frame.tsx`'s block-digit helper (the `big_digits` port) is already dead and goes here.

**Schema policy.** `cctop query dashboard` is a documented interface. Bump `schema` to 2 and say so in `docs/query.md`; there is no deprecation window because the only consumers are in this repo (`tests/pane`, the pane, the MCP tool) and the pane ships pinned to a cctop version through `plugin/.claude-plugin`.

**Regenerate:** `scripts/pane-fixtures.sh` for both fixtures, then `scripts/pane-fixtures.sh fixtures/session-b.jsonl b`. The `dashboard.json` / `dashboard-b.json` fixtures change shape; `tests/pane/fixtures.test.ts` will fail until Phase 3.

### Phase 3 — Both renderers (US-102, US-103, US-105 second half)

TUI — `src/ui/dashboard.rs`:

- Delete `tile_block`, `tile_rows`, `FOUR_TILES`, `TWO_TILES`, `L1_TILES`, `TILE_WIDTH`, and `big_digits` + `BIG` + its test module in `src/ui/widgets.rs`.
- `compose()` becomes: build every zone, then apply the declared give-way list until `need ≤ h`, then let the last drawn zone absorb the remainder. The list is a `const [&str]` of zone ids, so FR-11 is data.
- Column stops from PRD §4.2 as named constants. A test asserts every emitted row is exactly `width` cells at 40 / 60 / 80 / 100 / 122 — this caught 35 overflows in the mockups and will catch them here.
- The stacked context bar uses `stacked_bar` unchanged (it already alternates `▇`/`▆`), with `Theme::series(3)` on the overview and `series(5)` in panel 1.

Pane — `plugin/hooks/views/overview.tsx`:

- Delete `tileRows`, `tileLines`, `engineTiles`, `TILES_MIN`.
- Draw the same zones from the schema-2 object. The `engineTiles` fallback (what the pane shows before the binary answers) becomes an `engineMeters` with the same two meters it can build from `$.session.usage()` — context and limits — and the rest `—`.
- `MIN_DOCK_COLUMNS` already names the narrow breakpoint; §4.3's `< 60` form is a third branch.

**Regenerate, in this order:**

```
cargo test                                        # expect failures; read them
INSTA_UPDATE=always cargo test                    # accept; strip `assertion_line:`, delete *.snap.new
scripts/pane-fixtures.sh
scripts/pane-fixtures.sh fixtures/session-b.jsonl b
npm run typecheck && npm test
make check
```

The ASCII snapshot (`dashboard_ascii`) is the FR-8 test: if the three slices are not distinguishable there, the encoding has failed.

### Phase 4 — Honest figures (US-104)

Each is a one-line change with a test, and each is independent:

| Fix | Where |
|---|---|
| velocity `—` until two samples | `src/metrics/context.rs` — the EMA has no sample on turn 1; the *metric* returns `Option`, the renderer prints `—` |
| cost labelled API-equivalent | `dashboard.rs` spend zone + `panels/tokens.rs`; wording from the registry caveat |
| `8 stale` gains its noun and `⚠` at ≥ 3 | `dashboard.rs` health ribbon; threshold already in the registry (`file_rereads`) |
| `4c +145` spelled | `src/coach.rs::state_tokens` — the format string, and the coach view inherits it |
| rework figure → state word | `src/coach.rs` rework light `number`; the coach view shows the same word |
| `159 api calls · 159 tool calls` | investigate first. `api_calls` counts distinct `message.id`, `tool_calls` counts `tool_use` blocks. If they are genuinely equal here, keep both and say so; if one is standing in for the other, it is a Phase 0-class bug and moves there |

### Phase 5 — Chrome and keys (US-106, US-107)

- `Panel::keys(&State) -> Vec<(&str, &str)>` on the trait, default empty; the overview supplies its own. `FOOTER` becomes a function of the active view. This is the one trait change and it is additive.
- `Ctrl-E` collapse, `w` wide, `z` faults-only: flags on `State`, persisted in `~/.config/cctop/config.toml` alongside `coach` and `theme`.
- `/` filter: the tools, files and events panels already own their row lists; a filter string on `State` and a predicate per panel.
- `s` sort: the turn ledger's existing sort (`ledger_view.rs`) generalised to a `SortBy` on each table panel.
- `-` previous view and the breadcrumb: a small `Vec<View>` history on `State`.
- `bg = "default"`: `ThemeFile.bg` becomes `Option<String>`; `Theme.bg: Option<Color>`; the draw path skips the background fill. Check every `Style::default().bg(...)` call site.
- Pane: the view chips already do what `-` does; wide and faults-only become buttons.

### Phase 6 — The downstream surface (US-108)

This is where a redesign silently rots. `scripts/build-site.sh` parses the README mockup with four regexes, all of which match the current layout and none of which will error when they stop matching:

```python
re.sub(r"^ (\d) ([A-Z][a-z]+) ", ...)                    # ledger rows " 1 Context "
re.sub(r"([○◐●◆]) (context|cache|limits|rework)\b", ...)  # tile names
re.sub(r"^( ▸ .*)$", ...)                                 # the nudge
re.sub(r"^( \?help .*)$", ...)                            # the footer
```

- Rewrite all four for zones, and **add an assertion**: if a regex matches zero times, `build-site.sh` exits non-zero. A silently unmarked mockup is worse than a failed build.
- README: `## What it looks like` mockup regenerated from the real renderer (`cctop run --once --session fixtures/session-b.jsonl --size 100x30`), and the `panels:start` table reworded — the nine panels are still the nine panels behind the digits, but the overview is no longer nine rows.
- `site/index.template.html`: §2's heading "Four lights, one nudge, nine rows — and what each one is for", the tile paragraph, the `tiles` and `▸` list items, the Fig. 2 caption ("From 120 columns the four tiles sit in one row"), the key list in §3, and the `○ ◐ ●` symbol note. The `two-pane` alt text and Fig. 1 caption name the lights and the ledger.
- `scripts/build-guide.py`: the `coach_*` entries describe the lights and stay (the coach view is unchanged); the sample line `"coach_context": "◐ context  396k ▇▇▇▇▁▁▁▁▁▁ 40% · ≈$.08/call"` is the coach view's and stays. Add a guide page for the meters and the series ramp — PRD §5.3 in plain words is the content.
- `site/demo.tape`: the key sequence (`Tab`, `Enter`, `i`, `Esc`, `q`) still works; add `w` and `z` if Phase 5 shipped.
- `make demo` regenerates `two-pane.png/webp`, six `theme-*.png`, `demo.gif/webm`. It needs the release binary, Chrome, `ffmpeg` and `cwebp`.

  **Make it runnable in CI and in web sessions first — it is three lines.** `scripts/demo.py` resolves Chrome from a hardcoded list plus `shutil.which`, and ffmpeg from `shutil.which("ffmpeg")` alone. A Claude Code web container already ships both, just not on `PATH`: Chromium at `$PLAYWRIGHT_BROWSERS_PATH/chromium-*/chrome-linux/chrome` (141.0.7390.37, verified) and ffmpeg at `$PLAYWRIGHT_BROWSERS_PATH/ffmpeg-*/ffmpeg-linux`. Add those globs to the `CHROME` candidate list and an `FFMPEG` env override beside the existing `CCTOP` one, and the asset regeneration stops being a "local machine only" step. `cwebp` is still a real dependency and is not present — either add a Pillow fallback for the WebP poster or keep that one step local.

  Until that lands, split US-108 into "regenerate text" (anywhere) and "regenerate assets" (a machine with the four tools), and **do not tick the asset criteria on a container run**.
- `og-image.png`: check whether it shows the dashboard; regenerate from `brand/build.ts` if so.
- `cctop metrics --md > docs/metrics.md && cctop metrics --readme README.md` — the registry text changes in Phase 4 (velocity, cost wording) and Phase 1 (`context_anatomy` encoding note).
- `docs/query.md` line 47 — the schema-2 shape.

## 3. Keeping the two surfaces in sync

Unchanged from the coach plan and load-bearing here: `tests/pane` renders fixture B's moments through both the Rust and the TypeScript renderer and asserts the rows are identical. Any zone that cannot be made row-identical is a zone whose logic has leaked into a renderer — push it back into `dashboard.rs`.

The new risk is that the object is now width-aware (Phase 2). The row-identity tests must pin a width explicitly, and should run at 122, 80 and 40 rather than one width.

## 4. Risks and decisions to take early

- **The schema bump is the point of no return.** Everything after Phase 2 assumes `meters`/`zones`, and `cctop query dashboard` is documented. Phase 0 no longer gates it (the cost pair was not a defect), but the turn-state labels should land before the object is redrawn around them.
- **`Panel::ledger` does not exist** — `dashboard.rs` has free functions `row_context`, `row_tokens`, … The zones are not one-per-panel any more (`agents` rides `files`; `cache` and `spend` share digit `2`). Decide in Phase 2 whether zones keep a digit at all, or whether the digit becomes a *hint* printed in the gutter that maps to a panel. The PRD leaves this open on purpose (§9).
- **Two zones sharing digit `2`** is a real wart in the mockup. Either `spend` gets its own digit (there is no free one under 9) or the meters block gets one digit for all four and the panels are reached from there. Resolve before Phase 3.
- **`make demo` cannot run everywhere.** Phase 6 splits into "regenerate text" (anywhere) and "regenerate assets" (a machine with Chrome and ffmpeg). Do not mark US-108 done on a container run.
- **Live-terminal checks are never marked passed by automation** (CLAUDE.md). `Ctrl-E`, `w`, `z`, `/`, `s`, `-` all need a person at a real terminal; they go into `docs/verification/pane.md` as `Result: pending`.
- **Rustfmt reformats whole files**, so `make check` can go red from drift in files touched lightly — fold it in rather than leave it.
- **Commit subjects** are descriptive prose: `Dashboard: …`, `Theme: …`, `Docs: …`, `Site: …`.

## 5. Where things live after the plan

| File | After |
|---|---|
| `src/dashboard.rs` | schema 2: `header`, `act`, `meters`, `zones`, `nudge`, `lines`; width-aware `snapshot` |
| `src/ui/dashboard.rs` | zone composition, declared give-way list, column-stop constants; no tiles |
| `src/ui/widgets.rs` | `gauge`, `stacked_bar`, `sparkline`; `big_digits` and `BIG` deleted |
| `src/theme.rs` | `Theme::series(n)`, `bg: Option<Color>` |
| `src/ui/coach_view.rs` | unchanged — it was already right |
| `plugin/hooks/views/overview.tsx` | zones; no `tileRows` / `tileLines` / `engineTiles` |
| `plugin/hooks/views/frame.tsx` | block-digit port deleted |
| `scripts/build-site.sh` | zone-aware regexes that fail loudly on zero matches |
| `docs/verification/pane.md` | six new pending live checks |
