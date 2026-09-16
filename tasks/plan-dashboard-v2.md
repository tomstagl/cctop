# Implementation plan: dashboard v2

**For:** `tasks/prd-cctop-dashboard-v2.md` v1.0 — eight user stories (US-101 … US-108).
**Design:** the canvas *cctop Dashboard Redesign*. Working files in `tasks/design-dashboard-v2/`; every artboard's character grid is generated and measured by `check-grid.mjs`, so the mockups are exact cell counts.
**Relationship to `tasks/plan-dashboard-big-figures.md`:** Part B proposes to replace its §1 and §3. **It does not delete them without evidence** — see §1.2.
**Written:** 2026-09-16 · **Revised the same day** after two reviews (a factual pass against the code and an adversarial pass on the plan). §6 records what they overturned; three of their findings reversed decisions in the first draft, including its central one.

**Baseline:** `cargo test` on `78f8a2f` — 292 passed, 0 failed, 2 ignored, 5.1 s. Every "expect failures" below is measured against that.

## 1. The shape of the work

### 1.1 Two plans, not one

The first draft was one seven-phase march ending in a rewritten dashboard. The adversarial review's central objection stands: **the evidence in the PRD justifies the correctness work completely and the layout replacement not at all.** §3.1 (row budget, duplication, dead rows), §3.2 (a 2250× cost disagreement), §3.3 (four palette FAILs) are measured. "Zones read better than tiles" is one person's artboard, which is exactly what the design it replaces also was.

So the work splits, and the halves ship independently:

- **Part A — correctness.** US-101, US-104, US-105. Every measured finding in the PRD. No schema change, no renderer rewrite, no new keybinding, no layout change. Four small commits, every boundary green, on the dashboard that exists today.
- **Part B — the layout.** US-102, US-103, US-106, US-107, US-108. Gated on §1.2.

Part A is worth shipping whether or not Part B ever happens. That is the test of whether it was really the valuable half.

### 1.2 Part B does not start by deleting the thing it replaces

`tasks/plan-dashboard-big-figures.md` §5 step 5 committed to a dogfood week: *"the tile figures' thresholds tuned against real sessions; the ledger rows' wording trimmed to what a person actually reads."* `tasks/handoff-coach-round-2.md` records that it never happened — Claude Code 2.1.271 broke the pane (issue #3) before the pane half could start. **The design Part B deletes was never once evaluated under the conditions its own plan set**, and a v2 that deletes it makes the comparison impossible forever.

This repo already built the machinery for exactly this question in coach Phase 7: `cctop run --coach on|off|auto`, per-project × model-family × version arms in `~/.cctop/exposure.json`, fire records with `surface` and idle-at-fire, and `cctop coach-stats` computing causal lift.

**Part B therefore ships behind `layout = tiles | zones` on that mechanism**, defaulting to `tiles`, for one dogfood week, with time-to-act on a NOW nudge as the outcome. The tiles are deleted when the arm says so, in a separate commit, or they are not deleted. Anything else is replacing one unevaluated opinion with another.

If that is too much apparatus, the minimum is: keep both behind the flag for a week and decide from what a person actually did. Days of work against a rewrite that is weeks.

### 1.3 What the reviews changed

Three reversals, and they are load-bearing:

1. **Phase 0 was wrong to dismiss the cost disagreement.** The first draft reasoned "one `State` cannot render both values, therefore not a defect". Backwards: both were on one screen, so the surfaces were not on one `State`. `src/metrics/cost.rs:138–142` clears `since` when a `cost-state` line lands, and `:172–191`'s two branches produce exactly the observed pair. It is the highest-value finding in the PRD and it is now US-101, first, alone.
2. **`stacked_bar` cannot be reused as-is.** `src/ui/widgets.rs:31` alternates the glyph on `i % 2` over the *input* slices and `continue`s a zero-cell one, so a three-slice bar on a session with no extended thinking draws `fixed` and `yours` as adjacent runs of the same glyph. Under `Caps { mono: true }` they are indistinguishable by colour and glyph: FR-8 fails silently, and fixture B hides it.
3. **There is no `cctop_dashboard` MCP tool.** `src/mcp.rs:19–26` registers eight and none is the dashboard. The first draft asserted it in two places, as did PRD FR-1. Struck.

## 2. Part A — correctness (US-101, US-104, US-105)

Four commits. Nothing here touches a layout, a schema or a key.

### A1 — Cost (US-101)

The defect and its mechanism are PRD §3.2. `Cost::current()` (`src/metrics/cost.rs:172–191`) reports a confident `$0.000` from the authoritative branch when a `cost-state` line has cleared the estimates that would have carried `≈`.

- Reproduce with two readers over one transcript, one past the `cost-state` line and one before it, and assert they agree.
- Fix in `src/metrics/cost.rs`. Either the authoritative branch keeps what it displaced, or a near-zero total on a session with real usage carries `≈` and the registry caveat. **Not in a renderer.**
- `cctop metrics --md > docs/metrics.md && cctop metrics --readme README.md` **in this commit** — `src/metrics/registry.rs:278` and `:289` are ordinary tests, and deferring the regeneration leaves `cargo test` red.

### A2 — Turn state labels (US-101, second half)

`src/ui/panels/turn.rs:34` reads the turn; `dashboard::header` reads `coach::state_line` (`src/coach.rs:1038`). Both are honest; only the labelling implies they should match. Capture a turn-ended-while-tools-run fixture and name each field for what it reads.

### A3 — Honest figures (US-104)

Six independent one-liners, each with a test. Corrected from the first draft:

| Fix | Where |
|---|---|
| velocity `—` until two samples | `src/metrics/context.rs`; the metric returns `Option`, the renderer prints `—` |
| cost labelled API-equivalent | `src/ui/panels/tokens.rs` and `src/dashboard.rs::row_tokens` |
| `8 stale` gains its noun and `⚠` at ≥ 3 | `src/dashboard.rs`; threshold already in the registry |
| `4c +145` spelled | `src/coach.rs:1107`, inside `pub fn state_line` (**not** `state_tokens`, which does not exist) |
| rework figure → state word | `src/coach.rs` rework light |
| `159 api calls · 159 tool calls` | **investigate before writing a criterion.** "Confirmed or corrected" is a task, not an acceptance test. If they are genuinely equal here, the criterion is a fixture assertion; if one stands in for the other it is an A1-class bug |

Both coach changes invalidate the eight `src/ui/snapshots/cctop__ui__coach_view__tests__coach_*.snap` **and** the six `tests/pane/fixtures/coach-<moment>.json`, which `tests/pane/coach.test.ts` asserts row-for-row. Regenerate with `scripts/pane-fixtures.sh fixtures/session-b.jsonl b` in the same commit.

### A4 — The series ramp (US-105)

A new module: there is no colour crate in `Cargo.toml` and no OKLab code in the repo.

- `Theme::series(n)` from the **unreduced** accent. `Theme::for_caps` (`src/theme.rs:258–282`) consumes `self` and rewrites each field, so a ramp derived after reduction reads a `Color::Indexed` with no RGB. Keep `Caps` (or the source accent) on `Theme`.
- **Fix `stacked_bar` first.** Alternate on emitted segments, not on the input index. Test the zero-middle-slice case under `Caps { mono: true }`.
- Pin the six bundled hexes against `tasks/design-dashboard-v2/series-ramp.mjs`; properties alone let the Rust ramp drift from every mockup and from PRD §5.2.
- `series(3)` through `to_ansi16` for all six themes, asserting three distinct results — or declare 16-colour glyph-only and let the `stacked_bar` fix carry it.
- Apply it to **panel 1's existing five-slice bar only.** The overview's three-slice bar is Part B.

## 3. Part B — the layout, behind a flag

Ordering corrected from the first draft, whose Phase 2 boundary was red by its own admission and whose Phase 4 depended on objects Phase 2 created.

### B0 — Make the site fail loudly (before anything else)

`.github/workflows/pages.yml` deploys `site/` on every push to `main`, and `scripts/build-site.sh` cannot fail: its regexes `re.sub` to nothing and its two `re.search` extractions yield `""`. The moment a layout change lands, the public site documents a UI that does not exist, and nothing says so.

Make zero matches exit non-zero **first**, while it is a no-op. There are **five** layout-coupled regexes, not four: `build-site.sh:44` (ledger digits), `:45` (light names), `:46` (nudge), `:47` (footer), `:51` (the panel-view card — safe, the coach view is unchanged, but it must be named), plus the two `re.search` fences at `:42` and `:49`, plus `:38`'s `kv["bg"]`.

### B1 — The object, and the width handshake it needs

Schema `1 → 2` in `src/dashboard.rs:203` (current value verified). `tiles` out, `meters` / `act` / `zones` in, as the first draft described — **with three corrections:**

- **Resolve the digit question before writing the schema, not after.** `Zone.digit` and `Meter.digit` are wire fields; carrying an undecided one through a public bump means schema 3 within the week. Two zones on digit `2` is not a label collision, it is one hotkey with two owners (`src/app.rs:41`), the same defect FR-3 exists to prevent. `agents` riding the files row with no digit is hiding a panel, which the predecessor plan explicitly rejected. Decide: either the meters block owns one digit and the panels are reached from there, or `digit` becomes `Option<u8>` and the overview stops pretending to be one-row-per-panel.
- **Width.** Making `snapshot` width-aware has no caller story. `src/query.rs:394` is `pub fn dashboard(state)` with no width (and is **not** named `dashboard_json`); `src/main.rs:114` declares `Dashboard` as a bare unit variant with no flags; `plugin/hooks/poller.ts:135` sends `--session` and `--surface` on a 2 s/10 s timer, while the pane's real width arrives at render time (`pane.tsx:506`) and is `null` before the first render. A width in the object means a new `--columns` flag, a re-poll on resize, and up to ten seconds of rows cut for a stale width. **Prefer the alternative:** stop pre-cutting in the object, cut once in each surface at its own width. That removes the double cut without putting width in the schema.
- **The pane must not fail silently across a version skew.** There is no binary pin — `plugin/.claude-plugin/plugin.json` carries only the plugin's version, and the pane probes `cctop query --help` (`poller.ts:55–67`, `model.ts:334–342`). A schema-2 pane on a schema-1 binary hits `dashboardOf → null` (`overview.tsx:507–511`) and waits forever. Read `schema` and render one line saying which cctop it needs.

### B2 — Both renderers, one commit

B1 and B2 are **one shippable unit**, not two. The first draft admitted Phase 2 left `tests/pane` red until Phase 3; `.github/workflows/ci.yml` runs `npm run typecheck && npm test` on every push, so that is a deliberately red main for the length of an L-sized phase. Either land them together or make the pane a tolerant reader that accepts both shapes.

Corrections to the delete lists:

- TUI: `tile_block`, `tile_rows`, `FOUR_TILES`, `TWO_TILES`, `L1_TILES`, `TILE_WIDTH` (all in `src/ui/dashboard.rs`), and `big_digits` + `BIG` + `mod big_tests` in `src/ui/widgets.rs` (sole caller is `tile_rows`). **But `TWO_TILES` and `L1_TILES` gate ledger decisions, not tile ones** — `with_detail = width >= TWO_TILES` (`:236`) and the narrow `ledger_row` form (`:230`, `:189–194`). They need replacements.
- Pane: `tileRows`, `tileLines`, `engineTiles`, `TILES_MIN` — plus `L2_MAX`, `type Tile`, `tileOf`, and `dashboardOf`'s hard `if (!Array.isArray(tiles)) return null`. `TILES_MIN` also gates the pane's detail rows (`overview.tsx:635`).
- `frame.tsx`'s `bigDigits` / `BIG_GLYPHS` is **live**, not dead: imported at `overview.tsx:17`, called at `:569`, asserted at `tests/pane/overview.test.ts:20,127–139`. It goes here, with `tileLines` — the first draft deleted it a phase early, which does not compile.
- The failing test to expect is `tests/pane/overview.test.ts` (nine tests), **not** `fixtures.test.ts`, which only JSON-parses.
- `src/theme.rs:532` is a plain `assert!(text.contains("1 Context"))` inside `dashboard_no_color_is_monochrome` — a lowercase zone label breaks it and `INSTA_UPDATE` will not fix it.
- `src/dashboard.rs:922–1022` asserts `rows.len() == 9`, `digit == i+1`, both `ROW_WIDTH` bounds, `tiles.len() == 4` and `schema == 1`.
- The pane's digit model is unmentioned anywhere in the first draft: `Model.unfolded` (`model.ts:113,186`), `overview.toggle` (`model.ts:145,301–305`), `viewOfDigit` (`overview.tsx:717`), dispatcher `pane.tsx:253–255` (5–9 open a view, 1–4 unfold). This is the code the digit decision lands on.
- **The row-identity harness does not exist.** `coach.test.ts` covers the Coach card; `overview.test.ts` uses fixture **A** and regexes, not Rust-rendered rows, and the `-b` dashboard fixtures are generated but read by no test. PRD US-103 and §3 both say "as today" — it has to be built.

Regeneration, in order:

```
cargo test                                        # expect failures; read them
INSTA_UPDATE=always cargo test                    # accept; strip `assertion_line:`, delete *.snap.new
scripts/pane-fixtures.sh
scripts/pane-fixtures.sh fixtures/session-b.jsonl b
npm run typecheck && npm test
make check
```

### B3 — Chrome (US-106, US-107), cut to two

The first draft's Phase 5 was seven keybindings plus a theme format change, gating the site fix behind a keybinding project. Cut to:

- **The per-view footer** (FR-12). There are **three** footer sites, not one: `src/ui/dashboard.rs::FOOTER`, `src/ui/coach_view.rs:26`, and a hard-coded string at `src/app.rs:815`.
- **`w`, un-truncate** — the direct answer to the real double-cut finding.

Deferred to their own PRD, with reasons:

- `/`, `s`, `f` are **not greenfield**: `tools.rs:113,117,118` (s/S/f with an inline field), `files.rs:76` (s), `events.rs:107,111–112` (`/` with n/N), `ledger_view.rs:39–44` (s/S). The sort lives in `src/ledger.rs:40–65,165–186`, not `ledger_view.rs`. This is a consolidation across nine panels with an `f`-vs-`/` decision and an `n`/`N` conflict with `advisor.rs:43–44`.
- **`-` is already bound** to decrement the refresh interval (`src/app.rs:569`).
- **`bg = "default"` collides with A4.** The ramp solves its near endpoint against `bg`; with no background there is nothing to solve against and US-105's contrast guarantee becomes unenforceable for exactly the users who opted in. Also `build-site.sh:38` injects `background:{bg}` into CSS, and `Theme::parse` (`src/theme.rs:194–207`) returns `None` if *any* colour fails, so `bg = "default"` silently discards the whole theme today. And the only reader is `src/app.rs:802` — there is no background fill anywhere, so the first draft's "check every `Style::default().bg(...)` call site" is work that does not exist.

### B4 — The downstream surface (US-108)

Additions the reviews found, beyond the first draft's list:

- **`make site` was never run in any phase.** `site/index.html`, `site/metrics.html` and `site/guide/*.html` are committed build outputs.
- `README.md:66` — the paragraph under the mockup describes the tiles and the nine-row ledger and is **outside every marked block**, so `cctop metrics --readme` will not touch it.
- `plugin/README.md:35,44` — marketplace-shipped text.
- `src/main.rs:114` — clap help for `query dashboard`, mirrored in `tests/pane/poller.test.ts:26`.
- `src/query.rs:392` doc comment; module docs at `src/dashboard.rs:1–6`, `src/ui/dashboard.rs:1–8`, `src/ui/widgets.rs:120`.
- `scripts/build-guide.py:248, 251–252, 382` — three more blurbs. The `coach_*` sample lines at `:239–242` are the coach view's and **do** stay.
- `site/index.template.html:95` and `:203`, on top of the lines already listed.
- **`og-image.png` does not show the dashboard** — `brand/svg/cctop-og.svg` is the logo on a flat rect (`brand/build.ts:134`). Struck.

`make demo` regenerates the assets. It needs the release binary, Chrome, `ffmpeg` and `cwebp`. **Make it runnable first — it is three lines.** `scripts/demo.py` resolves Chrome from a hardcoded list plus `shutil.which`, and ffmpeg from `shutil.which("ffmpeg")`. A Claude Code web container already ships both, off `PATH`: Chromium at `$PLAYWRIGHT_BROWSERS_PATH/chromium-*/chrome-linux/chrome` (141.0.7390.37, verified) and ffmpeg at `$PLAYWRIGHT_BROWSERS_PATH/ffmpeg-*/ffmpeg-linux`. Add those globs and an `FFMPEG` env override beside the existing `CCTOP` one. `cwebp` is genuinely absent and needs a Pillow fallback or stays local.

## 4. Risks and decisions to take early

- **The digit question is not deferrable.** It is a schema field (§B1).
- **CI is what makes a red boundary expensive.** `ci.yml` runs the pane job on every push; `pages.yml` deploys the site and never diffs its output against what is committed.
- **Config migration.** `src/config.rs` already has the team's pattern (`layout`, `hidden_panels` read and ignored). Any persisted flag needs the same, plus a test.
- **Release ordering.** `Cargo.toml`, `plugin.json` and the tap formula update independently; the coach round-2 handoff records exactly that drift. A schema bump is the one release where they must not.
- **Performance and unbounded zones.** `compose()` runs at up to 4 Hz (`REFRESH_MIN_MS = 100`, `src/app.rs:26`). `w` wrapping has no ceiling, and the give-way list has no rule for a zone taller than the screen.
- **Live-terminal checks are never marked passed by automation** (CLAUDE.md). `docs/verification/pane.md` already holds 28 `Result: pending` items; adding six more to a 28-deep queue is not verification, which is a second argument for cutting B3.
- **Rustfmt reformats whole files**; fold the drift in.
- Commit subjects: `Dashboard: …`, `Theme: …`, `Metrics: …`, `Docs: …`, `Site: …`.

## 5. Where things live after the plan

| File | After Part A | After Part B |
|---|---|---|
| `src/metrics/cost.rs` | one reading per session, whichever branch | — |
| `src/coach.rs` | spelled tokens, rework as a word | — |
| `src/theme.rs` | `Theme::series(n)`, unreduced accent kept | — |
| `src/ui/widgets.rs` | `stacked_bar` alternates on emitted segments | `big_digits` / `BIG` deleted |
| `src/dashboard.rs` | unchanged | schema 2: `header`, `act`, `meters`, `zones` |
| `src/ui/dashboard.rs` | unchanged | zones, declared give-way list, no tiles |
| `plugin/hooks/views/overview.tsx` | unchanged | zones; a schema gate that says which cctop it needs |
| `plugin/hooks/views/frame.tsx` | unchanged | `bigDigits` / `BIG_GLYPHS` deleted |
| `scripts/build-site.sh` | exits non-zero on zero matches | zone-aware regexes |
| `scripts/demo.py` | finds Chrome and ffmpeg off `PATH` | — |

## 6. What the reviews overturned

Kept for provenance; the first draft is in this file's history.

| Claim in the first draft | Verdict |
|---|---|
| "Cost is already single-sourced… do not go looking for a bug here" | **Wrong, and backwards.** Now US-101, first (§1.3) |
| "`frame.tsx`'s block-digit helper is already dead" | **False** — live in `overview.tsx` and asserted in tests |
| "`src/mcp.rs`'s `cctop_dashboard`" | **Does not exist.** Eight tools, none is the dashboard |
| "the pane ships pinned to a cctop version" | **False** — no pin; the pane probes `query --help` |
| "clipped again at the terminal edge with no marker" | **False** — `spans()` cuts with `fmt::clip`, which appends `…` |
| "none on any table" (filter), "`s`/`S` on the turn ledger only" | **False** — tools, files, events and the ledger all have keys |
| "`stacked_bar` unchanged (it already alternates)" | **Insufficient** — alternates on input index, `continue`s zero slices |
| "degrade through the existing `Caps` path" | **Assumption** — `for_caps` consumes `self`; no RGB after reduction |
| "`state_tokens`", "`dashboard_json`" | Neither exists (`state_line`, `dashboard`) |
| "`tests/pane/fixtures.test.ts` will fail" | Wrong test; it is `overview.test.ts` |
| "each phase ships" | Three of six boundaries were red |
| "og-image.png if it shows the dashboard" | It does not |
| Four regexes in `build-site.sh` | Five, plus two silent `re.search` fences |
| Phase 5 in scope | Cut to two items (§B3) |
| Delete the tiles in Phase 3 | **Not without a comparison** (§1.2) |
